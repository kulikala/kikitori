//! Python 版（kikitori-py021 の tests/test_refine.py）の移植と、Python から作った基準出力との一致。
//! 基準データ: tests/fixtures/{prompts_golden,merge_golden}.json（gen_golden.py が作る）。

use std::collections::BTreeMap;
use std::sync::Mutex;

use kikitori_core::{ErrorKind, Segment};
use kikitori_refine::catalog::{self, Hardware, GB};
use kikitori_refine::external;
use kikitori_refine::local;
use kikitori_refine::ollama;
use kikitori_refine::prompts;
use kikitori_refine::route::{self, Facts};

fn fixture(name: &str) -> serde_json::Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {path}: {e}"));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn glossary(v: &serde_json::Value) -> Vec<(String, String)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p[0].as_str().unwrap().to_owned(),
                p[1].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

// --- 指示文と問い（fixtures/prompts_golden.json） ---

#[test]
fn prompts_match_python_golden() {
    let g = fixture("prompts_golden.json");
    let empty: Vec<(String, String)> = Vec::new();
    let lab = glossary(&serde_json::json!([["北山精機", ""]]));
    assert_eq!(prompts::system("merge", &lab), g["merge_system"]);
    assert_eq!(prompts::system("fixev", &empty), g["fixev_system"]);
    let lab2 = glossary(&serde_json::json!([["北山精機", "得意先"]]));
    assert_eq!(prompts::system("merge", &lab2), g["glossary_system"]);

    let a = strings(&serde_json::json!([
        "あ",
        "いA",
        "うB",
        "ええ",
        "おおおお",
        "かかか"
    ]));
    let b = strings(&serde_json::json!([
        "アア", "いB", "ウB", "エB", "オB", "きK"
    ]));
    let k = strings(&serde_json::json!(["ア", "カ", "ウ", "エ", "オ", "キ"]));
    assert_eq!(
        prompts::user("merge", &[2, 3, 4], &a, &b, &k),
        g["merge_user_3id"].as_str().unwrap()
    );
    assert_eq!(
        prompts::user(
            "fixev",
            &[0],
            &strings(&serde_json::json!(["電話しましたか？"])),
            &strings(&serde_json::json!(["電話しましたか"])),
            &strings(&serde_json::json!(["デンワシマシタカ"]))
        ),
        g["fixev_user_1id"].as_str().unwrap()
    );
}

#[test]
fn parse_and_chunks_match_python_golden() {
    let g = fixture("prompts_golden.json");
    let got = prompts::parse("説明の行\n[0] こんにちは\n  [12]  さようなら \n[x] 無視");
    let want: BTreeMap<usize, String> = g["parse_numbered"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.parse().unwrap(), v.as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(got, want);

    // Python の正規表現は [番号] のみがある行で、間の改行を「本文」に飲み込む（そのままの癖）
    let got = prompts::parse("[0]\n[1] foo");
    let want: BTreeMap<usize, String> = g["parse_eat_newline"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.parse().unwrap(), v.as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(got, want);

    let got = prompts::parse("[1]\n[9]佐藤");
    let want: BTreeMap<usize, String> = g["parse_no_space"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.parse().unwrap(), v.as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(got, want);

    let want: Vec<Vec<usize>> = g["chunks_t2"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            c.as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as usize)
                .collect()
        })
        .collect();
    assert_eq!(prompts::chunks(5, 2), want);
    let want: Vec<Vec<usize>> = g["chunks_t3_p2"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            c.as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as usize)
                .collect()
        })
        .collect();
    assert_eq!(prompts::chunks(7, 3), want);
}

// --- bundle / apply ---

#[test]
fn bundle_bytes_match_python_golden() {
    let g = fixture("prompts_golden.json");
    let segs = vec![
        Segment {
            start: 0.0,
            end: 1.0,
            speaker: 0,
            text: "あ".into(),
        },
        Segment {
            start: 1.0,
            end: 2.0,
            speaker: 1,
            text: "い".into(),
        },
    ];
    let b = external::bundle(
        &segs,
        &["あ".to_owned(), "い".to_owned()],
        &["ア".to_owned(), "イ".to_owned()],
        &["ア".to_owned(), "イ".to_owned()],
        &[("北山精機".to_owned(), "".to_owned())],
    );
    let dumped = external::dumps(&b).unwrap();
    assert_eq!(dumped, g["bundle_text"]);
    // 形の確認（test_bundle_apply_round_trip_keeps_missing_as_a）
    assert!(b["system"].as_str().unwrap().contains("用語集: 北山精機"));
    assert_eq!(b["chunks"][0]["ids"], serde_json::json!([0, 1]));
    let reply = "[1] いい\n";
    let (texts, miss) = external::apply(&b, reply);
    assert_eq!(texts, vec!["あ".to_owned(), "いい".to_owned()]);
    assert_eq!(miss, vec![0]);
}

