//! `kikitori-refine --setup`: 補正に使う環境（Ollama・Ollama のモデル・Hugging Face のモデル）を作る（Python `refine/setup.py`）。
//! 事実（State）からやることの並び（Step）を作る plan_setup は純粋な関数で、実行は run_setup が行う。

use kikitori_core::{CliError, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use crate::{catalog, evidence, ollama};

fn app_override() -> &'static Mutex<Option<PathBuf>> {
    static CELL: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// /Applications/Ollama.app の代わり（テスト用）。
pub fn set_app_override(path: Option<PathBuf>) -> Option<PathBuf> {
    std::mem::replace(&mut *app_override().lock().unwrap(), path)
}

fn app_path() -> PathBuf {
    app_override()
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| PathBuf::from(APP))
}

/// `install` 段の実行の差し替え（テスト用）。None を返したら公の導入スクリプトを動かす。
type InstallHook = Box<dyn Fn() -> std::process::ExitStatus + Send>;
fn install_override() -> &'static Mutex<Option<InstallHook>> {
    static CELL: OnceLock<Mutex<Option<InstallHook>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}
pub fn set_install_override(value: Option<InstallHook>) -> Option<InstallHook> {
    std::mem::replace(&mut *install_override().lock().unwrap(), value)
}

pub const OLLAMA_SIZE: u64 = catalog::GB; // Ollama 本体のおおよその大きさ
pub const QWEN_SIZE: u64 = (3.8 * catalog::GB as f64) as u64;
pub const KANA_SIZE: u64 = (3.0 * catalog::GB as f64) as u64;
pub const SPARE: u64 = 2 * catalog::GB;

pub const TOOLS: [(&str, &[&str]); 3] = [
    ("Darwin", &["curl", "unzip"]),
    (
        "Linux",
        &["curl", "awk", "grep", "sed", "tee", "xargs", "zstd"],
    ),
    ("Windows", &[]),
];
pub const INSTALL_DARWIN_LINUX: [&str; 3] =
    ["sh", "-c", "curl -fsSL https://ollama.com/install.sh | sh"];
pub const INSTALL_WINDOWS: [&str; 4] = [
    "powershell",
    "-NoProfile",
    "-Command",
    "irm https://ollama.com/install.ps1 | iex",
];

pub const APP: &str = "/Applications/Ollama.app";

#[derive(Clone, Debug)]
pub struct State {
    pub os: &'static str, // Darwin / Linux / Windows
    pub hardware: catalog::Hardware,
    /// Qwen3-ASR と kana-whisper を動かせる（モデルがそろっている）
    pub refine: bool,
    /// Ollama の向き先がこの PC
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
    pub sudo_needed: bool,
    pub sudo_ok: bool,
    pub tools_missing: Vec<String>,
    pub free_ollama: Option<u64>,
    pub free_hf: Option<u64>,
    pub hf_missing: Vec<String>,
    /// 遠くの Ollama で --hardware を指定したか
    pub hardware_given: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub kind: &'static str, // note / stop / install / start / pull / hf
    pub text: String,
    pub size: u64,
    pub arg: String,
    /// 止まった理由の区別（stop では sudo の stop に "privilege" を付ける）
    pub tag: &'static str,
}

