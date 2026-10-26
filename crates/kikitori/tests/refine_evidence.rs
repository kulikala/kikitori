//! `kikitori refine --save-evidence` の B・K が、同じ区間を engines に直接渡した結果と
//! 一致することを確かめる（区間の切り方と順番）。重いので ignored。手元で
//! `cargo test --release -p kikitori --test refine_evidence -- --ignored --nocapture`
//! のように走らせる。顧客の録音は使わず、合成の synth.wav だけを使う。

use kikitori_core::{
    pipeline::PipelineOptions, AsrEngine, AsrOutput, AsrRequest, Capabilities, DType, DeviceChoice,
    LoadOptions, Segment, Token,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

/// 試作の置き場（KIKITORI_TEST_PROTO、無ければ ~/work/kikitori-proto）の中の、合成会議と Python 版の出力
fn proto(rel: &str) -> std::path::PathBuf {
    std::env::var_os("KIKITORI_TEST_PROTO")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| kikitori_core::paths::home_dir().join("work/kikitori-proto"))
        .join(rel)
}

struct DummyAsr {
    waves: Vec<Vec<f32>>,
    capabilities: Capabilities,
}

impl DummyAsr {
    fn new() -> Self {
        Self {
            waves: Vec::new(),
            capabilities: Capabilities {
                timestamps: false,
                context: false,
                max_seconds: 30.0,
                dtypes: &[DType::F32],
            },
        }
    }
}

