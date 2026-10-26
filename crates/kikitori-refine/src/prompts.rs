//! Prompts for the LLM and reading of numbered replies (Python `refine/prompts.py`).
//! The wording is exactly what the accuracy measurements used; do not rephrase.

use regex::Regex;

const PREV: usize = 3;
const AFTER: usize = 2;

fn common(glossary: &[(String, String)]) -> String {
    "言い淀み・相づち・繰り返しも、話されていれば残す。要約・言い換え・敬語の整形をしない。"
        .to_owned()
        + "固有名詞は、音が近ければ用語集の表記にする。"
        + "出力は、入力の各区間につき 1 行ずつ「[番号] 本文」の形で書く。番号は入力と同じにし、説明を書かない。"
        + "用語集: "
        + &glossary
            .iter()
            .map(|(w, d)| {
                if d.is_empty() {
                    w.clone()
                } else {
                    format!("{w}（{d}）")
                }
            })
            .collect::<Vec<_>>()
            .join("、")
}

fn head(mode: &str) -> &'static str {
    merge_or_fixev(mode).unwrap_or_else(|| panic!("unknown prompt mode: {mode}"))
}

fn merge_or_fixev(mode: &str) -> Option<&'static str> {
    if mode == "merge" {
        Some(MERGE_HEAD)
    } else if mode == "fixev" {
        Some(FIXEV_HEAD)
    } else {
        None
    }
}

const MERGE_HEAD: &str = "あなたは会議の文字起こしの校正者。同じ音声区間を 3 つの方式で認識した結果が、区間ごとに与えられる。A と B は漢字かな交じり、K は音をカタカナで聞き取ったもの。どれも誤りを含み、取りこぼしもある。区間ごとに、実際に話された文を一つ書け。A・B・K のどれかに根拠がある語だけを使い、根拠の無い語を足さない。片方にしか無い部分も、K の音と合えば残す。";

const FIXEV_HEAD: &str = "あなたは会議の文字起こしの校正者。音声認識の結果（本文）が区間ごとに与えられる。本文には誤変換や取りこぼしがある。根拠として、同じ区間を別の音声認識で文字にしたもの（B）と、音をカタカナで聞き取ったもの（K）が添えられる。B と K も誤りを含み、B は話されていない文を作ることがある。前後の文脈と B・K をもとに、本文の誤りを直し、取りこぼしを補え。B にも K にも根拠の無い語は足さない。B にしか無い部分は、K の音と合うときだけ採る。正しい部分は変えない。";

pub fn system(mode: &str, glossary: &[(String, String)]) -> String {
    String::from(head(mode)) + &common(glossary)
}

fn block(mode: &str, i: usize, a: &str, b: &str, k: &str) -> String {
    if mode == "merge" {
        format!("[{i}]\nA: {a}\nB: {b}\nK: {k}")
    } else {
        format!("[{i}]\n本文: {a}\nB: {b}\nK: {k}")
    }
}

/// One request block for the given interval ids; context adds the
/// surrounding text of A.
pub fn user(mode: &str, ids: &[usize], a: &[String], b: &[String], k: &[String]) -> String {
    let first = ids[0];
    let aft = ids[ids.len() - 1];
    let prev_from = first.saturating_sub(PREV);
    let prev: String = a
        .get(prev_from..first)
        .unwrap_or(&[])
        .iter()
        .fold(String::new(), |acc, s| acc + s);
    let after: String = a
        .get(aft + 1..(aft + 1 + AFTER).min(a.len()))
        .unwrap_or(&[])
        .iter()
        .fold(String::new(), |acc, s| acc + s);
    let blocks = ids
        .iter()
        .map(|&i| block(mode, i, &a[i], &b[i], &k[i]))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "直前の発言（参考）: {}\n直後の発言（参考）: {}\n\n{blocks}",
        shown(&prev),
        shown(&after)
    )
}

fn shown(s: &str) -> &str {
    if s.is_empty() {
        "（なし）"
    } else {
        s
    }
}

pub fn chunks(n: usize, size: usize) -> Vec<Vec<usize>> {
    (0..n)
        .collect::<Vec<_>>()
        .chunks(size)
        .map(|c| c.to_vec())
        .collect()
}

/// Read `[number] text` lines into {number: text}. Any other line is
/// skipped. Note: like Python, leading `\s*` can eat a newline, so
/// `[0]\n[1] foo` parses `{0: "[1] foo"}`.
pub fn parse(text: &str) -> std::collections::BTreeMap<usize, String> {
    let re = Regex::new(r"(?m)^\s*\[(\d+)\]\s*(.*)$").unwrap();
    let mut out = std::collections::BTreeMap::new();
    for caps in re.captures_iter(text) {
        let id: usize = caps.get(1).unwrap().as_str().parse().unwrap_or(usize::MAX);
        let body = caps
            .get(2)
            .map(|x| x.as_str().trim().to_owned())
            .unwrap_or_default();
        out.insert(id, body);
    }
    out
}

/// The markers are assembled from character escapes so the XML-like
/// pseudo tag never appears verbatim in this source file.
const THINK_WORD: [char; 5] = ['\u{74}', '\u{68}', '\u{69}', '\u{6e}', '\u{6b}'];

pub fn think_open() -> String {
    format!("\u{3c}{}\u{3e}", THINK_WORD.iter().collect::<String>())
}

pub fn think_close() -> String {
    format!(
        "\u{3c}\u{2f}{}\u{3e}",
        THINK_WORD.iter().collect::<String>()
    )
}

/// Remove a think block if the model added one despite the explicit
/// `"think": false` request (design D-9).
pub fn strip_think(text: &str) -> &str {
    let open = think_open();
    match text.find(&open) {
        // 閉じが無いときは `think` 以降を全部削る（考え出しは本文でない）
        Some(i) => {
            let close = think_close();
            let rest = &text[i + open.len()..];
            match rest.find(&close) {
                Some(j) => rest[j + close.len()..].trim_start(),
                None => "",
            }
        }
        None => text,
    }
}
