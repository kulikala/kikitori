//! 外部 LLM に A・B・K と用語集を渡して直させる（Python `refine/external.py`）。
//!
//! - bundle: 指示と問いを JSON で出す。呼び出し元の LLM（エージェント）が直し、答えを --apply で戻す。
//! - claude: claude CLI に渡す。
//!
//! 本文は PC の外へ出るので、利用者が指定したときだけ使う。

use serde_json::{json, Serializer, Value};
use std::{
    io::{Read, Write},
    process::Command,
    sync::mpsc,
};

use crate::prompts;

pub const CHUNK: usize = 60;
pub const CLAUDE_MODEL: &str = "claude-haiku-5-5";
const CLAUDE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
/// 子が終わった後、出力の読み手を待つ上限。孫がパイプを開いたままでも止まらない
const READER_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// 1 回に渡す区間の数ごとに ask を作る。system は全体 1 回、chunks の各 user が 1 問。
pub fn bundle(
    segs: &[kikitori_core::Segment],
    a: &[String],
    b: &[String],
    k: &[String],
    glossary: &[(String, String)],
) -> Value {
    json!({
        "kind": "kikitori-refine/fixev",
        "how": "system を指示、chunks の各 user をプロンプトとして LLM に渡し、応答（「[番号] 本文」の行）をchunks の順につなげて kikitori refine --apply にこのファイルと一緒に渡す。",
        "system": prompts::system("fixev", glossary),
        "chunks": prompts::chunks(a.len(), CHUNK).iter().map(|ids| {
            json!({"ids": ids, "user": prompts::user("fixev", ids, a, b, k)})
        }).collect::<Vec<_>>(),
        "segments": segs.iter().zip(a).zip(b).zip(k).map(|(((s, a), b), k)| {
            json!({"start": s.start, "end": s.end, "speaker": s.speaker, "text": a, "b": b, "k": k})
        }).collect::<Vec<_>>(),
    })
}

fn warn(msg: impl std::fmt::Display) {
    kikitori_core::notice::warning(&msg.to_string());
}

/// 答えの行を本文に戻す。答えが無い区間は A のまま。戻せなかった区間の番号も返す。
pub fn apply(bundle: &Value, reply: &str) -> (Vec<String>, Vec<usize>) {
    let got = prompts::parse(reply);
    let segs = bundle
        .get("segments")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let miss: Vec<usize> = (0..segs.len()).filter(|i| !got.contains_key(i)).collect();
    let texts = segs
        .iter()
        .enumerate()
        .map(|(i, s)| {
            got.get(&i).cloned().unwrap_or_else(|| {
                s.get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_owned()
            })
        })
        .collect();
    (texts, miss)
}

fn warning_for_missing(miss: &[usize]) {
    if miss.is_empty() {
        return;
    }
    let shown: Vec<String> = miss.iter().take(10).map(|i| i.to_string()).collect();
    warn(format_args!(
        "警告: {} 区間の答えがありませんでした。本文のまま残します（{}）",
        miss.len(),
        shown.join(", ")
    ));
}

pub fn finish_missing_note(miss: &[usize]) {
    warning_for_missing(miss);
}

fn one_claude(system: &str, user: &str) -> String {
    // PATH から引いた実体で起動する。Windows の npm の claude.cmd も見つかる
    let program = crate::catalog::which("claude").unwrap_or_else(|| "claude".to_owned());
    let mut cmd = Command::new(program);
    cmd.args([
        "-p",
        "--model",
        CLAUDE_MODEL,
        "--system-prompt",
        system,
        "--tools",
        "",
        "--no-session-persistence",
    ])
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .current_dir(std::env::temp_dir()); // 作業フォルダの指示ファイルを読ませない
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            warn(format_args!("警告: claude CLI が始まりませんでした（{e}）"));
            return String::new();
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(user.as_bytes());
    }
    // パイプが埋まると子が止まるので、待つ間に stdout と stderr を読み切る
    let (out_tx, out_rx) = mpsc::channel();
    let (err_tx, err_rx) = mpsc::channel();
    if let Some(mut out) = child.stdout.take() {
        let tx = out_tx.clone();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
    }
    if let Some(mut err) = child.stderr.take() {
        let tx = err_tx.clone();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = err.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
    }
    drop(out_tx);
    drop(err_tx);
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => {
                if started.elapsed() > CLAUDE_TIMEOUT {
                    // 読み手は join しない。孫がパイプを開いたままだと join が返らない
                    let _ = child.kill();
                    let _ = child.wait();
                    warn(format_args!(
                        "警告: claude CLI が {} 秒で終わりませんでした。打ち切ります",
                        CLAUDE_TIMEOUT.as_secs()
                    ));
                    return String::new();
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    };
    // 子が終わっても孫がパイプを持ったままだと EOF が来ない。有限時間だけ待って読めた分を使う
    let stdout = out_rx.recv_timeout(READER_WAIT).unwrap_or_default();
    let stderr = err_rx.recv_timeout(READER_WAIT).unwrap_or_default();
    match status {
        Some(status) => {
            if !status.success() {
                let stderr = String::from_utf8_lossy(&stderr).trim().to_owned();
                let cut = stderr
                    .char_indices()
                    .nth(200)
                    .map(|(i, _)| i)
                    .unwrap_or(stderr.len());
                warn(format_args!(
                    "警告: claude CLI が失敗しました（{}）",
                    &stderr[..cut]
                ));
            }
            String::from_utf8_lossy(&stdout).into_owned()
        }
        None => {
            warn("警告: claude CLI が失敗しました");
            String::new()
        }
    }
}

/// claude CLI に分けて渡す。4 件ずつ並行で、答えは元の順につなげる。
pub fn claude(system: &str, users: &[String], log: &mut impl FnMut(&str)) -> String {
    log(&format!(
        "claude CLI（{CLAUDE_MODEL}）に {} 回に分けて渡します",
        users.len()
    ));
    const LANES: usize = 4;
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|s| {
        for lane in 0..LANES {
            let mut mine: Vec<(usize, &String)> = users
                .iter()
                .enumerate()
                .filter(|(i, _)| i % LANES == lane)
                .collect();
            let tx = tx.clone();
            s.spawn(move || {
                for (i, u) in mine.drain(..) {
                    let _ = tx.send((i, one_claude(system, u)));
                }
            });
        }
    });
    drop(tx);
    let mut parts: Vec<Option<String>> = vec![None; users.len()];
    for (i, text) in rx {
        parts[i] = Some(text);
    }
    parts
        .into_iter()
        .map(|p| p.unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Python `json.dumps(b, ensure_ascii=False, indent=1)` と同じ形（1 段 1 空白）。
pub fn dumps(b: &Value) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut ser = Serializer::with_formatter(
        &mut buf,
        serde_json::ser::PrettyFormatter::with_indent(b" "),
    );
    serde::Serialize::serialize(&b, &mut ser).map_err(|e: serde_json::Error| e.to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

/// --apply で戻した区間ごとの本文（答えが無い区间は A のまま）。
pub fn texts_for(bundle: &Value, reply: &str) -> Vec<String> {
    let (texts, _) = apply(bundle, reply);
    texts
}
