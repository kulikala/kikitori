//! 手元で完結する補正: 長さの規則、Ollama の大きなモデルでの突き合わせ、
//! tev1 での用語の置き換え (Python `refine/local.py`)。

use regex::Regex;
use std::{collections::BTreeMap, path::Path};

use crate::{ollama, prompts};

const RATIO: f64 = 0.34;

/// fugashi の代わり: UniDic を読み込んで形態素のバイト位置と読みを得る。
pub struct Tagger {
    dict: notmecab::Dict,
}

impl Tagger {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let sysdic = notmecab::Blob::open(dir.join("sys.dic"));
        let unkdic = notmecab::Blob::open(dir.join("unk.dic"));
        let matrix = notmecab::Blob::open(dir.join("matrix.bin"));
        let unkchar = notmecab::Blob::open(dir.join("char.bin"));
        for (name, blob) in [
            ("sys.dic", &sysdic),
            ("unk.dic", &unkdic),
            ("matrix.bin", &matrix),
            ("char.bin", &unkchar),
        ] {
            if blob.is_err() {
                return Err(format!(
                    "Japanese dictionary is missing or invalid at {}: ファイルが読めません",
                    dir.join(name).display()
                ));
            }
        }
        let dict = notmecab::Dict::load(
            sysdic.unwrap(),
            unkdic.unwrap(),
            matrix.unwrap(),
            unkchar.unwrap(),
        )
        .map_err(|e| {
            format!(
                "Japanese dictionary is missing or invalid at {}: {e}",
                dir.display()
            )
        })?;
        Ok(Self { dict })
    }

    /// text の形態素の (バイト位置の始まり, 終わり)。
    pub fn spans(&self, text: &str) -> Vec<(usize, usize)> {
        let Ok((tokens, _)) = self.dict.tokenize(text) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut pos = 0usize;
        for token in tokens {
            if token.range.start < pos {
                continue;
            }
            out.push((token.range.start, token.range.end));
            pos = token.range.end;
        }
        out
    }

    /// Python `local.reading`: UniDic の読み（無ければ表記）をつなぎ、平仮名を片仮名に。
    pub fn reading(&self, s: &str) -> String {
        let mut r = String::new();
        if let Ok((tokens, _)) = self.dict.tokenize(s) {
            for token in tokens {
                let fields: Vec<&str> = token.get_feature(&self.dict).split(',').collect();
                let kana = fields.get(17).copied().unwrap_or("");
                r.push_str(if !kana.is_empty() && kana != "*" {
                    kana
                } else {
                    token.get_text(s)
                });
            }
        }
        _kata(&r)
    }
}

