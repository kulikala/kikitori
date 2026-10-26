use kikitori_core::{CliError, ErrorKind};

pub fn run(yes: bool, dry_run: bool, offline: bool) -> Result<(), CliError> {
    if offline {
        // Keep libraries which consult the standard Hugging Face switch in sync with the CLI.
        // SAFETY: this is set before any command work starts and is never mutated concurrently.
        unsafe { std::env::set_var("HF_HUB_OFFLINE", "1") };
    }
    let plan = kikitori_fetch::setup_plan();
    let missing: Vec<_> = plan.iter().filter(|item| !item.present).collect();
    let total: u64 = missing.iter().map(|item| item.size).sum();
    if offline && !missing.is_empty() {
        return Err(CliError::new(
            ErrorKind::OfflineModel,
            format!(
                "オフラインでは必要なファイルを取得できません（{}）。",
                missing[0].name
            ),
            "不足分を取得するには `kikitori setup` を `--offline` なしで再実行してください。",
        ));
    }
    if missing.is_empty() {
        crate::log::info("必要なモデルと辞書はすでにインストールされています。");
        return Ok(());
    }
    crate::log::info(&format!(
        "{} 件（合計 {} バイト）を取得します。",
        missing.len(),
        total
    ));
    for item in &missing {
        crate::log::info(&format!(
            "{}: {} バイト（取得元: {}）",
            item.name, item.size, item.source
        ));
    }
    // 圧縮したまま落としてから展開するので、取得する大きさの 2 倍を見込む
    let need = total.saturating_mul(2);
    let root = kikitori_fetch::models_root();
    if let Some(free) = kikitori_refine::setup::free_space(&root) {
        if free < need {
            return Err(CliError::new(
                ErrorKind::Download,
                format!(
                    "{} のあるディスクの空きが {:.1}GB で、取得と展開に約 {:.1}GB 要ります。",
                    root.display(),
                    free as f64 / 1e9,
                    need as f64 / 1e9
                ),
                "ディスクを空けてから `kikitori setup` を再実行してください。置き場は環境変数 KIKITORI_MODELS で変えられます。",
            ));
        }
    }
    if dry_run {
        return Ok(());
    }
    if !crate::log::confirm(yes, "必要なモデルと辞書を取得しますか？")? {
        return Ok(());
    }
    let mut last_reported = std::collections::HashMap::<String, u64>::new();
    let mut on_progress = |name: &str, done: u64, total: Option<u64>| {
        let previous = last_reported.entry(name.to_owned()).or_default();
        if done.saturating_sub(*previous) < 8 * 1024 * 1024 && total != Some(done) {
            return;
        }
        *previous = done;
        crate::log::progress(name, done, total);
    };
    kikitori_fetch::fetch_core_with_progress(
        &kikitori_fetch::models_root(),
        "ja",
        offline,
        &mut on_progress,
    )
    .map_err(fetch_error)?;
    kikitori_fetch::fetch_dictionary_with_progress(
        &kikitori_fetch::dictionary_root(),
        offline,
        |done, total| on_progress("unidic-lite-1.0.8", done, total),
    )
    .map_err(fetch_error)?;
    crate::log::info(&format!(
        "モデルを {} にインストールしました。",
        kikitori_fetch::models_root().display()
    ));
    Ok(())
}

fn fetch_error(error: kikitori_fetch::FetchError) -> CliError {
    match error {
        kikitori_fetch::FetchError::Offline(message) => CliError::new(
            ErrorKind::OfflineModel,
            format!(
                "オフラインでは必要なファイルを取得できません（{}）。",
                message.split_whitespace().next().unwrap_or("モデル")
            ),
            "取得するには `kikitori setup` を `--offline` なしで再実行してください。",
        ),
        other if kikitori_core::error::is_storage_full(&other.to_string()) => CliError::new(
            ErrorKind::Download,
            format!("ディスクの空きが足りず、モデルを保存できません（{other}）。"),
            "ディスクを空けてから `kikitori setup` を再実行してください。置き場は環境変数 KIKITORI_MODELS で変えられます。",
        ),
        other => CliError::new(
            ErrorKind::Download,
            format!("モデルまたは辞書のダウンロードに失敗しました（{other}）。"),
            "ネットワーク接続を確認し、`kikitori setup` を再実行してください。",
        ),
    }
}
