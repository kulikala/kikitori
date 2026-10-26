use candle_core::{DType, Device, Result, Tensor};
use candle_nn::{Embedding, Linear, Module, VarBuilder};

const HIDDEN: usize = 2048;
const HEADS: usize = 16;
const KV_HEADS: usize = 8;
const HEAD_DIM: usize = 128;
const INTER: usize = 6144;
const VOCAB: usize = 151936;
const LAYERS: usize = 28;

struct Layer {
    q: Linear,
    k: Linear,
    v: Linear,
    o: Linear,
    qn: Tensor,
    kn: Tensor,
    inorm: Tensor,
    postnorm: Tensor,
    gate: Linear,
    up: Linear,
    down: Linear,
    key_cache: Option<Tensor>,
    value_cache: Option<Tensor>,
}

impl Layer {
    fn load(i: usize, vb: VarBuilder) -> Result<Self> {
        let p = vb.pp(format!("thinker.model.layers.{i}"));
        let a = p.pp("self_attn");
        Ok(Self {
            q: candle_nn::linear_no_bias(HIDDEN, HIDDEN, a.pp("q_proj"))?,
            k: candle_nn::linear_no_bias(HIDDEN, KV_HEADS * HEAD_DIM, a.pp("k_proj"))?,
            v: candle_nn::linear_no_bias(HIDDEN, KV_HEADS * HEAD_DIM, a.pp("v_proj"))?,
            o: candle_nn::linear_no_bias(HIDDEN, HIDDEN, a.pp("o_proj"))?,
            qn: a.get(HEAD_DIM, "q_norm.weight")?,
            kn: a.get(HEAD_DIM, "k_norm.weight")?,
            inorm: p.get(HIDDEN, "input_layernorm.weight")?,
            postnorm: p.get(HIDDEN, "post_attention_layernorm.weight")?,
            gate: candle_nn::linear_no_bias(HIDDEN, INTER, p.pp("mlp.gate_proj"))?,
            up: candle_nn::linear_no_bias(HIDDEN, INTER, p.pp("mlp.up_proj"))?,
            down: candle_nn::linear_no_bias(INTER, HIDDEN, p.pp("mlp.down_proj"))?,
            key_cache: None,
            value_cache: None,
        })
    }

    fn forward(
        &mut self,
        x: &Tensor,
        offset: usize,
        cos: &Tensor,
        sin: &Tensor,
        eps: f32,
    ) -> Result<Tensor> {
        let (b, seq, _) = x.dims3()?;
        let h = candle_nn::ops::rms_norm(x, &self.inorm, eps)?;
        let q = self
            .q
            .forward(&h)?
            .reshape((b, seq, HEADS, HEAD_DIM))?
            .transpose(1, 2)?
            .contiguous()?;
        let k = self
            .k
            .forward(&h)?
            .reshape((b, seq, KV_HEADS, HEAD_DIM))?
            .transpose(1, 2)?
            .contiguous()?;
        let v = self
            .v
            .forward(&h)?
            .reshape((b, seq, KV_HEADS, HEAD_DIM))?
            .transpose(1, 2)?;
        let q = candle_nn::ops::rms_norm(&q, &self.qn, eps)?;
        let k = candle_nn::ops::rms_norm(&k, &self.kn, eps)?;
        let cos = cos.narrow(0, offset, seq)?.contiguous()?;
        let sin = sin.narrow(0, offset, seq)?.contiguous()?;
        let q = candle_nn::rotary_emb::rope(&q.contiguous()?, &cos, &sin)?;
        let k = candle_nn::rotary_emb::rope(&k.contiguous()?, &cos, &sin)?;
        let k = if let Some(past) = &self.key_cache {
            Tensor::cat(&[past, &k], 2)?
        } else {
            k
        };
        let v = if let Some(past) = &self.value_cache {
            Tensor::cat(&[past, &v], 2)?
        } else {
            v
        };
        self.key_cache = Some(k.clone());
        self.value_cache = Some(v.clone());
        let k = candle_transformers::utils::repeat_kv(k, HEADS / KV_HEADS)?.contiguous()?;
        let v = candle_transformers::utils::repeat_kv(v, HEADS / KV_HEADS)?.contiguous()?;
        let mut scores =
            (q.matmul(&k.transpose(2, 3)?.contiguous()?)? * (1.0 / (HEAD_DIM as f64).sqrt()))?;
        if seq > 1 {
            let total = offset + seq;
            let mask = Tensor::from_vec(
                (0..seq)
                    .flat_map(|i| {
                        (0..total).map(move |j| {
                            if j <= offset + i {
                                0f32
                            } else {
                                f32::NEG_INFINITY
                            }
                        })
                    })
                    .collect(),
                (1, 1, seq, total),
                x.device(),
            )?
            .to_dtype(x.dtype())?;
            scores = scores.broadcast_add(&mask)?;
        }
        let attn = candle_nn::ops::softmax_last_dim(&scores)?
            .matmul(&v)?
            .transpose(1, 2)?
            .contiguous()?
            .reshape((b, seq, HIDDEN))?;
        let x = (x + self.o.forward(&attn)?)?;
        let h = candle_nn::ops::rms_norm(&x, &self.postnorm, eps)?;
        let gate = self.gate.forward(&h)?.silu()?;
        let ffn = self.down.forward(&(gate * self.up.forward(&h)?)?)?;
        x + ffn
    }
}

