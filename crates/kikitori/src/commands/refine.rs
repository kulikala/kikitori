use crate::RefineArgs;
use kikitori_core::{CliError, DType, DeviceChoice, ErrorKind, LoadOptions, Segment};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Instant;

use kikitori_refine::{catalog, evidence, external, local, ollama, route, setup};

fn err(kind: ErrorKind, message: impl Into<String>, hint: impl Into<String>) -> CliError {
    CliError::new(kind, message.into(), hint.into())
}

fn usage(message: impl Into<String>, hint: impl Into<String>) -> CliError {
    err(ErrorKind::Usage, message, hint)
}

fn dtype_choice(dtype: &str) -> Option<DType> {
    match dtype {
        "int8" => Some(DType::Int8),
        "fp16" => Some(DType::F16),
        "fp32" => Some(DType::F32),
        "bf16" => Some(DType::BF16),
        _ => None,
    }
}

struct EngineSetup {
    backend: &'static str,
    device: DeviceChoice,
    dtype: Option<DType>,
    label: String,
}

/// ビルドに入っている計算機（D-4: Metal → CUDA → CPU）。auto の既定を説明するのに使う。
fn built_backend() -> &'static str {
    if cfg!(all(target_os = "macos", feature = "metal")) {
        "metal"
    } else if cfg!(feature = "cuda") {
        "cuda"
    } else {
        "cpu"
    }
}

fn engine_setup(args: &RefineArgs) -> Result<EngineSetup, CliError> {
    let device = match args.device.as_str() {
        "auto" => DeviceChoice::Auto,
        "cpu" => DeviceChoice::Cpu,
        "metal" => DeviceChoice::Metal,
        "cuda" => DeviceChoice::Cuda(0),
        other => {
            return Err(usage(
                format!("--device は auto / cpu / metal / cuda のどれかです: {other}"),
                "--device auto、cpu、metal、cuda のどれかを指定してください。",
            ))
        }
    };
    let dtype = if args.dtype == "auto" {
        None
    } else {
        Some(dtype_choice(&args.dtype).ok_or_else(|| {
            usage(
                format!("--dtype の値が不正です: {}", args.dtype),
                "--dtype auto、fp16、fp32、bf16、int8 のどれかを指定してください。",
            )
        })?)
    };
    let backend = match device {
        DeviceChoice::Auto => built_backend(),
        DeviceChoice::Cpu => "cpu",
        DeviceChoice::Metal => "metal",
        DeviceChoice::Cuda(_) => "cuda",
    };
    let label = match (args.device.as_str(), args.dtype.as_str()) {
        ("auto", "auto") => format!(
            "auto（{backend} を優先） / dtype B {}、K {}",
            if backend == "cpu" { "fp32" } else { "fp16" },
            "fp32"
        ),
        ("auto", dtype) => format!("auto（{backend} を優先） / dtype {dtype}"),
        (device, "auto") => format!(
            "{device} / dtype B {}、K {}",
            if matches!(device, "metal" | "cuda") {
                "fp16"
            } else {
                "fp32"
            },
            "fp32"
        ),
        (device, dtype) => format!("{device} / dtype {dtype}"),
    };
    Ok(EngineSetup {
        backend,
        device,
        dtype,
        label,
    })
}

/// B と K は順に読み込むので、要るメモリは大きいほうの B に作業領域を足した分。
/// B（Qwen3-ASR 2.04B）は f32 で約 8.2GB、f16 で約 4.1GB。K（kana-whisper 0.81B）は f32 で約 3.0GB。
fn check_memory(backend: &str, dtype: &str) -> Result<(), CliError> {
    const GB: f64 = kikitori_refine::catalog::GB as f64;
    let half = match dtype {
        "fp16" | "bf16" => true,
        "auto" => backend != "cpu",
        _ => false,
    };
    let need = if half { 4.1 * GB } else { 8.2 * GB } + 1.5 * GB;
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let available = system.available_memory() as f64;
    if available == 0.0 || available >= need {
        return Ok(());
    }
    Err(CliError::new(
        ErrorKind::Memory,
        format!(
            "補正の認識（B・K）には約 {:.1}GB のメモリが要りますが、使えるのは約 {:.1}GB です。",
            need / GB,
            available / GB
        ),
        if half {
            "ほかのアプリを閉じてから再実行するか、そのまま続けるときは --force を付けてください。"
        } else {
            "--dtype fp16 を付けると必要なメモリが約半分になります。そのまま続けるときは --force を付けてください。"
        },
    ))
}

