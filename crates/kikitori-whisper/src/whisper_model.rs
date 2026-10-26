use candle_transformers::models::whisper::Config;
/// 長い系列（エンコーダ）の CPU f32 では行ブロックごとに matrixmultiply を並列に回す Linear。
/// それ以外は candle_nn::Linear と同じ。
#[derive(Debug, Clone)]
struct Linear(candle_nn::Linear);

fn linear(d1: usize, d2: usize, vb: VarBuilder) -> Result<Linear> {
    candle_nn::linear(d1, d2, vb).map(Linear)
}
fn linear_no_bias(d1: usize, d2: usize, vb: VarBuilder) -> Result<Linear> {
    candle_nn::linear_no_bias(d1, d2, vb).map(Linear)
}

/// f32 の CPU テンソルを連続領域のスライスとして借りる。
fn with_f32<R>(t: &Tensor, f: impl FnOnce(&[f32]) -> R) -> Option<R> {
    if !t.is_contiguous() {
        return None;
    }
    let (storage, layout) = t.storage_and_layout();
    match &*storage {
        candle::Storage::Cpu(candle::CpuStorage::F32(v)) => Some(f(
            &v[layout.start_offset()..layout.start_offset() + t.elem_count()]
        )),
        _ => None,
    }
}

/// 内積。8 本の部分和で SIMD になる。
#[inline(always)]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0f32; 8];
    let mut ca = a.chunks_exact(8);
    let mut cb = b.chunks_exact(8);
    for (x, y) in (&mut ca).zip(&mut cb) {
        for i in 0..8 {
            acc[i] += x[i] * y[i];
        }
    }
    let mut s: f32 = acc.iter().sum();
    for (x, y) in ca.remainder().iter().zip(cb.remainder()) {
        s += x * y;
    }
    s
}

/// 行が少ない（デコーダ）ときの x (rows, k) · w(n, k)^T。出力の列ごとに並列。
fn matvec_nt(
    xs: &[f32],
    ws: &[f32],
    bias: Option<&[f32]>,
    rows: usize,
    k: usize,
    n: usize,
) -> Vec<f32> {
    let mut c = vec![0f32; rows * n];
    let blk = (n / (rayon::current_num_threads() * 4)).max(64);
    for r in 0..rows {
        let x = &xs[r * k..(r + 1) * k];
        c[r * n..(r + 1) * n]
            .par_chunks_mut(blk)
            .enumerate()
            .for_each(|(i, cc)| {
                for (j, e) in cc.iter_mut().enumerate() {
                    let col = i * blk + j;
                    *e = dot(x, &ws[col * k..(col + 1) * k]) + bias.map_or(0.0, |b| b[col]);
                }
            });
    }
    c
}

/// 自前の CPU の経路（並列の GEMV・GEMM・注意・GELU）を使うか。Accelerate を入れたビルドは
/// candle の経路（Accelerate の行列演算）のほうが速いので、そちらを使う。
fn own_cpu_kernels(t: &Tensor) -> bool {
    !cfg!(feature = "accelerate") && t.device().is_cpu() && t.dtype() == candle::DType::F32
}

/// 出力語彙への射影（x · embeddings^T）。CPU f32 で行が少ないときの並列版。
fn project_nt(x: &Tensor, w: &Tensor) -> Result<Tensor> {
    let (n, k) = w.dims2()?;
    let rows: usize = x.dims().iter().rev().skip(1).product();
    if rows <= 16 && own_cpu_kernels(x) && x.dim(D::Minus1)? == k {
        let out = with_f32(x, |xs| {
            with_f32(w, |ws| matvec_nt(xs, ws, None, rows, k, n))
        })
        .flatten();
        if let Some(c) = out {
            let mut dims = x.dims().to_vec();
            *dims.last_mut().unwrap() = n;
            return Tensor::from_vec(c, dims, x.device());
        }
    }
    // 元の書き方のまま（Accelerate は転置したままの行列を写さずに掛ける）
    let w = w.broadcast_left(x.dim(0)?)?;
    x.matmul(&w.t()?)
}

