use candle_core::{DType, Device, Result, Tensor};
use candle_nn::{Conv2d, LayerNorm, Linear, Module, VarBuilder};

struct EncLayer {
    q: Linear,
    k: Linear,
    v: Linear,
    out: Linear,
    ln1: LayerNorm,
    ln2: LayerNorm,
    fc1: Linear,
    fc2: Linear,
}
impl EncLayer {
    fn load(i: usize, vb: VarBuilder) -> Result<Self> {
        let p = vb.pp(format!("thinker.audio_tower.layers.{i}"));
        let a = p.pp("self_attn");
        Ok(Self {
            q: candle_nn::linear(1024, 1024, a.pp("q_proj"))?,
            k: candle_nn::linear(1024, 1024, a.pp("k_proj"))?,
            v: candle_nn::linear(1024, 1024, a.pp("v_proj"))?,
            out: candle_nn::linear(1024, 1024, a.pp("out_proj"))?,
            ln1: candle_nn::layer_norm(1024, 1e-5, p.pp("self_attn_layer_norm"))?,
            ln2: candle_nn::layer_norm(1024, 1e-5, p.pp("final_layer_norm"))?,
            fc1: candle_nn::linear(1024, 4096, p.pp("fc1"))?,
            fc2: candle_nn::linear(4096, 1024, p.pp("fc2"))?,
        })
    }
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let (_, seq, _) = x.dims3()?;
        let h = self.ln1.forward(x)?;
        let q = self
            .q
            .forward(&h)?
            .reshape((seq, 16, 64))?
            .transpose(0, 1)?
            .unsqueeze(0)?
            .contiguous()?;
        let k = self
            .k
            .forward(&h)?
            .reshape((seq, 16, 64))?
            .transpose(0, 1)?
            .unsqueeze(0)?
            .contiguous()?;
        let v = self
            .v
            .forward(&h)?
            .reshape((seq, 16, 64))?
            .transpose(0, 1)?
            .unsqueeze(0)?
            .contiguous()?;
        let scores = (q.matmul(&k.transpose(2, 3)?.contiguous()?)? * (1.0 / 8.0f64))?;
        let ctx = candle_nn::ops::softmax_last_dim(&scores)?
            .matmul(&v)?
            .transpose(1, 2)?
            .contiguous()?
            .reshape((seq, 1024))?
            .unsqueeze(0)?
            .contiguous()?;
        let x = (x + self.out.forward(&ctx)?)?;
        let h = self.ln2.forward(&x)?;
        let h = self.fc1.forward(&h)?.gelu_erf()?;
        let f = self.fc2.forward(&h)?;
        x + f
    }
}

pub struct AudioEncoder {
    c1: Conv2d,
    c2: Conv2d,
    c3: Conv2d,
    conv_out: Linear,
    layers: Vec<EncLayer>,
    ln_post: LayerNorm,
    proj1: Linear,
    proj2: Linear,
    device: Device,
    dtype: DType,
}
impl AudioEncoder {
    pub fn load(vb: VarBuilder, device: &Device) -> Result<Self> {
        let c = |channels, stride, path| {
            candle_nn::conv2d(
                channels,
                480,
                3,
                candle_nn::Conv2dConfig {
                    padding: 1,
                    stride,
                    dilation: 1,
                    groups: 1,
                    cudnn_fwd_algo: None,
                },
                vb.pp(path),
            )
        };
        let mut layers = Vec::with_capacity(24);
        for i in 0..24 {
            layers.push(EncLayer::load(i, vb.clone())?);
        }
        let dtype = vb.dtype();
        Ok(Self {
            c1: c(1, 2, "thinker.audio_tower.conv2d1")?,
            c2: c(480, 2, "thinker.audio_tower.conv2d2")?,
            c3: c(480, 2, "thinker.audio_tower.conv2d3")?,
            conv_out: candle_nn::linear_no_bias(7680, 1024, vb.pp("thinker.audio_tower.conv_out"))?,
            layers,
            ln_post: candle_nn::layer_norm(1024, 1e-5, vb.pp("thinker.audio_tower.ln_post"))?,
            proj1: candle_nn::linear(1024, 1024, vb.pp("thinker.audio_tower.proj1"))?,
            proj2: candle_nn::linear(1024, 2048, vb.pp("thinker.audio_tower.proj2"))?,
            device: device.clone(),
            dtype,
        })
    }

    /// Encode [128, frames] Whisper features, preserving the model's 100-frame
    /// convolution windows. Transformers SDPA attends across the full flattened
    /// sequence; cu_seqlens is only consumed by its Flash Attention backend.
    pub fn forward(&self, mel: &[f32], frames: usize) -> Result<Tensor> {
        let mut pieces = Vec::new();
        for start in (0..frames).step_by(100) {
            let len = (frames - start).min(100);
            let mut chunk = vec![0f32; 128 * 100];
            for m in 0..128 {
                chunk[m * 100..m * 100 + len]
                    .copy_from_slice(&mel[m * frames + start..m * frames + start + len]);
            }
            let x =
                Tensor::from_vec(chunk, (1, 1, 128, 100), &self.device)?.to_dtype(self.dtype)?;
            let x = self.c1.forward(&x)?.gelu_erf()?;
            let x = self.c2.forward(&x)?.gelu_erf()?;
            let x = self.c3.forward(&x)?.gelu_erf()?;
            let x = x
                .permute((0, 3, 1, 2))?
                .contiguous()?
                .reshape((1, 13, 7680))?;
            let x = self.conv_out.forward(&x)?;
            let mut pos = vec![0f32; 13 * 1024];
            for t in 0..13 {
                for j in 0..512 {
                    let f = 10000f32.powf(-(j as f32) / 511.0);
                    pos[t * 1024 + j] = (t as f32 * f).sin();
                    pos[t * 1024 + 512 + j] = (t as f32 * f).cos();
                }
            }
            let pos = Tensor::from_vec(pos, (1, 13, 1024), &self.device)?.to_dtype(x.dtype())?;
            let x = (x + pos)?;
            let valid = ceil_div(ceil_div(ceil_div(len, 2), 2), 2);
            pieces.push(x.narrow(1, 0, valid)?);
        }
        let mut x = Tensor::cat(&pieces, 1)?;
        for layer in self.layers.iter() {
            x = layer.forward(&x)?;
        }
        let x = self.ln_post.forward(&x)?;
        let x = self.proj1.forward(&x)?.gelu_erf()?;
        self.proj2.forward(&x)
    }
}
fn ceil_div(x: usize, y: usize) -> usize {
    x.div_ceil(y)
}
