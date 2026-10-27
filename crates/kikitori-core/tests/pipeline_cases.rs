use kikitori_core::pipeline::{self, PipelineOptions};
use kikitori_core::{
    AsrEngine, AsrOutput, AsrRequest, Capabilities, DType, Diarizer, Segment, SpeakerSpan, Vad,
    VadSpan, SAMPLE_RATE,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    samples: usize,
    vad: Vec<[usize; 2]>,
    turns: Vec<(f64, f64, usize)>,
    segments: Vec<Segment>,
}

struct FakeVad(Vec<VadSpan>);

impl Vad for FakeVad {
    fn spans(&mut self, _pcm: &[f32]) -> Result<Vec<VadSpan>, String> {
        Ok(self.0.clone())
    }
}

struct FakeDiarizer(Vec<SpeakerSpan>);

impl Diarizer for FakeDiarizer {
    fn diarize(&mut self, _pcm: &[f32]) -> Result<Vec<SpeakerSpan>, String> {
        Ok(self.0.clone())
    }
}

struct LengthAsr {
    capabilities: Capabilities,
}

impl AsrEngine for LengthAsr {
    fn id(&self) -> &str {
        "length"
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn transcribe(&mut self, requests: &[AsrRequest<'_>]) -> Result<Vec<AsrOutput>, String> {
        Ok(requests
            .iter()
            .map(|request| AsrOutput {
                text: format!("n{}", request.pcm.len()),
                tokens: Vec::new(),
            })
            .collect())
    }
}

fn fake_asr() -> LengthAsr {
    LengthAsr {
        capabilities: Capabilities {
            timestamps: false,
            context: false,
            max_seconds: 30.0,
            dtypes: &[DType::F32],
        },
    }
}

#[test]
fn matches_python_recorded_pipeline_cases() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("../../../tests/fixtures/pipeline/cases.json"))
            .expect("valid recorded cases");

    for (case_idx, case) in cases.iter().enumerate() {
        let pcm = vec![0.0; case.samples];
        let mut vad = FakeVad(
            case.vad
                .iter()
                .map(|span| VadSpan {
                    start: span[0] as f64 / SAMPLE_RATE as f64,
                    end: span[1] as f64 / SAMPLE_RATE as f64,
                })
                .collect(),
        );
        let mut diarizer = FakeDiarizer(
            case.turns
                .iter()
                .map(|&(start, end, speaker)| SpeakerSpan {
                    start,
                    end,
                    speaker,
                })
                .collect(),
        );
        let mut asr = fake_asr();
        let actual = pipeline::run(
            &pcm,
            &mut asr,
            &mut vad,
            &mut diarizer,
            // The cases were recorded with the Python version's 0.5 s pad
            &PipelineOptions {
                pad: 0.5,
                ..PipelineOptions::default()
            },
            None,
        )
        .unwrap_or_else(|error| panic!("case {case_idx}: {error}"));

        assert_eq!(
            actual.len(),
            case.segments.len(),
            "case {case_idx} segment count"
        );
        for (segment_idx, (actual, expected)) in actual.iter().zip(&case.segments).enumerate() {
            assert!(
                (actual.start - expected.start).abs() <= 1e-9,
                "case {case_idx} segment {segment_idx} start: {} != {}",
                actual.start,
                expected.start
            );
            assert!(
                (actual.end - expected.end).abs() <= 1e-9,
                "case {case_idx} segment {segment_idx} end: {} != {}",
                actual.end,
                expected.end
            );
            assert_eq!(
                actual.speaker, expected.speaker,
                "case {case_idx} segment {segment_idx} speaker"
            );
            assert_eq!(
                actual.text, expected.text,
                "case {case_idx} segment {segment_idx} text"
            );
        }
    }
}
