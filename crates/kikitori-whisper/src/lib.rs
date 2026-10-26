//! kana-whisper Candle engine using the HF-exact Whisper frontend.

#[cfg(all(feature = "accelerate", target_os = "macos"))]
extern crate accelerate_src;
mod decoder;
mod kkmel;
mod multilingual;
mod profile;
mod whisper_model;

use anyhow::{bail, Result};
use candle::{DType as CandleDType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{self as m, Config};
use kikitori_core::{
    AsrEngine, AsrOutput, AsrRequest, Capabilities, DType, DeviceChoice, LoadOptions, Token,
};
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::{fs, path::Path, sync::Arc};
use tokenizers::Tokenizer;

const DTYPES: &[DType] = &[DType::F32, DType::F16, DType::BF16];

pub(crate) enum Model {
    Normal(whisper_model::Whisper),
}
impl Model {
    pub fn config(&self) -> &Config {
        match self {
            Self::Normal(x) => &x.config,
        }
    }
    pub fn encoder_forward(&mut self, x: &Tensor, flush: bool) -> candle::Result<Tensor> {
        match self {
            Self::Normal(model) => model.encoder.forward(x, flush),
        }
    }
    pub fn decoder_forward(
        &mut self,
        x: &Tensor,
        xa: &Tensor,
        flush: bool,
    ) -> candle::Result<Tensor> {
        match self {
            Self::Normal(m) => m.decoder.forward(x, xa, flush),
        }
    }
    pub fn decoder_final_linear(&self, x: &Tensor) -> candle::Result<Tensor> {
        match self {
            Self::Normal(m) => m.decoder.final_linear(x),
        }
    }
    pub fn reset_kv_cache(&mut self) {
        match self {
            Self::Normal(m) => m.reset_kv_cache(),
        }
    }
}

pub struct Engine {
    model: Option<Model>,
    tokenizer: Tokenizer,
    filters: Vec<f32>,
    device: Device,
    dtype: CandleDType,
    language: Option<u32>,
    capabilities: Capabilities,
    pool: Arc<ThreadPool>,
}
impl Engine {
    pub fn load(dir: impl AsRef<Path>, options: LoadOptions, threads: usize) -> Result<Self> {
        let dir = dir.as_ref();
        for file in ["config.json", "tokenizer.json", "model.safetensors"] {
            if !dir.join(file).is_file() {
                bail!("missing {}", dir.join(file).display());
            }
        }
        let config: Config = serde_json::from_slice(&fs::read(dir.join("config.json"))?)?;
        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let device = choose_device(options.device)?;
        let dtype = options
            .dtype
            .map(to_candle)
            .transpose()?
            .unwrap_or(CandleDType::F32);
        let weights = dir.join("model.safetensors");
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], dtype, &device)? };
        let model = Model::Normal(whisper_model::Whisper::load(&vb, config.clone())?);
        let mel_bytes: &[u8] = match config.num_mel_bins {
            80 => include_bytes!("melfilters.bytes"),
            128 => include_bytes!("melfilters128.bytes"),
            n => bail!("unexpected num_mel_bins {n}"),
        };
        let mut filters = vec![0f32; mel_bytes.len() / 4];
        <byteorder::LittleEndian as byteorder::ByteOrder>::read_f32_into(mel_bytes, &mut filters);
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads.max(1))
            .build()?;
        Ok(Self {
            model: Some(model),
            tokenizer,
            filters,
            device,
            dtype,
            language: None,
            capabilities: Capabilities {
                timestamps: false,
                context: false,
                max_seconds: 30.0,
                dtypes: DTYPES,
            },
            pool: Arc::new(pool),
        })
    }
    fn transcribe_one(&mut self, pcm: &[f32]) -> Result<String> {
        let config = self.model.as_ref().expect("loaded model").config().clone();
        if let Some(model) = self.model.as_mut() {
            model.reset_kv_cache();
        }
        let t = profile::start();
        let mel = kkmel::log_mel(pcm, &self.filters, config.num_mel_bins);
        profile::add("mel", t);
        let frames = mel.len() / config.num_mel_bins;
        let t = profile::start();
        let mel = Tensor::from_vec(mel, (1, config.num_mel_bins, frames), &self.device)?
            .to_dtype(self.dtype)?;
        profile::add("mel_tensor", t);
        if self.language.is_none() {
            let t = profile::start();
            self.language = Some(multilingual::detect_language(
                self.model.as_mut().expect("loaded model"),
                &self.tokenizer,
                &mel,
            )?);
            profile::add("language_detect", t);
        }
        let model = self.model.take().expect("loaded model");
        let mut decoder = decoder::Decoder::new(
            model,
            self.tokenizer.clone(),
            299_792_458,
            &self.device,
            self.language,
            false,
            None,
        )?;
        let t = profile::start();
        let result = decoder.run(&mel).map(|segments| {
            segments
                .iter()
                .map(|s| s.dr.text.clone())
                .collect::<Vec<_>>()
                .join(" ")
        });
        profile::add("decoder_run", t);
        self.model = Some(decoder.into_model());
        if profile::dump_on_each_clip() {
            profile::dump_and_reset("clip");
        }
        result
    }
}
impl AsrEngine for Engine {
    fn id(&self) -> &str {
        "kana-whisper"
    }
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    fn transcribe(
        &mut self,
        requests: &[AsrRequest<'_>],
    ) -> std::result::Result<Vec<AsrOutput>, String> {
        let pool = self.pool.clone();
        pool.install(|| {
            requests
                .iter()
                .map(|r| {
                    self.transcribe_one(r.pcm).map(|text| AsrOutput {
                        text,
                        tokens: Vec::<Token>::new(),
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .map_err(|e| e.to_string())
    }
}

pub(crate) fn token_id(tokenizer: &Tokenizer, token: &str) -> candle::Result<u32> {
    tokenizer
        .token_to_id(token)
        .ok_or_else(|| candle::Error::Msg(format!("no token-id for {token}")))
}

fn to_candle(d: DType) -> Result<CandleDType> {
    match d {
        DType::F16 => Ok(CandleDType::F16),
        DType::BF16 => Ok(CandleDType::BF16),
        DType::F32 => Ok(CandleDType::F32),
        DType::Int8 => bail!("int8 weights are not supported by this engine"),
    }
}
// 機能（metal・cuda）の有無で枝が消えるので、早めの return を使う
#[allow(clippy::needless_return)]
fn choose_device(choice: DeviceChoice) -> Result<Device> {
    match choice {
        DeviceChoice::Cpu => Ok(Device::Cpu),
        DeviceChoice::Auto => {
            #[cfg(all(target_os = "macos", feature = "metal"))]
            if let Ok(Ok(d)) = kikitori_core::probe::catch(|| Device::new_metal(0)) {
                return Ok(d);
            }
            #[cfg(feature = "cuda")]
            if let Ok(Ok(d)) = kikitori_core::probe::catch(|| Device::new_cuda(0)) {
                return Ok(d);
            }
            Ok(Device::Cpu)
        }
        DeviceChoice::Metal => {
            #[cfg(all(target_os = "macos", feature = "metal"))]
            {
                return kikitori_core::probe::catch(|| Device::new_metal(0))
                    .map_err(|_| anyhow::anyhow!("Metal initialization panicked"))?
                    .map_err(Into::into);
            }
            #[cfg(not(all(target_os = "macos", feature = "metal")))]
            bail!("Metal support was not built for this platform")
        }
        DeviceChoice::Cuda(n) => {
            #[cfg(feature = "cuda")]
            {
                return kikitori_core::probe::catch(|| Device::new_cuda(n))
                    .map_err(|_| anyhow::anyhow!("CUDA initialization panicked"))?
                    .map_err(Into::into);
            }
            #[cfg(not(feature = "cuda"))]
            {
                let _ = n;
                bail!("CUDA support was not built")
            }
        }
    }
}

pub fn specs() -> &'static [kikitori_core::registry::EngineSpec] {
    use kikitori_core::registry::{EngineSpec, Weights};
    fn load(w: &Weights, o: &LoadOptions) -> Result<Box<dyn AsrEngine>, String> {
        let dir = w
            .paths
            .first()
            .ok_or_else(|| "no model directory supplied".to_owned())?;
        Engine::load(dir, *o, 4)
            .map(|e| Box::new(e) as Box<dyn AsrEngine>)
            .map_err(|e| e.to_string())
    }
    const FILES: &[&str] = &["config.json", "tokenizer.json", "model.safetensors"];
    const SPECS: &[EngineSpec] = &[EngineSpec {
        name: "kana-whisper",
        hf_repo: "sbintuitions/kana-whisper",
        files: FILES,
        load,
    }];
    SPECS
}

#[cfg(test)]
fn wav16(path: &Path) -> Result<Vec<f32>> {
    let b = fs::read(path)?;
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let n = u32::from_le_bytes(b[i + 4..i + 8].try_into()?) as usize;
        if id == b"data" {
            return Ok(b[i + 8..(i + 8 + n).min(b.len())]
                .chunks_exact(2)
                .map(|x| i16::from_le_bytes([x[0], x[1]]) as f32 / 32768.0)
                .collect());
        }
        i += 8 + n + (n & 1);
    }
    bail!("WAV has no data chunk")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn engine_contract() {
        assert!(DTYPES.contains(&DType::F32));
    }
    #[test]
    #[ignore]
    fn cpu_f32_matches_reference_clips() {
        integration::run();
    }
    mod integration {
        use super::*;
        pub fn run() {
            let model = std::env::var("KIKITORI_TEST_KANA_DIR").expect("KIKITORI_TEST_KANA_DIR");
            let clips = std::env::var("KIKITORI_TEST_CLIPS").expect("KIKITORI_TEST_CLIPS");
            let reference: std::collections::BTreeMap<String, String> = serde_json::from_slice(
                &fs::read(
                    std::path::Path::new(
                        &std::env::var("KIKITORI_TEST_REF_DIR").expect("KIKITORI_TEST_REF_DIR"),
                    )
                    .join("kana_cpu.json"),
                )
                .unwrap(),
            )
            .unwrap();
            let mut engine = Engine::load(
                model,
                LoadOptions {
                    device: DeviceChoice::Cpu,
                    dtype: Some(DType::F32),
                },
                std::env::var("KIKITORI_TEST_THREADS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(4),
            )
            .unwrap();
            let mut paths: Vec<_> = fs::read_dir(clips)
                .unwrap()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "wav"))
                .collect();
            paths.sort();
            if let Ok(limit) = std::env::var("KIKITORI_TEST_LIMIT") {
                let limit: usize = limit.parse().expect("KIKITORI_TEST_LIMIT");
                paths.truncate(limit);
            }
            let expected = paths.len();
            let mut exact = 0;
            let mut mismatches = Vec::new();
            for p in &paths {
                let pcm = crate::wav16(p).unwrap();
                let ctx = [];
                let req = AsrRequest {
                    pcm: &pcm,
                    context: &ctx,
                    language: None,
                    want_timestamps: false,
                };
                let actual = engine.transcribe(&[req]).unwrap().remove(0).text;
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if reference.get(&name) == Some(&actual) {
                    exact += 1;
                } else {
                    mismatches.push(format!(
                        "{name}: expected {:?}, got {actual:?}",
                        reference.get(&name)
                    ));
                }
            }
            eprintln!("Exact matches: {exact}/{}", paths.len());
            assert_eq!(exact, expected, "mismatches: {mismatches:?}");
        }
    }
}