fn route_name(route: &str) -> &'static str {
    match route {
        "plain" => "kikitori の本文のまま",
        "external" => "外部 LLM で直す（A・B・K を渡す）",
        "merge" => "Ollama の大きなモデルで区間ごとに突き合わせ",
        "glossary" => "長さの規則＋tev1 で用語の置き換え",
        _ => "長さの規則だけ（LLM を使わない）",
    }
}

fn show(plan: &route::Plan) {
    for n in &plan.notes {
        crate::log::event("info", &format!("# {n}"));
    }
    let model = plan
        .model
        .as_deref()
        .map(|m| format!("（{m}）"))
        .unwrap_or_default();
    crate::log::event(
        "info",
        &format!(
            "# 補正の経路: {} - {}{model}",
            plan.route,
            route_name(plan.route)
        ),
    );
}

/// 既定の指定（用語集なし、外部の LLM なし）で `kikitori refine` を実行したときに選ばれる直し方（doctor が使う）
pub fn outlook() -> Result<route::Plan, CliError> {
    let at = ollama::base(None);
    let tags = ollama::models(&at, ollama::SMALL_TIMEOUT);
    let have_tags = tags.is_some();
    let hardware = if ollama::is_local(&at) {
        Some(catalog::detect(None)?)
    } else {
        None
    };
    Ok(route::choose(&route::Facts {
        evidence: evidence::available() && evidence::models_present(),
        external: "off".to_owned(),
        ollama: tags,
        systemone: have_tags && ollama::has_systemone(&at, ollama::SMALL_TIMEOUT),
        hardware,
        glossary: false,
        merge_model: std::env::var("KIKITORI_LLM").ok().filter(|m| !m.is_empty()),
        no_ollama: false,
    }))
}

fn facts(args: &RefineArgs, has_evidence: bool) -> Result<route::Facts, CliError> {
    let no_ollama = args.no_ollama || args.external != "off" || !has_evidence;
    let at = ollama::base(None);
    let tags = if no_ollama {
        None
    } else {
        ollama::models(&at, ollama::SMALL_TIMEOUT)
    };
    let here = ollama::is_local(&at);
    let hardware =
        if here || args.hardware.is_some() || std::env::var_os("KIKITORI_HARDWARE").is_some() {
            Some(catalog::detect(args.hardware.as_deref())?)
        } else {
            None
        };
    let have_tags = tags.is_some();
    Ok(route::Facts {
        evidence: has_evidence,
        external: args.external.clone(),
        ollama: tags,
        systemone: have_tags && ollama::has_systemone(&at, ollama::SMALL_TIMEOUT),
        hardware,
        glossary: args.glossary.is_some(),
        merge_model: args.merge_model.clone(),
        no_ollama: args.no_ollama,
    })
}

fn warn(msg: &str) {
    crate::log::warning(msg);
}

fn read_json_file(path: &Path, what: &str) -> Result<Value, CliError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        err(
            ErrorKind::Input,
            format!("{what} を読めません（{}）: {e}", path.display()),
            "ファイルのパスを確かめてください。",
        )
    })?;
    serde_json::from_str(&text).map_err(|e| {
        err(
            ErrorKind::Input,
            format!("{what} の JSON が読めません: {e}"),
            "--evidence / --apply で指定したファイルの形式を確かめてください。",
        )
    })
}

fn read_reply(args: &RefineArgs) -> Result<String, CliError> {
    match &args.reply {
        Some(path) => std::fs::read_to_string(path).map_err(|e| {
            err(
                ErrorKind::Input,
                format!("返ってきた答えを読めません（{path}）: {e}"),
                "ファイルのパスを確かめてください。",
            )
        }),
        None => {
            let mut out = String::new();
            std::io::stdin().read_to_string(&mut out).map_err(|e| {
                err(
                    ErrorKind::Input,
                    format!("標準入力から答えを読めません: {e}"),
                    "答えの行（[番号] 本文）を標準入力に送ってください。",
                )
            })?;
            Ok(out)
        }
    }
}