impl Module for Linear {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let w = self.0.weight();
        let (n, k) = w.dims2()?;
        let rows: usize = x.dims().iter().rev().skip(1).product();
        if rows <= 16 && own_cpu_kernels(x) && x.dim(D::Minus1)? == k {
            let bias = self.0.bias();
            let out = with_f32(x, |xs| {
                with_f32(w, |ws| {
                    let bs = bias.and_then(|b| with_f32(b, |b| b.to_vec()));
                    matvec_nt(xs, ws, bs.as_deref(), rows, k, n)
                })
            })
            .flatten();
            if let Some(c) = out {
                let mut dims = x.dims().to_vec();
                *dims.last_mut().unwrap() = n;
                return Tensor::from_vec(c, dims, x.device());
            }
        }
        let eligible = rows >= 256 && own_cpu_kernels(x) && x.dim(D::Minus1)? == k;
        if eligible {
            let bias = self.0.bias();
            let out = with_f32(x, |xs| {
                with_f32(w, |ws| {
                    let bs = bias.and_then(|b| with_f32(b, |b| b.to_vec()));
                    let mut c = vec![0f32; rows * n];
                    let blk = rows.div_ceil(rayon::current_num_threads()).max(32);
                    c.par_chunks_mut(blk * n).enumerate().for_each(|(i, cc)| {
                        let r = cc.len() / n;
                        unsafe {
                            matrixmultiply::sgemm(
                                r,
                                k,
                                n,
                                1.0,
                                xs.as_ptr().add(i * blk * k),
                                k as isize,
                                1,
                                ws.as_ptr(),
                                1,
                                k as isize,
                                0.0,
                                cc.as_mut_ptr(),
                                n as isize,
                                1,
                            );
                        }
                        if let Some(b) = &bs {
                            for row in cc.chunks_exact_mut(n) {
                                for (e, bv) in row.iter_mut().zip(b) {
                                    *e += *bv;
                                }
                            }
                        }
                    });
                    c
                })
            })
            .flatten();
            if let Some(c) = out {
                let mut dims = x.dims().to_vec();
                *dims.last_mut().unwrap() = n;
                return Tensor::from_vec(c, dims, x.device());
            }
        }
        self.0.forward(x)
    }
}
use candle::{Device, IndexOp, Result, Tensor, D};
use candle_nn::attention::{flash_attn, AttnMask};
use candle_nn::{embedding, Conv1d, Conv1dConfig, Embedding, LayerNorm, Module, VarBuilder};
use rayon::prelude::*;

fn conv1d(
    in_channels: usize,
    out_channels: usize,
    kernel_size: usize,
    config: Conv1dConfig,
    vb: VarBuilder,
) -> Result<Conv1d> {
    let weight = vb.get((out_channels, in_channels, kernel_size), "weight")?;
    let bias = vb.get(out_channels, "bias")?;
    Ok(Conv1d::new(weight, Some(bias), config))
}

fn layer_norm(size: usize, vb: VarBuilder) -> Result<LayerNorm> {
    let weight = vb.get(size, "weight")?;
    let bias = vb.get(size, "bias")?;
    Ok(LayerNorm::new(weight, bias, 1e-5))
}

/// candle の gelu は CPU で 1 スレッドのスカラー tanh なので、同じ式のまま並列に掛ける。
fn gelu(x: &Tensor) -> Result<Tensor> {
    if own_cpu_kernels(x) {
        let shape = x.shape().clone();
        let mut v = x.flatten_all()?.to_vec1::<f32>()?;
        v.par_chunks_mut(1 << 14).for_each(|chunk| {
            for e in chunk.iter_mut() {
                *e = gelu_scalar(*e);
            }
        });
        Tensor::from_vec(v, shape, x.device())
    } else {
        x.gelu()
    }
}

