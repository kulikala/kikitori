//! B（Qwen3-ASR）と K（kana-whisper）を、本文 A と同じ区間で認識する（Python `refine/evidence.py`）。
//! B は A と同じ音声（余白つきの区間）を、K は区間の前後 0.2 秒を足した音声を認識する。

use kikitori_core::{
    registry::{Registry, Weights},
    AsrEngine, AsrRequest, CliError, ErrorKind, LoadOptions,
};
use std::path::{Path, PathBuf};

pub const DEFAULT_ENGINE_B: &str = "qwen3asr";
pub const DEFAULT_ENGINE_K: &str = "kana-whisper";

pub const KANA_PAD: f64 = 0.2;
pub const SR: usize = 16_000;
pub const QWEN: &str = "neosophie/Qwen3-ASR-1.7B-JA";
pub const KANA: &str = "sbintuitions/kana-whisper";
pub const QWEN_COMMIT: &str = "987bda160f2dabfa6757550bcff7cdda2ba0648c";
pub const KANA_COMMIT: &str = "88ecb3d79c5846cb4fcf76f4107b84c8fa2acd82";

fn model_entry(role: &str) -> kikitori_fetch::Model {
    kikitori_fetch::MODELS
        .iter()
        .find(|m| m.role == role)
        .expect("refine models are pinned in kikitori_fetch::MODELS")
        .clone()
}

fn snapshot_dir(repo: &str, commit: &str, file: &'static str) -> Option<PathBuf> {
    kikitori_fetch::cached_hf(repo, commit, file, &kikitori_fetch::cache_root())
        .map(|p| {
            p.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("unknown"))
        })
        .filter(|dir| dir.is_dir())
}

/// The snapshot directory of a pinned HF model, if all its files are cached.
pub fn model_dir(role: &str) -> Option<PathBuf> {
    let m = model_entry(role);
    let (repo, commit) = match m.source.split_once('@') {
        Some((h, t)) => (h.trim_start_matches("hf:"), t),
        _ => (m.source.trim_start_matches("hf:"), ""),
    };
    let root = kikitori_fetch::cache_root();
    if !m.files.iter().all(|f| {
        kikitori_fetch::cached_hf(repo, commit, f.path, &root).is_some_and(|p| p.is_file())
    }) {
        return None;
    }
    snapshot_dir(repo, commit, m.files[0].path)
}

/// B（Qwen3-ASR）と K（kana-whisper）のモデルがどちらも置いてあるか。
pub fn models_present() -> bool {
    model_dir("refine-b").is_some() && model_dir("refine-k").is_some()
}

/// B・K の認識を動かせるか。Rust 版はどちらのエンジンも常に組み込まれている。
pub fn available() -> bool {
    true
}

/// 取得が要るモデル（1 つに付き 1 件、ファイルの不足分の合計サイズ）。
pub struct MissingModel {
    pub name: &'static str,
    pub repo: &'static str,
    pub size: u64,
}

/// まだキャッシュに無い B・K のモデル。
pub fn missing_models() -> Vec<MissingModel> {
    let root = kikitori_fetch::cache_root();
    let mut out = Vec::new();
    for role in ["refine-b", "refine-k"] {
        let m = model_entry(role);
        let Some((repo, commit)) = m.source.strip_prefix("hf:").and_then(|s| s.split_once('@'))
        else {
            continue;
        };
        let missing: u64 = m
            .files
            .iter()
            .filter(|f| {
                kikitori_fetch::cached_hf(repo, commit, f.path, &root).is_none_or(|p| !p.is_file())
            })
            .filter_map(|f| f.size)
            .sum();
        if m.files.iter().any(|f| {
            kikitori_fetch::cached_hf(repo, commit, f.path, &root).is_none_or(|p| !p.is_file())
        }) {
            out.push(MissingModel {
                name: m.name,
                repo,
                size: missing,
            });
        }
    }
    out
}

/// A の認識器の包み。本文を返しつつ、渡された音声区間を取っておく（B を同じ音声で起こすため）。
pub struct Tee<E: AsrEngine> {
    engine: Box<E>,
    waves: Vec<Vec<f32>>,
    texts: Vec<String>,
}

