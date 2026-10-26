//! `kikitori uninstall`: kikitori が置いたものだけを、一覧を出してから消す。
//!
//! 消すもの: インストール記録にある実行ファイル・スキル・PATH の行・HF のスナップショット・Ollama のモデル、
//! 記録が無くても既定のモデルと辞書の置き場（Python 版から移行した人のモデルもここにある）。
//! 消さないもの: Ollama 本体、記録に無い HF のキャッシュ、利用者が自分で入れたもの。

use crate::{log, receipt};
use kikitori_core::{paths, CliError};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const PATH_MARK: &str = "# added by kikitori";

#[derive(Default)]
struct Plan {
    /// 消すファイルとフォルダ
    paths: Vec<PathBuf>,
    /// 印つきの行を消す、シェルの設定ファイル
    path_lines: Vec<PathBuf>,
    ollama_models: Vec<String>,
    /// 残すが、利用者が自分で消せるように知らせるもの
    kept: Vec<String>,
}

fn plan(keep_models: bool) -> Plan {
    let mut p = Plan::default();
    for (kind, value) in receipt::entries() {
        match kind.as_str() {
            "bin" | "skill" => p.paths.push(PathBuf::from(value)),
            "models-dir" | "dict" if !keep_models => p.paths.push(PathBuf::from(value)),
            "hf-repo" if !keep_models => {
                let link = PathBuf::from(value);
                // スナップショットのファイルは blobs への参照なので、参照先の実体も消す
                if let Some(blob) = owned_blob(&link) {
                    p.paths.push(blob);
                }
                p.paths.push(link);
            }
            "path-line" => p.path_lines.push(PathBuf::from(value)),
            "ollama-model" if !keep_models => p.ollama_models.push(value),
            _ => {}
        }
    }
    // インストール記録が無くても、既定の置き場に置かれた実行ファイル（と旧い名前の転送）は消す
    if !p
        .paths
        .iter()
        .any(|x| x.file_stem().is_some_and(|n| n == "kikitori"))
    {
        if let Some(exe) = std::env::current_exe()
            .ok()
            .filter(|e| in_default_bin_dir(e))
        {
            let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
            for name in ["kikitori-refine", "kikitori-refine.cmd"] {
                if dir.join(name).exists() {
                    p.paths.push(dir.join(name));
                }
            }
            p.paths.push(exe);
        }
    }
    if !keep_models {
        for dir in [paths::models_root(), paths::dictionary_root()] {
            if dir.exists() {
                p.paths.push(dir);
            }
        }
        for repo in [
            "models--neosophie--Qwen3-ASR-1.7B-JA",
            "models--sbintuitions--kana-whisper",
        ] {
            let dir = paths::hf_cache_root().join(repo);
            let recorded = p.paths.iter().any(|x| x.starts_with(&dir));
            if dir.exists() && !recorded {
                p.kept.push(format!(
                    "{}（kikitori が取得したものではないので残します。消すには、このフォルダを削除してください）",
                    dir.display()
                ));
            }
        }
    }
    // 親のフォルダを消すなら、その中のものは並べない
    p.paths.sort();
    p.paths.dedup();
    let all = p.paths.clone();
    p.paths
        .retain(|x| !all.iter().any(|y| y != x && x.starts_with(y)));
    p.path_lines.sort();
    p.path_lines.dedup();
    p
}