fn text_of(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn segments_from(value: &Value, what: &str) -> Result<Vec<Segment>, CliError> {
    let bad = |i: usize, key: &str| {
        err(
            ErrorKind::Input,
            format!("{what} の {i} 番の区間に {key} がありません"),
            format!("{what} の形を確かめてください。"),
        )
    };
    let list = value
        .get("segments")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    list.iter()
        .enumerate()
        .map(|(i, s)| {
            Ok(Segment {
                start: s
                    .get("start")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| bad(i, "start"))?,
                end: s
                    .get("end")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| bad(i, "end"))?,
                speaker: s.get("speaker").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
                text: text_of(&s.get("text").cloned().unwrap_or_default()),
            })
        })
        .collect()
}

fn emit_stdout(segs: &[Segment], format: &str) -> Result<(), CliError> {
    let bytes = kikitori_core::emit(segs, format).map_err(|m| {
        err(
            ErrorKind::Processing,
            format!("出力できない形式です: {m}"),
            "",
        )
    })?;
    let mut out = std::io::stdout().lock();
    out.write_all(&bytes)
        .and_then(|_| out.flush())
        .map_err(|e| {
            err(
                ErrorKind::Processing,
                format!("書き出しに失敗しました: {e}"),
                "",
            )
        })
}

fn missing_warning(miss: &[usize]) -> String {
    let shown: Vec<String> = miss.iter().take(10).map(|i| i.to_string()).collect();
    format!(
        "警告: {} 区間の答えがありませんでした。本文のまま残します（{}）",
        miss.len(),
        shown.join(", ")
    )
}

fn apply_bundle(args: &RefineArgs) -> Result<(), CliError> {
    let bundle = read_json_file(
        Path::new(args.apply.as_deref().unwrap_or_default()),
        "--apply の JSON",
    )?;
    let reply = read_reply(args)?;
    let (texts, miss): (Vec<String>, Vec<usize>) = external::apply(&bundle, &reply);
    let segs = segments_from(&bundle, "bundle")?;
    if !miss.is_empty() {
        warn(&missing_warning(&miss));
    }
    let segs: Vec<Segment> = std::iter::zip(&segs, texts)
        .map(|(s, tx)| Segment {
            text: tx,
            ..s.clone()
        })
        .collect();
    emit_stdout(&segs, &args.format)
}

#[allow(clippy::type_complexity)]
fn load_evidence(
    path: &Path,
) -> Result<(Vec<Segment>, Vec<String>, Vec<String>, Vec<String>), CliError> {
    let value = read_json_file(path, "evidence の JSON")?;
    let rows = value.as_array().cloned().unwrap_or_default();
    let mut segs = Vec::new();
    let mut a = Vec::new();
    let mut b = Vec::new();
    let mut k = Vec::new();
    for row in rows {
        let err = || {
            err(
                ErrorKind::Input,
                "evidence の行が [start, end, speaker, 本文, B, K] の形ではありません",
                "evidence の形式を確かめてください。",
            )
        };
        let items = row.as_array().filter(|r| r.len() == 6).ok_or_else(err)?;
        let start = items[0].as_f64().ok_or_else(err)?;
        let end = items[1].as_f64().ok_or_else(err)?;
        let speaker = items[2].as_u64().ok_or_else(err)? as usize;
        let text = text_of(&items[3]);
        segs.push(Segment {
            start,
            end,
            speaker,
            text: text.clone(),
        });
        a.push(text);
        b.push(text_of(&items[4]));
        k.push(text_of(&items[5]));
    }
    Ok((segs, a, b, k))
}

/// Python の `json.dumps(..., ensure_ascii=False)` と同じ区切り（`, ` と `: `、インデントなし）。
struct PythonSpacing;

