//! CPU adapters around sherpa-onnx for the Japanese transcription pipeline.

use kikitori_core::{
    pipeline::merge_turns, AsrEngine, AsrOutput, AsrRequest, Capabilities, DType, Diarizer,
    SpeakerSpan, Token, Vad, VadSpan, SAMPLE_RATE,
};
use sherpa_onnx::{
    FastClusteringConfig, OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig,
    OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    OfflineStream, OfflineTransducerModelConfig, SileroVadModelConfig, SpeakerEmbeddingExtractor,
    SpeakerEmbeddingExtractorConfig, VadModelConfig, VoiceActivityDetector,
};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const BATCH_SAMPLES: usize = 16_000 * 60;
const CHUNK_SECONDS: f64 = 1500.0;
const SEGMENTATION: &str = "sherpa-onnx-pyannote-segmentation-3-0/model.onnx";
const EMBEDDING: &str = "3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx";

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub warning: bool,
    pub text: String,
}

static DIAGNOSTICS: Mutex<Vec<Diagnostic>> = Mutex::new(Vec::new());

fn diagnostic(warning: bool, text: impl Into<String>) {
    if let Ok(mut messages) = DIAGNOSTICS.lock() {
        messages.push(Diagnostic {
            warning,
            text: text.into(),
        });
    }
}

/// Take diagnostics emitted while creating or running sherpa-onnx engines.
pub fn take_diagnostics() -> Vec<Diagnostic> {
    DIAGNOSTICS
        .lock()
        .map(|mut messages| std::mem::take(&mut *messages))
        .unwrap_or_default()
}

/// Build the three model paths for one of the bundled ReazonSpeech models.
pub fn recognizer_paths(
    models: &Path,
    model: &str,
    precision: &str,
) -> Result<[PathBuf; 4], String> {
    let (directory, tag, allowed) = match model {
        "ja" => (
            "sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01",
            "epoch-99-avg-1",
            &["fp32", "int8"][..],
        ),
        "ja-en" => (
            "sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17",
            "epoch-35-avg-1",
            &["fp32", "fp16", "int8"][..],
        ),
        _ => return Err(format!("unknown recognition model: {model}")),
    };
    if !allowed.contains(&precision) {
        return Err(format!(
            "model {model} does not support precision {precision} ({} supported)",
            allowed.join(", ")
        ));
    }
    let suffix = match precision {
        "int8" => ".int8.onnx",
        "fp16" => ".fp16.onnx",
        _ => ".onnx",
    };
    let dir = models.join(directory);
    Ok([
        dir.join(format!("encoder-{tag}{suffix}")),
        dir.join(format!("decoder-{tag}{suffix}")),
        dir.join(format!("joiner-{tag}{suffix}")),
        dir.join("tokens.txt"),
    ])
}

pub struct ReazonGreedy {
    id: &'static str,
    recognizer: OfflineRecognizer,
    capabilities: Capabilities,
}

impl ReazonGreedy {
    pub fn new(
        models: &Path,
        model: &str,
        precision: &str,
        threads: usize,
    ) -> Result<Self, String> {
        let [encoder, decoder, joiner, tokens] = recognizer_paths(models, model, precision)?;
        for path in [&encoder, &decoder, &joiner, &tokens] {
            if !path.is_file() {
                return Err(format!("model file is missing: {}", path.display()));
            }
        }
        let mut config = OfflineRecognizerConfig {
            decoding_method: Some("greedy_search".to_owned()),
            ..OfflineRecognizerConfig::default()
        };
        config.model_config = OfflineModelConfig {
            transducer: OfflineTransducerModelConfig {
                encoder: Some(encoder.display().to_string()),
                decoder: Some(decoder.display().to_string()),
                joiner: Some(joiner.display().to_string()),
            },
            tokens: Some(tokens.display().to_string()),
            num_threads: threads as i32,
            provider: Some("cpu".to_owned()),
            ..OfflineModelConfig::default()
        };
        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| "sherpa-onnx の認識器を作成できませんでした。".to_owned())?;
        Ok(Self {
            id: "reazon-greedy",
            recognizer,
            capabilities: Capabilities {
                timestamps: false,
                context: false,
                // ReazonSpeech drops words from the front of longer input
                max_seconds: 20.0,
                dtypes: &[DType::F32, DType::F16, DType::Int8],
            },
        })
    }

    fn decode_batch(
        &self,
        requests: &[AsrRequest<'_>],
        indexes: &[usize],
        output: &mut [Option<AsrOutput>],
    ) {
        let streams: Vec<OfflineStream> = indexes
            .iter()
            .map(|&index| {
                let stream = self.recognizer.create_stream();
                stream.accept_waveform(16_000, requests[index].pcm);
                stream
            })
            .collect();
        let references = streams.iter().collect::<Vec<_>>();
        self.recognizer.decode_multiple_streams(&references);
        for (&index, stream) in indexes.iter().zip(&streams) {
            let text = stream
                .get_result()
                .map_or_else(String::new, |result| result.text.trim().to_owned());
            output[index] = Some(AsrOutput {
                text,
                tokens: Vec::<Token>::new(),
            });
        }
    }
}

