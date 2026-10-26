use crate::receipt;
use std::{fs, path::PathBuf, process::Command};

pub fn run(version: Option<String>, variant: Option<String>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = exe
        .file_name()
        .ok_or("executable has no filename")?
        .to_string_lossy();
    let target = std::env::consts::ARCH;
    let triple = match (std::env::consts::OS, target) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        _ => return Err("unsupported update platform".into()),
    };
    let variant = variant
        .or_else(|| {
            receipt::entries()
                .into_iter()
                .find(|(k, _)| k == "variant")
                .map(|(_, v)| v)
        })
        .unwrap_or_else(|| "cpu".into());
    let suffix = if variant == "cuda" { "-cuda" } else { "" };
    let mut version = version.unwrap_or_else(|| "latest".into());
    let base = std::env::var("KIKITORI_BASE_URL").unwrap_or_else(|_| {
        let repo = std::env::var("KIKITORI_REPO").unwrap_or_else(|_| "kulikala/kikitori".into());
        if version == "latest" {
            format!("https://github.com/{repo}/releases/latest/download")
        } else {
            format!("https://github.com/{repo}/releases/download/v{version}")
        }
    });
    if version == "latest" {
        if let Ok(entries) = fs::read_dir(&base) {
            let prefix = "kikitori-";
            let suffix = format!(
                "-{triple}{suffix}.{}",
                if cfg!(windows) { "zip" } else { "tar.gz" }
            );
            version = entries
                .filter_map(Result::ok)
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.starts_with(prefix) && n.ends_with(&suffix))
                .filter_map(|n| {
                    n.strip_prefix(prefix)
                        .and_then(|n| n.strip_suffix(&suffix))
                        .map(str::to_owned)
                })
                .max()
                .ok_or("no matching release asset in local directory")?;
        } else {
            let repo =
                std::env::var("KIKITORI_REPO").unwrap_or_else(|_| "kulikala/kikitori".into());
            let out = Command::new("curl")
                .args([
                    "-fsSL",
                    &format!("https://api.github.com/repos/{repo}/releases/latest"),
                ])
                .output()
                .map_err(|e| e.to_string())?;
            let json: serde_json::Value =
                serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
            version = json["tag_name"]
                .as_str()
                .ok_or("latest release has no tag")?
                .trim_start_matches('v')
                .to_owned();
        }
    }
    let asset = format!(
        "kikitori-{version}-{triple}{suffix}.{}",
        if cfg!(windows) { "zip" } else { "tar.gz" }
    );
    let tmp = std::env::temp_dir().join(format!("kikitori-update-{}", std::process::id()));
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let archive = tmp.join(&asset);
    let sums = tmp.join("SHA256SUMS");
    download(
        &format!("{}/{}", base.trim_end_matches('/'), asset),
        &archive,
    )?;
    download(&format!("{}/SHA256SUMS", base.trim_end_matches('/')), &sums)?;
    let sums_text = fs::read_to_string(&sums).map_err(|e| e.to_string())?;
    let expected = sums_text
        .lines()
        .find_map(|line| {
            let mut p = line.split_whitespace();
            let hash = p.next()?;
            let file = p.next()?.trim_start_matches('*');
            (file == asset).then(|| hash.to_owned())
        })
        .ok_or("asset missing from SHA256SUMS")?;
    let actual = hash_file(&archive)?;
    if actual != expected {
        return Err("SHA-256 mismatch".into());
    }
    let unpack = tmp.join("unpacked");
    fs::create_dir_all(&unpack).map_err(|e| e.to_string())?;
    if cfg!(windows) {
        run_cmd(
            "tar",
            &[
                "-xf",
                archive.to_str().unwrap(),
                "-C",
                unpack.to_str().unwrap(),
            ],
        )?;
    } else {
        run_cmd(
            "tar",
            &[
                "-xzf",
                archive.to_str().unwrap(),
                "-C",
                unpack.to_str().unwrap(),
            ],
        )?;
    }
    let candidate = unpack.join(if cfg!(windows) {
        "kikitori.exe"
    } else {
        "kikitori"
    });
    if !Command::new(&candidate)
        .arg("--version")
        .status()
        .map_err(|e| e.to_string())?
        .success()
    {
        return Err("new binary failed --version".into());
    }
    let old = PathBuf::from(format!("{}.old", exe.display()));
    let _ = fs::remove_file(&old);
    fs::rename(&exe, &old).map_err(|e| format!("could not stage current executable: {e}"))?;
    if let Err(e) = fs::rename(candidate, &exe) {
        let _ = fs::rename(&old, &exe);
        return Err(e.to_string());
    }
    let good = Command::new(&exe)
        .arg("--version")
        .status()
        .is_ok_and(|s| s.success());
    if !good {
        let _ = fs::remove_file(&exe);
        let _ = fs::rename(&old, &exe);
        return Err("new binary failed; previous version restored".into());
    }
    if !cfg!(windows) {
        let _ = fs::remove_file(old);
    }
    let bundled_skill = include_str!("../../skill/SKILL.md");
    for (_, path) in receipt::entries()
        .into_iter()
        .filter(|(kind, _)| kind == "skill")
    {
        let _ = fs::write(path, bundled_skill);
    }
    receipt::record("variant", &variant)?;
    let _ = fs::remove_dir_all(tmp);
    println!("updated {}", name);
    Ok(())
}
fn download(url: &str, path: &std::path::Path) -> Result<(), String> {
    if let Some(local) = url.strip_prefix("file://") {
        fs::copy(local, path).map_err(|e| e.to_string())?;
    } else if std::path::Path::new(url).exists() {
        fs::copy(url, path).map_err(|e| e.to_string())?;
    } else {
        run_cmd("curl", &["-fsSL", url, "-o", path.to_str().unwrap()])?;
    }
    Ok(())
}
fn run_cmd(cmd: &str, args: &[&str]) -> Result<(), String> {
    let s = Command::new(cmd)
        .args(args)
        .status()
        .map_err(|e| e.to_string())?;
    if s.success() {
        Ok(())
    } else {
        Err(format!("{cmd} failed"))
    }
}
fn hash_file(path: &std::path::Path) -> Result<String, String> {
    let output = if cfg!(windows) {
        Command::new("certutil")
            .args(["-hashfile", path.to_str().unwrap(), "SHA256"])
            .output()
    } else {
        Command::new("shasum")
            .args(["-a", "256", path.to_str().unwrap()])
            .output()
    }
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("SHA-256 utility failed".into());
    }
    let s = String::from_utf8_lossy(&output.stdout);
    Ok(if cfg!(windows) {
        s.lines().nth(1).unwrap_or("").trim().to_ascii_lowercase()
    } else {
        s.split_whitespace().next().unwrap_or("").to_owned()
    })
}
