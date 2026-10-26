//! `kikitori refine` の CLI の契約テスト（I-13 の終了コードと、stdout は結果だけ）。

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn cli(home: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("kikitori").unwrap();
    cmd.env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("KIKITORI_OLLAMA", "http://127.0.0.1:9")
        .env("KIKITORI_LOG_FORMAT", "json")
        .arg("refine");
    cmd
}

fn json_stderr(stderr: &[u8]) {
    let text = std::str::from_utf8(stderr).unwrap();
    assert!(!text.trim().is_empty(), "expected a diagnostic event");
    for line in text.lines() {
        let value: Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON: {line}: {e}"));
        assert!(value.get("type").is_some());
    }
}

const EVIDENCE: &str = r#"[
 [0.0, 1.0, 0, "あ", "ア", "ア"],
 [1.0, 2.0, 1, "い", "イ", "イ"],
 [2.0, 3.0, 1, "はい", "この商品は自社製品です", "コノシヨウヒンハ"]
]"#;

#[test]
fn refine_rejects_non_japanese_with_exit_two() {
    let home = tempfile::tempdir().unwrap();
    let output = cli(&home).args(["--model", "ja-en"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    json_stderr(&output.stderr);
    // このテストは --log-format json でなくても、エラーが 1 行出る
    let home = tempfile::tempdir().unwrap();
    let mut cmd = cli(&home);
    cmd.env_remove("KIKITORI_LOG_FORMAT")
        .args(["--model", "ja-en"]);
    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("日本語"), "stderr: {stderr:?}");
}

#[test]
fn refine_without_input_stops_with_three() {
    let home = tempfile::tempdir().unwrap();
    let output = cli(&home).args(["--no-ollama"]).output().unwrap();
    assert_eq!(output.status.code(), Some(3));
    json_stderr(&output.stderr);
}

#[test]
fn refine_evidence_bad_file_stops_with_three() {
    let home = tempfile::tempdir().unwrap();
    let output = cli(&home)
        .args(["--no-ollama", "--evidence", "not-a-file.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    json_stderr(&output.stderr);
}

#[test]
fn refine_plan_prints_route_and_stops_with_zero() {
    let home = tempfile::tempdir().unwrap();
    let output = cli(&home)
        .args(["--plan", "--no-ollama", "--evidence", "unused.json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    json_stderr(&output.stderr);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("経路"), "stderr: {stderr:?}");
    assert!(
        output.stdout.is_empty(),
        "stdout: {:?}",
        stdout_of(&output.stdout)
    );
}

fn stdout_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

#[test]
fn refine_rule_route_splices_texts_by_length_rule() {
    let home = tempfile::tempdir().unwrap();
    let evidence = home.path().join("ev.json");
    std::fs::write(&evidence, EVIDENCE).unwrap();
    let output = cli(&home)
        .args(["--no-ollama", "--evidence"])
        .arg(&evidence)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {:?}",
        stderr_of(&output.stderr)
    );
    json_stderr(&output.stderr);
    let got: Value = serde_json::from_slice(&output.stdout).unwrap();
    let segs = got.as_array().unwrap();
    assert_eq!(segs[0]["text"], "あ");
    assert_eq!(segs[2]["text"], "この商品は自社製品です");
}

fn stderr_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

#[test]
fn refine_bundle_ends_with_dumped_bundle_and_apply_round_trips() {
    let home = tempfile::tempdir().unwrap();
    let evidence = home.path().join("ev.json");
    std::fs::write(&evidence, EVIDENCE).unwrap();
    let dumped = cli(&home)
        .args(["--evidence"])
        .arg(&evidence)
        .args(["--external", "bundle"])
        .output()
        .unwrap();
    assert_eq!(
        dumped.status.code(),
        Some(0),
        "stderr: {:?}",
        stderr_of(&dumped.stderr)
    );
    json_stderr(&dumped.stderr);
    let bundle: Value = serde_json::from_slice(&dumped.stdout).unwrap();
    assert_eq!(bundle["kind"], "kikitori-refine/fixev");
    assert_eq!(bundle["chunks"][0]["ids"], serde_json::json!([0, 1, 2]));

    let bundle_path = home.path().join("bundle.json");
    std::fs::write(&bundle_path, &dumped.stdout).unwrap();
    let reply = "[1] イ\n[2] この商品は自社製品\n";
    let reply_path = home.path().join("reply.txt");
    std::fs::write(&reply_path, reply).unwrap();
    let got = cli(&home)
        .args(["--apply"])
        .arg(&bundle_path)
        .arg("--reply")
        .arg(&reply_path)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(
        got.status.code(),
        Some(0),
        "stderr: {:?}",
        stderr_of(&got.stderr)
    );
    let segs: Value = serde_json::from_slice(&got.stdout).unwrap();
    let segs = segs.as_array().unwrap();
    assert_eq!(segs[0]["text"], "あ");
    assert_eq!(segs[1]["text"], "イ");
    assert_eq!(segs[2]["text"], "この商品は自社製品");
}