#[inline(always)]
fn gelu_scalar(v: f32) -> f32 {
    const SQRT_TWO_OVER_PI: f32 = 0.797_884_6;
    let u = SQRT_TWO_OVER_PI * v * (1.0 + 0.044715 * v * v);
    // tanh(u) = sign(u) (1 - e) / (1 + e),  e = exp(-2|u|)。ループに並べると SIMD になる。
    let e = fast_exp(-2.0 * u.abs());
    let t = ((1.0 - e) / (1.0 + e)).copysign(u);
    0.5 * v * (1.0 + t)
}

// https://github.com/openai/whisper/blob/f572f2161ba831bae131364c3bffdead7af6d210/whisper/model.py#L62
#[derive(Debug, Clone)]
struct MultiHeadAttention {
    query: Linear,
    key: Linear,
    value: Linear,
    out: Linear,
    n_head: usize,
    span: tracing::Span,
    softmax_span: tracing::Span,
    matmul_span: tracing::Span,
    kv_cache: Option<(Tensor, Tensor)>,
}

impl MultiHeadAttention {
    fn load(n_state: usize, n_head: usize, vb: VarBuilder) -> Result<Self> {
        let span = tracing::span!(tracing::Level::TRACE, "multi-head-attn");
        let softmax_span = tracing::span!(tracing::Level::TRACE, "multi-head-attn-softmax");
        let matmul_span = tracing::span!(tracing::Level::TRACE, "multi-head-attn-matmul");
        let query = linear(n_state, n_state, vb.pp("q_proj"))?;
        let value = linear(n_state, n_state, vb.pp("v_proj"))?;
        let key = linear_no_bias(n_state, n_state, vb.pp("k_proj"))?;
        let out = linear(n_state, n_state, vb.pp("out_proj"))?;
        Ok(Self {
            query,
            key,
            value,
            out,
            n_head,
            span,
            softmax_span,
            matmul_span,
            kv_cache: None,
        })
    }

    fn forward(
        &mut self,
        x: &Tensor,
        xa: Option<&Tensor>,
        mask: Option<&Tensor>,
        flush_cache: bool,
    ) -> Result<Tensor> {
        let _enter = self.span.enter();
        let t0 = crate::profile::start();
        let q = self.query.forward(x)?;
        let (k, v) = match xa {
            None => {
                if flush_cache {
                    self.kv_cache = None;
                }
                let new_k = self.key.forward(x)?;
                let new_v = self.value.forward(x)?;
                let (k, v) = if let Some((past_k, past_v)) = &self.kv_cache {
                    (
                        Tensor::cat(&[past_k, &new_k], 1)?,
                        Tensor::cat(&[past_v, &new_v], 1)?,
                    )
                } else {
                    (new_k, new_v)
                };
                self.kv_cache = Some((k.clone(), v.clone()));
                (k, v)
            }
            Some(x) => {
                if flush_cache {
                    self.kv_cache = None;
                }
                if let Some((k, v)) = &self.kv_cache {
                    (k.clone(), v.clone())
                } else {
                    let k = self.key.forward(x)?;
                    let v = self.value.forward(x)?;
                    self.kv_cache = Some((k.clone(), v.clone()));
                    (k, v)
                }
            }
        };
        let offset = if xa.is_none() {
            k.dim(1)? - q.dim(1)?
        } else {
            0
        };
        crate::profile::add("attn.qkv_proj", t0);
        let t1 = crate::profile::start();
        let wv = self.qkv_attention(&q, &k, &v, mask, offset)?;
        crate::profile::add("attn.core", t1);
        let t2 = crate::profile::start();
        let out = self.out.forward(&wv)?;
        crate::profile::add("attn.out_proj", t2);
        Ok(out)
    }