pub fn run(keep_models: bool, yes: bool, dry_run: bool) -> Result<(), CliError> {
    let exe = std::env::current_exe().ok();
    let p = plan(keep_models);
    println!("削除するもの:");
    for x in &p.paths {
        println!("  {}", x.display());
    }
    for x in &p.path_lines {
        if cfg!(windows) {
            println!("  ユーザーの PATH にある {}", x.display());
        } else {
            println!("  {} の「{PATH_MARK}」の行", x.display());
        }
    }
    for m in &p.ollama_models {
        println!("  Ollama のモデル {m}");
    }
    if !p.kept.is_empty() {
        println!("残すもの:");
        for k in &p.kept {
            println!("  {k}");
        }
    }
    if dry_run || !log::confirm(yes, "これらを削除しますか？")? {
        return Ok(());
    }

    for m in &p.ollama_models {
        if let Err(e) = delete_ollama_model(m) {
            log::warning(&format!(
                "Ollama のモデル {m} を消せませんでした（{e}）。Ollama を起動して `ollama rm {m}` を実行してください。"
            ));
        }
    }
    for entry in &p.path_lines {
        if cfg!(windows) {
            remove_user_path_entry(entry);
        } else {
            remove_marked_lines(entry);
        }
    }
    for x in &p.paths {
        if exe.as_deref().is_some_and(|e| e == x) {
            continue; // 動いている実行ファイルは最後に消す
        }
        if let Err(e) = remove(x) {
            log::warning(&format!("{} を消せませんでした: {e}", x.display()));
        }
    }
    for x in p
        .paths
        .iter()
        .filter(|x| x.components().any(|c| c.as_os_str() == "skills"))
    {
        crate::commands::skill::remove_empty_dirs(x);
    }
    let _ = fs::remove_file(receipt::path());
    // 空になったフォルダだけを片付ける（中に利用者のものがあれば残る）
    for dir in [paths::config_dir(), kikitori_cache_dir()] {
        let _ = fs::remove_dir(dir);
    }
    if let Some(exe) = exe.filter(|e| p.paths.contains(e)) {
        remove_self(&exe);
    }
    log::info("kikitori を削除しました。");
    Ok(())
}

/// install.sh・install.ps1 が置く場所か
fn in_default_bin_dir(exe: &Path) -> bool {
    let dir = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs/kikitori"))
    } else {
        Some(paths::home_dir().join(".local/bin"))
    };
    dir.is_some_and(|d| exe.parent() == Some(d.as_path()))
}

fn kikitori_cache_dir() -> PathBuf {
    paths::models_root()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// Hugging Face のキャッシュで、`link`（snapshots の下の参照）が指す blobs の実体。
/// 同じリポジトリのほかのスナップショットからも参照されている実体は、消さないので返さない。
fn owned_blob(link: &Path) -> Option<PathBuf> {
    if !fs::symlink_metadata(link).ok()?.file_type().is_symlink() {
        return None;
    }
    let blob = fs::canonicalize(link).ok()?;
    let snapshots = link
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "snapshots"))?;
    let repo = snapshots.parent()?;
    if !blob.starts_with(fs::canonicalize(repo.join("blobs")).ok()?) {
        return None;
    }
    let mut stack = vec![snapshots.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path == link {
                continue;
            }
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(_) if fs::canonicalize(&path).ok().as_deref() == Some(blob.as_path()) => {
                    return None
                }
                _ => {}
            }
        }
    }
    Some(blob)
}

