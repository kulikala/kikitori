use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    Success = 0,
    Usage = 2,
    Input = 3,
    Dependency = 4,
    OfflineModel = 5,
    NeedsConfirmation = 6,
    Download = 7,
    Processing = 8,
    Refine = 9,
    Privilege = 10,
    Memory = 11,
}

impl ExitCode {
    pub const fn code(self) -> i32 {
        self as i32
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Usage => "usage",
            Self::Input => "input",
            Self::Dependency => "dependency",
            Self::OfflineModel => "offline_model",
            Self::NeedsConfirmation => "needs_confirmation",
            Self::Download => "download",
            Self::Processing => "processing",
            Self::Refine => "refine",
            Self::Privilege => "privilege",
            Self::Memory => "memory",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Usage,
    Input,
    Dependency,
    OfflineModel,
    NeedsConfirmation,
    Download,
    Processing,
    Refine,
    Privilege,
    Memory,
}

impl ErrorKind {
    pub const fn exit_code(self) -> ExitCode {
        match self {
            Self::Usage => ExitCode::Usage,
            Self::Input => ExitCode::Input,
            Self::Dependency => ExitCode::Dependency,
            Self::OfflineModel => ExitCode::OfflineModel,
            Self::NeedsConfirmation => ExitCode::NeedsConfirmation,
            Self::Download => ExitCode::Download,
            Self::Processing => ExitCode::Processing,
            Self::Refine => ExitCode::Refine,
            Self::Privilege => ExitCode::Privilege,
            Self::Memory => ExitCode::Memory,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliError {
    pub kind: ErrorKind,
    pub message: String,
    pub hint: String,
}

impl CliError {
    pub fn new(kind: ErrorKind, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            hint: hint.into(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for CliError {}

/// エラーの文がディスクの空き不足（ENOSPC・Windows の os error 112）を表すか
pub fn is_storage_full(text: &str) -> bool {
    let low = text.to_ascii_lowercase();
    low.contains("os error 28)")
        || low.contains("os error 112)")
        || low.contains("no space left")
        || low.contains("storagefull")
        || text.contains("十分な空き領域がありません")
}