impl AsrEngine for ReazonGreedy {
    fn id(&self) -> &str {
        self.id
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn transcribe(&mut self, requests: &[AsrRequest<'_>]) -> Result<Vec<AsrOutput>, String> {
        let mut output: Vec<Option<AsrOutput>> = vec![None; requests.len()];
        let mut order: Vec<usize> = (0..requests.len()).collect();
        order.sort_by_key(|&index| requests[index].pcm.len());
        let mut batch = Vec::new();
        for index in order {
            let longest = requests[index].pcm.len();
            if !batch.is_empty() && (batch.len() + 1).saturating_mul(longest) > BATCH_SAMPLES {
                self.decode_batch(requests, &batch, &mut output);
                batch.clear();
            }
            batch.push(index);
        }
        if !batch.is_empty() {
            self.decode_batch(requests, &batch, &mut output);
        }
        output
            .into_iter()
            .map(|output| {
                output.ok_or_else(|| "sherpa-onnx の認識結果がありませんでした。".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()
    }
}

pub struct Silero {
    vad: VoiceActivityDetector,
}

impl Silero {
    pub fn new(models: &Path, threads: usize) -> Result<Self, String> {
        let path = models.join("silero_vad.onnx");
        if !path.is_file() {
            return Err(format!("VAD model is missing: {}", path.display()));
        }
        let config = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(path.display().to_string()),
                threshold: 0.3,
                min_silence_duration: 0.5,
                min_speech_duration: 0.1,
                window_size: 512,
                max_speech_duration: 25.0,
            },
            sample_rate: 16_000,
            num_threads: threads as i32,
            provider: Some("cpu".to_owned()),
            ..VadModelConfig::default()
        };
        let vad = VoiceActivityDetector::create(&config, 60.0)
            .ok_or_else(|| "sherpa-onnx の VAD を作成できませんでした。".to_owned())?;
        Ok(Self { vad })
    }
}

impl Vad for Silero {
    fn spans(&mut self, pcm: &[f32]) -> Result<Vec<VadSpan>, String> {
        let mut spans = Vec::new();
        for chunk in pcm.chunks_exact(512) {
            self.vad.accept_waveform(chunk);
            self.drain(&mut spans);
        }
        self.vad.flush();
        self.drain(&mut spans);
        Ok(spans)
    }
}

impl Silero {
    fn drain(&self, spans: &mut Vec<VadSpan>) {
        while let Some(segment) = self.vad.front() {
            spans.push(VadSpan {
                start: segment.start() as f64 / SAMPLE_RATE as f64,
                end: (segment.start() + i64::from(segment.n())) as f64 / SAMPLE_RATE as f64,
            });
            self.vad.pop();
        }
    }
}

pub struct Pyannote {
    diarizer: OfflineSpeakerDiarization,
    embedding: PathBuf,
    models: PathBuf,
    config: OfflineSpeakerDiarizationConfig,
    threshold: f64,
    threads: usize,
    num_speakers: i32,
}

impl Pyannote {
    pub fn new(
        models: &Path,
        threads: usize,
        num_speakers: i32,
        threshold: f64,
    ) -> Result<Self, String> {
        let segmentation = models.join(SEGMENTATION);
        let embedding = models.join(EMBEDDING);
        for path in [&segmentation, &embedding] {
            if !path.is_file() {
                return Err(format!("diarization model is missing: {}", path.display()));
            }
        }
        let threshold = if num_speakers > 0 {
            threshold.min(0.35)
        } else {
            threshold
        };
        let config = OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(segmentation.display().to_string()),
                    ..OfflineSpeakerSegmentationPyannoteModelConfig::default()
                },
                num_threads: threads as i32,
                provider: Some("cpu".to_owned()),
                ..OfflineSpeakerSegmentationModelConfig::default()
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(embedding.display().to_string()),
                num_threads: threads as i32,
                provider: Some("cpu".to_owned()),
                ..SpeakerEmbeddingExtractorConfig::default()
            },
            clustering: FastClusteringConfig {
                num_clusters: -1,
                threshold: threshold as f32,
                compute_confidence: false,
            },
            min_duration_on: 0.3,
            min_duration_off: 0.5,
        };
        let diarizer = OfflineSpeakerDiarization::create(&config)
            .ok_or_else(|| "sherpa-onnx の話者分離を作成できませんでした。".to_owned())?;
        Ok(Self {
            diarizer,
            embedding,
            models: models.to_path_buf(),
            config,
            threshold,
            threads,
            num_speakers,
        })
    }