impl<E: AsrEngine> Tee<E> {
    pub fn new(engine: E) -> Self {
        Self {
            engine: Box::new(engine),
            waves: Vec::new(),
            texts: Vec::new(),
        }
    }

    /// pipeline.run が残した区間（本文が空でないもの）の音声。
    pub fn kept(&self) -> Vec<&[f32]> {
        self.waves
            .iter()
            .zip(&self.texts)
            .filter(|(_, t)| !t.is_empty())
            .map(|(w, _)| w.as_slice())
            .collect()
    }
}

impl<E: AsrEngine> AsrEngine for Tee<E> {
    fn id(&self) -> &str {
        self.engine.id()
    }
    fn capabilities(&self) -> &kikitori_core::Capabilities {
        self.engine.capabilities()
    }
    fn transcribe(
        &mut self,
        requests: &[AsrRequest<'_>],
    ) -> Result<Vec<kikitori_core::AsrOutput>, String> {
        let outputs = self.engine.transcribe(requests)?;
        self.waves = requests.iter().map(|r| r.pcm.to_vec()).collect();
        self.texts = outputs.iter().map(|o| o.text.clone()).collect();
        Ok(outputs)
    }
}

fn registry() -> Registry {
    Registry::new()
        .with(kikitori_qwen3asr::specs())
        .with(kikitori_whisper::specs())
}

/// B: 本文 A と同じ音声区間を別の認識で起こす。読み込んで使い終わったら手放す。
/// `progress` は 1 区間ごとに (処理済み, 全体) を受け取る。
pub fn recognize(
    role: &'static str,
    engine_name: Option<&str>,
    waves: &[&[f32]],
    options: LoadOptions,
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<String>, CliError> {
    let engine_name = engine_name
        .filter(|n| !n.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let engine = if role == "refine-b" {
                DEFAULT_ENGINE_B
            } else {
                DEFAULT_ENGINE_K
            };
            engine.to_owned()
        });
    let reg = registry();
    let Some(spec) = reg.lookup(&engine_name) else {
        return Err(CliError::new(
            ErrorKind::Usage,
            format!("認識エンジン {engine_name} はありません。"),
            "`kikitori engines list` で使えるエンジンを確認してください。",
        ));
    };
    let ok = |e: String| {
        CliError::new(
            ErrorKind::Processing,
            format!("音声認識（B・K）の処理に失敗しました: {e}"),
            "メモリやモデルの置き場を確かめてください。",
        )
    };
    let dir = model_dir(role).ok_or_else(|| {
        CliError::new(
            ErrorKind::OfflineModel,
            "B（Qwen3-ASR）と K（kana-whisper）のモデルがありません。",
            "ネットに繋がる状態で打ち直すか、`kikitori models pull --refine` で先に入れてください。",
        )
    })?;
    let mut engine = (spec.load)(&Weights { paths: vec![dir] }, &options).map_err(ok)?;
    let total = waves.len();
    let mut out = Vec::with_capacity(total);
    for (i, pcm) in waves.iter().enumerate() {
        // Python と同じく 1 区間ずつ渡す（B は language=Japanese のみ、context は渡さない）。
        let request = [AsrRequest {
            pcm,
            context: &[],
            language: Some("ja"),
            want_timestamps: false,
        }];
        let mut text = engine
            .transcribe(&request)
            .map_err(ok)?
            .into_iter()
            .next()
            .map(|o| o.text)
            .unwrap_or_default();
        // B は Python が strip する。K は batch_decode の結果をそのまま使う。
        if role == "refine-b" {
            text = text.trim().to_owned();
        }
        out.push(text);
        progress(i + 1, total);
    }
    drop(engine); // どちらも重いので、次を取りに行く前に手放す
    Ok(out)
}

/// K: 区間 samples の前後 KANA_PAD 秒を足した音声で kana-whisper に聞かせる。
pub fn kana_wave(spans: &[(f64, f64)], samples: &[f32]) -> Vec<Vec<f32>> {
    spans
        .iter()
        .map(|(a, b)| {
            let start = ((a - KANA_PAD).max(0.0) * SR as f64) as usize;
            let end = ((b + KANA_PAD) * SR as f64) as usize;
            let end = end.min(samples.len());
            samples.get(start..end).unwrap_or(&[]).to_vec()
        })
        .collect()
}