impl serde_json::ser::Formatter for PythonSpacing {
    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }
    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }
    fn begin_object_value<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        writer.write_all(b": ")
    }
}

fn save_evidence(
    path: &Path,
    segs: &[Segment],
    b: &[String],
    k: &[String],
) -> Result<(), CliError> {
    let rows: Vec<Value> = segs
        .iter()
        .zip(b)
        .zip(k)
        .map(|((s, b), k)| serde_json::json!([s.start, s.end, s.speaker, s.text, b, k]))
        .collect();
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, PythonSpacing);
    serde::Serialize::serialize(&rows, &mut ser).map_err(|e| {
        err(
            ErrorKind::Processing,
            format!("evidence を JSON にできません: {e}"),
            "",
        )
    })?;
    let text = String::from_utf8(buf).unwrap_or_else(|_| "[]".to_owned());
    std::fs::write(path, text).map_err(|e| {
        err(
            ErrorKind::Processing,
            format!("evidence を保存できません（{}）: {e}", path.display()),
            "保存先のパスを確かめてください。",
        )
    })
}

fn load_glossary(args: &RefineArgs) -> Result<Vec<(String, String)>, CliError> {
    match &args.glossary {
        Some(path) => kikitori_refine::load_glossary(Path::new(path)).map_err(|e| {
            err(
                ErrorKind::Input,
                e,
                "用語集のファイルのパスを確かめてください。",
            )
        }),
        None => Ok(Vec::new()),
    }
}

fn tagger_for_glossary(glossary: &[(String, String)]) -> Result<Option<local::Tagger>, CliError> {
    if glossary.is_empty() {
        return Ok(None);
    }
    let dir = kikitori_dict::default_dictionary_dir();
    local::Tagger::load(&dir).map(Some).map_err(|e| {
        err(
            ErrorKind::Processing,
            format!(
                "用語の置き換えに必要な日本語の辞書が {} で見つかりません。",
                dir.display()
            ),
            format!("`kikitori setup` で辞書を入れてから、実行し直してください（{e}）"),
        )
    })
}

fn fetch_error(error: kikitori_fetch::FetchError) -> CliError {
    match error {
        kikitori_fetch::FetchError::Offline(message) => err(
            ErrorKind::OfflineModel,
            format!(
                "オフラインでは補助モデルを取得できません（{}）。",
                message.split_whitespace().next().unwrap_or("モデル")
            ),
            "オンラインで `kikitori models pull --refine` を実行してください。",
        ),
        other => err(
            ErrorKind::Download,
            format!("補助モデルの取得に失敗しました: {other}"),
            "接続を確かめて、実行し直してください（続きから取得されます）。",
        ),
    }
}

/// B・K のモデルが無ければ、確認してから取得する。取得しない（了承されない）なら偽。
fn ensure_refine_models(offline: bool, yes: bool) -> Result<bool, CliError> {
    let missing = evidence::missing_models();
    if missing.is_empty() {
        return Ok(true);
    }
    crate::log::info("補正に使う B（Qwen3-ASR）と K（kana-whisper）のモデルを取得します。");
    for m in &missing {
        crate::log::info(&format!(
            "{}: {} バイト（取得元: {}）",
            m.name, m.size, m.repo
        ));
    }
    if offline {
        return Err(err(
            ErrorKind::OfflineModel,
            format!("オフラインでは {} を取得できません。", missing[0].name),
            "オンラインで `kikitori models pull --refine` を実行してください。",
        ));
    }
    if !crate::log::confirm(yes, "補正に使うモデル（B・K）を取得しますか？")? {
        return Ok(false);
    }
    let mut last = std::collections::HashMap::new();
    kikitori_fetch::fetch_refine_with_progress(offline, |name, done, total| {
        let previous: &mut u64 = last.entry(name.to_owned()).or_default();
        if done.saturating_sub(*previous) < 8 * 1024 * 1024 && total != Some(done) {
            return;
        }
        *previous = done;
        crate::log::progress(name, done, total);
    })
    .map_err(fetch_error)?;
    if !evidence::models_present() {
        return Err(err(
            ErrorKind::Processing,
            "モデルを取得しましたが、必要なファイルが見つかりません。",
            "`kikitori models pull --refine` を再実行してください。",
        ));
    }
    Ok(true)
}