fn remove(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn remove_marked_lines(rc: &Path) {
    let Ok(text) = fs::read_to_string(rc) else {
        return;
    };
    if !text.contains(PATH_MARK) {
        return;
    }
    let kept: String = text
        .split_inclusive('\n')
        .filter(|l| !l.contains(PATH_MARK))
        .collect();
    if let Err(e) = fs::write(rc, kept) {
        log::warning(&format!(
            "{} の PATH の行を消せませんでした: {e}",
            rc.display()
        ));
    }
}

fn delete_ollama_model(name: &str) -> Result<(), String> {
    let host = kikitori_refine::ollama::base(None);
    let body = serde_json::json!({ "model": name }).to_string();
    kikitori_fetch::http_agent(Duration::from_secs(10))
        .delete(format!("{}/api/delete", host.trim_end_matches('/')))
        .force_send_body()
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Windows のインストールスクリプトはユーザーの PATH（レジストリ）にフォルダを足し、そのフォルダをインストール記録に残す。
fn remove_user_path_entry(dir: &Path) {
    let quoted = dir.display().to_string().replace('\'', "''");
    let script = format!(
        "$d='{quoted}'; $p=[Environment]::GetEnvironmentVariable('Path','User'); \
         if ($p) {{ $k=@($p -split ';' | Where-Object {{ $_ -and ($_.TrimEnd('\\') -ne $d.TrimEnd('\\')) }}); \
         [Environment]::SetEnvironmentVariable('Path', ($k -join ';'), 'User') }}"
    );
    let ok = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        log::warning(&format!(
            "ユーザーの PATH から {} を外せませんでした。設定の「環境変数」から外してください。",
            dir.display()
        ));
    }
}

/// 動いている実行ファイルを消す。Windows は使用中のファイルを消せないので、名前を変えてから、終わった後に別のプロセスで消す
fn remove_self(exe: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let old = exe.with_extension("exe.old");
        if fs::rename(exe, &old).is_ok() {
            let dir = old.parent().unwrap_or(Path::new("."));
            let quote = |p: &Path| p.display().to_string().replace('\'', "''");
            // この実行ファイルが終わるのを待って消す。ウイルス対策が掴んでいる間は消せないので、10 秒まで繰り返す。
            // 空になったインストール先のフォルダも消す（中に利用者のものがあれば残る）。
            let script = format!(
                "$o='{}'; $d='{}'; Wait-Process -Id {} -Timeout 60 -ErrorAction SilentlyContinue; \
                 for ($i = 0; $i -lt 20; $i++) {{ \
                 Remove-Item -Force -LiteralPath $o -ErrorAction SilentlyContinue; \
                 if (-not (Test-Path -LiteralPath $o)) {{ break }}; Start-Sleep -Milliseconds 500 }}; \
                 try {{ [IO.Directory]::Delete($d) }} catch {{ }}",
                quote(&old),
                quote(dir),
                std::process::id()
            );
            let spawn = |flags: u32| {
                std::process::Command::new("powershell")
                    .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .creation_flags(flags)
                    .spawn()
            };
            // ジョブ（SSH のセッションなど）が閉じても残るように切り離す。許されないジョブでは切り離さずに起こす。
            let base = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP;
            if spawn(base | CREATE_BREAKAWAY_FROM_JOB).is_err() && spawn(base).is_err() {
                log::warning(&format!(
                    "{} を消せませんでした。kikitori の終了後に削除してください。",
                    old.display()
                ));
            }
        }
    }
    #[cfg(not(windows))]
    if let Err(e) = fs::remove_file(exe) {
        log::warning(&format!("{} を消せませんでした: {e}", exe.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::remove_marked_lines;

    #[cfg(unix)]
    #[test]
    fn blob_is_removed_only_when_no_other_snapshot_uses_it() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("models--a--b");
        let (abc, def) = (repo.join("snapshots/abc"), repo.join("snapshots/def"));
        for d in [repo.join("blobs"), abc.clone(), def.clone()] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(repo.join("blobs/own"), "x").unwrap();
        std::fs::write(repo.join("blobs/shared"), "y").unwrap();
        std::os::unix::fs::symlink("../../blobs/own", abc.join("own.bin")).unwrap();
        std::os::unix::fs::symlink("../../blobs/shared", abc.join("shared.bin")).unwrap();
        std::os::unix::fs::symlink("../../blobs/shared", def.join("shared.bin")).unwrap();
        let own = super::owned_blob(&abc.join("own.bin")).unwrap();
        assert_eq!(own, std::fs::canonicalize(repo.join("blobs/own")).unwrap());
        assert!(super::owned_blob(&abc.join("shared.bin")).is_none());
    }

    #[test]
    fn only_marked_lines_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        std::fs::write(
            &rc,
            "alias a=b\nexport PATH=\"$HOME/.local/bin:$PATH\" # added by kikitori\nexport X=1\n",
        )
        .unwrap();
        remove_marked_lines(&rc);
        assert_eq!(
            std::fs::read_to_string(&rc).unwrap(),
            "alias a=b\nexport X=1\n"
        );
    }
}
