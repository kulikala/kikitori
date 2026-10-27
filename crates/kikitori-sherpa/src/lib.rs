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
                threshold: if num_speakers > 0 {
                    threshold.min(0.35) as f32
                } else {
                    threshold as f32
                },
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
            threads,
            num_speakers,
        })
    }

    /// `indices` の区間の話者埋め込みを、長さ 1 に揃えて返す。
    fn embed(
        model: &std::path::Path,
        threads: usize,
        pcm: &[f32],
        turns: &[SpeakerSpan],
        indices: &[usize],
    ) -> Result<Vec<Vec<f64>>, String> {
        let extractor = SpeakerEmbeddingExtractor::create(&SpeakerEmbeddingExtractorConfig {
            model: Some(model.display().to_string()),
            num_threads: threads as i32,
            provider: Some("cpu".to_owned()),
            ..SpeakerEmbeddingExtractorConfig::default()
        })
        .ok_or_else(|| "話者埋め込みモデルを作成できませんでした。".to_owned())?;
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
    ) -> Result<Vec<SpeakerSpan>, String> {
        let keep: Vec<usize> = turns
            .iter()
            .enumerate()
            .filter_map(|(index, turn)| ((turn.end - turn.start) >= 0.4).then_some(index))
            .collect();
        // 区間を続きのままスレッドの数に分け、スレッドごとに抽出器を作って順に計算する。結果の順は変わらない
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let workers = (cores / self.threads.max(1))
            .clamp(1, 4)
            .min(keep.len().max(1));
        let chunk = keep.len().div_ceil(workers).max(1);
        let turns_ref: &[SpeakerSpan] = turns;
        let (model, threads) = (self.embedding.as_path(), self.threads);
        let parts: Vec<Result<Vec<Vec<f64>>, String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = keep
                .chunks(chunk)
                .map(|indices| {
                    scope.spawn(move || Self::embed(model, threads, pcm, turns_ref, indices))
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
}

impl Diarizer for Pyannote {
    fn diarize(&mut self, pcm: &[f32]) -> Result<Vec<SpeakerSpan>, String> {
        let mut turns = self
            .diarizer
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
        self.reembed(pcm, &mut turns)
    }
}

fn embedding_window(start: f64, end: f64) -> (f64, f64) {
    let middle = (start + end) / 2.0;
    ((middle - 7.5).max(start), (middle + 7.5).min(end))
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
                let a = &centroids[left];
                let b = &centroids[right];
                let dot = a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
                let a_norm = a.iter().map(|x| x * x).sum::<f64>().sqrt() + 1e-9;
                let b_norm = b.iter().map(|x| x * x).sum::<f64>().sqrt() + 1e-9;
                let similarity = dot / (a_norm * b_norm);
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
}