// --- 長さの規則 ---

#[test]
fn length_rule_takes_b_only_when_much_longer() {
    let a = strings(&serde_json::json!(["はい", "今日は晴れです", ""]));
    let b = strings(&serde_json::json!([
        "この商品は自社製品です",
        "今日は晴れです。明日も",
        "あ"
    ]));
    let want = strings(&fixture("prompts_golden.json")["length_rule"]);
    assert_eq!(local::length_rule(&a, &b), want);
}

// --- 用語の候補（UniDic が要るものは KIKITORI_TEST_UNIDIC で渡す） ---

#[test]
fn glossary_candidates_follow_sound() {
    let Some(dir) = std::env::var_os("KIKITORI_TEST_UNIDIC") else {
        eprintln!("skipping: KIKITORI_TEST_UNIDIC is unset");
        return;
    };
    let tagger = local::Tagger::load(std::path::Path::new(&dir)).unwrap();
    let gl = vec![("北山精機".to_owned(), "得意先".to_owned())];
    let g = fixture("prompts_golden.json");
    let opts = local::candidates(
        &tagger,
        "北山正気さんの工場",
        "キタヤマセーキサンノコージョー",
        &gl,
    );
    let want: Vec<(String, String)> = g["candidates_lab_1"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p[0].as_str().unwrap().to_owned(),
                p[1].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(opts, want);
    let opts = local::candidates(&tagger, "北山正気さんの工場", "アイウエオ", &gl);
    assert!(opts.is_empty());
}

// --- 経路（routes） ---

fn hw(kind: &'static str, gb: u64) -> Hardware {
    Hardware {
        kind,
        memory: Some(gb * GB),
        why: String::new(),
    }
}

fn map(models: &[(&str, u64)]) -> Option<BTreeMap<String, u64>> {
    Some(models.iter().map(|(k, v)| (k.to_string(), *v)).collect())
}

const BIG: Option<BTreeMap<String, u64>> = None; // map を使う

macro_rules! facts {
    ($($field:ident : $value:expr),* $(,)?) => {
        Facts { $($field: $value,)* ..Default::default() }
    };
}

#[test]
fn route_without_evidence_is_plain_even_if_external() {
    let f = facts!(evidence: false, external: "bundle".to_owned());
    assert_eq!(route::choose(&f).route, "plain");
}

#[test]
fn route_external_skips_local_models() {
    let f = facts!(
        evidence: true,
        external: "claude".to_owned(),
        ollama: map(&[("qwen3.8:27b-mlx", 18 * GB), ("tev1:0.8b-mxfp8", GB)]),
        systemone: true,
        glossary: true,
        hardware: Some(hw("apple", 128))
    );
    let p = route::choose(&f);
    assert_eq!((p.route, p.model.as_deref()), ("external", None));
}

#[test]
fn route_merge_when_big_model_fits() {
    let f = facts!(
        evidence: true,
        ollama: map(&[("qwen3.8:27b-mlx", 18 * GB), ("tev1:0.8b-mxfp8", GB)]),
        hardware: Some(hw("apple", 32))
    );
    let p = route::choose(&f);
    assert_eq!(
        (p.route, p.model.as_deref()),
        ("merge", Some("qwen3.8:27b-mlx"))
    );
}

#[test]
fn route_merge_on_remote_ollama_uses_any_catalog_model() {
    let f = facts!(
        evidence: true,
        ollama: map(&[("qwen3.8:27b-q4_K_M", 18 * GB), ("tev1:0.8b-q8_0", GB)]),
        hardware: None
    );
    assert_eq!(
        route::choose(&f).model.as_deref(),
        Some("qwen3.8:27b-q4_K_M")
    );
}

#[test]
fn route_small_mac_uses_tev1_glossary() {
    let f = facts!(
        evidence: true,
        ollama: map(&[("qwen3.8:27b-mlx", 18 * GB), ("tev1:0.8b-mxfp8", GB)]),
        systemone: true,
        glossary: true,
        hardware: Some(hw("apple", 16))
    );
    let p = route::choose(&f);
    assert_eq!(
        (p.route, p.model.as_deref()),
        ("glossary", Some("tev1:0.8b-mxfp8"))
    );
}

#[test]
fn route_ignores_models_for_other_hardware() {
    // Apple Silicon の PC に GGUF 版しか無ければ使わない（セットアップが選ぶものと同じ表を見る）
    let f = facts!(
        evidence: true,
        ollama: map(&[("qwen3.8:27b-q4_K_M", 18 * GB), ("tev1:0.8b-q8_0", GB)]),
        systemone: true,
        glossary: true,
        hardware: Some(Hardware { kind: "apple", memory: Some(128 * GB), why: String::new() })
    );
    assert_eq!(route::choose(&f).route, "rule");
}

#[test]
fn route_glossary_needs_glossary_systemone_and_tev1() {
    let small = map(&[("tev1:0.8b-mxfp8", GB)]);
    let f = |ollama: Option<BTreeMap<String, u64>>, glossary: bool, systemone: bool| {
        facts!(
            evidence: true,
            ollama: ollama,
            systemone: systemone,
            glossary: glossary,
            hardware: Some(hw("apple", 128)
            )
        )
    };
    assert_eq!(route::choose(&f(small.clone(), false, true)).route, "rule");
    assert_eq!(route::choose(&f(small.clone(), true, false)).route, "rule");
    assert_eq!(
        route::choose(&f(Some(BTreeMap::new()), true, true)).route,
        "rule"
    );
    assert_eq!(route::choose(&f(small, true, true)).route, "glossary");
}

#[test]
fn route_without_ollama_is_rule() {
    let f = facts!(evidence: true, ollama: None);
    assert_eq!(route::choose(&f).route, "rule");
    let p = route::choose(
        &facts!(evidence: true, ollama: map(&[("qwen3.8:27b-mlx", 18 * GB), ("tev1:0.8b-mxfp8", GB)]), no_ollama: true),
    );
    assert_eq!(p.route, "rule");
    assert!(p.notes.is_empty());
}

#[test]
fn route_named_merge_model_missing_falls_back() {
    let f = facts!(
        evidence: true,
        ollama: map(&[("qwen3.8:27b-mlx", 18 * GB), ("tev1:0.8b-mxfp8", GB)]),
        systemone: true,
        glossary: true,
        merge_model: Some("nope:1b".to_owned()),
        hardware: Some(hw("apple", 128))
    );
    let p = route::choose(&f);
    assert_eq!(p.route, "glossary");
    assert!(p.notes[0].contains("nope:1b"));
}

// --- モデルの表（catalog::pick） ---

#[test]
fn pick_by_hardware() {
    let pick = |kind: &'static str, gb: u64| catalog::pick(&hw(kind, gb)).1;
    assert!(pick("apple", 16)[0].as_deref().is_none());
    assert_eq!(pick("apple", 16)[1].as_deref(), Some("tev1:0.8b-mxfp8"));
    assert_eq!(pick("apple", 32)[0].as_deref(), Some("qwen3.8:27b-mlx"));
    assert_eq!(pick("apple", 128)[0].as_deref(), Some("qwen3.8:27b-mlx"));
    assert!(pick("nvidia", 24)[0].as_deref().is_none());
    assert_eq!(pick("nvidia", 24)[1].as_deref(), Some("tev1:0.8b-q8_0"));
    assert_eq!(pick("nvidia", 32)[0].as_deref(), Some("qwen3.8:27b-q4_K_M"));
    assert!(pick("cpu", 16)[0].as_deref().is_none());
    assert_eq!(pick("cpu", 16)[1].as_deref(), Some("tev1:0.8b-q8_0"));
    assert!(pick("cpu", 256)[0].as_deref().is_none());
}

// --- 機器の判定（catalog::detect）: ランタイムの関数を差し替える ---

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn detect_with_overrides() {
    let _g = ENV_LOCK.lock().unwrap();
    std::env::remove_var("KIKITORI_HARDWARE");
    let old_ram = catalog::set_ram_override(Some(64 * GB));

    // Rosetta 上の x86_64 の環境でも、sysctl hw.optional.arm64 で Apple Silicon と分かる
    let old_run = catalog::set_run_override(Some(Box::new(|_cmd: &str, args: &[&str]| {
        if args.join(" ").contains("hw.optional.arm64") {
            "1\n".to_owned()
        } else {
            String::new()
        }
    })));
    let hw = catalog::detect_with_darwin_arm(None, true).unwrap();
    assert_eq!(hw.kind, "apple");

    // NVIDIA は最大 1 枚の VRAM
    catalog::set_run_override(Some(Box::new(|_cmd: &str, args: &[&str]| {
        if args.contains(&"--query-gpu=memory.total") {
            "24576\n49152\n".to_owned()
        } else {
            String::new()
        }
    })));
    let old_which = catalog::set_which_override(Some(Box::new(|name: &str| {
        if name == "nvidia-smi" {
            Some("/usr/bin/nvidia-smi".to_owned())
        } else {
            None
        }
    })));
    let hw = catalog::detect_with_darwin_arm(None, false).unwrap();
    assert_eq!(hw.kind, "nvidia");
    // nvidia-smi は MiB で返す（49152 MiB）
    assert_eq!(hw.memory, Some(49152 * 1024 * 1024));

    // GPU が無ければ cpu
    catalog::set_which_override(Some(Box::new(|_| None)));
    let hw = catalog::detect_with_darwin_arm(None, false).unwrap();
    assert_eq!(hw.kind, "cpu");

    // KIKITORI_HARDWARE / --hardware による上書きと、未知の値は使い方の誤り（2）
    std::env::set_var("KIKITORI_HARDWARE", "apple");
    let hw = catalog::detect(None).unwrap();
    assert_eq!(hw.kind, "apple");
    std::env::remove_var("KIKITORI_HARDWARE");
    assert_eq!(
        catalog::detect(Some("bad")).unwrap_err().kind,
        ErrorKind::Usage
    );

    // 元の動きに戻す
    catalog::set_run_override(old_run);
    catalog::set_which_override(old_which);
    catalog::set_ram_override(old_ram);
    let _ = BIG;
}

// --- Ollama の接続先（OLLAMA_HOST と KIKITORI_OLLAMA） ---

#[test]
fn ollama_base_is_python_compatible() {
    let _g = ENV_LOCK.lock().unwrap();
    let saved_k = std::env::var("KIKITORI_OLLAMA").ok();
    let saved_o = std::env::var("OLLAMA_HOST").ok();
    std::env::remove_var("KIKITORI_OLLAMA");
    std::env::remove_var("OLLAMA_HOST");
    assert_eq!(ollama::base(None), "http://localhost:11434");
    assert!(ollama::is_local(&ollama::base(None)));
    std::env::set_var("OLLAMA_HOST", "0.0.0.0");
    assert_eq!(ollama::base(None), "http://localhost:11434");
    std::env::set_var("KIKITORI_OLLAMA", "https://llm.example");
    assert_eq!(ollama::base(None), "https://llm.example");
    assert!(!ollama::is_local(&ollama::base(None)));
    assert_eq!(ollama::base(Some("box:9000")), "http://box:9000");
    if let Some(v) = saved_k {
        std::env::set_var("KIKITORI_OLLAMA", v)
    } else {
        std::env::remove_var("KIKITORI_OLLAMA")
    }
    if let Some(v) = saved_o {
        std::env::set_var("OLLAMA_HOST", v)
    } else {
        std::env::remove_var("OLLAMA_HOST")
    }
}

// --- 用語集の読み込み ---

#[test]
fn glossary_loader_skips_comments_and_blanks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.tsv");
    std::fs::write(&path, "# 注記\n北山精機\t得意先\n白峰\n\n").unwrap();
    let got = kikitori_refine::load_glossary(&path).unwrap();
    assert_eq!(
        got,
        vec![
            ("北山精機".to_owned(), "得意先".to_owned()),
            ("白峰".to_owned(), "".to_owned())
        ]
    );
}

// --- `think` ブロック（`"think": false` を付けても出すことがある: DESIGN D-4/§11-13） ---

#[test]
fn strip_think_removes_block_and_marks_intact() {
    // 擬似タグは、このテストソースではエスケープで組む（`<`+`t h i n k`+`>` の 8 文字）
    let open: String = ['\u{3c}', 't', 'h', 'i', 'n', 'k', '\u{3e}']
        .iter()
        .collect();
    let close: String = ['\u{3c}', '/', 't', 'h', 'i', 'n', 'k', '\u{3e}']
        .iter()
        .collect();
    assert_eq!(prompts::think_open(), open);
    assert_eq!(prompts::think_close(), close);
    let body = format!("{open}考え中\n2 行目{close}[0] こんにちは");
    assert_eq!(prompts::strip_think(&body), "[0] こんにちは");
    assert_eq!(prompts::strip_think("[0] そのまま"), "[0] そのまま");
    let unterminated = format!("{open}だけ");
    assert_eq!(prompts::strip_think(&unterminated), "");
}
