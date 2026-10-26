//! Shared types, engine contracts, transcript pipeline, and output formats.

pub mod error;
mod format;
pub mod paths;
pub mod pipeline;
pub mod registry;

/// ライブラリのクレートが利用者に出す案内と警告。CLI が [`notice::set_sink`] で自分のログにつなぐ。
/// つながれていなければ標準エラーに書く。
pub mod notice {
    use std::sync::OnceLock;

    type Sink = fn(kind: &str, message: &str);
    static SINK: OnceLock<Sink> = OnceLock::new();

    /// 出口を 1 度だけ決める。2 度目からは何もしない。
    pub fn set_sink(sink: Sink) {
        let _ = SINK.set(sink);
    }

    fn emit(kind: &str, message: &str) {
        match SINK.get() {
            Some(sink) => sink(kind, message),
            None => eprintln!("{message}"),
        }
    }

    pub fn info(message: &str) {
        emit("info", message);
    }

    pub fn warning(message: &str) {
        emit("warning", message);
    }
}

/// Device probes that may panic (a GPU runtime missing at load time) and must fall back quietly.
pub mod probe {
    use std::cell::Cell;

    thread_local! {
        static ACTIVE: Cell<bool> = const { Cell::new(false) };
    }

    /// True while [`catch`] runs on this thread. A process-wide panic hook checks it and
    /// leaves the panic to `catch` instead of ending the process.
    pub fn active() -> bool {
        ACTIVE.with(Cell::get)
    }

    /// Run `f`, returning `Err` instead of unwinding when it panics.
    pub fn catch<T>(f: impl FnOnce() -> T) -> std::thread::Result<T> {
        let previous = ACTIVE.with(|a| a.replace(true));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        ACTIVE.with(|a| a.set(previous));
        result
    }
}

use serde::{Deserialize, Serialize};

pub const SAMPLE_RATE: usize = 16_000;

pub use error::{CliError, ErrorKind, ExitCode};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub speaker: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub text: String,
    pub start: f32,
    pub end: f32,
    pub confidence: Option<f32>,
}

pub struct AsrRequest<'a> {
    pub pcm: &'a [f32],
    pub context: &'a [String],
    pub language: Option<&'a str>,
    pub want_timestamps: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AsrOutput {
    pub text: String,
    pub tokens: Vec<Token>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Capabilities {
    pub timestamps: bool,
    pub context: bool,
    pub max_seconds: f32,
    pub dtypes: &'static [DType],
}

pub trait AsrEngine: Send {
    fn id(&self) -> &str;
    fn capabilities(&self) -> &Capabilities;
    fn transcribe(&mut self, requests: &[AsrRequest<'_>]) -> Result<Vec<AsrOutput>, String>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct VadSpan {
    pub start: f64,
    pub end: f64,
}

pub trait Vad: Send {
    fn spans(&mut self, pcm: &[f32]) -> Result<Vec<VadSpan>, String>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerSpan {
    pub start: f64,
    pub end: f64,
    pub speaker: usize,
}

pub trait Diarizer: Send {
    fn diarize(&mut self, pcm: &[f32]) -> Result<Vec<SpeakerSpan>, String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DType {
    F16,
    F32,
    BF16,
    Int8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceChoice {
    Auto,
    Cpu,
    Metal,
    Cuda(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadOptions {
    pub device: DeviceChoice,
    pub dtype: Option<DType>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpeakerTurn {
    pub start: f64,
    pub end: f64,
    pub speaker: usize,
}

pub fn emit(segments: &[Segment], format: &str) -> Result<Vec<u8>, String> {
    format::emit(segments, format)
}

#[cfg(test)]
pub mod testing {
    use super::*;

    pub struct DummyAsr {
        pub text: String,
        capabilities: Capabilities,
    }

    impl DummyAsr {
        pub fn new(text: impl Into<String>) -> Self {
            Self {
                text: text.into(),
                capabilities: Capabilities {
                    timestamps: false,
                    context: false,
                    max_seconds: 30.0,
                    dtypes: &[DType::F32],
                },
            }
        }
    }

    impl AsrEngine for DummyAsr {
        fn id(&self) -> &str {
            "dummy"
        }
        fn capabilities(&self) -> &Capabilities {
            &self.capabilities
        }
        fn transcribe(&mut self, requests: &[AsrRequest<'_>]) -> Result<Vec<AsrOutput>, String> {
            Ok(requests
                .iter()
                .map(|_| AsrOutput {
                    text: self.text.clone(),
                    tokens: Vec::new(),
                })
                .collect())
        }
    }

    pub struct DummyVad(pub Vec<VadSpan>);
    impl Vad for DummyVad {
        fn spans(&mut self, _pcm: &[f32]) -> Result<Vec<VadSpan>, String> {
            Ok(self.0.clone())
        }
    }

    pub struct DummyDiarizer(pub Vec<SpeakerSpan>);
    impl Diarizer for DummyDiarizer {
        fn diarize(&mut self, _pcm: &[f32]) -> Result<Vec<SpeakerSpan>, String> {
            Ok(self.0.clone())
        }
    }
}
