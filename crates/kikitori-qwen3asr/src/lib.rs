//! Qwen3-ASR 1.7B Japanese Candle engine.
mod audio;
mod encoder;
mod text;

use anyhow::{bail, Result};
use candle_core::{DType as CandleDType, Device, Tensor};
use candle_nn::VarBuilder;
use kikitori_core::{
    AsrEngine, AsrOutput, AsrRequest, Capabilities, DType, DeviceChoice, LoadOptions, Token,
};
use rayon::{ThreadPool, ThreadPoolBuilder};
use serde_json::Value;
use std::{fs, path::Path, sync::Arc};
use tokenizers::{AddedToken, Tokenizer};

const AUDIO_PAD: u32 = 151676;
const ASR_TEXT: u32 = 151704;
const EOS: [u32; 2] = [151643, 151645];
const DTYPES: &[DType] = &[DType::F16, DType::F32, DType::BF16];

pub struct Engine {
    encoder: encoder::AudioEncoder,
    decoder: text::TextModel,
    tokenizer: Tokenizer,
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
        let _config: Value = serde_json::from_slice(&fs::read(dir.join("config.json"))?)?;
        let tokenizer = load_tokenizer(dir)?;
        let device = choose_device(options.device)?;
        let dtype = options
            .dtype
            .map(to_candle)
            .transpose()?
            .unwrap_or_else(|| match device_kind(&device) {
                DeviceKind::Cpu => CandleDType::F32,
                DeviceKind::Accelerated => CandleDType::F16,
            });
        let weights = dir.join("model.safetensors");
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], dtype, &device)? };
        let encoder = encoder::AudioEncoder::load(vb.clone(), &device)?;
        let decoder = text::TextModel::load(vb, &device)?;
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads.max(1))
            .build()?;
        Ok(Self {
            encoder,
            decoder,
            tokenizer,
            capabilities: Capabilities {
                timestamps: false,
                context: true,
                max_seconds: 30.0,
                dtypes: DTYPES,
            },
            pool: Arc::new(pool),
        })
    }

    fn run_one(&mut self, pcm: &[f32], context: &[String]) -> Result<String> {
        self.decoder.clear_cache();
        let frames = audio::frame_count(pcm);
        let mel = audio::log_mel(pcm);
        let encoded = self.encoder.forward(&mel, frames)?;
        let prompt = build_prompt(&self.tokenizer, &encoded, &self.decoder, context)?;
        let seq = prompt.dim(1)?;
        let mut logits = self
            .decoder
            .forward_embeds(prompt, 0)?
            .squeeze(0)?
            .squeeze(0)?;
        let mut generated = Vec::new();
        for i in 0..512 {
            let id = logits.argmax(0)?.to_scalar::<u32>()?;
            if EOS.contains(&id) {
                break;
            }
            generated.push(id);
            logits = self
                .decoder
                .forward_ids(&[id], seq + i)?
                .squeeze(0)?
                .squeeze(0)?;
        }
        Ok(self
            .tokenizer
            .decode(&generated, true)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .trim()
            .to_owned())
    }
}

