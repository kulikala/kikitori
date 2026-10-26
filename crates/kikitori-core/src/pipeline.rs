use crate::{AsrEngine, AsrRequest, Diarizer, Segment, SpeakerSpan, Vad, SAMPLE_RATE};
use rand::{rngs::StdRng, SeedableRng};
use rand_distr::{Distribution, StandardNormal};

const PAD: f64 = 0.5;
const EXT: f64 = 0.6;

pub struct PipelineOptions<'a> {
    pub context: &'a [String],
    pub language: Option<&'a str>,
    pub num_speakers: i32,
    pub min_piece: f64,
    /// Skip diarization-only spans that overlap one already added from diarization.
    pub fix_overlapping_turn_spans: bool,
}

impl Default for PipelineOptions<'_> {
    fn default() -> Self {
        Self {
            context: &[],
            language: None,
            num_speakers: -1,
            min_piece: 0.5,
            fix_overlapping_turn_spans: true,
        }
    }
}

pub fn merge_turns(turns: &[SpeakerSpan], min_gap: f64) -> Vec<SpeakerSpan> {
    let mut merged: Vec<SpeakerSpan> = Vec::new();
    for turn in turns {
        let start = merged.len().saturating_sub(4);
        if let Some(m) = merged[start..]
            .iter_mut()
            .rev()
            .find(|m| m.speaker == turn.speaker && turn.start - m.end < min_gap)
        {
            m.end = m.end.max(turn.end);
        } else {
            merged.push(turn.clone());
        }
    }
    merged
}

fn dominant(turns: &[SpeakerSpan], a: f64, b: f64) -> usize {
    let mut overlap: Vec<(usize, f64)> = Vec::new();
    for turn in turns {
        let amount = b.min(turn.end) - a.max(turn.start);
        if amount > 0.0 {
            if let Some((_, total)) = overlap
                .iter_mut()
                .find(|(speaker, _)| *speaker == turn.speaker)
            {
                *total += amount;
            } else {
                overlap.push((turn.speaker, amount));
            }
        }
    }
    let winner = overlap
        .into_iter()
        .fold(None, |winner, candidate| match winner {
            Some((_, best)) if best >= candidate.1 => winner,
            _ => Some(candidate),
        });
    if let Some((speaker, _)) = winner {
        return speaker;
    }
    let mid = (a + b) / 2.0;
    turns
        .iter()
        .min_by(|x, y| {
            let dx = (x.start - mid).abs().min((x.end - mid).abs());
            let dy = (y.start - mid).abs().min((y.end - mid).abs());
            dx.total_cmp(&dy)
        })
        .map_or(0, |turn| turn.speaker)
}

/// Split VAD spans only at diarization boundaries; short pieces inherit the prior speaker (spec §3.6).
pub fn plan(spans: &[(f64, f64)], turns: &[SpeakerSpan], min_piece: f64) -> Vec<SpeakerSpan> {
    let mut pieces = Vec::new();
    for &(a, b) in spans {
        let mut cuts: Vec<f64> = turns
            .iter()
            .flat_map(|t| [t.start, t.end])
            .filter(|t| a + min_piece < *t && *t < b - min_piece)
            .collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let mut edges = vec![a];
        edges.extend(cuts);
        edges.push(b);
        let mut local: Vec<SpeakerSpan> = Vec::new();
        for pair in edges.windows(2) {
            let (x, y) = (pair[0], pair[1]);
            let speaker = dominant(turns, x, y);
            if let Some(last) = local
                .last_mut()
                .filter(|last| last.speaker == speaker || y - x < min_piece)
            {
                last.end = y;
            } else {
                local.push(SpeakerSpan {
                    start: x,
                    end: y,
                    speaker,
                });
            }
        }
        pieces.extend(local);
    }
    pieces
}

