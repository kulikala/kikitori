use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn cli() -> (Command, TempDir) {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("kikitori").unwrap();
    cmd.env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("KIKITORI_LOG_FORMAT", "json");
    (cmd, home)
}
fn assert_json_stderr(stderr: &[u8]) {
    let text = std::str::from_utf8(stderr).unwrap();
    assert!(!text.trim().is_empty(), "expected a diagnostic event");
    for line in text.lines() {
        let value: Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON: {line}: {e}"));
        assert!(value.get("type").is_some());
        if value["type"] == "error" {
            assert!(value.get("code").is_some());
            assert!(value.get("message").is_some());
            assert!(value.get("hint").is_some());
        }
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            assert!(
                !serde_json::from_str::<Value>(message).is_ok_and(|inner| inner.is_object()),
                "message was itself a JSON object: {line}"
            );
        }
    }
}

#[test]
fn usage_error_is_json_and_exit_two() {
    let (mut cmd, _home) = cli();
    cmd.env_remove("KIKITORI_LOG_FORMAT");
    let output = cmd
        .args(["--log-format", "json", "--bogus"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_json_stderr(&output.stderr);
}

#[test]
fn positional_audio_accepts_flags_after_it() {
    let (mut cmd, home) = cli();
    let input = home.path().join("sample.wav");
    std::fs::write(&input, b"not actually audio").unwrap();
    let output = cmd
        .arg(input)
        .args(["--num-speakers", "4", "--format", "yaml"])
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(2), "CLI rejected trailing flags");
    assert_json_stderr(&output.stderr);
}

#[test]
fn missing_input_is_three_even_with_closed_stdin() {
    let (mut cmd, _home) = cli();
    let output = cmd
        .args(["transcribe"])
        .write_stdin(Vec::<u8>::new())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert_json_stderr(&output.stderr);
}

#[test]
fn confirmation_without_tty_is_six_and_does_not_wait_for_stdin() {
    let (mut cmd, _home) = cli();
    let output = cmd
        .args(["setup"])
        .write_stdin(Vec::<u8>::new())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(6));
    assert_json_stderr(&output.stderr);
}

#[test]
fn setup_dry_run_text_prints_the_plan() {
    let (mut cmd, _home) = cli();
    cmd.env_remove("KIKITORI_LOG_FORMAT");
    let output = cmd.args(["setup", "--dry-run"]).output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(
        text.contains("件（合計"),
        "expected the plan in text: {text}"
    );
}

#[test]
fn refine_setup_dry_run_text_prints_the_steps() {
    let (mut cmd, _home) = cli();
    cmd.env_remove("KIKITORI_LOG_FORMAT")
        .env("KIKITORI_OLLAMA", "http://127.0.0.1:9");
    let output = cmd
        .args(["refine", "--setup", "--dry-run"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(
        text.contains("機器:"),
        "expected the setup steps in text: {text}"
    );
}

#[test]
fn setup_dry_run_json_is_json_lines_and_does_not_download() {
    let (mut cmd, _home) = cli();
    let output = cmd
        .args(["setup", "--dry-run", "--log-format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "dry-run wrote unexpected stdout");
    let text = std::str::from_utf8(&output.stderr).unwrap();
    assert!(text.lines().count() > 1, "expected plan events: {text}");
    for line in text.lines() {
        let value: Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON: {line}: {e}"));
        assert!(value.get("type").is_some());
        assert!(value.get("message").is_some());
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            assert!(
                !serde_json::from_str::<Value>(message).is_ok_and(|inner| inner.is_object()),
                "message was itself a JSON object: {line}"
            );
        }
    }
}

#[test]
fn setup_offline_with_missing_files_is_five_without_network() {
    let (mut cmd, _home) = cli();
    let output = cmd.args(["setup", "--offline"]).output().unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert_json_stderr(&output.stderr);
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    assert!(stderr.contains("`--offline` なし"), "{stderr}");
}

#[test]
fn help_and_human_errors_are_japanese() {
    let (mut help_cmd, _home) = cli();
    let help = help_cmd.arg("--help").output().unwrap();
    assert!(help.status.success());
    let help_text = String::from_utf8(help.stdout).unwrap();
    assert!(help_text.contains("使い方:"), "{help_text}");
    assert!(help_text.contains("コマンド:"), "{help_text}");
    assert!(!help_text.contains("Print help"), "{help_text}");

    let (mut error_cmd, _home) = cli();
    let error = error_cmd
        .env_remove("KIKITORI_LOG_FORMAT")
        .args(["setup", "--offline", "--log-format", "human"])
        .output()
        .unwrap();
    let stderr = String::from_utf8(error.stderr).unwrap();
    assert!(stderr.starts_with("エラー: "), "{stderr}");
    assert!(stderr.contains("\nヒント: "), "{stderr}");
}

#[test]
fn doctor_output_matches_schema() {
    let (mut cmd, _home) = cli();
    let output = cmd.args(["doctor", "--json"]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let schema: Value = serde_json::from_str(include_str!("../schemas/doctor.json")).unwrap();
    let validator = jsonschema::JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .compile(&schema)
        .unwrap();
    assert!(validator.is_valid(&value));
}

#[test]
fn transcript_schema_command_prints_schema_json() {
    let (mut cmd, _home) = cli();
    let output = cmd.args(["schema", "transcript"]).output().unwrap();
    assert!(output.status.success());
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[test]
fn transcript_v1_schema_command_prints_valid_json() {
    let (mut cmd, _home) = cli();
    let output = cmd.args(["schema", "transcript-v1"]).output().unwrap();
    assert!(output.status.success());
    let schema: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[test]
fn global_flags_before_a_subcommand_select_the_subcommand() {
    // install.sh は `kikitori --yes setup` の形で呼ぶ
    for args in [
        vec!["--yes", "setup", "--dry-run"],
        vec!["--log-format", "json", "setup", "--dry-run"],
    ] {
        let (mut cmd, _home) = cli();
        cmd.args(&args).write_stdin("").assert().code(0);
    }
}
