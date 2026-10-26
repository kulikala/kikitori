//! `kikitori refine --setup`: 補正に使うモデル（Ollama のモデル・Hugging Face のモデル）を取る。
//! Ollama 本体は入れも起こしもしない。入れ方は利用者ごとに違うので、無い・止まっているときは案内だけを出す。
//! 事実（State）からやることの並び（Step）を作る plan_setup は純粋な関数で、実行は run_setup が行う。

use kikitori_core::{CliError, ErrorKind};
use std::path::{Path, PathBuf};

use crate::{catalog, evidence, ollama};

pub const QWEN_SIZE: u64 = (3.8 * catalog::GB as f64) as u64;
pub const KANA_SIZE: u64 = (3.0 * catalog::GB as f64) as u64;
pub const SPARE: u64 = 2 * catalog::GB;

pub const APP: &str = "/Applications/Ollama.app";

#[derive(Clone, Debug)]
pub struct State {
    pub os: &'static str, // Darwin / Linux / Windows
    pub hardware: catalog::Hardware,
    /// Qwen3-ASR と kana-whisper を動かせる（モデルがそろっている）
    pub refine: bool,
    /// Ollama の接続先がこの PC
    pub local: bool,
    /// Ollama の API が答える
    pub api: bool,
    pub version: Option<(u32, u32, u32)>,
    /// PATH に ollama がある
    pub cli: bool,
    /// macOS の /Applications/Ollama.app がある
    pub app: bool,
    /// Linux で systemd の ollama サービスがある
    pub systemd: bool,
    /// Ollama にあるモデル {名前: バイト数}
    pub models: std::collections::BTreeMap<String, u64>,
    pub tty: bool,
    pub free_ollama: Option<u64>,
    pub free_hf: Option<u64>,
    pub hf_missing: Vec<String>,
    /// 遠くの Ollama で --hardware を指定したか
    pub hardware_given: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub kind: &'static str, // note / stop / pull / hf
    pub text: String,
    pub size: u64,
    pub arg: String,
}

impl Step {
    pub fn note(text: impl Into<String>) -> Self {
        Self {
            kind: "note",
            text: text.into(),
            size: 0,
            arg: String::new(),
        }
    }
    pub fn stop(text: impl Into<String>) -> Self {
        Self {
            kind: "stop",
            text: text.into(),
            size: 0,
            arg: String::new(),
        }
    }
    pub fn sized(
        kind: &'static str,
        text: impl Into<String>,
        size: u64,
        arg: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            text: text.into(),
            size,
            arg: arg.into(),
        }
    }
}

fn version_text(t: (u32, u32, u32)) -> String {
    format!("{}.{}.{}", t.0, t.1, t.2)
}

