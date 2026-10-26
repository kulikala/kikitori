//! Audio decoding and resampling for kikitori.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use thiserror::Error;

/// Target rate used throughout the transcription pipeline.
pub const SAMPLE_RATE: u32 = 16_000;
const RESAMPLE_CHUNK: usize = 1024;

/// Failure while reading or converting an audio input.
#[derive(Debug, Error)]
pub enum AudioError {
    #[error("audio file does not exist: {0}")]
    MissingFile(PathBuf),
    #[error("could not read audio file {path}: {message}")]
    UnreadableFile { path: PathBuf, message: String },
    #[error("audio file is empty: {0}")]
    EmptyFile(PathBuf),
    #[error("audio file is corrupt or unsupported: {path}: {message}")]
    CorruptFile { path: PathBuf, message: String },
    #[error("{format} audio requires ffmpeg. Install it with `brew install ffmpeg` (macOS), `sudo apt install ffmpeg` (Debian/Ubuntu), or `winget install ffmpeg` (Windows), then put it on PATH.")]
    FfmpegRequired { format: String },
    #[error("audio conversion failed: {0}")]
    Conversion(String),
}

impl AudioError {
    /// CLI exit code associated with this failure category.
    pub const fn exit_code(&self) -> i32 {
        match self {
            Self::FfmpegRequired { .. } => 4,
            Self::MissingFile(_)
            | Self::UnreadableFile { .. }
            | Self::EmptyFile(_)
            | Self::CorruptFile { .. } => 3,
            Self::Conversion(_) => 8,
        }
    }
}

/// Load an audio or video file and return mono, 16 kHz `f32` samples.
/// For video files, the first audio track is selected.
pub fn load(path: &Path) -> Result<Vec<f32>, AudioError> {
    let metadata = std::fs::metadata(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => AudioError::MissingFile(path.to_owned()),
        _ => AudioError::UnreadableFile {
            path: path.to_owned(),
            message: error.to_string(),
        },
    })?;
    if metadata.len() == 0 {
        return Err(AudioError::EmptyFile(path.to_owned()));
    }
    let file = File::open(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => AudioError::MissingFile(path.to_owned()),
        _ => AudioError::UnreadableFile {
            path: path.to_owned(),
            message: error.to_string(),
        },
    })?;

    match decode_symphonia(path, file) {
        Ok(pcm) => Ok(pcm),
        Err(primary) => match decode_ffmpeg(path) {
            Ok(pcm) => Ok(pcm),
            Err(FfmpegFailure::Missing) if might_need_ffmpeg(path) => {
                Err(AudioError::FfmpegRequired {
                    format: format_name(path),
                })
            }
            Err(FfmpegFailure::Missing) => Err(AudioError::CorruptFile {
                path: path.to_owned(),
                message: primary,
            }),
            Err(FfmpegFailure::Failed(message)) => Err(AudioError::CorruptFile {
                path: path.to_owned(),
                message: if message.is_empty() { primary } else { message },
            }),
        },
    }
}

fn decode_symphonia(path: &Path, file: File) -> Result<Vec<f32>, String> {
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|v| v.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| {
            track.codec_params.channels.is_some() || track.codec_params.sample_rate.is_some()
        })
        .ok_or_else(|| "no audio track was found".to_owned())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let mut mono = Vec::new();
    let mut sample_rate = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err("audio stream changed in a way the decoder cannot handle".into())
            }
            Err(error) => return Err(error.to_string()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(error.to_string()),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        if channels == 0 {
            return Err("audio track has no channels".into());
        }
        if sample_rate.is_some_and(|rate| rate != spec.rate) {
            return Err("sample rate changed during decoding".into());
        }
        sample_rate = Some(spec.rate);
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        samples.copy_interleaved_ref(decoded);
        mono.extend(
            samples
                .samples()
                .chunks_exact(channels)
                .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32),
        );
    }

    if mono.is_empty() {
        return Err("audio stream contains no samples".into());
    }
    let sample_rate = sample_rate.ok_or_else(|| "audio track has no sample rate".to_owned())?;
    if sample_rate == SAMPLE_RATE {
        return Ok(mono);
    }
    resample_mono(&mono, sample_rate)
}

fn resample_mono(samples: &[f32], input_rate: u32) -> Result<Vec<f32>, String> {
    let ratio = f64::from(SAMPLE_RATE) / f64::from(input_rate);
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };
    let mut resampler = SincFixedIn::<f32>::new(ratio, 2.0, params, RESAMPLE_CHUNK, 1)
        .map_err(|e| e.to_string())?;
    let desired_len = ((samples.len() as f64 * ratio).round() as usize).max(1);
    let mut output = Vec::with_capacity(desired_len);
    let mut offset = 0;
    while offset < samples.len() {
        let frames = resampler.input_frames_next();
        let available = (samples.len() - offset).min(frames);
        let mut input = vec![0.0; frames];
        input[..available].copy_from_slice(&samples[offset..offset + available]);
        let result = resampler
            .process(&[input.as_slice()], None)
            .map_err(|e| e.to_string())?;
        output.extend_from_slice(&result[0]);
        offset += available;
    }
    output.truncate(desired_len);
    Ok(output)
}

enum FfmpegFailure {
    Missing,
    Failed(String),
}

fn decode_ffmpeg(path: &Path) -> Result<Vec<f32>, FfmpegFailure> {
    let child = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "s16le", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                FfmpegFailure::Missing
            } else {
                FfmpegFailure::Failed(error.to_string())
            }
        })?;
    if !child.status.success() {
        return Err(FfmpegFailure::Failed(
            String::from_utf8_lossy(&child.stderr).trim().to_owned(),
        ));
    }
    if child.stdout.len() % 2 != 0 {
        return Err(FfmpegFailure::Failed(
            "ffmpeg returned an incomplete s16le sample".into(),
        ));
    }
    let output = child
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| i16::from_le_bytes(*bytes) as f32 / 32768.0)
        .collect::<Vec<_>>();
    if output.is_empty() {
        return Err(FfmpegFailure::Failed(
            "ffmpeg returned no audio samples".into(),
        ));
    }
    Ok(output)
}

fn might_need_ffmpeg(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("ogg" | "opus" | "webm" | "mkv" | "m4a" | "mp4" | "m4v" | "avi" | "mov")
    )
}

fn format_name(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_else(|| "unknown-format".into())
}