    /// 話者分離に使ってよいスレッド数。KIKITORI_DIAR_THREADS が設定されていればそちらが勝つ。
    fn budget(&self) -> usize {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        std::env::var("KIKITORI_DIAR_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or((cores * 3 / 4).max(1))
            .clamp(1, cores.saturating_sub(1).max(1))
    }

    /// スレッドごとに 1 つ作った抽出器を使い回す。
    fn extractor(
        model: &std::path::Path,
        threads: usize,
    ) -> Result<SpeakerEmbeddingExtractor, String> {
        SpeakerEmbeddingExtractor::create(&SpeakerEmbeddingExtractorConfig {
            model: Some(model.display().to_string()),
            num_threads: threads as i32,
            provider: Some("cpu".to_owned()),
            ..SpeakerEmbeddingExtractorConfig::default()
        })
        .ok_or_else(|| "話者埋め込みモデルを作成できませんでした。".to_owned())
    }

    /// `indices` の区間の話者埋め込みを、長さ 1 に揃えて返す。
    fn embed(
        extractor: &SpeakerEmbeddingExtractor,
        pcm: &[f32],
        turns: &[SpeakerSpan],
        indices: &[usize],
    ) -> Result<Vec<Vec<f64>>, String> {
        let mut vectors = Vec::with_capacity(indices.len());
        for &index in indices {
            let turn = &turns[index];
            let (start, end) = embedding_window(turn.start, turn.end);
            let lo = ((start * SAMPLE_RATE as f64) as usize).min(pcm.len());
            let hi = ((end * SAMPLE_RATE as f64) as usize).min(pcm.len());
            let stream = extractor
                .create_stream()
                .ok_or_else(|| "話者埋め込みのストリームを作成できませんでした。".to_owned())?;
            stream.accept_waveform(16_000, &pcm[lo..hi]);
            stream.input_finished();
            let embedding = extractor
                .compute(&stream)
                .ok_or_else(|| "話者埋め込みを計算できませんでした。".to_owned())?;
            let mut vector = embedding.into_iter().map(f64::from).collect::<Vec<_>>();
            let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt() + 1e-9;
            for value in &mut vector {
                *value /= norm;
            }
            vectors.push(vector);
        }
        Ok(vectors)
    }

    fn reembed(
        &mut self,
        pcm: &[f32],
        turns: &mut [SpeakerSpan],
        plan: Option<(usize, usize)>,
    ) -> Result<Vec<SpeakerSpan>, String> {
        let keep: Vec<usize> = turns
            .iter()
            .enumerate()
            .filter_map(|(index, turn)| ((turn.end - turn.start) >= 0.4).then_some(index))
            .collect();
        // 区間を続きのままスレッドの数に分け、スレッドごとに抽出器を作って順に計算する。結果の順は変わらない
        let (workers, threads) = match plan {
            Some((workers, threads)) => (workers.max(1), threads.max(1)),
            None => reembed_plan(self.budget(), self.instance_threads()),
        };
        let workers = workers.min(keep.len().max(1));
        let chunk = keep.len().div_ceil(workers).max(1);
        let turns_ref: &[SpeakerSpan] = turns;
        let (model, threads) = (self.embedding.as_path(), threads);
        let parts: Vec<Result<Vec<Vec<f64>>, String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = keep
                .chunks(chunk)
                .map(|indices| {
                    scope.spawn(move || {
                        let extractor = Self::extractor(model, threads)?;
                        Self::embed(&extractor, pcm, turns_ref, indices)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err("話者埋め込みの計算が止まりました。".to_owned()))
                })
                .collect()
        });
        let mut vectors = Vec::with_capacity(keep.len());
        for part in parts {
            vectors.extend(part?);
        }
        let weights: Vec<f64> = keep
            .iter()
            .map(|&index| (turns[index].end - turns[index].start).min(15.0))
            .collect();
        let labels = cluster_embeddings(&vectors, &weights, self.num_speakers as usize);
        let mut by_turn = std::collections::HashMap::new();
        for (&index, &label) in keep.iter().zip(&labels) {
            by_turn.insert(index, label);
        }
        let mut previous = 0;
        for (index, turn) in turns.iter_mut().enumerate() {
            if let Some(&label) = by_turn.get(&index) {
                previous = label;
            }
            turn.speaker = previous;
        }
        let found = turns
            .iter()
            .map(|turn| turn.speaker)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        diagnostic(
            false,
            format!("話者 {found} 人（指定 {} 人）", self.num_speakers),
        );
        Ok(merge_turns(turns, 0.6))
    }

    fn diarize_single(&mut self, pcm: &[f32]) -> Result<Vec<SpeakerSpan>, String> {
        self.diarize_single_threads(pcm, self.threads, None)
    }

    /// チャンクに分けられなかったときの 1 回処理。`threads` は呼び出し元の予算に収める。
    fn diarize_single_threads(
        &mut self,
        pcm: &[f32],
        threads: usize,
        plan: Option<(usize, usize)>,
    ) -> Result<Vec<SpeakerSpan>, String> {
        let diarizer = if threads == self.threads {
            None
        } else {
            let mut config = self.config.clone();
            config.segmentation.num_threads = threads as i32;
            config.embedding.num_threads = threads as i32;
            Some(
                OfflineSpeakerDiarization::create(&config)
                    .ok_or_else(|| "sherpa-onnx の話者分離を作成できませんでした。".to_owned())?,
            )
        };
        let diarizer = diarizer.as_ref().unwrap_or(&self.diarizer);
        let turns = diarizer
            .process(pcm)
            .ok_or_else(|| "話者分離に失敗しました。".to_owned())?
            .sort_by_start_time()
            .into_iter()
            .map(|segment| SpeakerSpan {
                start: f64::from(segment.start),
                end: f64::from(segment.end),
                speaker: segment.speaker.max(0) as usize,
            })
            .collect::<Vec<_>>();
        self.finish(pcm, turns, plan)
    }

    fn finish(
        &mut self,
        pcm: &[f32],
        mut turns: Vec<SpeakerSpan>,
        plan: Option<(usize, usize)>,
    ) -> Result<Vec<SpeakerSpan>, String> {
        turns.sort_by(|left, right| {
            left.start
                .total_cmp(&right.start)
                .then(left.end.total_cmp(&right.end))
        });
        turns = merge_turns(&turns, 0.6);
        if self.num_speakers <= 0 {
            return Ok(turns);
        }
        let found = turns
            .iter()
            .map(|turn| turn.speaker)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if found <= self.num_speakers as usize && turns.len() >= self.num_speakers as usize {
            if found < self.num_speakers as usize {
                diagnostic(
                    true,
                    format!(
                        "警告: 指定の {} 人に分けられず、{found} 人になりました。",
                        self.num_speakers
                    ),
                );
            }
            return Ok(turns);
        }
        self.reembed(pcm, &mut turns, plan)
    }

    /// チャンクに分けたときに話者分離へ使うスレッド数。分けないときは None。
    pub fn parallel_threads(&self, total_seconds: f64) -> Option<usize> {
        self.plan(total_seconds)
            .map(|(workers, _, instance)| workers * instance)
    }

