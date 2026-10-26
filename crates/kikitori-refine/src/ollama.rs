//! Talking to an Ollama endpoint over HTTP (Python `refine/ollama.py`).
//!
//! The endpoint is KIKITORI_OLLAMA, then OLLAMA_HOST, then localhost:11434.
//! Values like "http://host:port", "host:port", "host" and "https://host"
//! all work; a listening wildcard 0.0.0.0 is treated as localhost.

use serde_json::{json, Value};
use std::{collections::BTreeMap, io::BufRead, time::Duration};

use kikitori_fetch::http_agent;

pub const DEFAULT: &str = "http://localhost:11434";
pub const CHAT_TIMEOUT: Duration = Duration::from_secs(1200);
pub const PULL_TIMEOUT: Duration = Duration::from_secs(3600);
pub const SMALL_TIMEOUT: Duration = Duration::from_secs(3);

/// Resolve the endpoint base. `explicit` wins over the environment.
pub fn base(explicit: Option<&str>) -> String {
    let mut value = explicit
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::env::var("KIKITORI_OLLAMA")
                .or_else(|_| std::env::var("OLLAMA_HOST"))
                .unwrap_or_else(|_| DEFAULT.to_owned())
                .trim()
                .to_owned()
        });
    if !value.contains("://") {
        value = format!("http://{value}");
    }
    let Some((scheme, rest)) = value.split_once("://") else {
        return value;
    };
    let mut rest = rest.trim_end_matches('/').to_owned();
    let (scheme, rest): (String, String) = if scheme == "http" {
        let tail = rest.rsplit_once(']').map_or(&rest[..], |(_, tail)| tail);
        if !tail.contains(':') {
            rest.push_str(":11434");
        }
        (scheme.to_owned(), rest)
    } else {
        (scheme.to_owned(), rest)
    };
    let rest = if rest.starts_with("0.0.0.0:") {
        format!(
            "localhost:{}",
            rest.split_once(':').map(|(_, t)| t).unwrap_or("")
        )
    } else if rest == "0.0.0.0" {
        "localhost:11434".to_owned()
    } else {
        rest
    };
    format!("{scheme}://{rest}")
}

/// Whether the endpoint is this PC (localhost).
pub fn is_local(base: &str) -> bool {
    let host = match base.split_once("://") {
        Some((_, rest)) => match rest.split_once(':') {
            Some((h, _)) => h,
            None => rest,
        },
        None => return false,
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

fn parse_json(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice(bytes).ok()
}

fn get_json(at: &str, path: &str, timeout: Duration) -> Option<Value> {
    let agent = http_agent(timeout);
    let response = agent.get(format!("{at}{path}")).call().ok()?;
    let body = response.into_body().read_to_vec().ok()?;
    parse_json(&body)
}

fn post_json(at: &str, path: &str, body: &Value, timeout: Duration) -> Option<Value> {
    let agent = http_agent(timeout);
    let response = agent
        .post(format!("{at}{path}"))
        .header("Content-Type", "application/json")
        .send_json(body)
        .ok()?;
    let read = response.into_body().read_to_vec().ok()?;
    parse_json(&read)
}

/// Models on the endpoint: {name: bytes}. None when it does not answer.
pub fn models(at: &str, timeout: Duration) -> Option<BTreeMap<String, u64>> {
    let d = get_json(at, "/api/tags", timeout)?;
    let list: Vec<_> = d
        .get("models")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    Some(
        list.into_iter()
            .filter_map(|m| {
                let name = m.get("name")?.as_str()?.to_owned();
                let size = m.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
                Some((name, size))
            })
            .collect(),
    )
}

/// Whether the endpoint serves `/v1/systemone` (older Ollama does not).
pub fn has_systemone(at: &str, timeout: Duration) -> bool {
    let agent = http_agent(timeout);
    let request = agent
        .post(format!("{at}/v1/systemone"))
        .header("Content-Type", "application/json");
    match request.send(b"{}".as_slice()) {
        Ok(_) => true,
        Err(ureq::Error::StatusCode(code)) => code != 404 && code != 405,
        Err(_) => false,
    }
}

/// Whether the endpoint answers with a version: (0, 40, 8) shape.
pub fn version(at: &str, timeout: Duration) -> Option<(u32, u32, u32)> {
    let v = get_json(at, "/api/version", timeout)?
        .get("version")?
        .as_str()?
        .to_owned();
    let head = v.split('-').next().unwrap_or("");
    let mut nums: Vec<u32> = head
        .split('.')
        .take(3)
        .map(|p| p.parse::<u32>().unwrap_or(0))
        .collect();
    while nums.len() < 3 {
        nums.push(0);
    }
    Some((nums[0], nums[1], nums[2]))
}

/// One non-streaming chat turn; returns the reply content, with a think
/// block removed (the request asks `think: false` anyway).
pub fn chat(at: &str, model: &str, system: &str, user: &str) -> Result<String, String> {
    let body = json!({
        "model": model,
        "stream": false,
        "think": false,
        "options": {"temperature": 0, "num_ctx": 32768},
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    });
    let reply = post_json(at, "/api/chat", &body, CHAT_TIMEOUT)
        .ok_or_else(|| "Ollama が答えません".to_owned())?;
    let content = reply
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "Ollama の答えが読めません".to_owned())?;
    Ok(crate::prompts::strip_think(content).to_owned())
}

/// Ask the tev1 scorer and return the answer probabilities.
pub fn systemone(at: &str, body: &Value) -> Result<Value, String> {
    post_json(at, "/v1/systemone", body, Duration::from_secs(120))
        .ok_or_else(|| "Ollama が答えません".to_owned())
}

/// Pull a model via /api/pull. Re-running resumes an interrupted pull.
pub fn pull(at: &str, model: &str, mut log: impl FnMut(&str)) -> Result<(), String> {
    let body = json!({"model": model, "stream": true});
    let agent = http_agent(PULL_TIMEOUT);
    let response = agent
        .post(format!("{at}/api/pull"))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(|e: ureq::Error| e.to_string())?;
    let mut reader = std::io::BufReader::new(response.into_body().into_reader());
    let mut line = Vec::new();
    let mut last: Option<String> = None;
    loop {
        line.clear();
        let n = reader
            .read_until(b'\n', &mut line)
            .map_err(|e: std::io::Error| e.to_string())?;
        if n == 0 {
            break;
        }
        while line.last() == Some(&b'\n') || line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        let Some(d) = parse_json(&line) else {
            return Err("Ollama の進捗行が読めません".to_owned());
        };
        if let Some(e) = d.get("error").and_then(|e| e.as_str()) {
            return Err(e.to_owned());
        }
        let mut msg = d
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_owned();
        if let Some(total) = d.get("total").and_then(|t| t.as_u64()).filter(|t| *t > 0) {
            let completed = d.get("completed").and_then(|c| c.as_u64()).unwrap_or(0);
            msg.push_str(&format!(" {}%", completed * 100 / total));
        }
        if !msg.is_empty() && last.as_deref() != Some(msg.as_str()) {
            log(&msg);
            last = Some(msg);
        }
    }
    Ok(())
}