    fn reshape_head(&self, x: &Tensor) -> Result<Tensor> {
        let (n_batch, n_ctx, n_state) = x.dims3()?;
        let target_dims = &[n_batch, n_ctx, self.n_head, n_state / self.n_head];
        x.reshape(target_dims)?.transpose(1, 2)
    }

    fn reshape_head_seq(&self, x: &Tensor) -> Result<Tensor> {
        let (n_batch, n_ctx, n_state) = x.dims3()?;
        let target_dims = &[n_batch, n_ctx, self.n_head, n_state / self.n_head];
        x.reshape(target_dims)
    }

    fn qkv_attention(
        &self,
        q: &Tensor,
        k: &Tensor,
        v: &Tensor,
        mask: Option<&Tensor>,
        offset: usize,
    ) -> Result<Tensor> {
        let (_, n_ctx, n_state) = q.dims3()?;
        let head_dim = n_state / self.n_head;
        // CPU の f32 は flash attention（score 行列を作らない）を使う。
        // q/k/v は (batch, seq, head, dim) の並びにする。
        if mask.is_none() && n_ctx >= 64 && k.dim(1)? == n_ctx && own_cpu_kernels(q) {
            return self.encoder_attention_cpu(q, k, v);
        }
        if own_cpu_kernels(q) {
            let q = self.reshape_head_seq(q)?.contiguous()?;
            let k = self.reshape_head_seq(k)?.contiguous()?;
            let v = self.reshape_head_seq(v)?.contiguous()?;
            let attn_mask = if mask.is_some() {
                AttnMask::Causal { kv_offset: offset }
            } else {
                AttnMask::None
            };
            let scale = 1.0 / (head_dim as f32).sqrt();
            let out = flash_attn::<f32>(&q, &k, &v, scale, attn_mask, None, None)?;
            return out.transpose(1, 2)?.flatten_from(2);
        }
        let scale = (head_dim as f64).powf(-0.25);
        let q = (self.reshape_head(q)? * scale)?;
        let k = (self.reshape_head(k)?.transpose(2, 3)? * scale)?;
        let v = self.reshape_head(v)?.contiguous()?;
        let mut qk = {
            let _enter = self.matmul_span.enter();
            q.matmul(&k)?
        };
        if let Some(mask) = mask {
            let mask = mask.i((offset..offset + n_ctx, 0..offset + n_ctx))?;
            qk = qk.broadcast_add(&mask)?
        }
        let w = {
            let _enter = self.softmax_span.enter();
            candle_nn::ops::softmax_last_dim(&qk)?
        };
        let wv = {
            let _enter = self.matmul_span.enter();
            w.matmul(&v)?
        }
        .transpose(1, 2)?
        .flatten_from(2)?;
        Ok(wv)
    }