/// 事実（State）から、何をするかの並び（Step）を作る。何も変えない純粋な関数。
/// `--merge-model` があれば、Ollama のモデルはその 1 つだけを取る（D-9）。
pub fn plan_setup(s: &State, no_hf: bool, merge_model: Option<&str>) -> Vec<Step> {
    if !s.refine {
        return vec![Step::note(
            "Qwen3-ASR と kana-whisper を動かせない（kikitori[refine] が無い）ので、補正に LLM を使いません。Ollama とモデルは入れません",
        )];
    }
    let hw = &s.hardware;
    let mut steps = vec![Step::note(format!(
        "機器: {}（{}{}）",
        hw.kind,
        hw.why,
        hw.memory
            .map(|m| format!("、{}GB", (m as f64 / catalog::GB as f64).round()))
            .unwrap_or_default()
    ))];
    if !s.local && !s.hardware_given {
        steps.push(Step::note(
            "接続先の Ollama の機器が分からないので、GGUF の tev1 だけを入れます。Apple Silicon や NVIDIA なら --hardware apple / nvidia を付けてください",
        ));
    }
    // Ollama のモデルを取れないとき（無い・止まっている・古い）は、案内を出して B・K だけを取る
    let mut llm = true;
    if s.local {
        if !(s.api || s.cli || s.app) {
            steps.push(Step::note(
                "Ollama が見つかりません。LLM で直すには、Ollama（https://ollama.com）をインストールして起動してから、もう一度実行してください。それまでは LLM を使わない直し方（長さの規則）で直します",
            ));
            llm = false;
        } else if !s.api {
            let how = if s.systemd {
                "sudo systemctl start ollama"
            } else if s.app {
                "Ollama のアプリを開く"
            } else {
                "ollama serve"
            };
            steps.push(Step::note(format!(
                "Ollama が動いていません。起動してから（{how}）もう一度実行すると、LLM のモデルも取得します。それまでは LLM を使わない直し方（長さの規則）で直します"
            )));
            llm = false;
        } else {
            steps.push(Step::note("Ollama: 既にあります"));
        }
    } else if !s.api {
        steps.push(Step::stop(format!(
            "接続先の Ollama（{}）が応答しません。KIKITORI_OLLAMA / OLLAMA_HOST を確かめてください",
            ollama::base(None)
        )));
        return steps;
    }
    if let Some(v) = s.version.filter(|v| *v < catalog::MIN_OLLAMA) {
        let whose = if s.local { "" } else { "接続先の " };
        steps.push(Step::note(format!(
            "{whose}Ollama {} は古く、{} 以上が要ります。Ollama の案内に従って更新してから、もう一度実行してください。それまでは LLM のモデルを取得しません",
            version_text(v),
            version_text(catalog::MIN_OLLAMA)
        )));
        llm = false;
    }
    let (_, _, why) = catalog::pick(hw);
    if llm {
        steps.extend(why.into_iter().map(Step::note));
    }
    let mut pulls: Vec<String> = Vec::new();
    match merge_model.filter(|_| llm).filter(|m| !m.is_empty()) {
        // D-9: --setup --merge-model <名前> はそのモデルだけを取る
        Some(m) => pulls.push(m.to_owned()),
        None if llm => {
            let (_, picked, _) = catalog::pick(hw);
            for m in picked.into_iter().flatten() {
                if !pulls.contains(&m) {
                    pulls.push(m);
                }
            }
        }
        None => {}
    }
    for m in pulls {
        if s.models.contains_key(&m) {
            steps.push(Step::note(format!("{m}: 既にあります")));
        } else {
            steps.push(Step::sized(
                "pull",
                format!("{m} を取得する"),
                catalog::size_of(&m),
                m,
            ));
        }
    }
    if no_hf {
        steps.push(Step::note(
            "Hugging Face のモデルは取得しません（--no-hf。初めて補正するときに取得されます）",
        ));
    } else {
        for (repo, size) in [(evidence::QWEN, QWEN_SIZE), (evidence::KANA, KANA_SIZE)] {
            if s.hf_missing.iter().any(|m| m == repo) {
                steps.push(Step::sized("hf", format!("{repo} を取得する"), size, repo));
            } else {
                steps.push(Step::note(format!("{repo}: 既にあります")));
            }
        }
    }
    let need_ollama: u64 = if s.local {
        steps
            .iter()
            .filter(|x| x.kind == "pull")
            .map(|x| x.size)
            .sum()
    } else {
        0
    };
    let need_hf: u64 = steps
        .iter()
        .filter(|x| x.kind == "hf")
        .map(|x| x.size)
        .sum();
    for (need, free, where_) in [
        (need_ollama, s.free_ollama, "Ollama のモデルの置き場"),
        (need_hf, s.free_hf, "Hugging Face の置き場"),
    ] {
        if need > 0 && free.is_some_and(|free| free < need + SPARE) {
            steps.push(Step::stop(format!(
                "{where_}の空きが {:.1}GB で、{:.1}GB 要ります",
                free.unwrap() as f64 / catalog::GB as f64,
                (need + SPARE) as f64 / catalog::GB as f64
            )));
            return steps;
        }
    }
    steps
}

/// 入れることになった手だけ抽出する。
pub fn todo(steps: &[Step]) -> Vec<Step> {
    steps
        .iter()
        .filter(|x| matches!(x.kind, "pull" | "hf"))
        .cloned()
        .collect()
}

fn os_name() -> (&'static str, bool) {
    if cfg!(target_os = "macos") {
        ("Darwin", true)
    } else if cfg!(target_os = "windows") {
        ("Windows", false)
    } else {
        ("Linux", false)
    }
}

fn has_tty() -> bool {
    let tty_path = if cfg!(windows) { "CON" } else { "/dev/tty" };
    std::fs::File::open(tty_path).is_ok()
}