impl AsrEngine for Engine {
    fn id(&self) -> &str {
        "qwen3asr"
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
                    self.run_one(r.pcm, r.context).map(|text| AsrOutput {
                        text,
                        tokens: Vec::<Token>::new(),
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .map_err(|e| e.to_string())
    }
}

fn load_tokenizer(dir: &Path) -> Result<Tokenizer> {
    let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let p = dir.join("added_tokens.json");
    if p.exists() {
        let extras: std::collections::BTreeMap<String, u32> =
            serde_json::from_slice(&fs::read(p)?)?;
        let mut extras: Vec<_> = extras.into_iter().collect();
        extras.sort_by_key(|(_, id)| *id);
        let mut next = tokenizer.get_vocab_size(true) as u32;
        for (token, id) in extras {
            if id < next {
                continue;
            }
            if id != next {
                bail!("non-contiguous tokenizer additions: {token} has id {id}, expected {next}");
            }
            tokenizer.add_special_tokens(&[AddedToken::from(token.as_str(), true)]);
            if tokenizer.token_to_id(&token) != Some(id) {
                bail!("tokenizer addition id mismatch for {token}");
            }
            next += 1;
        }
    }
    Ok(tokenizer)
}

fn prompt_text(context: &[String]) -> String {
    let context = context.join(", ");
    format!("<|im_start|>system\n{context}<|im_end|>\n<|im_start|>user\n<|audio_start|><|audio_pad|><|audio_end|><|im_end|>\n<|im_start|>assistant\nlanguage Japanese")
}

fn build_prompt(
    tokenizer: &Tokenizer,
    audio: &Tensor,
    model: &text::TextModel,
    context: &[String],
) -> Result<Tensor> {
    let prompt = prompt_text(context);
    let mut ids = tokenizer
        .encode(prompt, false)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .get_ids()
        .to_vec();
    ids.push(ASR_TEXT);
    let mut parts = Vec::new();
    let mut inserted = false;
    for id in ids {
        if id == AUDIO_PAD {
            if inserted {
                bail!("prompt unexpectedly contains multiple audio placeholders");
            }
            parts.push(audio.clone());
            inserted = true;
        } else {
            parts.push(model.embed(&[id])?);
        }
    }
    if !inserted {
        bail!("tokenizer did not preserve <|audio_pad|> as a special token");
    }
    Ok(Tensor::cat(&parts, 1)?)
}

#[derive(Clone, Copy)]
enum DeviceKind {
    Cpu,
    Accelerated,
}
fn device_kind(d: &Device) -> DeviceKind {
    if matches!(d, Device::Cpu) {
        DeviceKind::Cpu
    } else {
        DeviceKind::Accelerated
    }
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
                use anyhow::Context;
                return kikitori_core::probe::catch(|| Device::new_metal(0))
                    .map_err(|_| anyhow::anyhow!("Metal initialization panicked"))?
                    .context("initialize Metal");
            }
            #[cfg(not(all(target_os = "macos", feature = "metal")))]
            bail!("Metal support was not built for this platform")
        }
        DeviceChoice::Cuda(n) => {
            #[cfg(feature = "cuda")]
            {
                use anyhow::Context;
                return kikitori_core::probe::catch(|| Device::new_cuda(n))
                    .map_err(|_| anyhow::anyhow!("CUDA initialization panicked"))?
                    .context("initialize CUDA");
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
        Engine::load(dir, *o, o.threads.unwrap_or(4))
            .map(|e| Box::new(e) as Box<dyn AsrEngine>)
            .map_err(|e| e.to_string())
    }
    const FILES: &[&str] = &["config.json", "tokenizer.json", "model.safetensors"];
    const SPEC: &[EngineSpec] = &[EngineSpec {
        name: "qwen3asr",
        hf_repo: "neosophie/Qwen3-ASR-1.7B-JA",
        files: FILES,
        load,
    }];
    SPEC
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_match_engine_contract() {
        let caps = Capabilities {
            timestamps: false,
            context: true,
            max_seconds: 30.0,
            dtypes: DTYPES,
        };
        assert!(!caps.timestamps);
        assert!(caps.context);
        assert_eq!(caps.max_seconds, 30.0);
        assert!(caps.dtypes.contains(&DType::F32));
    }
    #[test]
    fn prompt_matches_python_system_context_layout() {
        assert_eq!(prompt_text(&[]), "<|im_start|>system\n<|im_end|>\n<|im_start|>user\n<|audio_start|><|audio_pad|><|audio_end|><|im_end|>\n<|im_start|>assistant\nlanguage Japanese");
        assert_eq!(prompt_text(&["Kikitori".into(), "音声認識".into()]), "<|im_start|>system\nKikitori, 音声認識<|im_end|>\n<|im_start|>user\n<|audio_start|><|audio_pad|><|audio_end|><|im_end|>\n<|im_start|>assistant\nlanguage Japanese");
    }
    #[test]
    #[ignore]
    fn cpu_f32_matches_reference_clips() {
        super::integration::run();
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    pub fn run() {
        let model = std::env::var("KIKITORI_TEST_QWEN_DIR").expect("KIKITORI_TEST_QWEN_DIR");
        let clips = std::env::var("KIKITORI_TEST_CLIPS").expect("KIKITORI_TEST_CLIPS");
        let reference: std::collections::BTreeMap<String, String> = serde_json::from_slice(
            &fs::read(
                std::path::Path::new(
                    &std::env::var("KIKITORI_TEST_REF_DIR").expect("KIKITORI_TEST_REF_DIR"),
                )
                .join("qwen_cpu.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let mut engine = Engine::load(
            model,
            LoadOptions {
                device: DeviceChoice::Cpu,
                dtype: Some(DType::F32),
                threads: None,
            },
            4,
        )
        .unwrap();
        let mut paths: Vec<std::path::PathBuf> = fs::read_dir(clips)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "wav"))
            .collect();
        paths.sort();
        let mut exact = 0;
        let mut mismatches = Vec::new();
        for path in &paths {
            let pcm = audio::read_wav(path).unwrap();
            let context = Vec::new();
            let request = AsrRequest {
                pcm: &pcm,
                context: &context,
                language: None,
                want_timestamps: false,
            };
            let actual = engine.transcribe(&[request]).unwrap().remove(0).text;
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
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
        assert_eq!(exact, 38, "mismatches: {mismatches:?}");
        eprintln!("Qwen CPU f32 exact matches: {exact}/{}", paths.len());
    }
}

/// その計算機を、この実行ファイルで実際に初期化できるか（doctor が使う）
pub fn device_works(choice: DeviceChoice) -> bool {
    choose_device(choice).is_ok()
}