    /// 長い系列（エンコーダ）の自己注意。頭 × 問い合わせブロックごとに rayon で並列に、
    /// score のブロック (ROWS × 系列長) を作って softmax し、そのまま v に掛ける。
    /// q・k は元の経路と同じく head_dim^-0.25 を先に掛ける。
    fn encoder_attention_cpu(&self, q: &Tensor, k: &Tensor, v: &Tensor) -> Result<Tensor> {
        const ROWS: usize = 48;
        let (_, n, n_state) = q.dims3()?;
        let heads = self.n_head;
        let hd = n_state / heads;
        let scale = (hd as f32).powf(-0.25);
        let to_vec = |t: &Tensor| -> Result<Vec<f32>> { t.flatten_all()?.to_vec1::<f32>() };
        let mut qv = to_vec(q)?;
        let mut kv = to_vec(k)?;
        let vv = to_vec(v)?;
        qv.par_iter_mut().for_each(|e| *e *= scale);
        kv.par_iter_mut().for_each(|e| *e *= scale);
        let mut out = vec![0f32; n * n_state];
        let n_blocks = n.div_ceil(ROWS);
        // 出力 (n, n_state) は行ごとに別々の領域。タスク = (ブロック, 頭) を、ブロックごとの
        // 行範囲に分けて渡す。頭ごとの列範囲は重ならないので、生ポインタで書く。
        struct OutPtr(*mut f32);
        unsafe impl Sync for OutPtr {}
        unsafe impl Send for OutPtr {}
        let optr = OutPtr(out.as_mut_ptr());
        let (qs, ks, vs) = (&qv[..], &kv[..], &vv[..]);
        (0..n_blocks * heads).into_par_iter().for_each_init(
            || vec![0f32; ROWS * n],
            |scores, task| {
                let optr = &optr;
                let (blk, h) = (task / heads, task % heads);
                let r0 = blk * ROWS;
                let rows = ROWS.min(n - r0);
                let col = h * hd;
                let s = &mut scores[..rows * n];
                unsafe {
                    // S = Q_blk (rows x hd) . K^T (hd x n)
                    matrixmultiply::sgemm(
                        rows,
                        hd,
                        n,
                        1.0,
                        qs.as_ptr().add(r0 * n_state + col),
                        n_state as isize,
                        1,
                        ks.as_ptr().add(col),
                        1,
                        n_state as isize,
                        0.0,
                        s.as_mut_ptr(),
                        n as isize,
                        1,
                    );
                }
                let mut inv = [0f32; ROWS];
                for (r, row) in s.chunks_exact_mut(n).enumerate() {
                    let m = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                    let mut sum = 0f32;
                    for e in row.iter_mut() {
                        *e = fast_exp(*e - m);
                    }
                    for c in row.chunks(8) {
                        sum += c.iter().sum::<f32>();
                    }
                    inv[r] = 1.0 / sum;
                }
                unsafe {
                    // O = P (rows x n) . V_h (n x hd)、書き先は出力の該当列
                    matrixmultiply::sgemm(
                        rows,
                        n,
                        hd,
                        1.0,
                        s.as_ptr(),
                        n as isize,
                        1,
                        vs.as_ptr().add(col),
                        n_state as isize,
                        1,
                        0.0,
                        optr.0.add(r0 * n_state + col),
                        n_state as isize,
                        1,
                    );
                    for (r, &scale_r) in inv.iter().enumerate().take(rows) {
                        let o = std::slice::from_raw_parts_mut(
                            optr.0.add((r0 + r) * n_state + col),
                            hd,
                        );
                        for e in o.iter_mut() {
                            *e *= scale_r;
                        }
                    }
                }
            },
        );
        Tensor::from_vec(out, (1, n, n_state), q.device())
    }

    fn reset_kv_cache(&mut self) {
        self.kv_cache = None;
    }
}

/// expf の多項式近似（相対誤差 ~1e-7）。ループに並べると SIMD になる。x <= 0 を想定。
#[inline(always)]
fn fast_exp(x: f32) -> f32 {
    let x = x.max(-87.0);
    let t = x * std::f32::consts::LOG2_E;
    let n = (t + 0.5).floor();
    // r = x - n ln2（2 段に分けて精度を保つ）
    let r = (x - n * 0.693_359_4) - n * -2.121_944_4e-4;
    let p = 1.987_569_1e-4_f32;
    let p = p * r + 1.398_199_9e-3;
    let p = p * r + 8.333_452e-3;
    let p = p * r + 4.166_579_6e-2;
    let p = p * r + 1.666_666_5e-1;
    let p = p * r + 0.5;
    let y = p * r * r + r + 1.0;
    let bits = ((n as i32 + 127) << 23) as u32;
    y * f32::from_bits(bits)
}

// https://github.com/openai/whisper/blob/f572f2161ba831bae131364c3bffdead7af6d210/whisper/model.py#L111
#[derive(Debug, Clone)]
struct ResidualAttentionBlock {
    attn: MultiHeadAttention,
    attn_ln: LayerNorm,
    cross_attn: Option<(MultiHeadAttention, LayerNorm)>,
    mlp_linear1: Linear,
    mlp_linear2: Linear,
    mlp_ln: LayerNorm,
    span: tracing::Span,
}