/// `path`（無ければ、ある親）を含むディスクの空き。分からなければ None
pub fn free_space(path: &Path) -> Option<u64> {
    let mut p = path.to_path_buf();
    while !p.exists() && p.parent().is_some() {
        if !p.pop() {
            break;
        }
    }
    statvfs_free(&p)
}

#[cfg(unix)]
fn statvfs_free(p: &Path) -> Option<u64> {
    let bytes = std::ffi::CString::new(p.as_os_str().as_encoded_bytes().to_vec()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(bytes.as_ptr(), &mut st) } != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)]
    Some(st.f_frsize as u64 * st.f_bavail as u64)
}

/// `p` を含むドライブの空き。マウント先がいちばん長く一致するディスクを選ぶ
#[cfg(windows)]
fn statvfs_free(p: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| p.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

fn hf_missing() -> Vec<String> {
    let mut missing = Vec::new();
    for (role, repo) in [("refine-b", evidence::QWEN), ("refine-k", evidence::KANA)] {
        if evidence::model_dir(role).is_none() {
            missing.push(repo.to_owned());
        }
    }
    missing
}

/// 事実を調べる。止まらず、答えられない事実は初期値のままにする。
pub fn gather(hardware_flag: Option<&str>) -> Result<State, CliError> {
    let (osname, _) = os_name();
    let local = ollama::is_local(&ollama::base(None));
    let hardware_given =
        local || hardware_flag.is_some() || std::env::var_os("KIKITORI_HARDWARE").is_some();
    let hw = if hardware_given {
        catalog::detect(hardware_flag)?
    } else {
        catalog::Hardware {
            kind: "cpu",
            memory: None,
            why: "接続先の機器が分からない".to_owned(),
        }
    };
    let refine = evidence::available();
    let v = ollama::version(&ollama::base(None), ollama::SMALL_TIMEOUT);
    let mut s = State {
        os: osname,
        hardware: hw,
        refine,
        local,
        api: v.is_some(),
        version: v,
        cli: false,
        app: false,
        systemd: false,
        models: std::collections::BTreeMap::new(),
        tty: has_tty(),
        free_ollama: None,
        free_hf: None,
        hf_missing: Vec::new(),
        hardware_given,
    };
    if v.is_some() {
        s.models = ollama::models(&ollama::base(None), ollama::SMALL_TIMEOUT).unwrap_or_default();
    }
    if local {
        s.cli = catalog::which("ollama").is_some();
        s.app = osname == "Darwin" && Path::new(APP).exists();
        s.systemd = osname == "Linux" && Path::new("/etc/systemd/system/ollama.service").exists();
        s.free_ollama = free_space(
            &std::env::var_os("OLLAMA_MODELS")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".ollama")),
        );
    }
    if refine {
        s.hf_missing = hf_missing();
        s.free_hf = free_space(
            &std::env::var_os("HF_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".cache/huggingface")),
        );
    }
    Ok(s)
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn say(msg: &str) {
    kikitori_core::notice::info(msg);
}

/// 入れる前の確認。/dev/tty に `[y/N]:` を出して読む。端末が無ければ false。
pub fn confirm(total: u64) -> bool {
    use std::io::{Read, Write};
    let mut tty = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(if cfg!(windows) { "CON" } else { "/dev/tty" })
    {
        Ok(tty) => tty,
        Err(_) => return false,
    };
    let _ = write!(
        tty,
        "合わせて約 {:.1}GB を入れます。進めますか [y/N]: ",
        total as f64 / catalog::GB as f64
    );
    let _ = tty.flush();
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match tty.read(&mut byte) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if byte[0] == b'\n' || byte[0] == b'\r' {
                    break;
                }
                line.push(byte[0]);
                if line.len() > 128 {
                    break;
                }
            }
        }
    }
    let answer = String::from_utf8_lossy(&line).trim().to_ascii_lowercase();
    answer == "y" || answer == "yes"
}

pub enum SetupOutcome {
    Done,
    Failed,
    Stopped,
    NoTerminal,
    Declined,
}

