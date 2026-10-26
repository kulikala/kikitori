//! Ollama の代わりの簡易 HTTP サーバ。merge・glossary_fix・`--setup` の続きを確かめる。

use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Fakes {
    /// 例: http://127.0.0.1:5xxxx
    pub addr: String,
    pub version_on: Arc<AtomicBool>,
    /// /api/chat に 1 回ずつ、先頭から出す。NDJSON そのまま。
    pub chat: Arc<Mutex<VecDeque<String>>>,
    /// /v1/systemone に 1 回ずつ、先頭から出す。
    pub systemone: Arc<Mutex<VecDeque<String>>>,
    /// 届いた POST（path\tbody）の記録（バッファ的に見て、最後に検査する）。
    pub received: Arc<Mutex<Vec<String>>>,
    /// /api/pull への応答。モデル名で選ぶ（NDJSON そのまま）。
    pub pulls: Arc<Mutex<BTreeMap<String, String>>>,
}

fn read_request(stream: &mut TcpStream) -> Option<(String, String, String)> {
    let mut method = String::new();
    let mut path = String::new();
    let mut content_length = 0usize;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte).ok()?;
        if n == 0 {
            break;
        }
        line.push(byte[0]);
        if byte[0] == b'\n' {
            let text = String::from_utf8_lossy(&line).trim().to_string();
            line.clear();
            if text.is_empty() {
                break;
            }
            if text.starts_with("GET ") || text.starts_with("POST ") {
                let mut it = text.split_whitespace();
                method = it.next().unwrap_or("").to_owned();
                path = it.next().unwrap_or("").to_owned();
            }
            if let Some(v) = text.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream.read_exact(&mut body).ok()?;
    }
    Some((method, path, String::from_utf8_lossy(&body).to_string()))
}

fn respond(stream: &mut TcpStream, code: u16, body: &str) {
    let head = format!(
        "HTTP/1.1 {code} x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Both);
}

fn handle(f: Fakes, mut stream: TcpStream) {
    let Some((method, path, body)) = read_request(&mut stream) else {
        return;
    };
    if method != "GET" {
        f.received.lock().unwrap().push(format!("{path}\t{body}"));
    }
    if path.starts_with("/api/version") {
        if f.version_on.load(Ordering::Relaxed) {
            respond(&mut stream, 200, r#"{"version":"0.40.8"}"#);
        } else {
            respond(&mut stream, 500, r#"{"error":"down"}"#);
        }
        return;
    }
    if path.starts_with("/api/tags") {
        respond(&mut stream, 200, r#"{"models":[]}"#);
        return;
    }
    if path.starts_with("/api/chat") {
        let next = f.chat.lock().unwrap().pop_front();
        match next {
            Some(nd) => respond(&mut stream, 200, &nd),
            None => respond(&mut stream, 500, r#"{"error":"no canned chat"}"#),
        }
        return;
    }
    if path.starts_with("/api/pull") {
        let model = body
            .split("\"model\":")
            .nth(1)
            .and_then(|v| v.split('"').nth(1))
            .unwrap_or("")
            .to_owned();
        let streams = f.pulls.lock().unwrap();
        let nd = streams
            .get(&model)
            .cloned()
            .unwrap_or_else(|| "{}\n".to_owned());
        drop(streams);
        respond(&mut stream, 200, &nd);
        return;
    }
    if path.starts_with("/v1/systemone") {
        let next = f.systemone.lock().unwrap().pop_front();
        match next {
            Some(x) => respond(&mut stream, 200, &x),
            None => respond(&mut stream, 500, r#"{"error":"down"}"#),
        }
        return;
    }
    respond(&mut stream, 404, "{}");
}

pub fn spawn() -> Fakes {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let f = Fakes {
        addr: format!("http://{addr}"),
        version_on: Arc::new(AtomicBool::new(true)),
        chat: Arc::new(Mutex::new(VecDeque::new())),
        systemone: Arc::new(Mutex::new(VecDeque::new())),
        received: Arc::new(Mutex::new(Vec::new())),
        pulls: Arc::new(Mutex::new(BTreeMap::new())),
    };
    let persist = f.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let f = persist.clone();
            std::thread::spawn(move || handle(f, stream));
        }
    });
    f
}