fn _kata(s: &str) -> String {
    s.chars()
        .map(|c| {
            if ('ぁ'..='ゖ').contains(&c) {
                char::from_u32(c as u32 + 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

fn _norm(k: &str) -> String {
    // 長音・促音・「ウ」の揺れと記号を落として、音の比較をゆるくする
    _kata(k)
        .chars()
        .filter(|c| !"ーウッ、。 ・?？!！".contains(*c))
        .collect()
}

fn _lev(a: &[char], b: &[char]) -> usize {
    let mut d: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut prev = d[0];
        d[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let top = d[j + 1];
            let cost = usize::from(x != y);
            let next = (top + 1).min(d[j] + 1).min(prev + cost);
            prev = std::mem::replace(&mut d[j + 1], next);
        }
    }
    d[b.len()]
}

fn _fuzzy_in(sub: &[char], s: &[char]) -> usize {
    let n = sub.len();
    if n == 0 || s.is_empty() {
        return n;
    }
    let mut best = usize::MAX;
    for len in [n.saturating_sub(1).max(1), n, n + 1] {
        let limit = s.len().saturating_sub(len.saturating_sub(1)).max(1);
        for start in 0..limit {
            let window: &[char] = if start + len <= s.len() {
                &s[start..start + len]
            } else {
                s.get(start..).unwrap_or(&[])
            };
            best = best.min(_lev(window, sub));
        }
    }
    best
}

/// B（Qwen3-ASR）が A より十分に長い区間だけ B を採る。A の取りこぼしを B で補うための規則。
pub fn length_rule(a: &[String], b: &[String]) -> Vec<String> {
    let sep = Regex::new(r"[、。,.\s]").unwrap();
    a.iter()
        .zip(b)
        .map(|(a, b)| {
            let ra: String = sep.replace_all(a, "").into_owned();
            let rb: String = sep.replace_all(b, "").into_owned();
            if rb.chars().count() as f64 > ra.chars().count() as f64 * 1.6 + 3.0 {
                b.clone()
            } else {
                a.clone()
            }
        })
        .collect()
}

fn warn(msg: impl std::fmt::Display) {
    kikitori_core::notice::warning(&msg.to_string());
}

/// 区間ごとに 1 回、A・B・K を突き合わせさせる。答えが返らなかった区間は A のまま。
pub fn merge(
    at: &str,
    model: &str,
    a: &[String],
    b: &[String],
    k: &[String],
    glossary: &[(String, String)],
    log: &mut impl FnMut(&str),
) -> Vec<String> {
    let system = prompts::system("merge", glossary);
    let mut out = a.to_vec();
    let n = out.len();
    for (i, slot) in out.iter_mut().enumerate() {
        let user = prompts::user("merge", &[i], a, b, k);
        match ollama::chat(at, model, &system, &user) {
            Ok(reply) => {
                let got = prompts::parse(&reply);
                if let Some(text) = got.get(&i) {
                    *slot = text.clone();
                }
            }
            Err(e) => {
                warn(format_args!(
                    "警告: 区間 {i} の突き合わせに失敗しました（{e}）。本文のまま残します"
                ));
                continue;
            }
        }
        log(&format!("{i}/{n} {slot}"));
    }
    out
}

/// 本文のうち、読みが用語に近い形態素 1〜3 個を、その用語に置き換えた文の候補。
pub fn candidates(
    tagger: &Tagger,
    text: &str,
    kana: &str,
    glossary: &[(String, String)],
) -> Vec<(String, String)> {
    let nk: Vec<char> = _norm(kana).chars().collect();
    let toks = tagger.spans(text);
    let mut cands: Vec<(String, String)> = Vec::new();
    let push = |cands: &mut Vec<(String, String)>, key: String, value: String| {
        if let Some(slot) = cands.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            cands.push((key, value));
        }
    };
    for (w, _) in glossary {
        let rw: Vec<char> = _norm(&tagger.reading(w)).chars().collect();
        if text.contains(w)
            || _fuzzy_in(&rw, &nk) > (rw.len() as f64 * RATIO).max(1.0).trunc() as usize
        {
            // 本文に既にある語と、音に現れない語は候補にしない
            continue;
        }
        let mut best: Option<(usize, usize, usize)> = None;
        for a in 0..toks.len() {
            for b in a..toks.len().min(a + 3) {
                let (s, e) = (toks[a].0, toks[b].1);
                let span = &text[s..e];
                if span.contains(w)
                    || w.contains(span)
                    || span.chars().count().abs_diff(w.chars().count()) > 2
                {
                    continue;
                }
                let dist = _lev(
                    &_norm(&tagger.reading(span)).chars().collect::<Vec<_>>(),
                    &rw,
                );
                if best.is_none() || dist < best.unwrap().0 {
                    best = Some((dist, s, e));
                }
            }
        }
        let Some((dist, s, e)) = best else { continue };
        if dist <= (rw.len() / 2 + 1).max(2) {
            let sentence = format!("{}{}{}", &text[..s], w, &text[e..]);
            let why = format!("「{}」を「{}」に", &text[s..e], w);
            push(&mut cands, sentence, why);
        }
    }
    cands.into_iter().take(10).collect()
}

#[allow(clippy::too_many_arguments)]
fn _ask(
    at: &str,
    model: &str,
    tagger: &Tagger,
    sentence: &str,
    kana: &str,
    opts: &[(String, String)],
    before: &str,
    after: &str,
    glossary: &[(String, String)],
) -> Result<BTreeMap<String, f64>, String> {
    let mut crit = BTreeMap::new();
    crit.insert("keep".to_owned(), format!("そのまま: {sentence}"));
    for (i, (t, why)) in opts.iter().enumerate() {
        crit.insert(format!("c{i}"), format!("{t}（{why}）"));
    }
    let ins = format!(
        "sentence は会議の音声認識の結果で、誤変換を含むことがある。sound は同じ音声をカタカナで聞き取ったもので、\
         音の手がかりになる（ただし誤りも含む）。sentence の正しい文として最も可能性が高いものはどれか。before と after は前後の発言。\
         用語集: {}",
        glossary.iter().map(|(w, d)| format!("{}（{}。{}）", w, tagger.reading(w), d)).collect::<Vec<_>>().join("、")
    );
    let body = serde_json::json!({
        "model": model,
        "state": {"before": before, "sentence": sentence, "sound": kana, "after": after},
        "questions": {"fix": {"type": "choice", "instructions": ins, "criteria": crit}}
    });
    let answer: serde_json::Value = ollama::systemone(at, &body)?;
    let mut probs = BTreeMap::new();
    let entries = answer
        .get("answers")
        .and_then(|a| a.get("fix"))
        .and_then(|f| f.get("probabilities"))
        .and_then(|p| p.as_object())
        .ok_or_else(|| "Ollama の答えが読めません".to_owned())?;
    for (key, value) in entries {
        probs.insert(key.clone(), value.as_f64().unwrap_or(0.0));
    }
    Ok(probs)
}

/// 候補がある区間だけ tev1 に問い、「そのまま」以外が最も確からしければ置き換える。
pub fn glossary_fix(
    tagger: &Tagger,
    at: &str,
    model: &str,
    texts: &[String],
    k: &[String],
    glossary: &[(String, String)],
    log: &mut impl FnMut(&str),
) -> Vec<String> {
    let mut out = texts.to_vec();
    for (i, text) in texts.iter().enumerate() {
        let kana = k.get(i).map(String::as_str).unwrap_or("");
        let opts = candidates(tagger, text, kana, glossary);
        if opts.is_empty() {
            continue;
        }
        let before: String = texts[i.saturating_sub(1)..i].join("");
        let after: String = texts[(i + 1).min(texts.len())..(i + 2).min(texts.len())].join("");
        let probs = match _ask(
            at, model, tagger, text, kana, &opts, &before, &after, glossary,
        ) {
            Ok(probs) => probs,
            Err(e) => {
                warn(format_args!(
                    "警告: 区間 {i} の用語の確認に失敗しました（{e}）。そのまま残します"
                ));
                continue;
            }
        };
        // Python の max(pr, key=pr.get) は最初に現れた最大（keep が最初）を採る。
        // JSON の順序は失われているので、keep が同じ確率で並んだときは keep を優先する。
        let max_prob = probs.values().copied().fold(f64::NEG_INFINITY, f64::max);
        let best = if probs.get("keep").copied().unwrap_or(f64::NEG_INFINITY) == max_prob {
            "keep".to_owned()
        } else if let Some((key, value)) = probs.iter().max_by(|a, b| a.1.total_cmp(b.1)) {
            if *value == max_prob {
                key.clone()
            } else {
                continue;
            }
        } else {
            continue;
        };
        if best != "keep" {
            if let Ok(idx) = best.trim_start_matches('c').parse::<usize>() {
                if let Some((sentence, why)) = opts.get(idx) {
                    out[i] = sentence.clone();
                    log(&format!("{i} {why}"));
                }
            }
        }
    }
    out
}
