use crate::{capture, log, TranscribeArgs};
use kikitori_core::{
    pipeline::PipelineOptions, AsrEngine, CliError, Diarizer, ErrorKind, Vad, SAMPLE_RATE,
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AsrKind {
    Reazon,
    Greedy,
}

impl AsrKind {
    fn paths(self, root: &Path, model: &str, precision: &str) -> Result<[PathBuf; 4], String> {
        match self {
            Self::Reazon => Ok(kikitori_reazon::recognizer_paths(root)),
            Self::Greedy => kikitori_sherpa::recognizer_paths(root, model, precision),
        }
    }
}

/// 中核の文字起こしの結果。refine は A の区間・波形と、A に渡した余白つき波形を使う。
pub struct Transcription {
    pub segments: Vec<kikitori_core::Segment>,
    pub pcm: Vec<f32>,
    /// A の認識に渡した余白つき波形のうち、本文が空でないもの（B を同じ音声で起こすため）。
    pub a_waves: Vec<Vec<f32>>,
    /// 音声の読み込み以降（モデルの読み込みは含めない）にかかった時間。
    pub elapsed: std::time::Duration,
}

/// A の認識器を包み、渡された波形と戻り本文を控える（Python `refine/evidence.py` の Tee）。
struct Recorder {
    inner: Box<dyn AsrEngine>,
    waves: Vec<Vec<f32>>,
    texts: Vec<String>,
}

impl Recorder {
    fn new(inner: Box<dyn AsrEngine>) -> Self {
        Self {
            inner,
            waves: Vec::new(),
            texts: Vec::new(),
        }
    }

    fn kept(&mut self) -> Vec<Vec<f32>> {
        std::mem::take(&mut self.waves)
            .into_iter()
            .zip(std::mem::take(&mut self.texts))
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(wave, _)| wave)
            .collect()
    }
}