impl AsrEngine for DummyAsr {
    fn id(&self) -> &str {
        "dummy"
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn transcribe(&mut self, requests: &[AsrRequest<'_>]) -> Result<Vec<AsrOutput>, String> {
        self.waves = requests.iter().map(|r| r.pcm.to_vec()).collect();
        Ok(requests
            .iter()
            .map(|_| AsrOutput {
                text: "dummy".to_owned(),
                tokens: Vec::<Token>::new(),
            })
            .collect())
    }
}

fn real_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn models_root() -> PathBuf {
    std::env::var_os("KIKITORI_TEST_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| real_home().join(".cache/kikitori/models"))
}

fn real_hub() -> PathBuf {
    std::env::var_os("KIKITORI_TEST_HF_HUB")
        .map(PathBuf::from)
        .unwrap_or_else(|| real_home().join(".cache/huggingface/hub"))
}

fn audio() -> PathBuf {
    std::env::var_os("KIKITORI_TEST_SYNTH")
        .map(PathBuf::from)
        .unwrap_or_else(|| proto("ref-py021/audio/synth.wav"))
}

#[cfg(unix)]
fn symlink_dir(source: &Path, destination: &Path) {
    std::os::unix::fs::symlink(source, destination).unwrap();
}

#[cfg(windows)]
fn symlink_dir(source: &Path, destination: &Path) {
    std::os::windows::fs::symlink_dir(source, destination).unwrap();
}

fn link_repo(hub: &Path, name: &str, real_hub: &Path) {
    let source = real_hub.join(name);
    assert!(
        source.is_dir(),
        "Hugging Face のキャッシュがありません: {}",
        source.display()
    );
    symlink_dir(&source, &hub.join(name));
}

fn engine_dir(hub: &Path, repo: &str, commit: &str) -> PathBuf {
    kikitori_fetch::cached_hf(repo, commit, "config.json", hub)
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| panic!("{repo}@{commit} の snapshot がありません"))
}

fn run_engine(
    mut engine: Box<dyn AsrEngine>,
    waves: &[Vec<f32>],
    strip: bool,
) -> (Vec<String>, std::time::Duration) {
    let started = Instant::now();
    let mut texts = Vec::with_capacity(waves.len());
    for pcm in waves {
        let request = [AsrRequest {
            pcm,
            context: &[],
            language: Some("ja"),
            want_timestamps: false,
        }];
        let text = engine.transcribe(&request).unwrap().remove(0).text;
        texts.push(if strip { text.trim().to_owned() } else { text });
    }
    (texts, started.elapsed())
}

#[test]
#[ignore = "loads the 4GB Qwen3-ASR and 3GB kana-whisper models on CPU"]
fn refine_evidence_matches_direct_engine_cuts() {
    let audio = audio();
    assert!(audio.is_file(), "音声がありません: {}", audio.display());
    let models = models_root();
    let hub_source = real_hub();
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let hf = dir.path().join("hf");
    let hub = hf.join("hub");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&hub).unwrap();
    link_repo(&hub, "models--neosophie--Qwen3-ASR-1.7B-JA", &hub_source);
    link_repo(&hub, "models--sbintuitions--kana-whisper", &hub_source);

    let evidence_path = dir.path().join("evidence.json");
    let child = Instant::now();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kikitori"))
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("HF_HOME", &hf)
        .env("KIKITORI_MODELS", &models)
        .env("KIKITORI_OLLAMA", "http://127.0.0.1:9")
        .env("KIKITORI_LOG_FORMAT", "json")
        .args(["--log-format", "json", "refine"])
        .arg(&audio)
        .args([
            "--save-evidence",
            evidence_path.to_str().unwrap(),
            "--external",
            "bundle",
            "--no-ollama",
            "--offline",
            "--device",
            "cpu",
            "--dtype",
            "fp32",
        ])
        .output()
        .unwrap();
    let child_elapsed = child.elapsed();
    assert!(
        output.status.success(),
        "refine failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(&evidence_path).unwrap()).unwrap();
    assert!(!rows.is_empty(), "evidence is empty");
    let evidence: Vec<(f64, f64, String, String)> = rows
        .iter()
        .map(|row| {
            let items = row.as_array().unwrap();
            assert_eq!(items.len(), 6, "evidence row: {row}");
            (
                items[0].as_f64().unwrap(),
                items[1].as_f64().unwrap(),
                items[3].as_str().unwrap().to_owned(),
                items[4].as_str().unwrap().to_owned(),
            )
        })
        .collect();

    // A (Python 0.2.1 の基準出力) と区間がそろっているか。
    if let Ok(bytes) = fs::read(proto("ref-py021/synth.cpu.json")) {
        let reference: Vec<Segment> = serde_json::from_slice(&bytes).unwrap();
        let texts: Vec<&str> = evidence
            .iter()
            .map(|(_, _, text, _)| text.as_str())
            .collect();
        let expected: Vec<&str> = reference.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, expected, "A が Python 0.2.1 の出力と違います");
        for ((start, end, _, _), segment) in evidence.iter().zip(&reference) {
            assert!((start - segment.start).abs() < 1e-9, "start mismatch");
            assert!((end - segment.end).abs() < 1e-9, "end mismatch");
        }
    }

    // 同じ切り方で B の波形を作り直す（pipeline の余白つき波形。ASR の結果には依存しない）。
    let pcm = kikitori_audio::load(&audio).unwrap();
    let mut asr = DummyAsr::new();
    let mut vad = kikitori_sherpa::Silero::new(&models, 4).unwrap();
    let mut diarizer = kikitori_sherpa::Pyannote::new(&models, 4, -1, 0.6).unwrap();
    let pieces_started = Instant::now();
    let pieces = kikitori_core::pipeline::run(
        &pcm,
        &mut asr,
        &mut vad,
        &mut diarizer,
        &PipelineOptions {
            num_speakers: -1,
            fix_overlapping_turn_spans: true,
            ..PipelineOptions::default()
        },
        None,
    )
    .unwrap();
    let pieces_elapsed = pieces_started.elapsed();
    assert_eq!(pieces.len(), asr.waves.len());

    let mut b_waves = Vec::with_capacity(evidence.len());
    let mut k_spans = Vec::with_capacity(evidence.len());
    for (start, end, _, _) in &evidence {
        let index = pieces
            .iter()
            .position(|piece| (piece.start - start).abs() < 1e-9 && (piece.end - end).abs() < 1e-9)
            .unwrap_or_else(|| panic!("区間 {start}..{end} が pipeline の区切りにありません"));
        b_waves.push(asr.waves[index].clone());
        k_spans.push((*start, *end));
    }

    let b_dir = engine_dir(
        &hub_source,
        kikitori_refine::evidence::QWEN,
        kikitori_refine::evidence::QWEN_COMMIT,
    );
    let b_engine = kikitori_qwen3asr::Engine::load(
        &b_dir,
        LoadOptions {
            device: DeviceChoice::Cpu,
            dtype: Some(DType::F32),
        },
        4,
    )
    .unwrap();
    let (b_texts, b_elapsed) = run_engine(Box::new(b_engine), &b_waves, true);
    let b_expected: Vec<&str> = evidence.iter().map(|(_, _, _, b)| b.as_str()).collect();
    let b_actual: Vec<&str> = b_texts.iter().map(String::as_str).collect();
    assert_eq!(b_actual, b_expected, "B の本文が直接実行と一致しません");

    let k_waves = kikitori_refine::evidence::kana_wave(&k_spans, &pcm);
    let k_dir = engine_dir(
        &hub_source,
        kikitori_refine::evidence::KANA,
        kikitori_refine::evidence::KANA_COMMIT,
    );
    let k_engine = kikitori_whisper::Engine::load(
        &k_dir,
        LoadOptions {
            device: DeviceChoice::Cpu,
            dtype: Some(DType::F32),
        },
        4,
    )
    .unwrap();
    let (k_texts, k_elapsed) = run_engine(Box::new(k_engine), &k_waves, false);
    let k_expected: Vec<&str> = rows
        .iter()
        .map(|row| row.as_array().unwrap()[5].as_str().unwrap())
        .collect();
    let k_actual: Vec<&str> = k_texts.iter().map(String::as_str).collect();
    assert_eq!(k_actual, k_expected, "K の本文が直接実行と一致しません");

    eprintln!(
        "refine evidence: {} 区間 / CLI {:.1}s / pipeline {:.1}s / B {:.1}s / K {:.1}s",
        evidence.len(),
        child_elapsed.as_secs_f64(),
        pieces_elapsed.as_secs_f64(),
        b_elapsed.as_secs_f64(),
        k_elapsed.as_secs_f64()
    );
}
