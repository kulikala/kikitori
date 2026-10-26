//! HF WhisperFeatureExtractor と同じ log-mel（30 秒に 0 埋め、center=True の反射パディング、
//! 最終フレームを捨てる、log10 を max-8 で下限、(x+4)/4）。
use rustfft::{num_complex::Complex, FftPlanner};

const N_FFT: usize = 400;
const HOP: usize = 160;
const N_SAMPLES: usize = 480_000;
const N_FRAMES: usize = N_SAMPLES / HOP;

/// `filters` は melfilters128.bytes（[n_mels][201]）。戻り値は [n_mels][3000] を平らにしたもの。
pub fn log_mel(pcm: &[f32], filters: &[f32], n_mels: usize) -> Vec<f32> {
    let n_freq = N_FFT / 2 + 1;
    let n = pcm.len().min(N_SAMPLES);
    // 0 埋めして 30 秒にし、両端を反射パディング
    let mut x = vec![0f64; N_SAMPLES];
    for i in 0..n {
        x[i] = pcm[i] as f64;
    }
    let pad = N_FFT / 2;
    let mut p = vec![0f64; N_SAMPLES + 2 * pad];
    p[pad..pad + N_SAMPLES].copy_from_slice(&x);
    for i in 0..pad {
        p[i] = x[pad - i];
        p[pad + N_SAMPLES + i] = x[N_SAMPLES - 2 - i];
    }
    let window: Vec<f64> = (0..N_FFT)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N_FFT as f64).cos())
        .collect();
    let fft = FftPlanner::<f64>::new().plan_fft_forward(N_FFT);
    let mut mel = vec![0f64; n_mels * N_FRAMES];
    let mut buf = vec![Complex::new(0f64, 0f64); N_FFT];
    let mut power = vec![0f64; n_freq];
    // 音声の外のフレームは全部 0 になるので、一度だけ計算して使い回す
    let mut zero_col: Option<Vec<f64>> = None;
    for t in 0..N_FRAMES {
        let seg = &p[t * HOP..t * HOP + N_FFT];
        if seg.iter().all(|&v| v == 0.0) {
            if let Some(c) = &zero_col {
                for m in 0..n_mels {
                    mel[m * N_FRAMES + t] = c[m];
                }
                continue;
            }
        }
        for i in 0..N_FFT {
            buf[i] = Complex::new(seg[i] * window[i], 0.0);
        }
        fft.process(&mut buf);
        for k in 0..n_freq {
            power[k] = buf[k].norm_sqr();
        }
        let mut col = vec![0f64; n_mels];
        for m in 0..n_mels {
            let f = &filters[m * n_freq..(m + 1) * n_freq];
            let mut s = 0f64;
            for k in 0..n_freq {
                s += f[k] as f64 * power[k];
            }
            col[m] = s;
            mel[m * N_FRAMES + t] = s;
        }
        if seg.iter().all(|&v| v == 0.0) {
            zero_col = Some(col);
        }
    }
    let mut mx = f64::NEG_INFINITY;
    for v in mel.iter_mut() {
        *v = v.max(1e-10).log10();
        mx = mx.max(*v);
    }
    mel.iter()
        .map(|&v| ((v.max(mx - 8.0) + 4.0) / 4.0) as f32)
        .collect()
}
