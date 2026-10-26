use std::f32::consts::TAU;
use std::fs;
use std::path::{Path, PathBuf};

use kikitori_audio::{load, AudioError, SAMPLE_RATE};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!("kikitori-audio-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn make_wav(path: &Path, rate: u32, duration_seconds: f32) {
    let frames = (rate as f32 * duration_seconds) as u32;
    let data_len = frames * 4; // stereo PCM16
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for frame in 0..frames {
        let phase = TAU * 1_000.0 * frame as f32 / rate as f32;
        let value = (phase.sin() * 24_000.0) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

fn estimated_frequency(samples: &[f32]) -> f32 {
    let crossings = samples
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count();
    crossings as f32 * SAMPLE_RATE as f32 / samples.len() as f32
}

fn load_path(path: &Path) -> Result<Vec<f32>, AudioError> {
    load(path)
}

#[test]
fn decode_resample_fallback_paths_and_errors() {
    let dir = temp_dir();

    for rate in [8_000, 44_100] {
        let wav = dir.join(format!("{rate}.wav"));
        make_wav(&wav, rate, 0.5);
        let samples = load_path(&wav).unwrap();
        assert!(
            (samples.len() as i64 - 8_000).abs() <= 2,
            "rate {rate}: {} samples",
            samples.len()
        );
        assert!((estimated_frequency(&samples) - 1_000.0).abs() < 20.0);
    }

    let japanese_path = dir.join("日本語の 音声.wav");
    make_wav(&japanese_path, 16_000, 0.25);
    assert_eq!(load_path(&japanese_path).unwrap().len(), 4_000);

    for fixture in ["tone.m4a", "tone.mp3", "tone-vorbis.ogg", "tone-video.mp4"] {
        let samples = load_path(&Path::new(FIXTURES).join(fixture)).unwrap();
        assert!(samples.len() > 8_000, "fixture {fixture} was not decoded");
    }

    let opus = Path::new(FIXTURES).join("tone-opus.ogg");
    let has_ffmpeg = std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success());
    if has_ffmpeg {
        assert!(load_path(&opus).unwrap().len() > 8_000); // ffmpeg fallback
    }

    let old_path = std::env::var_os("PATH");
    std::env::set_var("PATH", &dir);
    for fixture in ["tone.m4a", "tone.mp3", "tone-vorbis.ogg", "tone-video.mp4"] {
        let samples = load_path(&Path::new(FIXTURES).join(fixture)).unwrap();
        assert!(
            samples.len() > 8_000,
            "built-in decoder failed for {fixture}"
        );
    }
    // HE-AAC v1 (SBR) and v2 (SBR + PS), 3 s at 44.1 kHz: the built-in
    // decoder must give the full length at 16 kHz, not half of it.
    for fixture in ["tone-heaac.m4a", "tone-heaac-v2.m4a"] {
        let samples = load_path(&Path::new(FIXTURES).join(fixture)).unwrap();
        assert!(
            (43_000..53_000).contains(&samples.len()),
            "built-in decoder gave {} samples for {fixture}",
            samples.len()
        );
    }
    let missing_ffmpeg = load_path(&opus).unwrap_err();
    if let Some(path) = old_path {
        std::env::set_var("PATH", path);
    } else {
        std::env::remove_var("PATH");
    }
    assert!(matches!(missing_ffmpeg, AudioError::FfmpegRequired { .. }));
    assert_eq!(missing_ffmpeg.exit_code(), 4);
    let hint = missing_ffmpeg.to_string();
    assert!(hint.contains("ogg"));
    assert!(hint.contains("brew install ffmpeg"));
    assert!(hint.contains("sudo apt install ffmpeg"));
    assert!(hint.contains("winget install ffmpeg"));

    let missing = load_path(&dir.join("missing.wav")).unwrap_err();
    assert!(matches!(missing, AudioError::MissingFile(_)));
    assert_eq!(missing.exit_code(), 3);

    let empty = dir.join("empty.wav");
    fs::write(&empty, []).unwrap();
    let empty_error = load_path(&empty).unwrap_err();
    assert!(matches!(empty_error, AudioError::EmptyFile(_)));
    assert_eq!(empty_error.exit_code(), 3);

    let corrupt = dir.join("corrupt.wav");
    fs::write(&corrupt, [0x12, 0x9a, 0x42, 0xff, 0x00, 0x31]).unwrap();
    let corrupt_error = load_path(&corrupt).unwrap_err();
    assert!(matches!(corrupt_error, AudioError::CorruptFile { .. }));
    assert_eq!(corrupt_error.exit_code(), 3);

    fs::remove_dir_all(dir).unwrap();
}
