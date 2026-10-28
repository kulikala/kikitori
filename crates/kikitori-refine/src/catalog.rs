//! Hardware detection and the Ollama model table (Python `refine/catalog.py`).
//!
//! | kind | merge | fix |
//! |---|---|---|
//! | apple (Apple Silicon) | qwen3.8:27b-mlx | tev1:0.8b-mxfp8 |
//! | nvidia | qwen3.8:27b-q4_K_M | tev1:0.8b-q8_0 |
//! | cpu | (unused) | tev1:0.8b-q8_0 |

use kikitori_core::CliError;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

fn ram_override() -> &'static Mutex<Option<u64>> {
    static CELL: OnceLock<Mutex<Option<u64>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

/// ram() の差し替え（テスト用）。返り値は元の値。
pub fn set_ram_override(value: Option<u64>) -> Option<u64> {
    std::mem::replace(&mut *ram_override().lock().unwrap(), value)
}

/// 外部コマンドの結果の差し替え（テスト用）: あればコマンドを動かさず、これを使う。
type RunHook = Box<dyn Fn(&str, &[&str]) -> String + Send>;
fn run_override() -> &'static Mutex<Option<RunHook>> {
    static CELL: OnceLock<Mutex<Option<RunHook>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}
pub fn set_run_override(value: Option<RunHook>) -> Option<RunHook> {
    std::mem::replace(&mut *run_override().lock().unwrap(), value)
}

type WhichHook = Box<dyn Fn(&str) -> Option<String> + Send>;
fn which_override() -> &'static Mutex<Option<WhichHook>> {
    static CELL: OnceLock<Mutex<Option<WhichHook>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}
pub fn set_which_override(value: Option<WhichHook>) -> Option<WhichHook> {
    std::mem::replace(&mut *which_override().lock().unwrap(), value)
}

pub const GB: u64 = 1_000_000_000;
pub const HEADROOM: u64 = 8 * GB;
pub const KINDS: [&str; 3] = ["apple", "nvidia", "cpu"];

/// Candidates per role and hardware kind, most preferred first.
#[allow(clippy::type_complexity)]
pub const CATALOG: [(&str, &[(&str, &[&str])]); 2] = [
    (
        "merge",
        &[
            ("apple", &["qwen3.8:27b-mlx"]),
            ("nvidia", &["qwen3.8:27b-q4_K_M"]),
            ("cpu", &[]),
        ],
    ),
    (
        "fix",
        &[
            ("apple", &["tev1:0.8b-mxfp8"]),
            ("nvidia", &["tev1:0.8b-q8_0"]),
            ("cpu", &["tev1:0.8b-q8_0"]),
        ],
    ),
];

pub const SIZES: [(&str, u64); 4] = [
    ("qwen3.8:27b-mlx", 18 * GB),
    ("qwen3.8:27b-q4_K_M", 18 * GB),
    ("tev1:0.8b-mxfp8", (0.8 * GB as f64) as u64),
    ("tev1:0.8b-q8_0", (0.8 * GB as f64) as u64),
];
/// tev1's `/v1/systemone` and `tev1:0.8b-mxfp8` need at least this Ollama version.
pub const MIN_OLLAMA: (u32, u32, u32) = (0, 40, 0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hardware {
    pub kind: &'static str,
    /// Size of where the model is placed: unified memory on apple, the largest
    /// VRAM on nvidia, and RAM on cpu.
    pub memory: Option<u64>,
    pub why: String,
}

pub fn size_of(model: &str) -> u64 {
    SIZES
        .iter()
        .find(|(name, _)| *name == model)
        .map(|(_, size)| *size)
        .unwrap_or(0)
}

/// 子が終わった後、出力の読み手を待つ上限。孫がパイプを開いたままでも止まらない
const READER_WAIT: Duration = Duration::from_secs(5);

fn run_capture(program: &str, args: &[&str]) -> String {
    if let Some(hook) = run_override().lock().unwrap().as_ref() {
        return hook(program, args).trim().to_owned();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut child = match Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return String::new(),
    };
    // 待つ間に stdout を読み切る。パイプが埋まって子が止まるのを避ける
    let (tx, rx) = mpsc::channel();
    if let Some(mut out) = child.stdout.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut out, &mut buf);
            let _ = tx.send(buf);
        });
    }
    drop(tx);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                // 子が終わっても、孫がパイプを持ったままだと EOF が来ない。
                // そのため読み手は有限時間だけ待ち、読めた分を使う
                let buf = rx.recv_timeout(READER_WAIT).unwrap_or_default();
                return String::from_utf8_lossy(&buf).trim().to_owned();
            }
            Ok(None) if Instant::now() >= deadline => {
                // 読み手は join しない（パイプが閉じないと join が返らない）
                let _ = child.kill();
                let _ = child.wait();
                return String::new();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return String::new();
            }
        }
    }
}

