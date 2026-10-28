use crate::{
    AsrEngine, AsrOutput, AsrRequest, Diarizer, Segment, SpeakerSpan, Token, Vad, VadSpan,
    SAMPLE_RATE,
};
use rand::{rngs::StdRng, SeedableRng};
use rand_distr::{Distribution, StandardNormal};

const EXT: f64 = 0.6;
/// Shortest silence between two raw VAD spans, in seconds, to cut a merged span at.
const CUT_GAP: f64 = 0.3;

/// Text and pause offsets to punctuated text (see [`PipelineOptions::punctuate`]).
pub type Punctuate<'a> = dyn Fn(&str, &[usize]) -> String + 'a;

pub struct PipelineOptions<'a> {
    pub context: &'a [String],
    pub language: Option<&'a str>,
    pub num_speakers: i32,
    pub min_piece: f64,
    /// Noise added before and after each piece, in seconds. ReazonSpeech drops the first words
    /// of a piece that starts with too little quiet before it.
    pub pad: f64,
    /// Skip diarization-only spans that overlap one already added from diarization.
    pub fix_overlapping_turn_spans: bool,
    /// Adds punctuation to a piece's text. Gets the text and the offsets (counted in
    /// non-whitespace characters) of the word boundaries followed by a pause in the audio.
    /// Not called when the recognizer returns a span per token, which would end every token.
    pub punctuate: Option<&'a Punctuate<'a>>,
    /// Run this many ASR engines while diarization is still going. 0 keeps them apart.
    pub overlap_engines: usize,
}

