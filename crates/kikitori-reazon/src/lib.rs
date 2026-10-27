//! Direct ONNX Runtime ReazonSpeech recognizer (the Python `ort_asr.py` path).
//!
//! The ONNX Runtime implementation comes from the static sherpa-onnx build, so
//! the process never carries a second copy of ONNX Runtime.

use kikitori_core::{AsrEngine, AsrOutput, AsrRequest, Capabilities, DType, Token};
use ndarray::{Array1, Array2, ArrayD, Axis};
use ort::{
    environment::Environment,
    session::Session,
    value::{DynValue, Tensor},
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MODEL: &str = "sherpa-onnx-zipformer-ja-reazonspeech-2024-08-01";
const TAG: &str = "epoch-99-avg-1";
const DEFAULT_BEAM: usize = 4;
const CACHE_LIMIT: usize = 20_000;

/// Files needed by the direct ONNX Runtime recognizer.
pub fn recognizer_paths(models: &Path) -> [PathBuf; 4] {
    let directory = models.join(MODEL);
    [
        directory.join(format!("encoder-{TAG}.onnx")),
        directory.join(format!("decoder-{TAG}.onnx")),
        directory.join(format!("joiner-{TAG}.onnx")),
        directory.join("tokens.txt"),
    ]
}

static ENVIRONMENT: OnceLock<Result<Environment, String>> = OnceLock::new();

fn environment() -> Result<&'static Environment, String> {
    ENVIRONMENT
        .get_or_init(|| {
            unsafe {
                let base = ort_sys::OrtGetApiBase();
                if base.is_null() {
                    return Err("ONNX Runtime の API を取得できませんでした。".to_owned());
                }
                let api = ((*base).GetApi)(ort_sys::ORT_API_VERSION);
                if api.is_null() {
                    return Err(format!(
                        "sherpa-onnx の ONNX Runtime は API {} に対応していません。",
                        ort_sys::ORT_API_VERSION
                    ));
                }
                if !ort::set_api((*api).clone()) {
                    return Err("ONNX Runtime の API を登録できませんでした。".to_owned());
                }
            }
            // Keep the static sherpa archive (which provides ONNX Runtime) linked.
            let _ = sherpa_onnx::version();
            ort::init()
                .with_telemetry(false)
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn session(path: &Path, threads: usize, environment: &Environment) -> Result<Session, String> {
    Session::builder(environment)
        .map_err(|error| error.to_string())?
        .with_intra_threads(threads)
        .map_err(|error| error.to_string())?
        .commit_from_file(path)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn tokens(path: &Path) -> Result<Vec<String>, String> {
    let contents =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(contents
        .lines()
        .map(|line| {
            line.rsplit_once(char::is_whitespace)
                .map_or(line, |(token, _)| token)
                .to_owned()
        })
        .collect())
}

/// ReazonSpeech zipformer transducer driven through ONNX Runtime.
pub struct Reazon {
    id: &'static str,
    encoder: Session,
    decoder: Session,
    joiner: Session,
    tokens: Vec<String>,
    beam: usize,
    cache: HashMap<(i64, i64), Vec<f32>>,
    capabilities: Capabilities,
}

impl Reazon {
    pub fn new(models: &Path, threads: usize) -> Result<Self, String> {
        let [encoder, decoder, joiner, token_file] = recognizer_paths(models);
        for path in [&encoder, &decoder, &joiner, &token_file] {
            if !path.is_file() {
                return Err(format!("モデルファイルがありません: {}", path.display()));
            }
        }
        let beam = match std::env::var("KIKITORI_BEAM") {
            Ok(value) => value
                .parse::<usize>()
                .ok()
                .filter(|beam| *beam > 0)
                .ok_or_else(|| format!("KIKITORI_BEAM が不正です: {value}"))?,
            Err(_) => DEFAULT_BEAM,
        };
        let environment = environment()?;
        let encoder = session(&encoder, threads, environment)?;
        let decoder = session(&decoder, threads, environment)?;
        let joiner = session(&joiner, threads, environment)?;
        let tokens = tokens(&token_file)?;
        Ok(Self {
            id: "reazon",
            encoder,
            decoder,
            joiner,
            tokens,
            beam,
            cache: HashMap::new(),
            capabilities: Capabilities {
                timestamps: false,
                context: false,
                // ReazonSpeech drops words from the front of longer input (20 s already loses some)
                max_seconds: 15.0,
                dtypes: &[DType::F32],
            },
        })
    }

    fn decoder_output(&mut self, context: (i64, i64)) -> Result<Vec<f32>, String> {
        if let Some(cached) = self.cache.get(&context) {
            return Ok(cached.clone());
        }
        let input = Tensor::from_array(([1, 2], vec![context.0, context.1])).map_err(string)?;
        let outputs = self
            .decoder
            .run(ort::inputs!["y" => input])
            .map_err(string)?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>().map_err(string)?;
        let width = shape[1] as usize;
        let value = data[..width].to_vec();
        if self.cache.len() > CACHE_LIMIT {
            self.cache.clear();
        }
        self.cache.insert(context, value.clone());
        Ok(value)
    }

    fn joiner_logits(
        &mut self,
        encoded: &Array2<f32>,
        decoder_outputs: &Array2<f32>,
    ) -> Result<Array2<f32>, String> {
        let outputs = self
            .joiner
            .run(ort::inputs![
                "encoder_out" => Tensor::from_array(encoded.clone()).map_err(string)?,
                "decoder_out" => Tensor::from_array(decoder_outputs.clone()).map_err(string)?,
            ])
            .map_err(string)?;
        output_f32(&outputs[0])?
            .into_dimensionality::<ndarray::Ix2>()
            .map_err(string)
    }

    fn transcribe_one(&mut self, wave: &[f32]) -> Result<String, String> {
        let features = fbank(wave)?;
        let frames = features.nrows();
        let encoded = {
            let outputs = self
                .encoder
                .run(ort::inputs![
                    "x" => Tensor::from_array(features.insert_axis(Axis(0))).map_err(string)?,
                    "x_lens" => Tensor::from_array(Array1::from_vec(vec![frames as i64])).map_err(string)?,
                ])
                .map_err(string)?;
            let encoded = output_f32(&outputs[0])?;
            let lens = outputs[1].try_extract_tensor::<i64>().map_err(string)?.1[0] as usize;
            encoded
                .index_axis(Axis(0), 0)
                .slice_move(ndarray::s![0..lens, ..])
                .to_owned()
        };

        let mut beams: Vec<((i64, i64), Vec<i64>, f64)> = vec![((0, 0), Vec::new(), 0.0)];
        for frame in 0..encoded.nrows() {
            let contexts: Vec<(i64, i64)> = beams.iter().map(|beam| beam.0).collect();
            let mut decoder_rows = Vec::with_capacity(contexts.len());
            for context in &contexts {
                decoder_rows.extend(self.decoder_output(*context)?);
            }
            let width = decoder_rows.len() / contexts.len();
            let decoder_matrix =
                Array2::from_shape_vec((contexts.len(), width), decoder_rows).map_err(string)?;
            let encoded_row = encoded.index_axis(Axis(0), frame).to_owned();
            let encoded_matrix =
                Array2::from_shape_fn((contexts.len(), encoded_row.len()), |(_, column)| {
                    encoded_row[column]
                });
            let logits = self.joiner_logits(&encoded_matrix, &decoder_matrix)?;

            let mut candidates: Vec<((i64, i64), Vec<i64>, f64)> = Vec::new();
            let mut candidate_indexes: HashMap<(i64, i64), usize> = HashMap::new();
            for (row_index, context) in contexts.iter().enumerate() {
                let (_, sequence, base) = &beams[row_index];
                let row = logits.row(row_index);
                let norm = row.iter().copied().fold(f32::NEG_INFINITY, log_add_f32);
                let mut ids: Vec<usize> = (0..row.len()).collect();
                let take = (self.beam + 1).min(ids.len());
                if take < ids.len() {
                    ids.select_nth_unstable_by(take, |&a, &b| row[b].total_cmp(&row[a]));
                }
                for &token in &ids[..take] {
                    let score = base + f64::from(row[token] - norm);
                    let (key, next) = if token == 0 {
                        (*context, sequence.clone())
                    } else {
                        let mut next = sequence.clone();
                        next.push(token as i64);
                        ((context.1, token as i64), next)
                    };
                    if let Some(&index) = candidate_indexes.get(&key) {
                        if candidates[index].2 < score {
                            candidates[index].1 = next;
                        }
                        candidates[index].2 = log_add(candidates[index].2, score);
                    } else {
                        candidate_indexes.insert(key, candidates.len());
                        candidates.push((key, next, score));
                    }
                }
            }
            candidates.sort_by(|a, b| b.2.total_cmp(&a.2));
            beams = candidates.into_iter().take(self.beam).collect();
        }

        let mut best: Option<&((i64, i64), Vec<i64>, f64)> = None;
        for beam in &beams {
            if best.is_none_or(|current| beam.2 > current.2) {
                best = Some(beam);
            }
        }
        let sequence = best.map_or(&[][..], |beam| beam.1.as_slice());
        Ok(sequence
            .iter()
            .filter_map(|&index| self.tokens.get(index as usize))
            .cloned()
            .collect::<String>()
            .replace('▁', " ")
            .trim()
            .to_owned())
    }
}

impl AsrEngine for Reazon {
    fn id(&self) -> &str {
        self.id
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn transcribe(&mut self, requests: &[AsrRequest<'_>]) -> Result<Vec<AsrOutput>, String> {
        requests
            .iter()
            .map(|request| {
                Ok(AsrOutput {
                    text: self.transcribe_one(request.pcm)?,
                    tokens: Vec::<Token>::new(),
                })
            })
            .collect()
    }
}

fn fbank(wave: &[f32]) -> Result<Array2<f32>, String> {
    if wave.is_empty() {
        return Ok(Array2::zeros((0, 80)));
    }
    let mut result = unsafe { kikitori_knf_sys::ComputeFbank(wave.as_ptr(), wave.len() as i32) };
    let length = (result.num_frames * result.num_bins) as usize;
    let frames = unsafe { std::slice::from_raw_parts(result.frames, length) }.to_vec();
    let shape = (result.num_frames as usize, result.num_bins as usize);
    unsafe { kikitori_knf_sys::DestroyFbankResult(&mut result) };
    Array2::from_shape_vec(shape, frames).map_err(string)
}

fn output_f32(value: &DynValue) -> Result<ArrayD<f32>, String> {
    let (shape, data) = value.try_extract_tensor::<f32>().map_err(string)?;
    ArrayD::from_shape_vec(
        shape.iter().map(|&size| size as usize).collect::<Vec<_>>(),
        data.to_vec(),
    )
    .map_err(string)
}

fn log_add(a: f64, b: f64) -> f64 {
    let maximum = a.max(b);
    maximum + ((a - maximum).exp() + (b - maximum).exp()).ln()
}

fn log_add_f32(a: f32, b: f32) -> f32 {
    a.max(b) + (-(a - b).abs()).exp().ln_1p()
}

fn string(error: impl ToString) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_python_model_paths() {
        let paths = recognizer_paths(Path::new("/models"));
        assert_eq!(
            paths[0],
            Path::new("/models")
                .join(MODEL)
                .join("encoder-epoch-99-avg-1.onnx")
        );
        assert_eq!(
            paths[3],
            Path::new("/models").join(MODEL).join("tokens.txt")
        );
    }

    #[test]
    fn log_add_matches_the_definition() {
        let value = log_add(0.0, 0.0);
        assert!((value - 2.0_f64.ln()).abs() < 1e-12);
        let value = log_add_f32(0.0, 0.0);
        assert!((value - 2.0_f32.ln()).abs() < 1e-6);
    }
}
