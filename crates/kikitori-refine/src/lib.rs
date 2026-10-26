//! kikitori で文字起こしした本文を、別の認識結果（Qwen3-ASR・kana-whisper）と LLM で補正する。
//! Python 版 `kikitori/refine` を書き写した箱。

pub mod catalog;
pub mod evidence;
pub mod external;
pub mod local;
pub mod ollama;
pub mod prompts;
pub mod route;
pub mod setup;

/// 経路選びの前に出す、1 行の進捗や注記（本体側の --log-format にあわせる）。
pub fn log(msg: &str) {
    kikitori_core::notice::info(&format!("# {msg}"));
}

/// 1 行 1 語、「`表記<TAB>補足`」。補足（読み・説明）は無くてもよい。
pub fn load_glossary(path: &std::path::Path) -> Result<Vec<(String, String)>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("用語集を読めません（{}）: {e}", path.display()))?;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let (w, d) = line.split_once('\t').unwrap_or((line, ""));
        out.push((w.trim().to_owned(), d.trim().to_owned()));
    }
    Ok(out)
}