impl Default for PipelineOptions<'_> {
    fn default() -> Self {
        Self {
            context: &[],
            language: None,
            num_speakers: -1,
            min_piece: 0.5,
            pad: 1.0,
            fix_overlapping_turn_spans: true,
            punctuate: None,
            overlap_engines: 0,
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

/// Cut merged VAD spans at the middle of the silences between the raw VAD spans inside them.
fn cut_at_silences(
    spans: &[(f64, f64)],
    raw: &[(f64, f64)],
    min_gap: f64,
    min_piece: f64,
) -> Vec<SpeakerSpan> {
    let mut pieces = Vec::new();
    for &(a, b) in spans {
        let mut cuts: Vec<f64> = raw
            .windows(2)
            .map(|w| (w[0].1, w[1].0))
            .filter(|(x, y)| y - x >= min_gap)
            .map(|(x, y)| (x + y) / 2.0)
            .filter(|cut| a < *cut && *cut < b)
            .collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let mut edges = vec![a];
        edges.extend(cuts);
        edges.push(b);
        let mut local: Vec<SpeakerSpan> = Vec::new();
        for pair in edges.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            if let Some(last) = local.last_mut().filter(|_| end - start < min_piece) {
                last.end = end;
            } else {
                local.push(SpeakerSpan {
                    start,
                    end,
                    speaker: 0,
                });
            }
        }
        if local.len() > 1 && local[0].end - local[0].start < min_piece {
            let start = local[0].start;
            local.remove(0);
            local[0].start = start;
        }
        pieces.extend(local);
    }
    pieces
}

/// A piece longer than `max_seconds` times this, with no VAD gap to cut at, is cut at its quietest point.
const HARD: f64 = 1.25;

/// Split pieces longer than the ASR's `max_seconds`: models drop the first part of longer input.
/// Cut at a VAD gap; only a piece longer than `HARD` times that with no gap is cut at the quietest
/// 0.1 s, since a cut inside speech costs more than the length does.
pub fn split_long(
    pieces: Vec<SpeakerSpan>,
    vad: &[(f64, f64)],
    pcm: &[f32],
    max_seconds: f64,
) -> Vec<SpeakerSpan> {
    if max_seconds <= 0.0 {
        return pieces;
    }
    let mut out = Vec::new();
    for piece in pieces {
        let mut start = piece.start;
        while piece.end - start > max_seconds {
            let low = start + max_seconds / 2.0;
            let high = start + max_seconds;
            let hard = start + max_seconds * HARD;
            let gap_in = |to: f64| {
                vad.windows(2)
                    .map(|w| (w[0].1, w[1].0))
                    // Only a gap that lies inside what is left of the piece
                    .rfind(|(a, b)| {
                        start < *a
                            && a <= b
                            && *b < piece.end
                            && low <= (a + b) / 2.0
                            && (a + b) / 2.0 <= to
                    })
            };
            // Leave the silence out of both pieces, so neither pad picks up the other's speech
            let (end, next) = match gap_in(high).or_else(|| gap_in(hard)) {
                Some(gap) => gap,
                None if piece.end > hard => {
                    let cut = quietest(pcm, low, high);
                    (cut, cut)
                }
                None => break,
            };
            out.push(SpeakerSpan {
                start,
                end,
                speaker: piece.speaker,
            });
            start = next;
        }
        out.push(SpeakerSpan {
            start,
            end: piece.end,
            speaker: piece.speaker,
        });
    }
    out
}

fn quietest(pcm: &[f32], low: f64, high: f64) -> f64 {
    let window = SAMPLE_RATE / 10;
    let first = (low * SAMPLE_RATE as f64) as usize;
    let last = ((high * SAMPLE_RATE as f64) as usize).min(pcm.len());
    let mut best = (f64::MAX, high);
    let mut at = first;
    while at + window <= last {
        let energy: f64 = pcm[at..at + window]
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum();
        if energy < best.0 {
            best = (energy, (at + window / 2) as f64 / SAMPLE_RATE as f64);
        }
        at += window / 2;
    }
    best.1
}

/// Run VAD, diarization, padded ASR and speaker assignment over 16 kHz mono samples.
/// Shortest silence, in seconds, that counts as a pause between two words.
const PAUSE: f64 = 0.15;
/// A frame is silent when it is this many dB below the piece's loud frames.
const SILENCE_DB: f32 = 25.0;
/// Tokens come out of the recognizer a little after they are spoken, so the search for the
/// silence after a word starts this much before that word's last token.
const TOKEN_LAG: f64 = 0.2;
/// A run with fewer non-whitespace characters, or shorter than this, joins its neighbour.
const RUN_CHARS: usize = 4;
const RUN_SECONDS: f64 = 0.6;

/// Offsets (in non-whitespace characters) where the audio between two tokens has a pause.
/// `origin` is the time in `pcm` that token time 0 refers to.
fn pauses(pcm: &[f32], origin: f64, tokens: &[Token]) -> Vec<usize> {
    let times: Vec<f64> = tokens
        .iter()
        .flat_map(|token| {
            let at = origin + f64::from(token.start);
            token
                .text
                .chars()
                .filter(|c| !c.is_whitespace())
                .map(move |_| at)
        })
        .collect();
    let (Some(&first), Some(&last)) = (times.first(), times.last()) else {
        return Vec::new();
    };
    const HOP: usize = SAMPLE_RATE / 100;
    let db = |frame: usize| {
        let samples = pcm
            .get(frame * HOP..((frame + 1) * HOP).min(pcm.len()))
            .unwrap_or(&[]);
        let power = samples.iter().map(|x| x * x).sum::<f32>() / samples.len().max(1) as f32;
        10.0 * (power + 1e-10).log10()
    };
    let frame = |t: f64| (t.max(0.0) * 100.0) as usize;
    let mut levels: Vec<f32> = (frame(first)..=frame(last)).map(db).collect();
    if levels.is_empty() {
        return Vec::new();
    }
    let rank = (levels.len() - 1) * 9 / 10;
    let loud = *levels.select_nth_unstable_by(rank, f32::total_cmp).1;
    let silent = loud - SILENCE_DB;
    let mut out = Vec::new();
    for k in 1..times.len() {
        if times[k] <= times[k - 1] {
            continue;
        }
        let (mut run, mut longest) = (0, 0);
        for f in frame(times[k - 1] - TOKEN_LAG)..=frame(times[k]) {
            run = if db(f) < silent { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        if longest as f64 / 100.0 >= PAUSE {
            out.push(k);
        }
    }
    out
}

/// Consecutive tokens of one speaker inside a first-pass piece.
struct Run {
    speaker: usize,
    tokens: Vec<Token>,
    first: f64,
    last: f64,
}

fn run_is_short(run: &Run) -> bool {
    let chars = run
        .tokens
        .iter()
        .flat_map(|token| token.text.chars())
        .filter(|c| !c.is_whitespace())
        .count();
    chars < RUN_CHARS || run.last - run.first < RUN_SECONDS
}

/// Split an overlap-path piece at speaker changes, using the token times to place the cuts.
fn split_at_speakers(
    piece: &SpeakerSpan,
    output: &AsrOutput,
    turns: &[SpeakerSpan],
    pcm: &[f32],
    options: &PipelineOptions<'_>,
    replace: &mut Option<&mut dyn FnMut(&str) -> String>,
) -> Vec<Segment> {
    let origin = piece.start - options.pad;
    let mut runs: Vec<Run> = Vec::new();
    for token in &output.tokens {
        let time = (origin + f64::from(token.start) - TOKEN_LAG).clamp(piece.start, piece.end);
        let speaker = dominant(turns, time - 0.1, time + 0.1);
        if let Some(run) = runs.last_mut().filter(|run| run.speaker == speaker) {
            run.tokens.push(token.clone());
            run.last = time;
        } else {
            runs.push(Run {
                speaker,
                tokens: vec![token.clone()],
                first: time,
                last: time,
            });
        }
    }
    if runs.is_empty() {
        runs.push(Run {
            speaker: dominant(turns, piece.start, piece.end),
            tokens: Vec::new(),
            first: piece.start,
            last: piece.end,
        });
    }
    while runs.len() > 1 {
        let Some(index) = runs.iter().position(run_is_short) else {
            break;
        };
        if index == 0 {
            let first = runs.remove(0);
            let next = &mut runs[0];
            let mut tokens = first.tokens;
            tokens.extend(std::mem::take(&mut next.tokens));
            next.tokens = tokens;
            next.first = first.first;
        } else {
            let run = runs.remove(index);
            let previous = &mut runs[index - 1];
            previous.tokens.extend(run.tokens);
            previous.last = run.last;
        }
    }
    let mut segments = Vec::with_capacity(runs.len());
    for (index, run) in runs.iter().enumerate() {
        let start = if index == 0 { piece.start } else { run.first };
        let end = runs.get(index + 1).map_or(piece.end, |next| next.first);
        let mut text = if output.tokens.is_empty() {
            output.text.clone()
        } else {
            run.tokens.iter().map(|token| token.text.as_str()).collect()
        };
        text = text.trim().to_owned();
        if let Some(punctuate) = options.punctuate {
            let at = pauses(pcm, origin, &run.tokens);
            text = punctuate(&text, &at);
        }
        if let Some(replacer) = replace.as_deref_mut() {
            text = replacer(&text);
        }
        if !text.is_empty() {
            segments.push(Segment {
                start,
                end,
                speaker: run.speaker,
                text,
            });
        }
    }
    segments
}

pub fn run(
    pcm: &[f32],
    asr: &mut dyn AsrEngine,
    vad: &mut dyn Vad,
    diarizer: &mut dyn Diarizer,
    options: &PipelineOptions<'_>,
    mut replace: Option<&mut dyn FnMut(&str) -> String>,
) -> Result<Vec<Segment>, String> {
    let vad_started = std::time::Instant::now();
    let raw_vad = vad.spans(pcm)?;
    let vad_seconds = vad_started.elapsed().as_secs_f64();
    let total = pcm.len() as f64 / SAMPLE_RATE as f64;
    let spans = build_spans(&raw_vad, total, pcm);
    let raw: Vec<(f64, f64)> = raw_vad.iter().map(|s| (s.start, s.end)).collect();
    let max_seconds = f64::from(asr.capabilities().max_seconds);
    let timestamps = asr.capabilities().timestamps;

    let (turns, pieces, outputs, diar_seconds, asr_seconds, overlap_seconds) =
        if options.overlap_engines > 0 {
            let limit = asr.limit();
            let _restore = RestoreLimit(limit.clone());
            if let Some(limit) = &limit {
                limit.store(
                    options.overlap_engines,
                    std::sync::atomic::Ordering::Relaxed,
                );
            }
            let mut first_pieces = split_long(
                cut_at_silences(&spans, &raw, CUT_GAP, options.min_piece),
                &raw,
                pcm,
                max_seconds,
            );
            let diar_micros = std::sync::atomic::AtomicU64::new(0);
            let mut diarization: Option<Result<Vec<SpeakerSpan>, String>> = None;
            let mut first_outputs: Option<Result<Vec<AsrOutput>, String>> = None;
            let mut first_seconds = 0.0;
            std::thread::scope(|scope| {
                let handle = {
                    let diar_micros = &diar_micros;
                    scope.spawn(move || {
                        let _restore = RestoreLimit(limit.clone());
                        let started = std::time::Instant::now();
                        let result = diarizer.diarize_with_speech(pcm, &raw_vad);
                        diar_micros.store(
                            started.elapsed().as_micros() as u64,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        result
                    })
                };
                let started = std::time::Instant::now();
                first_outputs = Some(transcribe_pieces(asr, &first_pieces, pcm, total, options));
                first_seconds = started.elapsed().as_secs_f64();
                diarization = Some(match handle.join() {
                    Ok(result) => result,
                    Err(_) => Err("話者分離の処理が止まりました。".to_owned()),
                });
            });
            let turns = turn_list(diarization.unwrap()?);
            let first_outputs = first_outputs.unwrap()?;
            let diar_seconds =
                diar_micros.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1_000_000.0;
            let overlap_seconds = first_seconds.min(diar_seconds);
            let additions = additions_for(&turns, &spans, options.fix_overlapping_turn_spans);
            let (add_pieces, add_outputs, add_seconds) = if additions.is_empty() {
                (Vec::new(), Vec::new(), 0.0)
            } else {
                let add_pieces = split_long(
                    plan(&additions, &turns, options.min_piece),
                    &raw,
                    pcm,
                    max_seconds,
                );
                let started = std::time::Instant::now();
                let add_outputs = transcribe_pieces(asr, &add_pieces, pcm, total, options)?;
                (add_pieces, add_outputs, started.elapsed().as_secs_f64())
            };
            for piece in &mut first_pieces {
                piece.speaker = dominant(&turns, piece.start, piece.end);
            }
            let mut entries: Vec<(SpeakerSpan, AsrOutput)> =
                first_pieces.into_iter().zip(first_outputs).collect();
            entries.extend(add_pieces.into_iter().zip(add_outputs));
            entries.sort_by(|left, right| left.0.start.total_cmp(&right.0.start));
            let (pieces, outputs): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
            (
                turns,
                pieces,
                outputs,
                diar_seconds,
                first_seconds + add_seconds,
                overlap_seconds,
            )
        } else {
            let started = std::time::Instant::now();
            let turns = turn_list(diarizer.diarize_with_speech(pcm, &raw_vad)?);
            let diar_seconds = started.elapsed().as_secs_f64();
            let additions = additions_for(&turns, &spans, options.fix_overlapping_turn_spans);
            let mut all = spans.clone();
            all.extend(additions);
            all.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
            let pieces = split_long(
                plan(&all, &turns, options.min_piece),
                &raw,
                pcm,
                max_seconds,
            );
            let started = std::time::Instant::now();
            let outputs = transcribe_pieces(asr, &pieces, pcm, total, options)?;
            (
                turns,
                pieces,
                outputs,
                diar_seconds,
                started.elapsed().as_secs_f64(),
                0.0,
            )
        };

    let segments = build_segments(
        &pieces,
        outputs,
        &turns,
        pcm,
        timestamps,
        options,
        &mut replace,
    );
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
    if options.overlap_engines > 0 {
        let after = (asr_seconds - overlap_seconds).max(0.0);
        crate::notice::info(&format!(
            "所要時間: VAD {vad_seconds:.1} 秒 / 話者分離 {diar_seconds:.1} 秒 / 認識 {asr_seconds:.1} 秒（うち話者分離と同時 {overlap_seconds:.1} 秒、その後 {after:.1} 秒）"
        ));
    } else {
        crate::notice::info(&format!(
            "所要時間: VAD {vad_seconds:.1} 秒 / 話者分離 {diar_seconds:.1} 秒 / 認識 {asr_seconds:.1} 秒"
        ));
    }
    Ok(segments)
}

/// 途中で戻っても、認識器の上限を元に戻す。
struct RestoreLimit(Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>);

impl Drop for RestoreLimit {
    fn drop(&mut self) {
        if let Some(limit) = &self.0 {
            limit.store(usize::MAX, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// VAD の区間を前後 0.6 秒広げてつなぎ、発話が無くても音が大きければ全体を 1 区間にする。
fn build_spans(raw_vad: &[VadSpan], total: f64, pcm: &[f32]) -> Vec<(f64, f64)> {
    let mut spans: Vec<(f64, f64)> = Vec::new();
    for span in raw_vad.iter() {
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
    spans
}

/// VAD から漏れた話者分離だけの区間。既に VAD が覆っている区間は除く。
fn additions_for(turns: &[SpeakerSpan], spans: &[(f64, f64)], fix: bool) -> Vec<(f64, f64)> {
    let mut additions = Vec::new();
    for turn in turns {
        if turn.end - turn.start < 0.5
            || spans
                .iter()
                .any(|(a, b)| turn.end.min(*b) - turn.start.max(*a) > 0.2)
        {
            continue;
        }
        let span = (turn.start, turn.end);
        if fix
            && additions
                .iter()
                .any(|(a, b): &(f64, f64)| span.1.min(*b) - span.0.max(*a) > 0.0)
        {
            continue;
        }
        additions.push(span);
    }
    additions
}

fn turn_list(mut turns: Vec<SpeakerSpan>) -> Vec<SpeakerSpan> {
    turns.sort_by(|a, b| {
        a.start
            .total_cmp(&b.start)
            .then(a.end.total_cmp(&b.end))
            .then(a.speaker.cmp(&b.speaker))
    });
    merge_turns(&turns, 0.6)
}

/// 区間の前後に、隣の区間との隙間の半分（最大 0.4 秒）と指定の余白を足した波形を作る。
fn pad_waves(pieces: &[SpeakerSpan], pcm: &[f32], total: f64, pad: f64) -> Vec<Vec<f32>> {
    let mut bounds: Vec<f64> = pieces.iter().flat_map(|p| [p.start, p.end]).collect();
    bounds.sort_by(f64::total_cmp);
    bounds.dedup_by(|a, b| *a == *b);
    let mut rng = StdRng::seed_from_u64(0);
    let mut waves = Vec::with_capacity(pieces.len());
    for piece in pieces {
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
        let mut wave = noise(pad - left_pad, &mut rng);
        wave.extend_from_slice(&pcm[start.min(pcm.len())..end.min(pcm.len())]);
        wave.extend(noise(pad - right_pad, &mut rng));
        waves.push(wave);
    }
    waves
}

fn transcribe_pieces(
    asr: &mut dyn AsrEngine,
    pieces: &[SpeakerSpan],
    pcm: &[f32],
    total: f64,
    options: &PipelineOptions<'_>,
) -> Result<Vec<AsrOutput>, String> {
    let waves = pad_waves(pieces, pcm, total, options.pad);
    let timestamps = asr.capabilities().timestamps;
    let requests: Vec<AsrRequest<'_>> = waves
        .iter()
        .map(|wave| AsrRequest {
            pcm: wave,
            context: options.context,
            language: options.language,
            want_timestamps: timestamps,
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
    Ok(outputs)
}

fn build_segments(
    pieces: &[SpeakerSpan],
    outputs: Vec<AsrOutput>,
    turns: &[SpeakerSpan],
    pcm: &[f32],
    timestamps: bool,
    options: &PipelineOptions<'_>,
    replace: &mut Option<&mut dyn FnMut(&str) -> String>,
) -> Vec<Segment> {
    let mut segments = Vec::new();
    for (piece, mut output) in pieces.iter().zip(outputs) {
        if options.overlap_engines > 0 && !timestamps && !output.tokens.is_empty() {
            segments.extend(split_at_speakers(
                piece, &output, turns, pcm, options, replace,
            ));
        } else if timestamps && !output.tokens.is_empty() {
            for token in output.tokens {
                let mut text = token.text;
                if let Some(replacer) = replace.as_deref_mut() {
                    text = replacer(&text);
                }
                if !text.is_empty() {
                    let start = piece.start - options.pad + f64::from(token.start);
                    let end = piece.start - options.pad + f64::from(token.end);
                    segments.push(Segment {
                        start,
                        end,
                        speaker: dominant(turns, start, end),
                        text,
                    });
                }
            }
        } else {
            output.text = output.text.trim().to_owned();
            if let Some(punctuate) = options.punctuate {
                let at = pauses(pcm, piece.start - options.pad, &output.tokens);
                output.text = punctuate(&output.text, &at);
            }
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
    segments
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
    fn long_pieces_split_in_vad_gaps_and_keep_the_silence_out() {
        let piece = SpeakerSpan {
            start: 0.0,
            end: 40.0,
            speaker: 3,
        };
        let vad = [(0.0, 9.0), (9.6, 18.0), (18.8, 27.0), (27.4, 40.0)];
        let pcm = vec![0.0; 40 * SAMPLE_RATE];
        let pieces = split_long(vec![piece], &vad, &pcm, 15.0);
        let edges: Vec<(f64, f64)> = pieces.iter().map(|p| (p.start, p.end)).collect();
        assert_eq!(edges, [(0.0, 9.0), (9.6, 18.0), (18.8, 27.0), (27.4, 40.0)]);
        assert!(pieces.iter().all(|p| p.speaker == 3));
    }

    #[test]
    fn a_gap_reaching_outside_the_piece_is_not_a_cut() {
        // The diarizer cut this piece inside a long VAD gap (9 s to 30 s)
        let piece = SpeakerSpan {
            start: 12.0,
            end: 40.0,
            speaker: 0,
        };
        let vad = [(0.0, 9.0), (30.0, 40.0)];
        let pcm = vec![0.5; 40 * SAMPLE_RATE];
        let pieces = split_long(vec![piece], &vad, &pcm, 20.0);
        assert!(pieces.iter().all(|p| p.start < p.end), "{pieces:?}");
        assert_eq!(pieces.first().unwrap().start, 12.0);
        assert_eq!(pieces.last().unwrap().end, 40.0);
    }

    #[test]
    fn a_piece_without_a_gap_stays_whole_up_to_the_hard_limit() {
        let piece = SpeakerSpan {
            start: 0.0,
            end: 24.0,
            speaker: 0,
        };
        let pcm = vec![0.5; 24 * SAMPLE_RATE];
        let pieces = split_long(vec![piece], &[(0.0, 24.0)], &pcm, 20.0);
        assert_eq!(pieces.len(), 1);
    }

    #[test]
    fn long_pieces_without_a_gap_split_at_the_quietest_point() {
        let piece = SpeakerSpan {
            start: 0.0,
            end: 20.0,
            speaker: 0,
        };
        let mut pcm = vec![0.5; 20 * SAMPLE_RATE];
        pcm[12 * SAMPLE_RATE..12 * SAMPLE_RATE + SAMPLE_RATE / 5].fill(0.0);
        let pieces = split_long(vec![piece], &[(0.0, 20.0)], &pcm, 15.0);
        assert_eq!(pieces.len(), 2);
        assert!((pieces[0].end - 12.1).abs() < 0.06, "{}", pieces[0].end);
        assert_eq!(pieces[0].end, pieces[1].start);
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
    fn overlap_runs_the_first_pass_before_diarization_finishes() {
        let pcm = vec![0.01; SAMPLE_RATE * 4];
        let mut asr = DummyAsr::new("hello");
        let mut vad = DummyVad(vec![crate::VadSpan {
            start: 0.0,
            end: 2.0,
        }]);
        let mut diarizer = DummyDiarizer(vec![
            SpeakerSpan {
                start: 0.0,
                end: 2.0,
                speaker: 0,
            },
            SpeakerSpan {
                start: 3.0,
                end: 4.0,
                speaker: 1,
            },
        ]);
        let result = run(
            &pcm,
            &mut asr,
            &mut vad,
            &mut diarizer,
            &PipelineOptions {
                overlap_engines: 1,
                ..PipelineOptions::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!((result[0].speaker, result[1].speaker), (0, 1));
        assert_eq!((result[1].start, result[1].end), (3.0, 4.0));
    }

    fn token(text: &str, start: f32) -> Token {
        Token {
            text: text.to_owned(),
            start,
            end: start + 0.1,
            confidence: None,
        }
    }

    #[test]
    fn speaker_changes_split_a_piece_at_the_tokens() {
        let piece = SpeakerSpan {
            start: 0.0,
            end: 3.0,
            speaker: 0,
        };
        let turns = vec![
            SpeakerSpan {
                start: 0.0,
                end: 1.5,
                speaker: 0,
            },
            SpeakerSpan {
                start: 1.5,
                end: 3.0,
                speaker: 1,
            },
        ];
        let output = AsrOutput {
            text: "Hello there Good day".to_owned(),
            tokens: vec![
                token("Hello", 0.3),
                token(" there", 1.0),
                token("Good", 1.9),
                token(" day", 2.6),
            ],
        };
        let options = PipelineOptions {
            pad: 0.0,
            ..PipelineOptions::default()
        };
        let mut replace: Option<&mut dyn FnMut(&str) -> String> = None;
        let segments = split_at_speakers(
            &piece,
            &output,
            &turns,
            &vec![0.0; SAMPLE_RATE * 3],
            &options,
            &mut replace,
        );
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Hello there");
        assert_eq!(segments[1].text, "Good day");
        assert_eq!(segments[0].start, 0.0);
        assert!((segments[0].end - 1.7).abs() < 1e-6);
        assert!((segments[1].start - 1.7).abs() < 1e-6);
        assert_eq!(segments[1].end, 3.0);
        assert_eq!((segments[0].speaker, segments[1].speaker), (0, 1));
    }

    #[test]
    fn a_short_blip_joins_the_previous_run() {
        let piece = SpeakerSpan {
            start: 0.0,
            end: 5.0,
            speaker: 0,
        };
        let turns = vec![
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
            SpeakerSpan {
                start: 4.0,
                end: 5.0,
                speaker: 0,
            },
        ];
        let output = AsrOutput {
            text: "Hello there ab Good day".to_owned(),
            tokens: vec![
                token("Hello", 0.3),
                token(" there", 1.0),
                token("ab", 2.4),
                token("Good", 4.3),
                token(" day", 5.0),
            ],
        };
        let options = PipelineOptions {
            pad: 0.0,
            ..PipelineOptions::default()
        };
        let mut replace: Option<&mut dyn FnMut(&str) -> String> = None;
        let segments = split_at_speakers(
            &piece,
            &output,
            &turns,
            &vec![0.0; SAMPLE_RATE * 5],
            &options,
            &mut replace,
        );
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Hello thereab");
        assert_eq!(segments[0].speaker, 0);
        assert_eq!(segments[1].speaker, 0);
    }

    #[test]
    fn a_piece_without_tokens_stays_one_run() {
        let piece = SpeakerSpan {
            start: 1.0,
            end: 3.0,
            speaker: 5,
        };
        let turns = vec![
            SpeakerSpan {
                start: 1.0,
                end: 2.0,
                speaker: 1,
            },
            SpeakerSpan {
                start: 2.0,
                end: 3.0,
                speaker: 0,
            },
        ];
        let output = AsrOutput {
            text: " hello ".to_owned(),
            tokens: Vec::new(),
        };
        let options = PipelineOptions::default();
        let mut replace: Option<&mut dyn FnMut(&str) -> String> = None;
        let segments = split_at_speakers(
            &piece,
            &output,
            &turns,
            &vec![0.0; SAMPLE_RATE * 4],
            &options,
            &mut replace,
        );
        assert_eq!(segments.len(), 1);
        assert_eq!((segments[0].start, segments[0].end), (1.0, 3.0));
        assert_eq!(segments[0].text, "hello");
    }

    #[test]
    fn silences_inside_a_merged_span_cut_it_at_their_midpoints() {
        let span = (0.0, 10.0);
        let raw = [(1.0, 2.0), (2.5, 3.0), (3.5, 4.0)];
        let pieces = cut_at_silences(&[span], &raw, 0.3, 0.5);
        let edges: Vec<(f64, f64)> = pieces.iter().map(|p| (p.start, p.end)).collect();
        assert_eq!(edges, [(0.0, 2.25), (2.25, 3.25), (3.25, 10.0)]);
        assert!(pieces.iter().all(|p| p.speaker == 0));
    }

    #[test]
    fn a_silence_shorter_than_the_minimum_does_not_cut() {
        let span = (0.0, 3.0);
        let raw = [(1.0, 2.0), (2.2, 3.0)];
        let pieces = cut_at_silences(&[span], &raw, 0.3, 0.5);
        let edges: Vec<(f64, f64)> = pieces.iter().map(|p| (p.start, p.end)).collect();
        assert_eq!(edges, [(0.0, 3.0)]);
    }

    #[test]
    fn a_piece_shorter_than_the_minimum_joins_its_previous_neighbour() {
        let span = (0.0, 10.0);
        let raw = [(1.0, 2.0), (2.4, 2.6), (3.0, 4.0)];
        let pieces = cut_at_silences(&[span], &raw, 0.3, 1.0);
        let edges: Vec<(f64, f64)> = pieces.iter().map(|p| (p.start, p.end)).collect();
        assert_eq!(edges.len(), 2);
        assert!((edges[0].0 - 0.0).abs() < 1e-9);
        assert!((edges[0].1 - 2.8).abs() < 1e-9);
        assert!((edges[1].0 - 2.8).abs() < 1e-9);
        assert!((edges[1].1 - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_short_first_piece_joins_the_next_one() {
        let span = (0.0, 10.0);
        let raw = [(0.0, 0.2), (0.6, 1.0)];
        let pieces = cut_at_silences(&[span], &raw, 0.3, 1.0);
        let edges: Vec<(f64, f64)> = pieces.iter().map(|p| (p.start, p.end)).collect();
        assert_eq!(edges, [(0.0, 10.0)]);
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