/// Run VAD, diarization, padded ASR and speaker assignment over 16 kHz mono samples.
pub fn run(
    pcm: &[f32],
    asr: &mut dyn AsrEngine,
    vad: &mut dyn Vad,
    diarizer: &mut dyn Diarizer,
    options: &PipelineOptions<'_>,
    mut replace: Option<&mut dyn FnMut(&str) -> String>,
) -> Result<Vec<Segment>, String> {
    let mut turns = diarizer.diarize(pcm)?;
    turns.sort_by(|a, b| {
        a.start
            .total_cmp(&b.start)
            .then(a.end.total_cmp(&b.end))
            .then(a.speaker.cmp(&b.speaker))
    });
    turns = merge_turns(&turns, 0.6);
    let total = pcm.len() as f64 / SAMPLE_RATE as f64;
    let raw_vad = vad.spans(pcm)?;
    let mut spans: Vec<(f64, f64)> = Vec::new();
    for span in raw_vad {
        let a = 0.0_f64.max(span.start - EXT);
        let b = total.min(span.end + EXT);
        if let Some(last) = spans.last_mut().filter(|last| a <= last.1) {
            last.1 = last.1.max(b);
        } else {
            spans.push((a, b));
        }
    }
    if spans.is_empty() && pcm.len() as f64 > 0.2 * SAMPLE_RATE as f64 {
        let rms =
            (pcm.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / pcm.len() as f64).sqrt();
        if rms > 0.005 {
            spans.push((0.0, total));
        }
    }
    // Evaluate coverage against the original VAD spans, matching Python's list-comprehension order (spec §3.3).
    let mut additions = Vec::new();
    for turn in &turns {
        if turn.end - turn.start < 0.5
            || spans
                .iter()
                .any(|(a, b)| turn.end.min(*b) - turn.start.max(*a) > 0.2)
        {
            continue;
        }
        let span = (turn.start, turn.end);
        if options.fix_overlapping_turn_spans
            && additions
                .iter()
                .any(|(a, b): &(f64, f64)| span.1.min(*b) - span.0.max(*a) > 0.0)
        {
            continue;
        }
        additions.push(span);
    }
    spans.extend(additions);
    spans.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let pieces = plan(&spans, &turns, options.min_piece);
    let mut bounds: Vec<f64> = pieces.iter().flat_map(|p| [p.start, p.end]).collect();
    bounds.sort_by(f64::total_cmp);
    bounds.dedup_by(|a, b| *a == *b);
    let mut rng = StdRng::seed_from_u64(0);
    let mut waves = Vec::with_capacity(pieces.len());
    for piece in &pieces {
        let prev = bounds
            .iter()
            .copied()
            .filter(|t| *t < piece.start - 1e-6)
            .max_by(f64::total_cmp)
            .unwrap_or(0.0);
        let next = bounds
            .iter()
            .copied()
            .filter(|t| *t > piece.end + 1e-6)
            .min_by(f64::total_cmp)
            .unwrap_or(total);
        let left_pad = 0.4_f64.min(0.0_f64.max((piece.start - prev) / 2.0));
        let right_pad = 0.4_f64.min(0.0_f64.max((next - piece.end) / 2.0));
        let start = ((piece.start - left_pad) * SAMPLE_RATE as f64) as usize;
        let end = ((piece.end + right_pad) * SAMPLE_RATE as f64) as usize;
        let mut wave = noise(PAD - left_pad, &mut rng);
        wave.extend_from_slice(&pcm[start.min(pcm.len())..end.min(pcm.len())]);
        wave.extend(noise(PAD - right_pad, &mut rng));
        waves.push(wave);
    }
    let requests: Vec<AsrRequest<'_>> = waves
        .iter()
        .map(|wave| AsrRequest {
            pcm: wave,
            context: options.context,
            language: options.language,
            want_timestamps: asr.capabilities().timestamps,
        })
        .collect();
    let outputs = asr.transcribe(&requests)?;
    if outputs.len() != pieces.len() {
        return Err(format!(
            "ASR returned {} outputs for {} spans",
            outputs.len(),
            pieces.len()
        ));
    }
    let mut segments = Vec::new();
    for (piece, mut output) in pieces.iter().zip(outputs) {
        if asr.capabilities().timestamps && !output.tokens.is_empty() {
            for token in output.tokens {
                let mut text = token.text;
                if let Some(replacer) = replace.as_deref_mut() {
                    text = replacer(&text);
                }
                if !text.is_empty() {
                    let start = piece.start - PAD + f64::from(token.start);
                    let end = piece.start - PAD + f64::from(token.end);
                    segments.push(Segment {
                        start,
                        end,
                        speaker: dominant(&turns, start, end),
                        text,
                    });
                }
            }
        } else {
            output.text = output.text.trim().to_owned();
            if let Some(replacer) = replace.as_deref_mut() {
                output.text = replacer(&output.text);
            }
            if !output.text.is_empty() {
                segments.push(Segment {
                    start: piece.start,
                    end: piece.end,
                    speaker: piece.speaker,
                    text: output.text,
                });
            }
        }
    }
    if options.num_speakers > 0 {
        let found = segments
            .iter()
            .map(|s| s.speaker)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if found < options.num_speakers as usize {
            crate::notice::warning(&format!(
                "警告: 指定の {} 人に対し、発言が見つかった話者は {found} 人でした。",
                options.num_speakers
            ));
        }
    }
    Ok(segments)
}