impl Step {
    pub fn note(text: impl Into<String>) -> Self {
        Self {
            kind: "note",
            text: text.into(),
            size: 0,
            arg: String::new(),
            tag: "",
        }
    }
    pub fn stop(text: impl Into<String>) -> Self {
        Self {
            kind: "stop",
            text: text.into(),
            size: 0,
            arg: String::new(),
            tag: "",
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
            tag: "",
        }
    }
    pub fn plain(kind: &'static str, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            size: 0,
            arg: String::new(),
            tag: "",
        }
    }
    pub fn tagged(kind: &'static str, text: impl Into<String>, tag: &'static str) -> Self {
        Self {
            kind,
            text: text.into(),
            size: 0,
            arg: String::new(),
            tag,
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
            "向き先の Ollama の機器が分からないので、GGUF の tev1 だけを入れます。Apple Silicon や NVIDIA なら --hardware apple / nvidia を付けてください",
        ));
    }
    let mut will_install = false;
    if s.local {
        if s.api || s.cli || s.app {
            steps.push(Step::note("Ollama: 既にあります"));
        } else {
            if !s.tools_missing.is_empty() {
                let listed = s.tools_missing.join(" ");
                let cmd = if s.os == "Darwin" {
                    "brew install "
                } else {
                    "sudo apt-get install -y "
                }
                .to_owned()
                    + &listed;
                steps.push(Step::stop(format!(
                    "Ollama を入れるのに要る道具がありません: {listed}。入れてから打ち直してください（例: {cmd}）"
                )));
                return steps;
            }
            if s.sudo_needed && !s.sudo_ok && !s.tty {
                let cmd = INSTALL_DARWIN_LINUX[2..].join(" ");
                steps.push(Step::tagged(
                    "stop",
                    format!(
                        "Ollama の導入に管理者の権限（sudo）が要りますが、パスワードを聞ける端末がありません。端末で次を打ってから、打ち直してください: {cmd}"
                    ),
                    "privilege",
                ));
                return steps;
            }
            steps.push(Step::sized(
                "install",
                "Ollama を公式の導入スクリプトで入れる",
                OLLAMA_SIZE,
                "",
            ));
            will_install = true;
        }
        if !s.api {
            if s.systemd && !will_install {
                steps.push(Step::stop(
                    "Ollama のサービスが止まっています。sudo systemctl start ollama で起こしてから、打ち直してください",
                ));
                return steps;
            }
            steps.push(Step::plain("start", "Ollama を起こす"));
        }
    } else if !s.api {
        steps.push(Step::stop(format!(
            "向き先の Ollama（{}）が答えません。KIKITORI_OLLAMA / OLLAMA_HOST を確かめてください",
            ollama::base(None)
        )));
        return steps;
    }
    if let Some(v) = s.version {
        if v < catalog::MIN_OLLAMA {
            let how = if s.local {
                "同じ導入スクリプト（curl -fsSL https://ollama.com/install.sh | sh）で更新できます"
                    .to_owned()
            } else {
                "向き先の Ollama を更新してください".to_owned()
            };
            let warn = if s.local && s.os == "Darwin" {
                "。macOS では、実行中の Ollama とモデルが止まります"
            } else {
                ""
            };
            steps.push(Step::stop(format!(
                "Ollama {} は古く、{} 以上が要ります。{how}{warn}",
                version_text(v),
                version_text(catalog::MIN_OLLAMA)
            )));
            return steps;
        }
    }
    let (_, _, why) = catalog::pick(hw);
    steps.extend(why.into_iter().map(Step::note));
    let mut pulls: Vec<String> = Vec::new();
    match merge_model.filter(|m| !m.is_empty()) {
        // D-9: --setup --merge-model <名前> はそのモデルだけを取る
        Some(m) => pulls.push(m.to_owned()),
        None => {
            let (_, picked, _) = catalog::pick(hw);
            for m in picked.into_iter().flatten() {
                if !pulls.contains(&m) {
                    pulls.push(m);
                }
            }
        }
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
        .filter(|x| matches!(x.kind, "install" | "start" | "pull" | "hf"))
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

fn which(name: &str) -> bool {
    catalog::which(name).is_some()
}

#[cfg(unix)]
fn euid() -> u32 {
    unsafe { libc::geteuid() }
}

/// Windows には root が無いので、いつも一般の利用者として扱う
#[cfg(windows)]
fn euid() -> u32 {
    1
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
            why: "向き先の機器が分からない".to_owned(),
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
        sudo_needed: false,
        sudo_ok: false,
        tools_missing: Vec::new(),
        free_ollama: None,
        free_hf: None,
        hf_missing: Vec::new(),
        hardware_given,
    };
    if v.is_some() {
        s.models = ollama::models(&ollama::base(None), ollama::SMALL_TIMEOUT).unwrap_or_default();
    }
    if local {
        s.cli = which("ollama");
        s.app = osname == "Darwin" && Path::new(APP).exists();
        s.systemd = osname == "Linux" && Path::new("/etc/systemd/system/ollama.service").exists();
        let tools = TOOLS
            .iter()
            .find(|(o, _)| *o == osname)
            .map(|(_, t)| *t)
            .unwrap_or(&[]);
        s.tools_missing = tools
            .iter()
            .map(|t| t.to_string())
            .filter(|t| !which(t))
            .collect();
        s.sudo_needed = match osname {
            "Darwin" => writable_dir("/usr/local/bin").is_none(),
            "Linux" => euid() != 0,
            _ => false,
        };
        if s.sudo_needed && which("sudo") {
            s.sudo_ok = Command::new("sudo")
                .arg("-n")
                .arg("true")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
        }
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

#[cfg(unix)]
fn writable_dir(path: &str) -> Option<()> {
    // os.access(path, os.W_OK) と同じ
    let c = std::ffi::CString::new(path).ok()?;
    if unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0 {
        Some(())
    } else {
        None
    }
}

#[cfg(windows)]
fn writable_dir(_path: &str) -> Option<()> {
    Some(())
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

/// 導入スクリプトの終了コードによらず、Ollama 本体が置かれたか。
pub fn installed(s: &State) -> bool {
    (s.os == "Darwin" && app_path().exists()) || catalog::which("ollama").is_some()
}

fn start_ollama(s: &State) -> bool {
    if s.os == "Darwin" && app_path().exists() {
        // 入れた直後は名前（open -a）で引けないことがあるので、パスで開く
        let _ = Command::new("open")
            .arg(app_path())
            .args(["--args", "hidden"])
            .spawn();
    } else {
        let Some(exe) = catalog::which("ollama") else {
            return false;
        };
        let log = home().join(".cache/kikitori/ollama.log");
        let _ = std::fs::create_dir_all(log.parent().unwrap_or(Path::new(".")));
        let Ok(file) = std::fs::File::options()
            .append(true)
            .create(true)
            .open(&log)
        else {
            return false;
        };
        let Ok(file2) = file.try_clone() else {
            return false;
        };
        let mut cmd = Command::new(exe);
        cmd.arg("serve")
            .stdout(Stdio::from(file))
            .stderr(Stdio::from(file2))
            .stdin(Stdio::null());
        #[cfg(unix)]
        {
            // start_new_session=True に当たる
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        if cmd.spawn().is_err() {
            return false;
        }
    }
    for _ in 0..30 {
        if ollama::version(&ollama::base(None), ollama::SMALL_TIMEOUT).is_some() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    false
}

pub enum SetupOutcome {
    Done,
    Failed,
    Stopped,
    Privilege,
    NoTerminal,
    Declined,
}

pub fn to_exit_code(outcome: SetupOutcome) -> Result<(), CliError> {
    match outcome {
        SetupOutcome::Done => Ok(()),
        SetupOutcome::Failed => Err(CliError::new(
            ErrorKind::Processing,
            "補正の環境づくりに失敗しました。",
            "同じコマンドを打ち直すと、済んだところは飛ばし、取得は続きから進みます。",
        )),
        SetupOutcome::Stopped => Err(CliError::new(
            ErrorKind::Dependency,
            "補正に必要なものが足りません。",
            "上に出た手順を行ってから、同じコマンドを打ち直してください。",
        )),
        SetupOutcome::Privilege => Err(CliError::new(
            ErrorKind::Privilege,
            "Ollama の導入に管理者の権限が要ります。",
            "端末で導入コマンドを打ってから、同じコマンドを打ち直してください。",
        )),
        SetupOutcome::NoTerminal => Err(CliError::new(
            ErrorKind::NeedsConfirmation,
            "端末が無いので確認できません。",
            "入れてよければ --yes を付けて打ち直してください。",
        )),
        SetupOutcome::Declined => Err(CliError::new(
            ErrorKind::NeedsConfirmation,
            "入れる前に確認が得られませんでした。",
            "確認に進めるには端末で同じコマンドを打ち直してください。",
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
            return if x.tag == "privilege" {
                SetupOutcome::Privilege
            } else {
                SetupOutcome::Stopped
            };
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
        say("全部あります。入れるものはありません");
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
        if let Err(e) = run_step(s, &x) {
            say(&format!("失敗しました: {}: {e}", x.text));
            say("同じコマンドを打ち直すと、済んだところは飛ばし、取得は続きから進みます");
            return SetupOutcome::Failed;
        }
    }
    say("済みました");
    SetupOutcome::Done
}

fn run_step(s: &State, x: &Step) -> Result<(), String> {
    match x.kind {
        "install" => {
            let code: std::process::ExitStatus = match install_override().lock().unwrap().as_ref() {
                Some(hook) => hook(),
                None => {
                    let args: &[&str] = match s.os {
                        "Windows" => &INSTALL_WINDOWS[1..],
                        _ => &INSTALL_DARWIN_LINUX[1..],
                    };
                    let program = match s.os {
                        "Windows" => INSTALL_WINDOWS[0],
                        _ => INSTALL_DARWIN_LINUX[0],
                    };
                    Command::new(program)
                        .args(args)
                        .status()
                        .map_err(|e| e.to_string())?
                }
            };
            if code.success() {
                return Ok(());
            }
            if !installed(s) {
                return Err("公式の導入スクリプトが失敗しました".to_owned());
            }
            if s.os == "Darwin" {
                // macOS の公式スクリプトは、入れた直後の起動（open -a）で失敗することがある。起動は次の段で行う
                say(
                    "公式の導入スクリプトは失敗を返しましたが、Ollama は入りました。起動に進みます",
                );
            }
            Ok(())
        }
        "start" => {
            if ollama::version(&ollama::base(None), ollama::SMALL_TIMEOUT).is_none()
                && !start_ollama(s)
            {
                return Err("30 秒待っても Ollama が答えません（~/.cache/kikitori/ollama.log を見てください）".to_owned());
            }
            if let Some(v) = ollama::version(&ollama::base(None), ollama::SMALL_TIMEOUT) {
                if v < catalog::MIN_OLLAMA {
                    return Err(format!(
                        "Ollama {} は古く、{} 以上が要ります",
                        version_text(v),
                        version_text(catalog::MIN_OLLAMA)
                    ));
                }
            }
            Ok(())
        }
        "pull" => {
            if ollama::version(&ollama::base(None), ollama::SMALL_TIMEOUT).is_none() {
                start_ollama(s);
            }
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