#[cfg(unix)]
fn ram() -> Option<u64> {
    if let Some(v) = *ram_override().lock().unwrap() {
        return Some(v);
    }
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    if page > 0 && pages > 0 {
        Some((page as u64) * (pages as u64))
    } else {
        None
    }
}

#[cfg(windows)]
fn ram() -> Option<u64> {
    if let Some(v) = *ram_override().lock().unwrap() {
        return Some(v);
    }
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    Some(sys.total_memory()).filter(|&m| m > 0)
}

#[cfg(unix)]
fn ram_and_detect(kind: &'static str, why: &str) -> Hardware {
    Hardware {
        kind,
        memory: ram(),
        why: why.to_owned(),
    }
}

#[cfg(windows)]
fn ram_and_detect(kind: &'static str, why: &str) -> Hardware {
    Hardware {
        kind,
        memory: None,
        why: why.to_owned(),
    }
}

fn nvidia_vram() -> Option<u64> {
    which("nvidia-smi")?;
    let out = run_capture(
        "nvidia-smi",
        &["--query-gpu=memory.total", "--format=csv,noheader,nounits"],
    );
    let max = out
        .split_whitespace()
        .filter_map(|v| v.parse::<u64>().ok())
        .max()?;
    (max > 0).then_some(max * 2u64.pow(20))
}

#[cfg(unix)]
pub fn which(name: &str) -> Option<String> {
    if let Some(hook) = which_override().lock().unwrap().as_ref() {
        return hook(name);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
        .map(|p| p.display().to_string())
}

/// Windows の選択規則を、あるフォルダにあるファイル名の一覧に対して適用する。
/// PATHEXT の拡張子を先に試し、裸の名前は既に PATHEXT の拡張子を持つ場合だけ受け入れる。
/// 全 OS でテストできるよう、ファイルシステムに触れない純関数にしてある。
#[cfg_attr(not(windows), allow(dead_code))]
fn pick_windows(name: &str, extensions: &[&str], existing: &[&str]) -> Option<String> {
    for extension in extensions {
        let candidate = format!("{name}{extension}");
        if let Some(found) = existing.iter().find(|f| f.eq_ignore_ascii_case(&candidate)) {
            return Some((*found).to_owned());
        }
    }
    let listed = std::path::Path::new(name)
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy().to_ascii_lowercase()))
        .is_some_and(|ext| extensions.iter().any(|e| e.to_ascii_lowercase() == ext));
    if listed {
        if let Some(found) = existing.iter().find(|f| f.eq_ignore_ascii_case(name)) {
            return Some((*found).to_owned());
        }
    }
    None
}

#[cfg(windows)]
pub fn which(name: &str) -> Option<String> {
    if let Some(hook) = which_override().lock().unwrap().as_ref() {
        return hook(name);
    }
    let path = std::env::var_os("PATH")?;
    // npm の claude.cmd のような .cmd / .bat も見つける
    let pathext = std::env::var_os("PATHEXT")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
    let extensions: Vec<&str> = pathext.split(';').filter(|ext| !ext.is_empty()).collect();
    for dir in std::env::split_paths(&path) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        if let Some(found) = pick_windows(name, &extensions, &refs) {
            return Some(dir.join(found).display().to_string());
        }
    }
    None
}

fn detect_kind(override_value: Option<&str>, with_darwin_arm: bool) -> Result<Hardware, CliError> {
    let kind = override_value
        .map(ToOwned::to_owned)
        .or_else(|| std::env::var("KIKITORI_HARDWARE").ok());
    if let Some(kind) = kind {
        let kind: &'static str = KINDS
            .iter()
            .copied()
            .find(|k| *k == kind.as_str())
            .ok_or_else(|| invalid_kind(&kind))?;
        let memory = if kind == "nvidia" {
            nvidia_vram()
        } else {
            ram()
        };
        return Ok(Hardware {
            kind,
            memory,
            why: "指定による".to_owned(),
        });
    }
    if with_darwin_arm && run_capture("/usr/sbin/sysctl", &["-n", "hw.optional.arm64"]) == "1" {
        return Ok(Hardware {
            kind: "apple",
            memory: ram(),
            why: "Apple Silicon".to_owned(),
        });
    }
    if let Some(vram) = nvidia_vram() {
        return Ok(Hardware {
            kind: "nvidia",
            memory: Some(vram),
            why: "NVIDIA の GPU".to_owned(),
        });
    }
    Ok(ram_and_detect(
        "cpu",
        "Apple Silicon・NVIDIA のどちらでもない",
    ))
}

