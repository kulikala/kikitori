use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

static JSON: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);
static QUIET: AtomicBool = AtomicBool::new(false);
static YES: AtomicBool = AtomicBool::new(false);

pub fn configure(json: bool, verbose: bool, quiet: bool) {
    configure_with_yes(json, verbose, quiet, false);
}
pub fn configure_with_yes(json: bool, verbose: bool, quiet: bool, yes: bool) {
    JSON.store(json, Ordering::Relaxed);
    VERBOSE.store(verbose, Ordering::Relaxed);
    QUIET.store(quiet, Ordering::Relaxed);
    YES.store(yes, Ordering::Relaxed);
}

#[derive(Serialize)]
struct Event<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    message: &'a str,
}
#[derive(Serialize)]
struct ProgressEvent<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    message: String,
    item: &'a str,
    bytes: u64,
    total: Option<u64>,
}
#[derive(Serialize)]
struct ErrorEvent<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    code: &'a str,
    message: &'a str,
    hint: &'a str,
}

pub fn event(kind: &'static str, message: &str) {
    if QUIET.load(Ordering::Relaxed) && kind != "error" {
        return;
    }
    if JSON.load(Ordering::Relaxed) {
        if let Ok(s) = serde_json::to_string(&Event { kind, message }) {
            crate::capture::write_line(&s);
        }
    } else if kind == "summary"
        || kind == "warning"
        || kind == "info"
        || kind == "progress"
        || VERBOSE.load(Ordering::Relaxed)
    {
        crate::capture::write_line(message);
    }
}
pub fn info(message: &str) {
    event("info", message);
}
pub fn warning(message: &str) {
    event("warning", message);
}
pub fn progress(item: &str, bytes: u64, total: Option<u64>) {
    if QUIET.load(Ordering::Relaxed) {
        return;
    }
    let message = format!(
        "{item}: {bytes}/{} バイト",
        total.map_or("不明".into(), |n| n.to_string())
    );
    if JSON.load(Ordering::Relaxed) {
        if let Ok(s) = serde_json::to_string(&ProgressEvent {
            kind: "progress",
            message,
            item,
            bytes,
            total,
        }) {
            crate::capture::write_line(&s);
        }
    } else {
        crate::capture::write_line(&message);
    }
}
pub fn error(code: &str, message: &str, hint: &str) {
    if JSON.load(Ordering::Relaxed) {
        if let Ok(s) = serde_json::to_string(&ErrorEvent {
            kind: "error",
            code,
            message,
            hint,
        }) {
            crate::capture::write_line(&s);
        }
    } else {
        crate::capture::write_line(&format!(
            "エラー: {message}{}",
            if hint.is_empty() {
                String::new()
            } else {
                format!("\nヒント: {hint}")
            }
        ));
    }
}
pub fn captured(line: &str) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }
    // ONNX Runtime uses its generic kernels when it does not know the CPU vendor; nothing to act on.
    let known_harmless = trimmed.starts_with("onnxruntime cpuid_info warning: Unknown CPU vendor");
    let is_warning = !known_harmless
        && (trimmed.to_ascii_lowercase().contains("warn") || trimmed.starts_with("警告:"));
    if !JSON.load(Ordering::Relaxed) && !VERBOSE.load(Ordering::Relaxed) && !is_warning {
        return;
    }
    // Our own logs are redirected while native stderr is captured. Preserve them
    // as-is instead of wrapping their JSON representation in another log event.
    if JSON.load(Ordering::Relaxed)
        && serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .is_some_and(|value| value.get("type").is_some())
    {
        crate::capture::write_line(trimmed);
        return;
    }
    if is_warning {
        warning(trimmed);
    } else {
        info(trimmed);
    }
}

pub fn confirm(yes: bool, prompt: &str) -> Result<bool, kikitori_core::CliError> {
    use std::io::{IsTerminal, Write};
    if yes || YES.load(Ordering::Relaxed) {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        return Err(kikitori_core::CliError::new(
            kikitori_core::ErrorKind::NeedsConfirmation,
            "この操作には確認が必要ですが、標準入力がターミナルではありません。",
            "`--yes` を付けるか、KIKITORI_YES=1 を設定して再実行してください。",
        ));
    }
    if JSON.load(Ordering::Relaxed) {
        event("progress", prompt);
    } else {
        eprint!("{prompt} [y/N] ");
        let _ = std::io::stderr().flush();
    }
    let mut answer = String::new();
    let _ = std::io::stdin().read_line(&mut answer);
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}
