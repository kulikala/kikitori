use crate::ModelAction;
use kikitori_core::{CliError, ErrorKind};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn run(action: ModelAction, yes: bool, dry_run: bool, offline: bool) -> Result<(), CliError> {
    match action {
        ModelAction::List => list(false).map_err(|_e| {
            CliError::new(
                ErrorKind::Processing,
                "モデル一覧の取得に失敗しました。".to_owned(),
                "`kikitori models list` を再実行してください。",
            )
        }),
        ModelAction::Path => {
            println!("{}", kikitori_fetch::models_root().display());
            println!("{}", kikitori_fetch::cache_root().display());
            println!("{}", kikitori_fetch::dictionary_root().display());
            Ok(())
        }
        ModelAction::Pull { core, refine } => pull(core, refine, yes, dry_run, offline),
        ModelAction::Remove => remove_command(yes, dry_run),
    }
}

fn list(json: bool) -> Result<(), String> {
    let root = kikitori_fetch::cache_root();
    let mut rows: Vec<serde_json::Value> = Vec::new();
    for m in kikitori_fetch::MODELS {
        let present = m.files.iter().all(|f| {
            kikitori_fetch::cached_hf(repo(m.source), commit(m.source), f.path, &root)
                .is_some_and(|p| p.is_file())
        });
        let size: u64 = m.files.iter().filter_map(|f| f.size).sum();
        rows.push(serde_json::json!({"name":m.name,"role":m.role,"source":m.source,"license":m.license,"size":size,"present":present}));
    }
    let models = kikitori_fetch::models_root();
    for a in kikitori_fetch::CORE_ASSETS {
        let present = a.is_present(&models);
        rows.push(serde_json::json!({"name":a.name,"role":a.role,"source":a.source,"license":a.license,"size":a.size,"present":present}));
    }
    rows.push(serde_json::json!({"name":"unidic-lite-1.0.8","role":"dict","source":kikitori_fetch::DICT_URL,"license":"BSD-3-Clause (UniDic)","size":kikitori_fetch::DICT_SIZE,"present":kikitori_fetch::dictionary_root().join("unidic-lite-1.0.8/sys.dic").is_file()}));
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?
        )
    } else {
        for r in rows {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                r["name"].as_str().unwrap_or(""),
                r["role"].as_str().unwrap_or(""),
                r["source"].as_str().unwrap_or(""),
                r["license"].as_str().unwrap_or(""),
                r["size"],
                r["present"]
            );
        }
    }
    Ok(())
}

fn pull(core: bool, refine: bool, yes: bool, dry_run: bool, offline: bool) -> Result<(), CliError> {
    if offline {
        // SAFETY: set before model work begins; command execution is single threaded here.
        unsafe { std::env::set_var("HF_HUB_OFFLINE", "1") };
    }
    let core = core || !refine;
    let mut items = Vec::new();
    if core {
        items.extend(kikitori_fetch::models_pull_plan());
    }
    if refine {
        items.extend(kikitori_fetch::refine_pull_plan());
    }
    let missing: Vec<_> = items.iter().filter(|item| !item.present).collect();
    let total: u64 = missing.iter().map(|item| item.size).sum();
    if offline && !missing.is_empty() {
        return Err(CliError::new(
            ErrorKind::OfflineModel,
            format!(
                "オフラインでは必要なファイルを取得できません（{}）。",
                missing[0].name
            ),
            "不足分を取得するには `kikitori models pull` を `--offline` なしで再実行してください。",
        ));
    }
    if missing.is_empty() {
        crate::log::info("必要なモデルはすでにインストールされています。");
        return Ok(());
    }
    crate::log::info(&format!(
        "{} 件のファイル（合計 {} バイト）を取得します。",
        missing.len(),
        total
    ));
    for item in &missing {
        crate::log::info(&format!(
            "{}: {} バイト（取得元: {}）",
            item.name, item.size, item.source
        ));
    }
    if dry_run {
        return Ok(());
    }
    if !crate::log::confirm(yes, "必要なモデルを取得しますか？")? {
        return Ok(());
    }
    if core {
        let mut last_reported = 0u64;
        kikitori_fetch::fetch_core_with_progress(
            &kikitori_fetch::models_root(),
            "ja",
            offline,
            |name, done, total| {
                if done.saturating_sub(last_reported) < 8 * 1024 * 1024 && total != Some(done) {
                    return;
                }
                last_reported = done;
                crate::log::progress(name, done, total)
            },
        )
        .map_err(fetch_error)?;
    }
    if refine {
        let mut last = std::collections::HashMap::<String, u64>::new();
        kikitori_fetch::fetch_refine_with_progress(offline, |name, done, total| {
            let previous = last.entry(name.to_owned()).or_default();
            if done.saturating_sub(*previous) < 8 * 1024 * 1024 && total != Some(done) {
                return;
            }
            *previous = done;
            crate::log::progress(name, done, total)
        })
        .map_err(fetch_error)?;
    }
    Ok(())
}

