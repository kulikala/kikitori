//! Python 版（tests/test_setup_output.py）の plan_setup / todo の移植。実行はせず、手の並びだけを見る。

use kikitori_refine::catalog::{Hardware, GB};
use kikitori_refine::evidence;
use kikitori_refine::setup::{plan_setup, todo, State};

fn hardware(kind: &'static str, why: &str, memory_gb: Option<u64>) -> Hardware {
    Hardware {
        kind,
        memory: memory_gb.map(|g| g * GB),
        why: why.to_owned(),
    }
}

/// Python の state()。必要なところだけ、`os` / `kind` / `gib`/ 個別の調整後に `.state()` を呼べる。
fn base() -> State {
    State {
        os: "Darwin",
        hardware: hardware("apple", "Apple Silicon", Some(64)),
        refine: true,
        local: true,
        api: false,
        version: None,
        cli: false,
        app: false,
        systemd: false,
        models: Default::default(),
        tty: true,
        free_ollama: Some(500 * GB),
        free_hf: Some(500 * GB),
        hf_missing: Vec::new(),
        hardware_given: true,
    }
}

fn linux_cpu() -> State {
    let mut s = base();
    s.os = "Linux";
    s.hardware = hardware("cpu", "不明", Some(16));
    s
}

fn kinds(s: &State, no_hf: bool) -> Vec<(String, String)> {
    todo(&plan_setup(s, no_hf, None))
        .iter()
        .map(|x| (x.kind.to_string(), x.arg.clone()))
        .collect()
}

#[test]
fn setup_without_ollama_never_installs_and_still_fetches_b_k() {
    let mut s = base();
    s.hf_missing = vec![evidence::QWEN.to_owned(), evidence::KANA.to_owned()];
    let steps = plan_setup(&s, false, None);
    assert_eq!(
        kinds(&s, false),
        vec![
            ("hf".to_owned(), evidence::QWEN.to_owned()),
            ("hf".to_owned(), evidence::KANA.to_owned()),
        ]
    );
    assert!(steps
        .iter()
        .any(|x| x.text.contains("Ollama が見つかりません")));
}

#[test]
fn setup_second_run_has_nothing_to_do() {
    let mut s = base();
    s.api = true;
    s.version = Some((0, 40, 8));
    s.cli = true;
    s.models = ["qwen3.8:27b-mlx", "tev1:0.8b-mxfp8"]
        .iter()
        .map(|m| (m.to_string(), 1))
        .collect();
    assert!(todo(&plan_setup(&s, false, None)).is_empty());
}

#[test]
fn setup_stopped_app_is_not_started() {
    let mut s = base();
    s.app = true;
    let steps = plan_setup(&s, true, None);
    assert!(todo(&steps).is_empty());
    assert!(steps
        .iter()
        .any(|x| x.text.contains("Ollama のアプリを開く")));
}

#[test]
fn setup_remote_never_installs_and_defaults_to_cpu_models() {
    let mut s = base();
    s.hardware = hardware("cpu", "不明", None);
    s.local = false;
    s.api = true;
    s.version = Some((0, 40, 8));
    s.cli = true;
    s.hardware_given = false;
    let steps = plan_setup(&s, true, None);
    let got: Vec<(String, String)> = todo(&steps)
        .iter()
        .map(|x| (x.kind.to_string(), x.arg.clone()))
        .collect();
    assert_eq!(got, vec![("pull".to_owned(), "tev1:0.8b-q8_0".to_owned())]);
    assert!(steps.iter().any(|x| x.text.contains("--hardware")));
}

#[test]
fn setup_remote_not_answering_stops() {
    let mut s = base();
    s.local = false;
    assert_eq!(plan_setup(&s, false, None).last().unwrap().kind, "stop");
}

#[test]
fn setup_old_ollama_skips_llm_models() {
    let mut s = base();
    s.api = true;
    s.version = Some((0, 39, 1));
    s.hf_missing = vec![evidence::QWEN.to_owned()];
    let steps = plan_setup(&s, false, None);
    assert!(steps.iter().any(|x| x.text.contains("0.40.0")));
    assert!(todo(&steps).iter().all(|x| x.kind == "hf"));
}

#[test]
fn setup_low_disk_stops() {
    let mut s = base();
    s.api = true;
    s.version = Some((0, 40, 8));
    s.free_ollama = Some(10 * GB);
    let last = plan_setup(&s, false, None).into_iter().last().unwrap();
    assert_eq!(last.kind, "stop");
    assert!(last.text.contains("空き"));
}

#[test]
fn setup_without_refine_installs_nothing() {
    let mut s = base();
    s.refine = false;
    let steps = plan_setup(&s, false, None);
    assert!(todo(&steps).is_empty());
    assert!(steps[0].text.contains("入れません"));
}

#[test]
fn setup_systemd_stopped_asks_user() {
    let mut s = linux_cpu();
    s.cli = true;
    s.systemd = true;
    let got = plan_setup(&s, false, None);
    assert!(got.iter().any(|x| x.text.contains("systemctl")));
    assert!(todo(&got).iter().all(|x| x.kind == "hf"));
}

#[test]
fn setup_merge_model_pulls_only_that_model() {
    // D-9: --setup --merge-model <名前> はそのモデルだけを取る
    let mut s = base();
    s.api = true;
    s.version = Some((0, 40, 8));
    s.cli = true;
    let pulls: Vec<String> = todo(&plan_setup(&s, false, Some("m:1b")))
        .iter()
        .filter(|x| x.kind == "pull")
        .map(|x| x.arg.clone())
        .collect();
    assert_eq!(pulls, vec!["m:1b".to_owned()]);
}