fn invalid_kind(given: &str) -> CliError {
    let kinds = KINDS.join(" / ");
    CliError::new(
        kikitori_core::ErrorKind::Usage,
        format!("--hardware は {kinds} のどれかです: {given}"),
        "--hardware apple / nvidia / cpu のいずれかを指定してください。",
    )
}

/// Detect the hardware kind of this PC; `override_value` (`--hardware` /
/// `KIKITORI_HARDWARE`) takes precedence.
pub fn detect(override_value: Option<&str>) -> Result<Hardware, CliError> {
    let darwin_arm = cfg!(target_os = "macos");
    detect_kind(override_value, darwin_arm)
}

/// arm64 sysctl introspection forced on/off (test hook).
pub fn detect_with_darwin_arm(
    override_value: Option<&str>,
    with_arm: bool,
) -> Result<Hardware, CliError> {
    detect_kind(override_value, with_arm)
}

pub fn fits(model: &str, hw: &Hardware) -> bool {
    hw.memory.is_none() || hw.memory.unwrap() >= size_of(model) + HEADROOM
}

/// For each role, the model to use on this hardware and a line explaining why.
pub fn pick(hw: &Hardware) -> (Vec<&'static str>, Vec<Option<String>>, Vec<String>) {
    let mut roles = Vec::new();
    let mut out = Vec::new();
    let mut why = Vec::new();
    for (role, by_kind) in CATALOG {
        let kind = hw.kind;
        let (_, cands) = by_kind
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(("", &[][..]), |(k, c)| (*k, *c));
        let ok = cands.iter().find(|m| fits(m, hw)).map(|m| m.to_string());
        roles.push(role);
        out.push(ok.clone());
        if cands.is_empty() {
            why.push(format!("{role}: {kind} では使わない"));
        } else if ok.is_none() {
            let first = cands[0];
            let gb = size_of(first) / GB;
            let mem_gb = hw.memory.unwrap_or(0) as f64 / GB as f64;
            why.push(format!(
                "{role}: {first} は {gb}GB で、この機器の {mem_gb:.0}GB に載らない（{}GB 以上が要る）",
                (size_of(first) + HEADROOM) / GB
            ));
        } else {
            why.push(format!("{role}: {}", ok.unwrap()));
        }
    }
    (roles, out, why)
}

/// All table candidates for a role, deduplicated in table order (remote Ollama).
pub fn all_candidates(role: &str) -> Vec<&'static str> {
    let mut seen = Vec::new();
    for (r, by_kind) in CATALOG {
        if r != role {
            continue;
        }
        for (_, cands) in by_kind {
            for m in *cands {
                if !seen.contains(m) {
                    seen.push(*m);
                }
            }
        }
    }
    seen
}

pub fn candidates_for(kind: &str, role: &str) -> Vec<&'static str> {
    for (r, by_kind) in CATALOG {
        if r != role {
            continue;
        }
        for (k, cands) in by_kind {
            if *k == kind {
                return cands.to_vec();
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::pick_windows;

    const PATHEXT: [&str; 4] = [".COM", ".EXE", ".BAT", ".CMD"];

    #[test]
    fn windows_prefers_a_pathext_file_over_a_bare_shim() {
        // npm の sh シム `claude` より `claude.cmd` を選ぶ
        assert_eq!(
            pick_windows("claude", &PATHEXT, &["claude", "claude.cmd"]).as_deref(),
            Some("claude.cmd")
        );
    }

    #[test]
    fn windows_tries_pathext_in_order() {
        assert_eq!(
            pick_windows("claude", &PATHEXT, &["claude.cmd", "claude.exe"]).as_deref(),
            Some("claude.exe")
        );
    }

    #[test]
    fn windows_accepts_a_bare_name_only_with_a_listed_extension() {
        assert_eq!(
            pick_windows("tool.cmd", &PATHEXT, &["tool.cmd"]).as_deref(),
            Some("tool.cmd")
        );
        assert_eq!(pick_windows("claude", &PATHEXT, &["claude"]), None);
        assert_eq!(pick_windows("tool.py", &PATHEXT, &["tool.py"]), None);
    }
}
