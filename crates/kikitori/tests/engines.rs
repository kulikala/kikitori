use hound::{SampleFormat, WavReader};
use kikitori_core::{AsrEngine, AsrRequest};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn models() -> Option<PathBuf> {
    let path = std::env::var_os("KIKITORI_TEST_MODELS")?;
    Some(PathBuf::from(path))
}

fn proto() -> PathBuf {
    std::env::var_os("KIKITORI_TEST_PROTO").map_or_else(
        || kikitori_core::paths::home_dir().join("work/kikitori-proto"),
        PathBuf::from,
    )
}

fn wav(path: &Path) -> Vec<f32> {
    let mut reader = WavReader::open(path).unwrap();
    let spec = reader.spec();
    assert_eq!(spec.channels, 1, "{} is not mono", path.display());
    assert_eq!(spec.sample_rate, 16_000, "{} is not 16 kHz", path.display());
    match spec.sample_format {
        SampleFormat::Int if spec.bits_per_sample == 16 => reader
            .samples::<i16>()
            .map(|sample| sample.unwrap() as f32 / 32768.0)
            .collect(),
        SampleFormat::Float if spec.bits_per_sample == 32 => {
            reader.samples::<f32>().map(Result::unwrap).collect()
        }
        _ => panic!("{} is not PCM16 or float32", path.display()),
    }
}

fn clips() -> (Vec<String>, Vec<Vec<f32>>) {
    let directory = proto().join("clips");
    let mut paths = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "wav"))
        .collect::<Vec<_>>();
    paths.sort();
    let names = paths
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let waves = paths.iter().map(|path| wav(path)).collect();
    (names, waves)
}

fn references(name: &str) -> BTreeMap<String, String> {
    let path = proto().join("ref-py021").join(name);
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

fn requests<'a>(waves: &'a [Vec<f32>]) -> Vec<AsrRequest<'a>> {
    waves
        .iter()
        .map(|wave| AsrRequest {
            pcm: wave,
            context: &[],
            language: Some("ja"),
            want_timestamps: false,
        })
        .collect()
}

fn compare(names: &[String], expected: &BTreeMap<String, String>, actual: &[String]) {
    assert_eq!(names.len(), expected.len());
    let mismatches = names
        .iter()
        .zip(actual)
        .filter(|(name, text)| expected.get(*name) != Some(*text))
        .map(|(name, text)| {
            format!(
                "{name}\n  expected: {}\n  actual:   {text}",
                expected.get(name.as_str()).map_or("", String::as_str)
            )
        })
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "{} / {} clips differ:\n{}",
        mismatches.len(),
        names.len(),
        mismatches.join("\n")
    );
}

#[test]
#[ignore = "requires KIKITORI_TEST_MODELS and the proto clips"]
fn reazon_matches_python_on_clips() {
    let Some(models) = models() else {
        eprintln!("KIKITORI_TEST_MODELS is not set");
        return;
    };
    let (names, waves) = clips();
    let requests = requests(&waves);
    let mut engine = kikitori_reazon::Reazon::new(&models, 4).unwrap();
    let actual = engine
        .transcribe(&requests)
        .unwrap()
        .into_iter()
        .map(|output| output.text)
        .collect::<Vec<_>>();
    compare(&names, &references("clips.ort.json"), &actual);
}

#[test]
#[ignore = "requires KIKITORI_TEST_MODELS and the proto clips"]
fn reazon_greedy_matches_python_on_clips() {
    let Some(models) = models() else {
        eprintln!("KIKITORI_TEST_MODELS is not set");
        return;
    };
    let (names, waves) = clips();
    let requests = requests(&waves);
    let mut engine = kikitori_sherpa::ReazonGreedy::new(&models, "ja", "fp32", 4).unwrap();
    let actual = engine
        .transcribe(&requests)
        .unwrap()
        .into_iter()
        .map(|output| output.text)
        .collect::<Vec<_>>();
    compare(&names, &references("clips.greedy_search.json"), &actual);
}

struct PipelineCase<'a> {
    asr: &'a str,
    reference: &'a str,
    num_speakers: Option<&'a str>,
}

fn run_pipeline_parity(audio: &str, case: &PipelineCase<'_>, python_compat: bool) -> String {
    let Some(models) = models() else {
        return "KIKITORI_TEST_MODELS is not set".to_owned();
    };
    let root = proto();
    let directory = tempfile::tempdir().unwrap();
    let hyp = directory.path().join("hyp.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_kikitori"));
    command
        .arg("transcribe")
        .arg(root.join("ref-py021/audio").join(format!("{audio}.wav")))
        .args(["--models", models.to_str().unwrap()])
        .args(["--asr", case.asr, "--format", "json"])
        .env("KIKITORI_LOG_FORMAT", "json");
    if let Some(count) = case.num_speakers {
        command.args(["--num-speakers", count]);
    }
    if python_compat {
        command.arg("--python-compat");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{} {audio} compat={python_compat}\n{}",
        case.asr,
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::write(&hyp, &output.stdout).unwrap();
    let reference = root.join("ref-py021").join(case.reference);
    let max_cer = if audio == "synth2" && case.asr == "reazon" {
        "2.0"
    } else {
        "0.5"
    };
    let parity = Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/parity.py"))
        .arg(&reference)
        .arg(&hyp)
        .args(["--max-cer", max_cer, "--max-speaker", "2.0"])
        .output()
        .unwrap();
    let report = format!(
        "{audio} {} compat={python_compat} max-cer={max_cer}: {}{}",
        case.asr,
        String::from_utf8_lossy(&parity.stdout).trim(),
        if parity.status.success() {
            ""
        } else {
            " (FAILED)"
        }
    );
    assert!(
        parity.status.success(),
        "{report}\n{}",
        String::from_utf8_lossy(&parity.stderr)
    );
    report
}

#[test]
#[ignore = "requires KIKITORI_TEST_MODELS and the proto audio"]
fn full_pipeline_matches_python() {
    let cases = [
        PipelineCase {
            asr: "reazon",
            reference: "synth.cpu.json",
            num_speakers: None,
        },
        PipelineCase {
            asr: "reazon-greedy",
            reference: "synth.greedy.json",
            num_speakers: None,
        },
    ];
    for audio in ["synth", "synth2"] {
        for case in &cases {
            let reference = case.reference.replace("synth.", &format!("{audio}."));
            let case = PipelineCase {
                reference: &reference,
                ..*case
            };
            for python_compat in [true, false] {
                println!("{}", run_pipeline_parity(audio, &case, python_compat));
            }
        }
    }
}

#[test]
#[ignore = "requires KIKITORI_TEST_MODELS and the proto audio"]
fn num_speakers_reclustering_matches_python() {
    let case = PipelineCase {
        asr: "reazon",
        reference: "synth.cpu.n4.json",
        num_speakers: Some("4"),
    };
    println!("{}", run_pipeline_parity("synth", &case, true));
}