pub struct TextModel {
    embedding: Embedding,
    layers: Vec<Layer>,
    norm: Tensor,
    cos: Tensor,
    sin: Tensor,
    device: Device,
}

impl TextModel {
    pub fn load(vb: VarBuilder, device: &Device) -> Result<Self> {
        let embedding = candle_nn::embedding(VOCAB, HIDDEN, vb.pp("thinker.model.embed_tokens"))?;
        let mut layers = Vec::with_capacity(LAYERS);
        for i in 0..LAYERS {
            layers.push(Layer::load(i, vb.clone())?);
        }
        let norm = vb.get(HIDDEN, "thinker.model.norm.weight")?;
        let dtype = vb.dtype();
        let inv: Vec<f32> = (0..HEAD_DIM)
            .step_by(2)
            .map(|i| 1.0f32 / 1_000_000f32.powf(i as f32 / HEAD_DIM as f32))
            .collect();
        let inv = Tensor::from_vec(inv, (1, HEAD_DIM / 2), device)?;
        let pos = Tensor::arange(0u32, 65536, device)?
            .to_dtype(DType::F32)?
            .reshape((65536, 1))?;
        let freq = pos.matmul(&inv)?;
        let cos = freq.cos()?.to_dtype(dtype)?;
        let sin = freq.sin()?.to_dtype(dtype)?;
        Ok(Self {
            embedding,
            layers,
            norm,
            cos,
            sin,
            device: device.clone(),
        })
    }

    pub fn embed(&self, ids: &[u32]) -> Result<Tensor> {
        let ids = Tensor::from_vec(ids.to_vec(), (1, ids.len()), &self.device)?;
        self.embedding.forward(&ids)
    }

    pub fn forward_embeds(&mut self, mut x: Tensor, offset: usize) -> Result<Tensor> {
        for l in self.layers.iter_mut() {
            x = l.forward(&x, offset, &self.cos, &self.sin, 1e-6)?;
        }
        let h = candle_nn::ops::rms_norm(&x, &self.norm, 1e-6)?;
        let h = h.narrow(1, h.dim(1)? - 1, 1)?.reshape((1, HIDDEN))?;
        let w = self.embedding.embeddings();
        h.matmul(&w.t()?)?.reshape((1, 1, VOCAB))
    }

    pub fn forward_ids(&mut self, ids: &[u32], offset: usize) -> Result<Tensor> {
        self.forward_embeds(self.embed(ids)?, offset)
    }

    pub fn clear_cache(&mut self) {
        for layer in &mut self.layers {
            layer.key_cache = None;
            layer.value_cache = None;
        }
    }
}