impl ResidualAttentionBlock {
    fn load(n_state: usize, n_head: usize, ca: bool, vb: VarBuilder) -> Result<Self> {
        let span = tracing::span!(tracing::Level::TRACE, "residual-attn");
        let attn = MultiHeadAttention::load(n_state, n_head, vb.pp("self_attn"))?;
        let attn_ln = layer_norm(n_state, vb.pp("self_attn_layer_norm"))?;
        let cross_attn = if ca {
            let cross_attn = MultiHeadAttention::load(n_state, n_head, vb.pp("encoder_attn"))?;
            let cross_attn_ln = layer_norm(n_state, vb.pp("encoder_attn_layer_norm"))?;
            Some((cross_attn, cross_attn_ln))
        } else {
            None
        };
        let n_mlp = n_state * 4;
        let mlp_linear1 = linear(n_state, n_mlp, vb.pp("fc1"))?;
        let mlp_linear2 = linear(n_mlp, n_state, vb.pp("fc2"))?;
        let mlp_ln = layer_norm(n_state, vb.pp("final_layer_norm"))?;
        Ok(Self {
            attn,
            attn_ln,
            cross_attn,
            mlp_linear1,
            mlp_linear2,
            mlp_ln,
            span,
        })
    }

    fn forward(
        &mut self,
        x: &Tensor,
        xa: Option<&Tensor>,
        mask: Option<&Tensor>,
        flush_kv_cache: bool,
        encoder: bool,
    ) -> Result<Tensor> {
        use crate::profile::{add, start};
        let _enter = self.span.enter();
        let (self_name, cross_name) = if encoder {
            ("enc.self_attn", "enc.cross_attn")
        } else {
            ("dec.self_attn", "dec.cross_attn")
        };
        let t = start();
        let attn = self
            .attn
            .forward(&self.attn_ln.forward(x)?, None, mask, flush_kv_cache)?;
        add(self_name, t);
        let mut x = (x + attn)?;
        if let Some((attn, ln)) = &mut self.cross_attn {
            let t = start();
            x = (&x + attn.forward(&ln.forward(&x)?, xa, None, flush_kv_cache)?)?;
            add(cross_name, t);
        }
        let tag = if encoder { "enc" } else { "dec" };
        let t = start();
        let ln = self.mlp_ln.forward(&x)?;
        add(if encoder { "enc.mlp_ln" } else { "dec.mlp_ln" }, t);
        let t = start();
        let h = self.mlp_linear1.forward(&ln)?;
        add(if encoder { "enc.mlp1" } else { "dec.mlp1" }, t);
        let t = start();
        let h = gelu(&h)?;
        add(if encoder { "enc.gelu" } else { "dec.gelu" }, t);
        let t = start();
        let mlp = self.mlp_linear2.forward(&h)?;
        add(if encoder { "enc.mlp2" } else { "dec.mlp2" }, t);
        add(if encoder { "enc.mlp" } else { "dec.mlp" }, t);
        let _ = tag;
        x + mlp
    }

    fn reset_kv_cache(&mut self) {
        self.attn.reset_kv_cache();
        if let Some((attn, _)) = &mut self.cross_attn {
            attn.reset_kv_cache();
        }
    }
}

fn sinusoids(length: usize, channels: usize, device: &Device) -> Result<Tensor> {
    let max_timescale = 10000f32;
    let log_timescale_increment = max_timescale.ln() / (channels / 2 - 1) as f32;
    let inv_timescales: Vec<_> = (0..channels / 2)
        .map(|i| (i as f32 * (-log_timescale_increment)).exp())
        .collect();
    let inv_timescales = Tensor::new(inv_timescales.as_slice(), device)?.unsqueeze(0)?;
    let arange = Tensor::arange(0, length as u32, device)?
        .to_dtype(candle::DType::F32)?
        .unsqueeze(1)?;
    let sh = (length, channels / 2);
    let scaled_time = (arange.broadcast_as(sh)? * inv_timescales.broadcast_as(sh)?)?;
    let sincos = Tensor::cat(&[scaled_time.sin()?, scaled_time.cos()?], 1)?;
    Ok(sincos)
}