impl AsrEngine for Recorder {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn capabilities(&self) -> &kikitori_core::Capabilities {
        self.inner.capabilities()
    }
    fn transcribe(
        &mut self,
        requests: &[kikitori_core::AsrRequest<'_>],
    ) -> Result<Vec<kikitori_core::AsrOutput>, String> {
        let outputs = self.inner.transcribe(requests)?;
        self.waves = requests.iter().map(|r| r.pcm.to_vec()).collect();
        self.texts = outputs.iter().map(|o| o.text.clone()).collect();
        Ok(outputs)
    }
}

/// 中核の文字起こしを、出力を書かずに済ませる。refine から同じ経路を使う。
/// `capture_a_waves` が真なら、A の認識に渡した波形も控える（B 用）。
pub fn transcribe_core(
    args: &TranscribeArgs,
    yes: bool,
    offline: bool,
    dry_run: bool,
    capture_a_waves: bool,
) -> Result<Option<Transcription>, CliError> {
    let root = args
        .models
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(kikitori_fetch::models_root);
    let model = args.model.as_str();
    let precision = args.precision.as_str();
    let asr_name = args.asr.as_deref().unwrap_or("reazon");
    let diarizer_name = args.diarizer.as_deref().unwrap_or("pyannote");
    if diarizer_name != "pyannote" {
        return Err(CliError::new(
            ErrorKind::Usage,
            format!("未対応の話者分離エンジンです: {diarizer_name}"),
            "`--diarizer pyannote` を指定してください。",
        ));
    }
    let kind = select_asr(asr_name, model, precision)?;
    let asr_paths = kind
        .paths(&root, model, precision)
        .map_err(|message| CliError::new(ErrorKind::Usage, message, usage_hint()))?;

    if dry_run {
        log::info(&format!(
            "入力: {}",
            args.input.as_deref().unwrap_or_default()
        ));
        log::info(&format!(
            "認識: {asr_name}（--model {model}、--precision {precision}）、話者分離: {diarizer_name}"
        ));
        log::info(if models_present(&root, &asr_paths) {
            "文字起こしのモデル: 取得済み"
        } else {
            "文字起こしのモデル: 未取得（実行すると、確認してから取得する。量は `kikitori setup --dry-run` で分かる）"
        });
        if args.dictionary.is_some() {
            let dict = kikitori_fetch::dictionary_root().join("unidic-lite-1.0.8");
            log::info(if dict.join("sys.dic").is_file() {
                "辞書のモデル: 取得済み"
            } else {
                "辞書のモデル: 未取得（実行すると、確認してから取得する）"
            });
        }
        log::info("--dry-run のため、文字起こしせずに終えます");
        return Ok(None);
    }

    if !models_present(&root, &asr_paths) {
        if offline {
            return Err(CliError::new(
                ErrorKind::OfflineModel,
                "オフラインでは必要なモデルを利用できません。",
                "モデルを取得するには `kikitori setup` をオンラインで実行してください。",
            ));
        }
        if !log::confirm(yes, "文字起こしに必要なモデルを取得しますか？")? {
            return Ok(None);
        }
        kikitori_fetch::fetch_core(&root, model, false).map_err(fetch_error)?;
        if !models_present(&root, &asr_paths) {
            return Err(CliError::new(
                ErrorKind::Processing,
                "モデルを取得しましたが、必要なファイルが見つかりません。",
                "`kikitori setup` を再実行してください。",
            ));
        }
    }

    let dict_replacer = if let Some(path) = args.dictionary.as_deref() {
        let dict_root = kikitori_fetch::dictionary_root().join("unidic-lite-1.0.8");
        if !dict_root.join("sys.dic").is_file() {
            if offline {
                return Err(CliError::new(
                    ErrorKind::OfflineModel,
                    "辞書モデルがオフラインで利用できません。",
                    "`kikitori setup` をオンラインで実行してください。",
                ));
            }
            if !log::confirm(yes, "辞書モデルを取得しますか？")? {
                return Ok(None);
            }
            kikitori_fetch::fetch_dictionary(&kikitori_fetch::dictionary_root(), false)
                .map_err(fetch_error)?;
        }
        Some(
            kikitori_dict::Replacer::from_file(path, dict_root)
                .map_err(|e| {
                    CliError::new(
                        ErrorKind::Input,
                        e.to_string(),
                        "辞書ファイルと UniDic を確認してください。",
                    )
                })?
                .python_compat(args.python_compat),
        )
    } else {
        None
    };

    let punctuator = if args.no_punctuation {
        None
    } else {
        let dir = kikitori_dict::default_dictionary_dir();
        match kikitori_dict::Punctuator::new(&dir) {
            Ok(punctuator) => Some(punctuator),
            Err(e) => {
                log::warning(&format!("句読点を付けずに続けます: {e}"));
                None
            }
        }
    };

    let started = Instant::now();
    let input = args.input.as_deref().unwrap_or_default();
    let pcm = kikitori_audio::load(Path::new(input)).map_err(|e| {
        CliError::new(
            ErrorKind::Input,
            e.to_string(),
            "音声ファイルの形式と ffmpeg のインストールを確認してください。",
        )
    })?;

    let _ = kikitori_sherpa::take_diagnostics();
    let mut asr: Box<dyn AsrEngine> = match kind {
        AsrKind::Reazon => capture::capture_stderr(|| {
            let engines = (0..asr_workers(args.asr_workers, args.threads))
                .map(|_| {
                    kikitori_reazon::Reazon::new(&root, args.threads)
                        .map(|engine| Box::new(engine) as Box<dyn AsrEngine>)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Box::new(kikitori_core::pool::AsrPool::new(engines)) as Box<dyn AsrEngine>)
        }),
        // 長さでまとめて起こすので、区間を1つずつ配ると結果が変わりうる。並列にしない
        AsrKind::Greedy => capture::capture_stderr(|| {
            kikitori_sherpa::ReazonGreedy::new(&root, model, precision, args.threads)
                .map(|engine| Box::new(engine) as Box<dyn AsrEngine>)
        }),
    }
    .map_err(process_error)?;
    let mut vad = capture::capture_stderr(|| kikitori_sherpa::Silero::new(&root, args.threads))
        .map_err(process_error)?;
    let mut diarizer = capture::capture_stderr(|| {
        kikitori_sherpa::Pyannote::new(&root, args.threads, args.num_speakers, args.threshold)
    })
    .map_err(process_error)?;

    let mut replaced = 0usize;
    let mut replace = |text: &str| -> String {
        let out = dict_replacer
            .as_ref()
            .and_then(|replacer| replacer.apply(text).ok())
            .unwrap_or_else(|| text.to_owned());
        if out != text {
            replaced += 1;
        }
        match &punctuator {
            Some(punctuator) => punctuator.punctuate(&out),
            None => out,
        }
    };
    let options = PipelineOptions {
        num_speakers: args.num_speakers,
        fix_overlapping_turn_spans: !args.python_compat,
        ..PipelineOptions::default()
    };
    let (segments, a_waves) = if capture_a_waves {
        let mut recorder = Recorder::new(asr);
        let segments = capture::capture_stderr(|| {
            kikitori_core::pipeline::run(
                &pcm,
                &mut recorder,
                &mut vad as &mut dyn Vad,
                &mut diarizer as &mut dyn Diarizer,
                &options,
                (dict_replacer.is_some() || punctuator.is_some())
                    .then_some(&mut replace as &mut dyn FnMut(&str) -> String),
            )
        })
        .map_err(process_error)?;
        (segments, recorder.kept())
    } else {
        let segments = capture::capture_stderr(|| {
            kikitori_core::pipeline::run(
                &pcm,
                asr.as_mut(),
                &mut vad as &mut dyn Vad,
                &mut diarizer as &mut dyn Diarizer,
                &options,
                (dict_replacer.is_some() || punctuator.is_some())
                    .then_some(&mut replace as &mut dyn FnMut(&str) -> String),
            )
        })
        .map_err(process_error)?;
        (segments, Vec::new())
    };
    if dict_replacer.is_some() {
        // 辞書は認識結果の読みが辞書の読みと一致したときだけ置き換えるので、効いたかを知らせる
        crate::log::info(&format!(
            "辞書で {replaced} 区間を置き換えました（読みが一致した語だけを置き換えます）。"
        ));
    }
    Ok(Some(Transcription {
        segments,
        pcm,
        a_waves,
        elapsed: started.elapsed(),
    }))
}

/// 中核の文字起こしを実行して、結果を書き出す。
pub fn run(args: TranscribeArgs, yes: bool, dry_run: bool, offline: bool) -> Result<(), CliError> {
    let Some(transcription) = transcribe_core(&args, yes, offline, dry_run, false)? else {
        return Ok(());
    };
    report_diagnostics();
    let output =
        kikitori_core::emit(&transcription.segments, &args.format).map_err(process_error)?;
    use std::io::Write;
    std::io::stdout()
        .write_all(&output)
        .map_err(|e| process_error(e.to_string()))?;
    // Python 版と同じ最後の 1 行（-v が無くても出す）。機械向けの指定では summary の JSON 行になる
    crate::log::event(
        "summary",
        &format!(
            "# 音声 {:.1}s / 処理 {:.1}s / provider cpu",
            transcription.pcm.len() as f64 / SAMPLE_RATE as f64,
            transcription.elapsed.as_secs_f64()
        ),
    );
    Ok(())
}

/// sherpa-onnx などが溜めた診断をログに流す。
pub fn report_diagnostics() {
    for diagnostic in kikitori_sherpa::take_diagnostics() {
        if diagnostic.warning {
            log::warning(&diagnostic.text);
        } else {
            log::info(&diagnostic.text);
        }
    }
}

/// 認識器の数。0 なら、認識器ごとのスレッド数の合計がコア数を超えない範囲で、2 までにする。
/// 認識器を1つ足すごとにモデルの分（約 0.8GB）だけメモリが増えるので、メモリが 12GB 未満なら 1 つ。
fn asr_workers(requested: usize, threads: usize) -> usize {
    if requested > 0 {
        return requested;
    }
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    if system.total_memory() < 12_000_000_000 {
        return 1;
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / threads.max(1)).clamp(1, 2)
}

fn select_asr(name: &str, model: &str, precision: &str) -> Result<AsrKind, CliError> {
    match name {
        "reazon" if model == "ja" && precision == "fp32" => Ok(AsrKind::Reazon),
        "reazon" | "reazon-greedy" => Ok(AsrKind::Greedy),
        _ => Err(CliError::new(
            ErrorKind::Usage,
            format!("未対応の認識エンジンです: {name}"),
            usage_hint(),
        )),
    }
}

fn usage_hint() -> &'static str {
    "`--asr reazon` または `--asr reazon-greedy` を指定してください。"
}

fn models_present(root: &Path, asr_paths: &[PathBuf; 4]) -> bool {
    asr_paths.iter().all(|path| path.is_file())
        && root.join("silero_vad.onnx").is_file()
        && root
            .join("sherpa-onnx-pyannote-segmentation-3-0/model.onnx")
            .is_file()
        && root
            .join("3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx")
            .is_file()
}

fn process_error(message: impl ToString) -> CliError {
    CliError::new(
        ErrorKind::Processing,
        message.to_string(),
        "モデルと入力音声を確認して、もう一度実行してください。",
    )
}

fn fetch_error(error: kikitori_fetch::FetchError) -> CliError {
    match error {
        kikitori_fetch::FetchError::Offline(message) => CliError::new(
            ErrorKind::OfflineModel,
            message,
            "`kikitori setup` をオンラインで実行してください。",
        ),
        other if kikitori_core::error::is_storage_full(&other.to_string()) => CliError::new(
            ErrorKind::Download,
            format!("ディスクの空きが足りず、モデルを保存できません（{other}）。"),
            "ディスクを空けてから `kikitori setup` を再実行してください。置き場は環境変数 KIKITORI_MODELS で変えられます。",
        ),
        other => CliError::new(
            ErrorKind::Download,
            other.to_string(),
            "ネットワーク接続を確認して `kikitori setup` を再実行してください。",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ort_is_used_only_for_japanese_fp32() {
        assert_eq!(select_asr("reazon", "ja", "fp32").unwrap(), AsrKind::Reazon);
        assert_eq!(
            select_asr("reazon", "ja-en", "fp32").unwrap(),
            AsrKind::Greedy
        );
        assert_eq!(select_asr("reazon", "ja", "int8").unwrap(), AsrKind::Greedy);
        assert_eq!(
            select_asr("reazon-greedy", "ja", "fp32").unwrap(),
            AsrKind::Greedy
        );
        assert!(select_asr("unknown", "ja", "fp32").is_err());
    }
}