pub fn to_exit_code(outcome: SetupOutcome) -> Result<(), CliError> {
    match outcome {
        SetupOutcome::Done => Ok(()),
        SetupOutcome::Failed => Err(CliError::new(
            ErrorKind::Processing,
            "補正の環境づくりに失敗しました。",
            "同じコマンドを実行し直すと、済んだところは飛ばし、取得は続きから進みます。",
        )),
        SetupOutcome::Stopped => Err(CliError::new(
            ErrorKind::Dependency,
            "補正に必要なものが足りません。",
            "上に出た手順を行ってから、同じコマンドを実行し直してください。",
        )),
        SetupOutcome::NoTerminal => Err(CliError::new(
            ErrorKind::NeedsConfirmation,
            "端末が無いので確認できません。",
            "入れてよければ --yes を付けて実行し直してください。",
        )),
        SetupOutcome::Declined => Err(CliError::new(
            ErrorKind::NeedsConfirmation,
            "入れる前に確認が得られませんでした。",
            "確認に進めるには端末で同じコマンドを実行し直してください。",
        )),
    }
}

/// 実行する。確認は端末（/dev/tty）で行う。`--yes` のときは省く。
pub fn run_setup(s: &State, steps: &[Step], yes: bool, dry_run: bool) -> SetupOutcome {
    for x in steps {
        if x.kind == "note" {
            say(&format!("- {}", x.text));
        } else if x.kind == "stop" {
            say(&format!("止まりました: {}", x.text));
            return SetupOutcome::Stopped;
        } else {
            let size = if x.size > 0 {
                format!("（約 {:.1}GB）", x.size as f64 / catalog::GB as f64)
            } else {
                String::new()
            };
            say(&format!("+ {}{size}", x.text));
        }
    }
    let work = todo(steps);
    if work.is_empty() {
        say("取得するものはありません");
        return SetupOutcome::Done;
    }
    if dry_run {
        say("--dry-run のため、入れずに終えます");
        return SetupOutcome::Done;
    }
    let total: u64 = work.iter().map(|x| x.size).sum();
    if !yes {
        if !s.tty {
            // 理由と次の手は、呼び出し側が終了コード 6 のエラーとして出す
            return SetupOutcome::NoTerminal;
        }
        if !confirm(total) {
            say("入れずに終えます");
            return SetupOutcome::Declined;
        }
    }
    for x in work {
        say(&format!("{} ...", x.text));
        if let Err(e) = run_step(&x) {
            say(&format!("失敗しました: {}: {e}", x.text));
            say("同じコマンドを実行し直すと、済んだところは飛ばし、取得は続きから進みます");
            return SetupOutcome::Failed;
        }
    }
    say("済みました");
    SetupOutcome::Done
}

fn run_step(x: &Step) -> Result<(), String> {
    match x.kind {
        "pull" => {
            let at = ollama::base(None);
            ollama::pull(&at, &x.arg, |m| say(&format!("  {m}")))?;
            // 取り直しが効くよう、kikitori が入れた Ollama のモデルを記録する（D-12）
            record_ollama_model_receipt(&x.arg);
            Ok(())
        }
        "hf" => {
            for role in if x.arg == evidence::QWEN {
                ["refine-b"]
            } else {
                ["refine-k"]
            } {
                let m = model_of(role);
                let (repo, commit) = repo_commit(&m);
                for file in m.files {
                    if kikitori_fetch::cached_hf(
                        repo,
                        commit,
                        file.path,
                        &kikitori_fetch::cache_root(),
                    )
                    .is_some()
                    {
                        continue;
                    }
                    kikitori_fetch::fetch_hf(
                        repo,
                        commit,
                        file.path,
                        file.sha256,
                        file.git_sha1,
                        file.size,
                        false,
                        |_, _| {},
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn model_of(role: &str) -> kikitori_fetch::Model {
    kikitori_fetch::MODELS
        .iter()
        .find(|m| m.role == role)
        .expect("pinned refine model")
        .clone()
}

fn repo_commit(m: &kikitori_fetch::Model) -> (&str, &str) {
    m.source
        .strip_prefix("hf:")
        .and_then(|s| s.split_once('@'))
        .unwrap_or((m.source, ""))
}

/// ollama-model 取得の記録モデル一覧に加える。
pub fn record_ollama_model_receipt(name: &str) {
    let _ = kikitori_fetch::record_receipt("ollama-model", name);
}