// https://github.com/openai/whisper/blob/f572f2161ba831bae131364c3bffdead7af6d210/whisper/model.py#L143
#[derive(Debug, Clone)]
pub struct AudioEncoder {
    conv1: Conv1d,
    conv2: Conv1d,
    positional_embedding: Tensor,
    blocks: Vec<ResidualAttentionBlock>,
    ln_post: LayerNorm,
    span: tracing::Span,
    conv1_span: tracing::Span,
    conv2_span: tracing::Span,
}

impl AudioEncoder {
    fn load(vb: VarBuilder, cfg: &Config) -> Result<Self> {
        let span = tracing::span!(tracing::Level::TRACE, "audio-encoder");
        let conv1_span = tracing::span!(tracing::Level::TRACE, "conv1");
        let conv2_span = tracing::span!(tracing::Level::TRACE, "conv2");
        let n_state = cfg.d_model;
        let n_head = cfg.encoder_attention_heads;
        let n_ctx = cfg.max_source_positions;
        let cfg1 = Conv1dConfig {
            padding: 1,
            stride: 1,
            groups: 1,
            dilation: 1,
            cudnn_fwd_algo: None,
        };
        let cfg2 = Conv1dConfig {
            padding: 1,
            stride: 2,
            groups: 1,
            dilation: 1,
            cudnn_fwd_algo: None,
        };
        let conv1 = conv1d(cfg.num_mel_bins, n_state, 3, cfg1, vb.pp("conv1"))?;
        let conv2 = conv1d(n_state, n_state, 3, cfg2, vb.pp("conv2"))?;
        let positional_embedding = sinusoids(n_ctx, n_state, vb.device())?;
        let blocks = (0..cfg.encoder_layers)
            .map(|i| {
                ResidualAttentionBlock::load(n_state, n_head, false, vb.pp(format!("layers.{i}")))
            })
            .collect::<Result<Vec<_>>>()?;
        let ln_post = layer_norm(n_state, vb.pp("layer_norm"))?;
        Ok(Self {
            conv1,
            conv2,
            positional_embedding,
            blocks,
            ln_post,
            conv1_span,
            conv2_span,
            span,
        })
    }

    pub fn forward(&mut self, x: &Tensor, flush_kv_cache: bool) -> Result<Tensor> {
        use crate::profile::{add, start};
        let _enter = self.span.enter();
        let t = start();
        let x = {
            let _enter = self.conv1_span.enter();
            gelu(&self.conv1.forward(x)?)?
        };
        add("enc.conv1", t);
        let t = start();
        let x = {
            let _enter = self.conv2_span.enter();
            gelu(&self.conv2.forward(&x)?)?
        };
        add("enc.conv2", t);
        let t = start();
        let x = x.transpose(1, 2)?;
        let (_bsize, seq_len, _hidden) = x.dims3()?;
        let positional_embedding = self
            .positional_embedding
            .narrow(0, 0, seq_len)?
            .to_dtype(x.dtype())?;
        let mut x = x.broadcast_add(&positional_embedding)?;
        add("enc.pos", t);
        for block in self.blocks.iter_mut() {
            x = block.forward(&x, None, None, flush_kv_cache, true)?
        }
        let t = start();
        let x = self.ln_post.forward(&x)?;
        add("enc.ln", t);
        Ok(x)
    }
}

// https://github.com/openai/whisper/blob/f572f2161ba831bae131364c3bffdead7af6d210/whisper/model.py#L176
#[derive(Debug, Clone)]
pub struct TextDecoder {
    token_embedding: Embedding,
    positional_embedding: Tensor,
    blocks: Vec<ResidualAttentionBlock>,
    ln: LayerNorm,
    mask: Tensor,
    span: tracing::Span,
    span_final: tracing::Span,
    past_len: usize,
}