fn noise(seconds: f64, rng: &mut StdRng) -> Vec<f32> {
    let count = (seconds * SAMPLE_RATE as f64) as usize;
    (0..count)
        .map(|_| {
            let value: f64 = StandardNormal.sample(rng);
            (value * 0.003) as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{DummyAsr, DummyDiarizer, DummyVad};

    #[test]
    fn dummy_pipeline_assigns_speakers_across_turns() {
        let pcm = vec![0.01; SAMPLE_RATE * 4];
        let mut asr = DummyAsr::new("hello");
        let mut vad = DummyVad(vec![crate::VadSpan {
            start: 0.0,
            end: 4.0,
        }]);
        let mut diarizer = DummyDiarizer(vec![
            SpeakerSpan {
                start: 0.0,
                end: 2.0,
                speaker: 0,
            },
            SpeakerSpan {
                start: 2.0,
                end: 4.0,
                speaker: 1,
            },
        ]);
        let result = run(
            &pcm,
            &mut asr,
            &mut vad,
            &mut diarizer,
            &PipelineOptions::default(),
            None,
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!((result[0].speaker, result[1].speaker), (0, 1));
        assert_eq!((result[0].start, result[0].end), (0.0, 2.0));
    }

    #[test]
    fn turn_merge_checks_only_last_four() {
        let turns = (0..6)
            .map(|i| SpeakerSpan {
                start: i as f64,
                end: i as f64 + 0.1,
                speaker: i,
            })
            .collect::<Vec<_>>();
        assert_eq!(merge_turns(&turns, 0.6).len(), 6);
    }

    #[test]
    fn can_skip_overlapping_diarization_only_spans() {
        let pcm = vec![0.0; SAMPLE_RATE * 4];
        let turns = vec![
            SpeakerSpan {
                start: 1.0,
                end: 2.0,
                speaker: 0,
            },
            SpeakerSpan {
                start: 1.5,
                end: 2.5,
                speaker: 1,
            },
        ];
        let run_with_option = |fix_overlapping_turn_spans| {
            let mut asr = DummyAsr::new("text");
            let mut vad = DummyVad(Vec::new());
            let mut diarizer = DummyDiarizer(turns.clone());
            run(
                &pcm,
                &mut asr,
                &mut vad,
                &mut diarizer,
                &PipelineOptions {
                    fix_overlapping_turn_spans,
                    ..PipelineOptions::default()
                },
                None,
            )
            .unwrap()
        };

        let default_segments = run_with_option(false);
        let fixed_segments = run_with_option(true);
        assert!(default_segments.len() > fixed_segments.len());
        assert_eq!(fixed_segments.len(), 1);
        assert_eq!((fixed_segments[0].start, fixed_segments[0].end), (1.0, 2.0));
    }
}