fn transcribe_args(args: &RefineArgs) -> crate::TranscribeArgs {
    crate::TranscribeArgs {
        input: args.input.clone(),
        setup: false,
        models: args.models.clone(),
        model: "ja".to_owned(),
        device: "cpu".to_owned(),
        precision: "fp32".to_owned(),
        threads: args.threads,
        num_speakers: args.num_speakers,
        threshold: args.threshold,
        dictionary: args.dict.clone(),
        format: "txt".to_owned(),
        asr: None,
        diarizer: None,
        python_compat: false,
        // 補正は B・K のモデルも読むので、A の認識器は1つにしてメモリを抑える
        asr_workers: 1,
    }
}

fn summary(t0: Instant, route: &str) {
    crate::log::event(
        "summary",
        &format!("処理 {:.1}s / 経路 {route}", t0.elapsed().as_secs_f64()),
    );
}

#[allow(clippy::too_many_lines)]
pub(crate) fn run(
    args: &RefineArgs,
    yes: bool,
    dry_run: bool,
    offline: bool,
) -> Result<(), CliError> {
    if args.external == "claude" && catalog::which("claude").is_none() {
        return Err(CliError::new(
            ErrorKind::Refine,
            "--external claude には claude コマンド（Claude Code）が要りますが、見つかりません。".to_owned(),
            "Claude Code を入れるか、--external bundle でプロンプトの JSON を出し、呼び出し元の LLM に渡してください。",
        ));
    }
    let setup = engine_setup(args)?;
    if args.model != "ja" {
        return Err(err(
            ErrorKind::Usage,
            "kikitori refine の補正は、日本語の会議（--model ja）だけに対応します。\
             英語が混ざる会議は kikitori --model ja-en で起こしてください（補正はできません）",
            "--model ja を指定してください。",
        ));
    }
    if args.setup {
        let st = setup::gather(args.hardware.as_deref())?;
        let steps = setup::plan_setup(&st, args.no_hf, args.merge_model.as_deref());
        let outcome = setup::run_setup(&st, &steps, yes, dry_run);
        if matches!(outcome, setup::SetupOutcome::Done) {
            show(&route::choose(&facts(args, evidence::available())?));
        }
        return setup::to_exit_code(outcome);
    }
    if args.apply.is_some() {
        return apply_bundle(args);
    }
    let glossary = load_glossary(args)?;
    let t0 = Instant::now();
    let evidence_given = args.evidence.is_some();
    let mut plan = route::choose(&facts(args, evidence_given || evidence::available())?);
    show(&plan);
    if args.plan || dry_run {
        return Ok(());
    }
    let (segs, a, b, k) = if let Some(path) = args.evidence.as_deref() {
        load_evidence(Path::new(path))?
    } else {
        if args.input.is_none() {
            return Err(err(
                ErrorKind::Input,
                "音声ファイルを指定してください",
                "`kikitori refine <音声ファイル>` を実行してください。",
            ));
        }
        let mut use_evidence = plan.route != "plain";
        if use_evidence && !ensure_refine_models(offline, yes)? {
            warn("補助モデルを取得しないため、本文のまま出します。");
            use_evidence = false;
            plan.route = "plain";
        }
        // B・K を読み込むときだけ見積もる。--plan・--evidence・入力の誤りでは止めない。
        if use_evidence && setup.backend != "cuda" && !args.force {
            check_memory(setup.backend, &args.dtype)?;
        }
        let transcription = crate::commands::transcribe::transcribe_core(
            &transcribe_args(args),
            yes,
            offline,
            false,
            use_evidence,
        )?;
        let Some(transcription) = transcription else {
            return Ok(());
        };
        crate::commands::transcribe::report_diagnostics();
        if !use_evidence {
            emit_stdout(&transcription.segments, &args.format)?;
            summary(t0, plan.route);
            return Ok(());
        }
        let a: Vec<String> = transcription
            .segments
            .iter()
            .map(|s| s.text.clone())
            .collect();
        if transcription.a_waves.len() != a.len() {
            return Err(err(
                ErrorKind::Processing,
                format!(
                    "本文の区間数 {} と、B に渡す音声の数 {} が合いません。",
                    a.len(),
                    transcription.a_waves.len()
                ),
                "音声とモデルを確かめて、もう一度実行してください。",
            ));
        }
        let spans: Vec<(f64, f64)> = transcription
            .segments
            .iter()
            .map(|s| (s.start, s.end))
            .collect();
        let k_waves = evidence::kana_wave(&spans, &transcription.pcm);
        crate::log::info(&format!("B・K の計算機: {}", setup.label));
        let b_refs: Vec<&[f32]> = transcription.a_waves.iter().map(Vec::as_slice).collect();
        let b = evidence::recognize(
            "refine-b",
            args.engine_b.as_deref(),
            &b_refs,
            LoadOptions {
                device: setup.device,
                dtype: setup.dtype,
            },
            |done, total| crate::log::progress("B（Qwen3-ASR）", done as u64, Some(total as u64)),
        )?;
        // B のエンジンは recognize の中で手放してから K を読み込む（I-28）。
        let k_refs: Vec<&[f32]> = k_waves.iter().map(Vec::as_slice).collect();
        let k = evidence::recognize(
            "refine-k",
            args.engine_k.as_deref(),
            &k_refs,
            LoadOptions {
                device: setup.device,
                dtype: setup.dtype,
            },
            |done, total| {
                crate::log::progress("K（kana-whisper）", done as u64, Some(total as u64))
            },
        )?;
        (transcription.segments, a, b, k)
    };
    if plan.route == "plain" {
        emit_stdout(&segs, &args.format)?;
        summary(t0, plan.route);
        return Ok(());
    }
    // Python は `--evidence` で読み直した場合も、指定があれば保存し直す。
    if let Some(path) = args.save_evidence.as_deref() {
        save_evidence(Path::new(path), &segs, &b, &k)?;
    }
    let texts: Vec<String> = match plan.route {
        "external" => {
            let bun = external::bundle(&segs, &a, &b, &k, &glossary);
            if args.external == "bundle" {
                let dumped = external::dumps(&bun).map_err(|m| {
                    err(
                        ErrorKind::Processing,
                        format!("bundle の出力に失敗しました: {m}"),
                        "",
                    )
                })?;
                println!("{dumped}");
                return Ok(());
            }
            let mut log = |m: &str| crate::log::info(m);
            let users: Vec<String> = bun["chunks"]
                .as_array()
                .map(|cs| -> Vec<String> {
                    cs.iter()
                        .filter_map(|c| {
                            c.get("user")
                                .and_then(|u| u.as_str())
                                .map(ToOwned::to_owned)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let reply =
                external::claude(bun["system"].as_str().unwrap_or_default(), &users, &mut log);
            let (texts, miss) = external::apply(&bun, &reply);
            if !miss.is_empty() {
                warn(&missing_warning(&miss));
            }
            texts
        }
        "merge" => {
            let at = ollama::base(None);
            let mut log = |m: &str| crate::log::info(m);
            local::merge(
                &at,
                plan.model.as_deref().unwrap_or_default(),
                &a,
                &b,
                &k,
                &glossary,
                &mut log,
            )
        }
        "glossary" => {
            let mut texts = local::length_rule(&a, &b);
            let tagger = tagger_for_glossary(&glossary)?;
            if let Some(tagger) = &tagger {
                let at = ollama::base(None);
                let mut log = |m: &str| crate::log::info(m);
                texts = local::glossary_fix(
                    tagger,
                    &at,
                    plan.model.as_deref().unwrap_or_default(),
                    &texts,
                    &k,
                    &glossary,
                    &mut log,
                );
            }
            texts
        }
        _ => local::length_rule(&a, &b),
    };
    let segs: Vec<Segment> = std::iter::zip(&segs, texts)
        .map(|(s, tx)| Segment {
            text: tx,
            ..s.clone()
        })
        .collect();
    emit_stdout(&segs, &args.format)?;
    summary(t0, plan.route);
    Ok(())
}