impl TextDecoder {
    fn load(vb: VarBuilder, cfg: &Config) -> Result<Self> {
        let span = tracing::span!(tracing::Level::TRACE, "text-decoder");
        let span_final = tracing::span!(tracing::Level::TRACE, "text-decoder-final");
        let n_state = cfg.d_model;
        let n_head = cfg.decoder_attention_heads;
        let n_ctx = cfg.max_target_positions;
        let token_embedding = embedding(cfg.vocab_size, n_state, vb.pp("embed_tokens"))?;
        let positional_embedding = vb.get((n_ctx, n_state), "embed_positions.weight")?;
        let blocks = (0..cfg.decoder_layers)
            .map(|i| {
                ResidualAttentionBlock::load(n_state, n_head, true, vb.pp(format!("layers.{i}")))
            })
            .collect::<Result<Vec<_>>>()?;
        let ln = layer_norm(n_state, vb.pp("layer_norm"))?;
        let mask: Vec<_> = (0..n_ctx)
            .flat_map(|i| (0..n_ctx).map(move |j| if j > i { f32::NEG_INFINITY } else { 0f32 }))
            .collect();
        let mask = Tensor::from_vec(mask, (n_ctx, n_ctx), vb.device())?.to_dtype(vb.dtype())?;
        Ok(Self {
            token_embedding,
            positional_embedding,
            blocks,
            ln,
            mask,
            span,
            span_final,
            past_len: 0,
        })
    }

    pub fn forward(&mut self, x: &Tensor, xa: &Tensor, flush_kv_cache: bool) -> Result<Tensor> {
        let _enter = self.span.enter();
        if flush_kv_cache {
            self.past_len = 0;
        }
        let offset = self.past_len;
        let last = x.dim(D::Minus1)?;
        use crate::profile::{add, start};
        let t = start();
        let token_embedding = self.token_embedding.forward(x)?;
        let positional_embedding = self.positional_embedding.narrow(0, offset, last)?;
        let mut x = token_embedding.broadcast_add(&positional_embedding)?;
        add("dec.embed", t);
        for block in self.blocks.iter_mut() {
            x = block.forward(&x, Some(xa), Some(&self.mask), flush_kv_cache, false)?;
        }
        self.past_len += last;
        let t = start();
        let x = self.ln.forward(&x)?;
        add("dec.ln", t);
        Ok(x)
    }

    pub fn final_linear(&self, x: &Tensor) -> Result<Tensor> {
        let w = self.token_embedding.embeddings();
        let logits = {
            use crate::profile::{add, start};
            let _enter = self.span_final.enter();
            let t = start();
            let r = project_nt(x, w)?;
            add("dec.final_linear", t);
            r
        };
        Ok(logits)
    }

    pub fn reset_kv_cache(&mut self) {
        for block in self.blocks.iter_mut() {
            block.reset_kv_cache();
        }
    }
}

// https://github.com/openai/whisper/blob/f572f2161ba831bae131364c3bffdead7af6d210/whisper/model.py#L221
#[derive(Debug, Clone)]
pub struct Whisper {
    pub encoder: AudioEncoder,
    pub decoder: TextDecoder,
    pub config: Config,
}

impl Whisper {
    pub fn load(vb: &VarBuilder, config: Config) -> Result<Self> {
        let encoder = AudioEncoder::load(vb.pp("model.encoder"), &config)?;
        let decoder = TextDecoder::load(vb.pp("model.decoder"), &config)?;
        Ok(Self {
            encoder,
            decoder,
            config,
        })
    }

    pub fn reset_kv_cache(&mut self) {
        self.encoder
            .blocks
            .iter_mut()
            .for_each(|b| b.reset_kv_cache());
        self.decoder.reset_kv_cache();
    }
}