    /// チャンクごとの話者分離に使うスレッド数。KIKITORI_DIAR_THREADS は全体の上限のまま。
    fn instance_threads(&self) -> usize {
        std::env::var("KIKITORI_DIAR_INSTANCE_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(3)
            .max(1)
    }

    /// 環境変数を含めた設定から、チャンクに分けるときの (並列数, 目標の長さ, 1 つあたりのスレッド数) を返す。
    fn plan(&self, total: f64) -> Option<(usize, f64, usize)> {
        chunk_plan(
            total,
            self.budget(),
            self.instance_threads(),
            chunk_override(),
        )
    }

    fn diarize_chunked(
        &mut self,
        pcm: &[f32],
        speech: &[VadSpan],
        workers: usize,
        target: f64,
        instance_threads: usize,
    ) -> Result<Vec<SpeakerSpan>, String> {
        let total = pcm.len() as f64 / SAMPLE_RATE as f64;
        let speech = speech
            .iter()
            .map(|span| (span.start, span.end))
            .collect::<Vec<_>>();
        // 分けられなかったときも、全体の上限（parallel_threads）を超えないようにする
        let budget = workers * instance_threads;
        let single_threads = self.threads.min(budget.max(1)).max(1);
        let single_plan = Some((workers, instance_threads));
        let chunks = plan_chunks(total, &speech, target);
        if chunks.len() <= 1 {
            diagnostic(false, "話者分離は分割せずに 1 回で処理します。".to_owned());
            return self.diarize_single_threads(pcm, single_threads, single_plan);
        }
        diagnostic(
            false,
            format!("話者分離を {} チャンクに分けて処理します。", chunks.len()),
        );
        let mut config = self.config.clone();
        config.segmentation.num_threads = instance_threads as i32;
        config.embedding.num_threads = instance_threads as i32;
        let sample_rate = SAMPLE_RATE as f64;
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let failed = std::sync::atomic::AtomicBool::new(false);
        let processed = std::sync::atomic::AtomicUsize::new(0);
        let local: Vec<LocalTurn> = std::thread::scope(|scope| {
            let chunks: &[(f64, f64)] = &chunks;
            let config = &config;
            let next = &counter;
            let failed = &failed;
            let processed = &processed;
            let handles: Vec<_> = (0..workers.min(chunks.len()))
                .map(|_| {
                    scope.spawn(move || {
                        let mut turns = Vec::new();
                        // 1 チャンクごとに作らず、このスレッドで 1 つ作って使い回す
                        let Some(diarizer) = OfflineSpeakerDiarization::create(config) else {
                            failed.store(true, std::sync::atomic::Ordering::Relaxed);
                            return turns;
                        };
                        loop {
                            let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(&(start, end)) = chunks.get(index) else {
                                break;
                            };
                            let lo = ((start * sample_rate) as usize).min(pcm.len());
                            let hi = ((end * sample_rate) as usize).min(pcm.len());
                            // 1 秒未満のチャンクは話者分離に値する発話がないので飛ばす
                            if hi.saturating_sub(lo) < SAMPLE_RATE {
                                continue;
                            }
                            let Some(result) = diarizer.process(&pcm[lo..hi]) else {
                                failed.store(true, std::sync::atomic::Ordering::Relaxed);
                                continue;
                            };
                            processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            turns.extend(result.sort_by_start_time().into_iter().map(|segment| {
                                LocalTurn {
                                    chunk: index,
                                    speaker: segment.speaker.max(0) as usize,
                                    turn: SpeakerSpan {
                                        start: start + f64::from(segment.start),
                                        end: start + f64::from(segment.end),
                                        speaker: segment.speaker.max(0) as usize,
                                    },
                                }
                            }));
                        }
                        turns
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| match handle.join() {
                    Ok(turns) => turns,
                    Err(_) => {
                        failed.store(true, std::sync::atomic::Ordering::Relaxed);
                        Vec::new()
                    }
                })
                .collect()
        });
        if failed.load(std::sync::atomic::Ordering::Relaxed) {
            diagnostic(
                true,
                "警告: 話者分離を分けて処理できなかったため、全体を 1 回で処理します。",
            );
            return self.diarize_single_threads(pcm, single_threads, single_plan);
        }
        let processed = processed.load(std::sync::atomic::Ordering::Relaxed);
        if local.is_empty() || processed == 0 {
            diagnostic(
                true,
                "警告: 話者分離を分けて処理できなかったため、全体を 1 回で処理します。",
            );
            return self.diarize_single_threads(pcm, single_threads, single_plan);
        }
        let cuts = chunks[..chunks.len() - 1]
            .iter()
            .map(|(_, end)| format!("{end:.1} 秒"))
            .collect::<Vec<_>>()
            .join(", ");
        diagnostic(
            false,
            format!("話者分離を {processed} つに分けて並列に処理しました（区切り {cuts}）"),
        );

        // (chunk, local speaker) ごとにまとめ、代表的な区間から埋め込みを作る
        let mut grouped: std::collections::BTreeMap<(usize, usize), Vec<usize>> =
            std::collections::BTreeMap::new();
        for (index, turn) in local.iter().enumerate() {
            grouped
                .entry((turn.chunk, turn.speaker))
                .or_default()
                .push(index);
        }
        let groups = grouped.into_iter().collect::<Vec<_>>();
        let mut selected = Vec::new();
        let mut selected_indices = Vec::with_capacity(groups.len());
        let mut group_chunk = Vec::with_capacity(groups.len());
        let mut group_weights = Vec::with_capacity(groups.len());
        let mut group_of = std::collections::HashMap::new();
        // 埋め込みを作れるグループだけを結び付ける。0.4 秒以上の区間がないグループは後で埋める
        let mut link_of = Vec::with_capacity(groups.len());
        for (id, ((chunk, speaker), indices)) in groups.iter().enumerate() {
            group_of.insert((*chunk, *speaker), id);
            let chosen = select_turns(&local, indices);
            if chosen.is_empty() {
                link_of.push(None);
                continue;
            }
            link_of.push(Some(selected_indices.len()));
            let mut group = Vec::with_capacity(chosen.len());
            let mut weight = 0.0;
            for &index in &chosen {
                let turn = &local[index].turn;
                weight += (turn.end - turn.start).min(15.0);
                group.push(selected.len());
                selected.push(turn.clone());
            }
            selected_indices.push(group);
            group_chunk.push(*chunk);
            group_weights.push(weight);
        }

        let mut by_chunk: std::collections::BTreeMap<usize, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (group, &chunk) in group_chunk.iter().enumerate() {
            by_chunk.entry(chunk).or_default().push(group);
        }
        let tasks = by_chunk.into_values().collect::<Vec<_>>();
        let embedding = self.embedding.as_path();
        let threads = instance_threads;
        let selected_ref: &[SpeakerSpan] = &selected;
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let parts: Vec<Result<GroupVectors, String>> = std::thread::scope(|scope| {
            let tasks: &[Vec<usize>] = &tasks;
            let indices: &[Vec<usize>] = &selected_indices;
            let next = &counter;
            let handles: Vec<_> = (0..workers.min(tasks.len()))
                .map(|_| {
                    scope.spawn(move || {
                        let extractor = Self::extractor(embedding, threads)?;
                        let mut output = Vec::new();
                        loop {
                            let task = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(groups) = tasks.get(task) else {
                                break;
                            };
                            let mut flat = Vec::new();
                            for &group in groups {
                                flat.extend_from_slice(&indices[group]);
                            }
                            let vectors = Self::embed(&extractor, pcm, selected_ref, &flat)?;
                            let mut vectors = vectors.into_iter();
                            output.extend(groups.iter().map(|&group| {
                                let count = indices[group].len();
                                let part = vectors.by_ref().take(count).collect::<Vec<_>>();
                                (group, part)
                            }));
                        }
                        Ok(output)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err("話者埋め込みの計算が止まりました。".to_owned()))
                })
                .collect()
        });
        let mut vectors = vec![None; selected_indices.len()];
        for part in parts {
            for (group, values) in part? {
                vectors[group] = Some(values);
            }
        }
        let mut centroids = Vec::with_capacity(selected_indices.len());
        for (group, values) in vectors.into_iter().enumerate() {
            let values = values.ok_or_else(|| "話者埋め込みがありませんでした。".to_owned())?;
            let mut centroid = vec![0.0; values.first().map_or(0, Vec::len)];
            for (value, &index) in values.iter().zip(&selected_indices[group]) {
                let turn = &selected[index];
                let weight = (turn.end - turn.start).min(15.0);
                for (total, other) in centroid.iter_mut().zip(value) {
                    *total += other * weight;
                }
            }
            let sum = group_weights[group];
            if sum > 0.0 {
                for value in &mut centroid {
                    *value /= sum;
                }
            }
            let norm = centroid
                .iter()
                .map(|value| value * value)
                .sum::<f64>()
                .sqrt()
                + 1e-9;
            for value in &mut centroid {
                *value /= norm;
            }
            centroids.push(centroid);
        }

        let requested = (self.num_speakers > 0).then_some(self.num_speakers as usize);
        let labels = link_speakers(
            &centroids,
            &group_chunk,
            &group_weights,
            self.threshold,
            requested,
        );
        let mut turns = local
            .iter()
            .map(|local| {
                let group = group_of[&(local.chunk, local.speaker)];
                (local.turn.clone(), link_of[group].map(|link| labels[link]))
            })
            .collect::<Vec<_>>();
        turns.sort_by(|left, right| {
            left.0
                .start
                .total_cmp(&right.0.start)
                .then(left.0.end.total_cmp(&right.0.end))
        });
        // 話者の分からない区間は、時間順で直前の区間の話者を引き継ぐ
        let mut previous = None;
        for (_, speaker) in &mut turns {
            if speaker.is_some() {
                previous = *speaker;
            } else if previous.is_some() {
                *speaker = previous;
            }
        }
        // 先頭に分からない区間が続くときは、直後の区間の話者にする
        let mut following = None;
        for (_, speaker) in turns.iter_mut().rev() {
            if speaker.is_some() {
                following = *speaker;
            } else if following.is_some() {
                *speaker = following;
            }
        }
        let mut turns = turns
            .into_iter()
            .map(|(turn, speaker)| SpeakerSpan {
                speaker: speaker.unwrap_or(0),
                ..turn
            })
            .collect::<Vec<_>>();
        // 最初に話した人を 0 にする
        let mut rename = std::collections::HashMap::new();
        let mut next_speaker = 0;
        for turn in &mut turns {
            let label = *rename.entry(turn.speaker).or_insert_with(|| {
                let label = next_speaker;
                next_speaker += 1;
                label
            });
            turn.speaker = label;
        }
        self.finish(pcm, turns, Some((workers, instance_threads)))
    }
}

impl Diarizer for Pyannote {
    fn diarize(&mut self, pcm: &[f32]) -> Result<Vec<SpeakerSpan>, String> {
        let total = pcm.len() as f64 / SAMPLE_RATE as f64;
        let Some((workers, target, instance)) = self.plan(total) else {
            return self.diarize_single(pcm);
        };
        let speech = Silero::new(&self.models, self.threads)?.spans(pcm)?;
        self.diarize_chunked(pcm, &speech, workers, target, instance)
    }

    fn diarize_with_speech(
        &mut self,
        pcm: &[f32],
        speech: &[VadSpan],
    ) -> Result<Vec<SpeakerSpan>, String> {
        let total = pcm.len() as f64 / SAMPLE_RATE as f64;
        match self.plan(total) {
            Some((workers, target, instance)) => {
                self.diarize_chunked(pcm, speech, workers, target, instance)
            }
            None => self.diarize_single(pcm),
        }
    }
}

struct LocalTurn {
    chunk: usize,
    speaker: usize,
    turn: SpeakerSpan,
}

type GroupVectors = Vec<(usize, Vec<Vec<f64>>)>;

/// 30 秒か 8 区間まで、長い区間から埋め込み用に選ぶ。0.4 秒未満の区間は使わない。
fn select_turns(turns: &[LocalTurn], indices: &[usize]) -> Vec<usize> {
    let duration = |index: usize| turns[index].turn.end - turns[index].turn.start;
    let mut candidates = indices
        .iter()
        .copied()
        .filter(|&index| duration(index) >= 0.4)
        .collect::<Vec<_>>();
    candidates.sort_by(|&left, &right| duration(right).total_cmp(&duration(left)));
    let mut chosen = Vec::new();
    let mut total = 0.0;
    for index in candidates {
        if chosen.len() >= 8 || total >= 30.0 {
            break;
        }
        total += duration(index);
        chosen.push(index);
    }
    chosen
}

/// 長さから決まるチャンク数。コア数には依存しない。`override_seconds` があればそちらが優先。
fn chunk_count(total: f64, override_seconds: Option<f64>) -> usize {
    match override_seconds {
        Some(seconds) if seconds > 0.0 => (total / seconds).round().max(1.0) as usize,
        _ => (total / CHUNK_SECONDS).ceil().max(1.0) as usize,
    }
}

/// KIKITORI_DIAR_CHUNK の秒数。未設定や 0 以下なら None。
fn chunk_override() -> Option<f64> {
    std::env::var("KIKITORI_DIAR_CHUNK")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|seconds| *seconds > 0.0)
}

/// 音声の長さとスレッドの予算から、チャンクに分けるときの (並列数, 目標の長さ, 1 つあたりのスレッド数) を返す。
fn chunk_plan(
    total: f64,
    budget: usize,
    instance_threads: usize,
    override_seconds: Option<f64>,
) -> Option<(usize, f64, usize)> {
    let count = chunk_count(total, override_seconds);
    if count < 2 {
        return None;
    }
    let target = total / count as f64;
    let instance = instance_threads.min(budget).max(1);
    let workers = count.min((budget / instance).max(1));
    Some((workers, target, instance))
}

/// チャンクに分けないときの並列数と、1 つあたりのスレッド数。積が予算を超えない。
fn reembed_plan(budget: usize, instance_threads: usize) -> (usize, usize) {
    let instance = instance_threads.min(budget).max(1);
    ((budget / instance).max(1), instance)
}

/// 無音の中央で区切り、長さのそろったチャンクに分ける。
fn plan_chunks(total: f64, speech: &[(f64, f64)], target: f64) -> Vec<(f64, f64)> {
    let gaps = speech
        .windows(2)
        .map(|pair| (pair[0].1, pair[1].0))
        .filter(|(start, end)| end > start)
        .collect::<Vec<_>>();
    let count = ((total / target.max(f64::MIN_POSITIVE)).round() as usize).max(1);
    let step = total / count as f64;
    let mut cuts = Vec::with_capacity(count.saturating_sub(1));
    for index in 1..count {
        let ideal = index as f64 * step;
        let mut cut = None;
        // 理想の区切りから ±10%、だめなら ±25% まで広げる
        for widen in [0.1, 0.25] {
            let low = ideal - widen * step;
            let high = ideal + widen * step;
            let inside = gaps
                .iter()
                .copied()
                .filter(|&(start, end)| {
                    let middle = (start + end) / 2.0;
                    middle >= low && middle <= high
                })
                .collect::<Vec<_>>();
            if inside.is_empty() {
                continue;
            }
            // 0.5 秒以上の隙間のうち理想に最も近いもの、なければ窓で最も長いもの
            let mut pick: Option<(f64, f64)> = None;
            for &(start, end) in &inside {
                let length = end - start;
                if length < 0.5 {
                    continue;
                }
                let middle = (start + end) / 2.0;
                let distance = (middle - ideal).abs();
                let replace = match pick {
                    None => true,
                    Some((best, _)) => distance < (best - ideal).abs(),
                };
                if replace {
                    pick = Some((middle, length));
                }
            }
            cut = match pick {
                Some((middle, _)) => Some(middle),
                None => inside
                    .iter()
                    .map(|&(start, end)| ((start + end) / 2.0, end - start))
                    .max_by(|left, right| left.1.total_cmp(&right.1))
                    .map(|(middle, _)| middle),
            };
            break;
        }
        let Some(cut) = cut else {
            continue;
        };
        if cuts.last().is_some_and(|&last| cut <= last) || !cut.is_finite() {
            continue;
        }
        cuts.push(cut);
    }
    let mut chunks = Vec::with_capacity(cuts.len() + 1);
    let mut pos = 0.0;
    for cut in cuts {
        chunks.push((pos, cut));
        pos = cut;
    }
    chunks.push((pos, total));
    chunks
}

/// チャンクをまたいで同じ話者を結び付ける。しきい値は sherpa-onnx と同じコサイン距離。
fn link_speakers(
    centroids: &[Vec<f64>],
    chunk_of: &[usize],
    weights: &[f64],
    threshold: f64,
    requested: Option<usize>,
) -> Vec<usize> {
    if centroids.is_empty() {
        return Vec::new();
    }
    let mut groups: Vec<Vec<usize>> = (0..centroids.len()).map(|index| vec![index]).collect();
    let mut vectors = centroids
        .iter()
        .zip(weights)
        .map(|(centroid, weight)| {
            centroid
                .iter()
                .map(|value| value * weight)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut group_weights = weights.to_vec();
    let stop = 1.0 - threshold;
    loop {
        if let Some(count) = requested {
            if groups.len() <= count {
                break;
            }
        }
        let mut best = f64::NEG_INFINITY;
        let mut pair = None;
        for left in 0..groups.len() {
            for right in left + 1..groups.len() {
                // 同じチャンクの話者は別の人として扱う
                let chunks = groups[left]
                    .iter()
                    .map(|&group| chunk_of[group])
                    .collect::<std::collections::HashSet<_>>();
                if groups[right]
                    .iter()
                    .any(|&group| chunks.contains(&chunk_of[group]))
                {
                    continue;
                }
                let similarity = cosine(&vectors[left], &vectors[right]);
                if similarity > best {
                    best = similarity;
                    pair = Some((left, right));
                }
            }
        }
        let Some((left, right)) = pair else {
            break;
        };
        if requested.is_none() && best < stop {
            break;
        }
        let removed = groups.remove(right);
        let removed_vector = vectors.remove(right);
        let removed_weight = group_weights.remove(right);
        groups[left].extend(removed);
        for (value, other) in vectors[left].iter_mut().zip(removed_vector) {
            *value += other;
        }
        group_weights[left] += removed_weight;
    }
    let mut labels = vec![0; centroids.len()];
    for (label, members) in groups.iter().enumerate() {
        for &group in members {
            labels[group] = label;
        }
    }
    labels
}

fn embedding_window(start: f64, end: f64) -> (f64, f64) {
    let middle = (start + end) / 2.0;
    ((middle - 7.5).max(start), (middle + 7.5).min(end))
}

/// コサイン類似度。ノルムが 0 でも割れるように小さな値を足す。
fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let dot = a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let a_norm = a.iter().map(|x| x * x).sum::<f64>().sqrt() + 1e-9;
    let b_norm = b.iter().map(|x| x * x).sum::<f64>().sqrt() + 1e-9;
    dot / (a_norm * b_norm)
}

/// Average-linkage cosine clustering, matching Python's `_cluster`.
fn cluster_embeddings(vectors: &[Vec<f64>], weights: &[f64], requested: usize) -> Vec<usize> {
    if vectors.is_empty() || requested == 0 {
        return Vec::new();
    }
    let mut groups: Vec<Vec<usize>> = (0..vectors.len()).map(|index| vec![index]).collect();
    let mut centroids: Vec<Vec<f64>> = vectors
        .iter()
        .zip(weights)
        .map(|(vector, weight)| vector.iter().map(|value| value * weight).collect())
        .collect();
    let mut group_weights = weights.to_vec();
    while groups.len() > requested {
        let mut best = f64::NEG_INFINITY;
        let mut pair = None;
        for left in 0..groups.len() {
            for right in left + 1..groups.len() {
                let similarity = cosine(&centroids[left], &centroids[right]);
                if similarity > best {
                    best = similarity;
                    pair = Some((left, right));
                }
            }
        }
        let Some((left, right)) = pair else {
            break;
        };
        let removed = groups.remove(right);
        let removed_centroid = centroids.remove(right);
        let removed_weight = group_weights.remove(right);
        groups[left].extend(removed);
        for (value, other) in centroids[left].iter_mut().zip(removed_centroid) {
            *value += other;
        }
        group_weights[left] += removed_weight;
    }
    let mut labels = vec![0; vectors.len()];
    for (label, members) in groups.iter().enumerate() {
        for &index in members {
            labels[index] = label;
        }
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_paths_follow_python() {
        let paths = recognizer_paths(Path::new("/models"), "ja", "int8").unwrap();
        assert_eq!(
            paths[0],
            Path::new("/models")
                .join("sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01")
                .join("encoder-epoch-99-avg-1.int8.onnx")
        );
        assert!(recognizer_paths(Path::new("/models"), "ja", "fp16").is_err());
        let paths = recognizer_paths(Path::new("/models"), "ja-en", "fp16").unwrap();
        assert_eq!(
            paths[3],
            Path::new("/models")
                .join("sherpa-onnx-zipformer-ja-en-reazonspeech-2025-01-17")
                .join("tokens.txt")
        );
    }

    #[test]
    fn embedding_window_caps_at_fifteen_seconds() {
        assert_eq!(embedding_window(0.0, 30.0), (7.5, 22.5));
        assert_eq!(embedding_window(10.0, 16.0), (10.0, 16.0));
    }

    #[test]
    fn clustering_merges_the_closest_vectors() {
        let vectors = vec![vec![1.0, 0.0], vec![0.9, 0.1], vec![0.0, 1.0]];
        let labels = cluster_embeddings(&vectors, &[1.0, 1.0, 1.0], 2);
        assert_eq!(labels[0], labels[1]);
        assert_ne!(labels[0], labels[2]);
    }

    #[test]
    fn clustering_keeps_empty_vectors_empty() {
        assert!(cluster_embeddings(&[], &[], 2).is_empty());
    }

    #[test]
    fn chunk_plan_depends_only_on_length_and_budget() {
        assert_eq!(chunk_count(4077.0, None), 3);
        for budget in [1, 2, 3, 4, 6, 8, 9, 12, 16, 32] {
            let (_, target, _) = chunk_plan(4077.0, budget, 3, None).unwrap();
            assert!((target - 4077.0 / 3.0).abs() < 1e-9);
        }
        assert!(chunk_plan(1400.0, 12, 3, None).is_none());
        assert_eq!(chunk_plan(4077.0, 3, 3, None).unwrap().0, 1);
        assert_eq!(chunk_plan(4077.0, 9, 3, None).unwrap().0, 3);
        assert_eq!(chunk_plan(4077.0, 12, 3, None).unwrap().0, 3);
        assert_eq!(chunk_plan(7500.0, 12, 3, None).unwrap().0, 4);
    }

    #[test]
    fn chunk_override_decides_the_chunk_count() {
        assert_eq!(chunk_count(300.0, Some(100.0)), 3);
        let (workers, target, _) = chunk_plan(300.0, 12, 3, Some(100.0)).unwrap();
        assert!((target - 100.0).abs() < 1e-9);
        assert_eq!(workers, 3);
        let speech = vec![(0.0, 99.0), (101.0, 199.0), (201.0, 300.0)];
        assert_eq!(plan_chunks(300.0, &speech, target).len(), 3);
    }

    #[test]
    fn chunk_count_without_override_follows_chunk_seconds() {
        assert_eq!(chunk_count(1400.0, None), 1);
        assert!(chunk_plan(1400.0, 12, 3, None).is_none());
        assert_eq!(chunk_count(4077.0, None), 3);
        assert_eq!(chunk_count(300.0, Some(0.0)), 1);
        assert_eq!(chunk_count(300.0, Some(200.0)), 2);
    }

    #[test]
    fn reembed_plan_never_exceeds_the_budget() {
        for budget in 1..=32 {
            for instance in 1..=8 {
                let (workers, threads) = reembed_plan(budget, instance);
                assert!(
                    workers * threads <= budget,
                    "budget {budget}, instance {instance}: {workers} * {threads}"
                );
            }
        }
    }

    #[test]
    fn plan_chunks_covers_the_whole_range() {
        let speech = vec![
            (0.0, 100.0),
            (150.0, 400.0),
            (450.0, 700.0),
            (750.0, 2400.0),
        ];
        let chunks = plan_chunks(2400.0, &speech, 600.0);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.first().unwrap().0, 0.0);
        assert_eq!(chunks.last().unwrap().1, 2400.0);
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
        assert!(chunks.iter().all(|(start, end)| end > start));
    }

    #[test]
    fn plan_chunks_cuts_at_the_longest_gap_in_the_window() {
        let speech = vec![
            (0.0, 460.0),
            (540.0, 560.0),
            (720.0, 730.0),
            (730.0, 2400.0),
        ];
        let chunks = plan_chunks(2400.0, &speech, 600.0);
        assert_eq!(chunks[0], (0.0, 640.0));
    }

    #[test]
    fn plan_chunks_without_gaps_is_one_chunk() {
        assert_eq!(plan_chunks(1200.0, &[], 600.0), vec![(0.0, 1200.0)]);
        assert_eq!(
            plan_chunks(1200.0, &[(0.0, 1200.0)], 600.0),
            vec![(0.0, 1200.0)]
        );
    }

    #[test]
    fn plan_chunks_keeps_chunk_lengths_even() {
        let speech = vec![
            (0.0, 293.0),
            (297.0, 593.0),
            (597.0, 903.0),
            (907.0, 1200.0),
        ];
        let chunks = plan_chunks(1200.0, &speech, 300.0);
        assert_eq!(chunks.len(), 4);
        let lengths = chunks
            .iter()
            .map(|(start, end)| end - start)
            .collect::<Vec<_>>();
        let longest = lengths.iter().copied().fold(f64::MIN, f64::max);
        let shortest = lengths.iter().copied().fold(f64::MAX, f64::min);
        assert!(longest - shortest < 0.1 * 1200.0 / 4.0);
    }

    #[test]
    fn linking_never_merges_groups_from_the_same_chunk() {
        let centroids = vec![vec![1.0, 0.0], vec![1.0, 0.0]];
        let labels = link_speakers(&centroids, &[0, 0], &[1.0, 1.0], 0.6, None);
        assert_ne!(labels[0], labels[1]);
    }

    #[test]
    fn linking_merges_similar_vectors_across_chunks() {
        let centroids = vec![vec![1.0, 0.0], vec![0.99, 0.01]];
        let labels = link_speakers(&centroids, &[0, 1], &[1.0, 1.0], 0.6, None);
        assert_eq!(labels[0], labels[1]);
    }

    #[test]
    fn linking_threshold_stops_dissimilar_vectors() {
        let centroids = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let labels = link_speakers(&centroids, &[0, 1], &[1.0, 1.0], 0.5, None);
        assert_ne!(labels[0], labels[1]);
    }

    #[test]
    fn linking_honours_the_requested_count() {
        let centroids = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        let labels = link_speakers(&centroids, &[0, 1, 2], &[1.0, 1.0, 1.0], 0.1, Some(2));
        assert_eq!(
            labels
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            2
        );
    }
}