fn remove_command(yes: bool, dry_run: bool) -> Result<(), CliError> {
    let targets = removal_targets().map_err(processing_error)?;
    if targets.is_empty() {
        return Err(CliError::new(
            ErrorKind::Input,
            "削除対象のモデルが見つかりません。",
            "インストール済みモデルは `kikitori models list` で確認できます。",
        ));
    }
    crate::log::info(&format!(
        "記録にあるモデル {} 件を削除します。",
        targets.len()
    ));
    for path in &targets {
        crate::log::info(&path.display().to_string());
    }
    if dry_run {
        return Ok(());
    }
    if !crate::log::confirm(yes, "記録にあるモデルを削除しますか？")? {
        return Ok(());
    }
    remove().map_err(processing_error)
}

fn removal_targets() -> Result<Vec<PathBuf>, String> {
    let text = match fs::read_to_string(kikitori_fetch::receipt_path()) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let mut targets = Vec::new();
    for line in text.lines() {
        let Some((kind, value)) = line.split_once('\t') else {
            continue;
        };
        let path = PathBuf::from(value);
        let allowed = match kind {
            "models-dir" => path.starts_with(kikitori_fetch::models_root()),
            "dict" => path.starts_with(kikitori_fetch::dictionary_root()),
            "hf-repo" => {
                path.starts_with(kikitori_fetch::cache_root())
                    && path.components().any(|c| c.as_os_str() == "snapshots")
            }
            _ => false,
        };
        if allowed && path.exists() && !targets.contains(&path) {
            targets.push(path);
        }
    }
    Ok(targets)
}

fn remove() -> Result<(), String> {
    let receipt = kikitori_fetch::receipt_path();
    let text = match fs::read_to_string(&receipt) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("kikitori の記録がないため、削除できるものはありません。".into())
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut removed = 0usize;
    for line in text.lines() {
        let Some((kind, value)) = line.split_once('\t') else {
            continue;
        };
        match kind {
            "models-dir" => {
                let p = PathBuf::from(value);
                if p.starts_with(kikitori_fetch::models_root()) && remove_path(&p)? {
                    removed += 1
                }
            }
            "dict" => {
                let p = PathBuf::from(value);
                if p.starts_with(kikitori_fetch::dictionary_root()) && remove_path(&p)? {
                    removed += 1
                }
            }
            "hf-repo" => {
                let path = PathBuf::from(value);
                let is_snapshot = path.starts_with(kikitori_fetch::cache_root())
                    && path.components().any(|c| c.as_os_str() == "snapshots");
                if is_snapshot && remove_path(&path)? {
                    removed += 1;
                }
            }
            _ => {}
        }
    }
    if removed == 0 {
        return Err("削除対象のモデルが見つかりません。".into());
    }
    crate::log::info(&format!("モデルを {removed} 件削除しました。"));
    Ok(())
}
fn remove_path(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|e| e.to_string())?;
    Ok(true)
}
fn fetch_error(e: kikitori_fetch::FetchError) -> CliError {
    match e {
        kikitori_fetch::FetchError::Offline(message) => CliError::new(
            ErrorKind::OfflineModel,
            format!(
                "オフラインでは必要なファイルを取得できません（{}）。",
                message.split_whitespace().next().unwrap_or("モデル")
            ),
            "不足分を取得するには `kikitori models pull` を `--offline` なしで再実行してください。",
        ),
        other if kikitori_core::error::is_storage_full(&other.to_string()) => CliError::new(
            ErrorKind::Download,
            format!("ディスクの空きが足りず、モデルを保存できません（{other}）。"),
            "ディスクを空けてから `kikitori models pull` を再実行してください。置き場は環境変数 KIKITORI_MODELS で変えられます。",
        ),
        other => CliError::new(
            ErrorKind::Download,
            format!("モデルのダウンロードに失敗しました（{other}）。"),
            "ネットワーク接続を確認し、`kikitori models pull` を再実行してください。",
        ),
    }
}
fn processing_error(_message: String) -> CliError {
    CliError::new(
        ErrorKind::Processing,
        "モデルの処理に失敗しました。".to_owned(),
        "モデルの記録を確認し、コマンドを再実行してください。",
    )
}
fn repo(source: &str) -> &str {
    source
        .strip_prefix("hf:")
        .unwrap_or(source)
        .split('@')
        .next()
        .unwrap_or(source)
}
fn commit(source: &str) -> &str {
    source.split('@').nth(1).unwrap_or("main")
}
