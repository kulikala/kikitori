use crate::Segment;
use serde::Serialize;
use std::io::Write;

#[derive(Serialize)]
struct Versioned<'a> {
    schema_version: u8,
    segments: &'a [Segment],
}

pub(super) fn emit(segments: &[Segment], format: &str) -> Result<Vec<u8>, String> {
    let text = match format {
        "txt" => segments
            .iter()
            .map(|s| {
                format!(
                    "[{}] 話者{}: {}\n",
                    clock(s.start, 2),
                    s.speaker + 1,
                    s.text
                )
            })
            .collect(),
        "json" => json(segments)?,
        "json-v1" => json(&Versioned {
            schema_version: 1,
            segments,
        })?,
        "yaml" => yaml(segments),
        "srt" => srt(segments),
        "vtt" => vtt(segments),
        other => return Err(format!("unsupported format: {other}")),
    };
    Ok(text.into_bytes())
}

fn json<T: Serialize + ?Sized>(value: &T) -> Result<String, String> {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value
        .serialize(&mut serializer)
        .map_err(|e| e.to_string())?;
    out.push(b'\n');
    String::from_utf8(out).map_err(|e| e.to_string())
}

fn yaml(segments: &[Segment]) -> String {
    if segments.is_empty() {
        return "[]\n".to_owned();
    }
    let mut out = String::new();
    for s in segments {
        out.push_str(&format!(
            "- start: {:?}\n  end: {:?}\n  speaker: {}\n  text: \"{}\"\n",
            s.start,
            s.end,
            s.speaker,
            yaml_escape(&s.text)
        ));
    }
    out
}

fn yaml_escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || matches!(c as u32, 0x7f | 0x85 | 0x2028 | 0x2029) => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

fn clock(seconds: f64, precision: usize) -> String {
    let mins = (seconds / 60.0).floor() as i64;
    let secs = seconds.rem_euclid(60.0);
    format!(
        "{mins:02}:{secs:0width$.precision$}",
        width = precision + 3,
        precision = precision
    )
}

fn subtitle_time(seconds: f64, comma: bool) -> String {
    let hours = (seconds / 3600.0).floor() as i64;
    let minutes = ((seconds % 3600.0) / 60.0).floor() as i64;
    let secs = seconds % 60.0;
    let rendered = format!("{hours:02}:{minutes:02}:{secs:06.3}");
    if comma {
        rendered.replace('.', ",")
    } else {
        rendered
    }
}

fn srt(segments: &[Segment]) -> String {
    let mut out = String::new();
    for (i, s) in segments.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n話者{}: {}\n\n",
            i + 1,
            subtitle_time(s.start, true),
            subtitle_time(s.end.max(s.start + 1.0), true),
            s.speaker + 1,
            s.text
        ));
    }
    out
}

fn vtt(segments: &[Segment]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for s in segments {
        let text = s
            .text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        out.push_str(&format!(
            "{} --> {}\n<v 話者{}>{}\n\n",
            subtitle_time(s.start, false),
            subtitle_time(s.end, false),
            s.speaker + 1,
            text
        ));
    }
    out
}

#[allow(dead_code)]
fn write_utf8(mut writer: impl Write, segments: &[Segment], format: &str) -> Result<(), String> {
    writer
        .write_all(&super::emit(segments, format)?)
        .map_err(|e| e.to_string())
}
