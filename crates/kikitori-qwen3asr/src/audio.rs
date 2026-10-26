use rustfft::{num_complex::Complex, FftPlanner};

/// Read the mono, 16 kHz, signed 16-bit PCM WAV format used by the evaluation set.
#[cfg(test)]
pub fn read_wav(path: &std::path::Path) -> anyhow::Result<Vec<f32>> {
    use anyhow::{bail, Context};
    use std::fs;
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("{} is not a RIFF/WAVE file", path.display());
    }
    let mut rate = 0u32;
    let mut channels = 0u16;
    let mut bits = 0u16;
    let mut format = 0u16;
    let mut data = None;
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let n = u32::from_le_bytes(bytes[i + 4..i + 8].try_into()?) as usize;
        let end = i.saturating_add(8).saturating_add(n).min(bytes.len());
        match &bytes[i..i + 4] {
            b"fmt " if n >= 16 => {
                format = u16::from_le_bytes(bytes[i + 8..i + 10].try_into()?);
                channels = u16::from_le_bytes(bytes[i + 10..i + 12].try_into()?);
                rate = u32::from_le_bytes(bytes[i + 12..i + 16].try_into()?);
                bits = u16::from_le_bytes(bytes[i + 22..i + 24].try_into()?);
            }
            b"data" => data = Some(&bytes[i + 8..end]),
            _ => {}
        }
        i = end + (n & 1);
    }
    if format != 1 || channels != 1 || rate != 16_000 || bits != 16 {
        bail!("expected mono 16 kHz PCM16 WAV, got format={format}, channels={channels}, rate={rate}, bits={bits}");
    }
    let raw = data.context("WAV has no data chunk")?;
    if raw.len() % 2 != 0 {
        bail!("odd PCM16 data size")
    }
    Ok(raw
        .chunks_exact(2)
        .map(|x| i16::from_le_bytes([x[0], x[1]]) as f32 / 32768.0)
        .collect())
}

fn hz_to_mel(hz: f64) -> f64 {
    const MIN_LOG_HZ: f64 = 1000.0;
    const MIN_LOG_MEL: f64 = 15.0;
    const LOGSTEP: f64 = 0.06875177742094912; // ln(6.4) / 27
    if hz < MIN_LOG_HZ {
        hz * (3.0 / 200.0)
    } else {
        MIN_LOG_MEL + (hz / MIN_LOG_HZ).ln() / LOGSTEP
    }
}
fn mel_to_hz(mel: f64) -> f64 {
    const MIN_LOG_HZ: f64 = 1000.0;
    const MIN_LOG_MEL: f64 = 15.0;
    const LOGSTEP: f64 = 0.06875177742094912;
    if mel < MIN_LOG_MEL {
        mel * (200.0 / 3.0)
    } else {
        MIN_LOG_HZ * (LOGSTEP * (mel - MIN_LOG_MEL)).exp()
    }
}

/// WhisperFeatureExtractor-compatible 128-bin log-mel. Output is [mel, frame].
pub fn log_mel(wave: &[f32]) -> Vec<f32> {
    const NFFT: usize = 400;
    const HOP: usize = 160;
    const BINS: usize = NFFT / 2 + 1;
    // HF computes an extra centered frame, then drops the final one.
    let frames = wave.len() / HOP;
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(NFFT);
    let window: Vec<f64> = (0..NFFT)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / NFFT as f64).cos())
        .collect();
    let mel_min = hz_to_mel(0.0);
    let mel_max = hz_to_mel(8000.0);
    let points: Vec<f64> = (0..130)
        .map(|i| mel_to_hz(mel_min + (mel_max - mel_min) * i as f64 / 129.0))
        .collect();
    let freqs: Vec<f64> = (0..BINS).map(|i| i as f64 * 40.0).collect();
    let mut filters = vec![0.0f64; 128 * BINS];
    for m in 0..128 {
        let norm = 2.0 / (points[m + 2] - points[m]);
        for k in 0..BINS {
            let down = (freqs[k] - points[m]) / (points[m + 1] - points[m]);
            let up = (points[m + 2] - freqs[k]) / (points[m + 2] - points[m + 1]);
            filters[m * BINS + k] = down.min(up).max(0.0) * norm;
        }
    }
    let mut power = vec![0.0f64; frames * BINS];
    let mut buf = vec![Complex::new(0.0, 0.0); NFFT];
    // HF centers frames at multiples of HOP and reflect-pads by 200 samples.
    for frame in 0..frames {
        let center = frame as isize * HOP as isize;
        for j in 0..NFFT {
            let p = center + j as isize - (NFFT / 2) as isize;
            let v = if p < 0 {
                wave.get((-p) as usize).copied().unwrap_or(0.0)
            } else if p >= wave.len() as isize {
                // Qwen3ASRProcessor extracts each unpadded waveform independently;
                // the centered STFT therefore reflects the signal at its end.
                let reflected = (2 * wave.len() as isize - 2 - p).max(0) as usize;
                wave.get(reflected).copied().unwrap_or(0.0)
            } else {
                wave[p as usize]
            } as f64;
            buf[j] = Complex::new(v * window[j], 0.0);
        }
        fft.process(&mut buf);
        for k in 0..BINS {
            power[frame * BINS + k] = buf[k].norm_sqr();
        }
    }
    let mut result = vec![0.0f32; 128 * frames];
    let mut max_log = f64::NEG_INFINITY;
    for m in 0..128 {
        for t in 0..frames {
            let val = (0..BINS)
                .map(|k| filters[m * BINS + k] * power[t * BINS + k])
                .sum::<f64>()
                .max(1e-10)
                .log10();
            result[m * frames + t] = val as f32;
            max_log = max_log.max(val);
        }
    }
    for v in &mut result {
        *v = ((*v as f64).max(max_log - 8.0) as f32 + 4.0) / 4.0;
    }
    result
}

pub fn frame_count(wave: &[f32]) -> usize {
    wave.len() / 160
}
