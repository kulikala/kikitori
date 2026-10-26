//! 仮の Ollama サーバ（server.rs）で、merge・glossary_fix・`--setup` の続きを確かめる。

use kikitori_refine::catalog::GB;
use kikitori_refine::local;
use kikitori_refine::setup::{self, SetupOutcome, State};
use serde_json::{json, Value};

mod server;

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

fn chat_stream(content: &str) -> String {
    let reply = json!({ "message": {"role": "assistant", "content": content}, "done": false });
    format!("{reply}\n")
}

// --- merge: Python 版の merge_golden（Ollama を仮で差し替えたもの）と一致 ---

#[test]
fn merge_golden_with_fake_ollama() {
    let f = server::spawn();
    // 区間 1 は Ollama が壊れた答え（`message` が無い JSON）を返して失敗させる
    f.chat.lock().unwrap().extend([
        chat_stream("[0] 三郎はケーキを作った"),
        "\"ANSWER_FAIL\"".to_owned(),
        chat_stream("[2] 三郎はベッドで寝る"),
    ]);
    let g = fixture("merge_golden.json");
    let a = strings(&json!(["一郎", "二郎", "三郎"]));
    let b = strings(&json!(["一ろ", "二ろ", "三ろ"]));
    let k = strings(&json!(["イチ", "ニ", "サン"]));
    let gl = vec![("北山精機".to_owned(), "".to_owned())];
    let mut log = Vec::new();
    let got = local::merge(&f.addr, "m1", &a, &b, &k, &gl, &mut |m| {
        log.push(m.to_owned())
    });
    assert_eq!(got, strings(&g["merge_texts"]));
    assert_eq!(log, strings(&g["merge_log"]));
}

// --- glossary_fix: /v1/systemone の代わりに、Python 版の問いの形と、確率の並びを確かめる ---

#[test]
fn glossary_fix_golden_with_fake_ollama() {
    let Some(dir) = std::env::var_os("KIKITORI_TEST_UNIDIC") else {
        eprintln!("skipping: KIKITORI_TEST_UNIDIC is unset");
        return;
    };
    let f = server::spawn();
    let g = fixture("merge_golden.json");
    f.systemone.lock().unwrap().extend([
        json!({"answers": {"fix": {"probabilities": {"keep": 0.4, "c0": 0.6}}}}).to_string(),
        json!({"answers": {"fix": {"probabilities": {"keep": 0.9, "c0": 0.1}}}}).to_string(),
    ]);
    let tagger = local::Tagger::load(std::path::Path::new(&dir)).unwrap();
    let texts = strings(&g["glossary_inputs"]);
    let kana = strings(&json!([
        "キタヤマセーキサンノコージョー",
        "キタヤマセイキサ",
        "アイウエオ"
    ]));
    let gl = vec![("北山精機".to_owned(), "得意先".to_owned())];
    let mut log = Vec::new();
    let got = local::glossary_fix(&tagger, &f.addr, "m1", &texts, &kana, &gl, &mut |m| {
        log.push(m.to_owned())
    });
    assert_eq!(got, strings(&g["glossary_texts"]));
    assert_eq!(log, strings(&g["glossary_log"]));
    // 問いの本文（先頭の /v1/systemone）は Python 版と同じ形
    let sent = f.received.lock().unwrap();
    let body: Value = serde_json::from_str(sent[0].split_once('\t').unwrap().1).unwrap();
    assert_eq!(body, g["systemone_body"]);
}

// --- --setup: 導入スクリプトが失敗を返しても、Ollama 本体が入っていれば続く ---

fn hw(kind: &'static str, gb: u64) -> kikitori_refine::catalog::Hardware {
    kikitori_refine::catalog::Hardware {
        kind,
        memory: Some(gb * GB),
        why: "Apple Silicon".into(),
    }
}

fn state() -> State {
    State {
        os: "Darwin",
        hardware: hw("apple", 24),
        refine: true,
        local: true,
        api: false,
        version: None,
        cli: false,
        app: false,
        systemd: false,
        models: Default::default(),
        tty: false,
        free_ollama: Some(500 * GB),
        free_hf: Some(500 * GB),
        hf_missing: Vec::new(),
        hardware_given: true,
    }
}

#[test]
fn run_setup_pulls_from_a_running_ollama_and_records_the_model() {
    let f = server::spawn();
    f.pulls.lock().unwrap().insert(
        "tev1:0.8b-mxfp8".to_owned(),
        "{\"status\":\"pulling manifest\"}\n{\"status\":\"success\"}\n".to_owned(),
    );
    let dir = tempfile::tempdir().unwrap();
    {
        let saved_home = std::env::var("HOME").ok();
        let saved_config = std::env::var("XDG_CONFIG_HOME").ok();
        let saved_ollama = std::env::var("KIKITORI_OLLAMA").ok();
        std::env::set_var("HOME", dir.path());
        std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
        std::env::set_var("KIKITORI_OLLAMA", &f.addr);

        let mut s = state();
        s.api = true;
        s.app = true;
        s.version = Some((0, 40, 8));
        let steps = setup::plan_setup(&s, true, None);
        let kinds: Vec<(&str, String)> = setup::todo(&steps)
            .iter()
            .map(|x| (x.kind, x.arg.clone()))
            .collect();
        assert_eq!(kinds, vec![("pull", "tev1:0.8b-mxfp8".to_owned())]);

        let outcome = setup::run_setup(&s, &steps, true, false);
        assert!(matches!(outcome, SetupOutcome::Done));

        // Ollama のモデルの取得を、確実に受取り付きに上げて記録する（D-12）
        let receipt_path = dir.path().join("config/kikitori/receipt.txt");
        let receipt = std::fs::read_to_string(receipt_path).unwrap();
        assert!(
            receipt.contains("ollama-model\ttev1:0.8b-mxfp8"),
            "receipt: {receipt:?}"
        );

        if let Some(v) = saved_home {
            std::env::set_var("HOME", v)
        } else {
            std::env::remove_var("HOME")
        }
        if let Some(v) = saved_config {
            std::env::set_var("XDG_CONFIG_HOME", v)
        } else {
            std::env::remove_var("XDG_CONFIG_HOME")
        }
        if let Some(v) = saved_ollama {
            std::env::set_var("KIKITORI_OLLAMA", v)
        } else {
            std::env::remove_var("KIKITORI_OLLAMA")
        }
    }
}
