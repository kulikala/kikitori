//! UniDic-backed dictionary replacement and glossary readings.

use notmecab::{Blob, Dict};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization;

/// A dictionary entry, stored as the replacement surface and normalized reading.
pub type Pair = (String, String);

/// Error returned when loading a replacement list or the UniDic files fails.
#[derive(Debug)]
pub struct DictionaryError {
    message: String,
    missing_dictionary: bool,
}

impl DictionaryError {
    fn io(context: &str, error: io::Error) -> Self {
        Self {
            message: format!("{context}: {error}"),
            missing_dictionary: false,
        }
    }

    fn dictionary(path: &Path, error: impl std::fmt::Display) -> Self {
        Self {
            message: format!(
                "Japanese dictionary is missing or invalid at {}: {error}",
                path.display()
            ),
            missing_dictionary: true,
        }
    }

    /// CLI exit status. Missing UniDic uses the offline model status (5).
    pub fn exit_code(&self) -> i32 {
        if self.missing_dictionary {
            5
        } else {
            3
        }
    }

    /// Suggested recovery action for a missing or invalid UniDic installation.
    pub fn hint(&self) -> Option<&'static str> {
        self.missing_dictionary
            .then_some("Run `kikitori setup` to install the Japanese dictionary.")
    }
}

impl std::fmt::Display for DictionaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for DictionaryError {}

/// Normalize a reading like Python `dictionary.norm`: NFKC, hiragana to
/// katakana, then remove long vowels and expand small kana.
pub fn norm(reading: &str) -> String {
    let mut out = String::new();
    for c in reading.nfkc() {
        if c == 'ー' {
            continue;
        }
        let c = if ('ぁ'..='ゖ').contains(&c) {
            char::from_u32(c as u32 + 0x60).unwrap_or(c)
        } else {
            c
        };
        out.push(match c {
            'ァ' => 'ア',
            'ィ' => 'イ',
            'ゥ' => 'ウ',
            'ェ' => 'エ',
            'ォ' => 'オ',
            'ッ' => 'ツ',
            'ャ' => 'ヤ',
            'ュ' => 'ユ',
            'ョ' => 'ヨ',
            'ヮ' => 'ワ',
            _ => c,
        });
    }
    out
}

/// Read tab- or comma-separated surface/reading pairs.
pub fn load(path: impl AsRef<Path>) -> Result<Vec<Pair>, DictionaryError> {
    let path = path.as_ref();
    let text = fs::read_to_string(path)
        .map_err(|e| DictionaryError::io("cannot read dictionary list", e))?;
    let mut pairs = Vec::new();
    for (line_no, original) in text.lines().enumerate() {
        let line = if line_no == 0 {
            original.strip_prefix('\u{feff}').unwrap_or(original)
        } else {
            original
        };
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let split_at = line
            .char_indices()
            .find(|(_, c)| *c == '\t' || *c == ',')
            .map(|(i, _)| i);
        if let Some(i) = split_at {
            let surface = line[..i].trim();
            let reading = line[i + 1..].trim();
            if !surface.is_empty() && !reading.is_empty() {
                pairs.push((surface.to_owned(), norm(reading)));
            }
        }
    }
    pairs.sort_by_key(|pair| std::cmp::Reverse(pair.1.chars().count()));
    Ok(pairs)
}

fn load_tagger(dir: &Path) -> Result<Dict, DictionaryError> {
    let files = ["sys.dic", "unk.dic", "matrix.bin", "char.bin"];
    for file in files {
        let path = dir.join(file);
        if !path.is_file() {
            return Err(DictionaryError::dictionary(&path, "file not found"));
        }
    }
    let result = (|| {
        Dict::load(
            Blob::open(dir.join("sys.dic")).map_err(|e| e.to_string())?,
            Blob::open(dir.join("unk.dic")).map_err(|e| e.to_string())?,
            Blob::open(dir.join("matrix.bin")).map_err(|e| e.to_string())?,
            Blob::open(dir.join("char.bin")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    })();
    result.map_err(|e| DictionaryError::dictionary(dir, e))
}

struct Token {
    surface: String,
    pron: Option<String>,
    kana: Option<String>,
}

fn tokenize(dict: &Dict, text: &str) -> Result<Vec<Token>, DictionaryError> {
    let (tokens, _) = dict.tokenize(text).map_err(|e| DictionaryError {
        message: format!("Japanese dictionary tokenization failed: {e:?}"),
        missing_dictionary: true,
    })?;
    Ok(tokens
        .into_iter()
        .map(|token| {
            let fields: Vec<_> = token.get_feature(dict).split(',').collect();
            let value = |index: usize| fields.get(index).map(|v| (*v).to_owned());
            Token {
                surface: token.get_text(text).to_owned(),
                pron: value(9),
                kana: value(17),
            }
        })
        .collect())
}

/// Token-based surface replacement. The dictionary path is the installed
/// `unidic-lite-1.0.8` directory containing `sys.dic` and its companion files.
pub struct Replacer {
    pairs: Vec<Pair>,
    tagger: Option<Dict>,
    python_compat: bool,
}

impl Replacer {
    pub fn new(
        pairs: Vec<Pair>,
        dictionary_dir: impl AsRef<Path>,
    ) -> Result<Self, DictionaryError> {
        let tagger = if pairs.is_empty() {
            None
        } else {
            Some(load_tagger(dictionary_dir.as_ref())?)
        };
        Ok(Self {
            pairs,
            tagger,
            python_compat: false,
        })
    }

    pub fn from_file(
        path: impl AsRef<Path>,
        dictionary_dir: impl AsRef<Path>,
    ) -> Result<Self, DictionaryError> {
        Self::new(load(path)?, dictionary_dir)
    }

    /// Python 版 0.2.1 と同じ照合だけにする（発音の読みだけで照合する）
    pub fn python_compat(mut self, on: bool) -> Self {
        self.python_compat = on;
        self
    }

    /// 位置 `i` から始まる、辞書の読みと一致する語の並び。辞書は読みの長い順
    fn find(&self, readings: &[String], i: usize) -> Option<(&str, usize)> {
        for (surface, reading) in &self.pairs {
            let target_len = reading.chars().count();
            if target_len < 2 {
                continue;
            }
            let mut accumulated = String::new();
            let mut j = i;
            while j < readings.len() && accumulated.chars().count() < target_len {
                accumulated.push_str(&readings[j]);
                j += 1;
            }
            if accumulated == *reading {
                return Some((surface.as_str(), j));
            }
        }
        None
    }

    pub fn apply(&self, text: &str) -> Result<String, DictionaryError> {
        let Some(tagger) = &self.tagger else {
            return Ok(text.to_owned());
        };
        let tokens = tokenize(tagger, text)?;
        let readings: Vec<String> = tokens
            .iter()
            .map(|t| {
                match t
                    .pron
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .or_else(|| t.kana.as_deref().filter(|s| !s.is_empty()))
                {
                    Some(value) if value != "*" => norm(value),
                    _ => norm(&t.surface),
                }
            })
            .collect();
        // 発音（pron）は長音を「ー」で書く（トーキョー）が、利用者は読みを綴りどおりに書く（とうきょう）ことが多い。
        // 発音で一致しなければ、綴りの読み（kana）でも照合する（Python 版との互換の指定では照合しない）
        let spelled: Vec<String> = tokens
            .iter()
            .map(
                |t| match t.kana.as_deref().filter(|s| !s.is_empty() && *s != "*") {
                    Some(value) => norm(value),
                    None => norm(&t.surface),
                },
            )
            .collect();
        let mut out = String::new();
        let mut i = 0;
        while i < tokens.len() {
            let mut found = self.find(&readings, i);
            if found.is_none() && !self.python_compat {
                found = self.find(&spelled, i);
            }
            if let Some((surface, end)) = found {
                out.push_str(surface);
                i = end;
            } else {
                out.push_str(&tokens[i].surface);
                i += 1;
            }
        }
        Ok(out)
    }
}

/// Glossary reading matching `refine/local.py::reading`: use UniDic kanaBase
/// only, falling back to the token surface, and convert hiragana to katakana.
pub fn reading(text: &str, dictionary_dir: impl AsRef<Path>) -> Result<String, DictionaryError> {
    let tagger = load_tagger(dictionary_dir.as_ref())?;
    let tokens = tokenize(&tagger, text)?;
    let mut out = String::new();
    for token in tokens {
        let value = token
            .kana
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "*")
            .unwrap_or(&token.surface);
        for c in value.chars() {
            out.push(if ('ぁ'..='ゖ').contains(&c) {
                char::from_u32(c as u32 + 0x60).unwrap_or(c)
            } else {
                c
            });
        }
    }
    Ok(out)
}

/// Default dictionary directory under the user's kikitori cache.
pub fn default_dictionary_dir() -> PathBuf {
    kikitori_core::paths::dictionary_root().join("unidic-lite-1.0.8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_matches_expected_kana_rules() {
        assert_eq!(norm("たいぷすくりぷと"), norm("タイプスクリプト"));
        assert_eq!(norm("コンピューター"), norm("コンピュータ"));
        assert_eq!(norm("ｶﾞｯﾂﾎﾟｰｽﾞ"), "ガツツポズ");
    }

    #[test]
    fn loads_tab_and_comma_rows_and_sorts_by_codepoints() {
        let path = std::env::temp_dir().join(format!("kikitori-dict-{}.txt", std::process::id()));
        fs::write(
            &path,
            "\u{feff}# comment\n\n short , かな\n長い\tかなかな\nignored\n",
        )
        .unwrap();
        let pairs = load(&path).unwrap();
        let _ = fs::remove_file(path);
        assert_eq!(
            pairs,
            vec![
                ("長い".into(), "カナカナ".into()),
                ("short".into(), "カナ".into())
            ]
        );
    }

    #[test]
    fn missing_dictionary_has_setup_hint_and_exit_code_five() {
        let error = match Replacer::new(
            vec![("例".into(), "レイ".into())],
            "/definitely/missing/unidic",
        ) {
            Ok(_) => panic!("missing UniDic unexpectedly loaded"),
            Err(error) => error,
        };
        assert_eq!(error.exit_code(), 5);
        assert!(error.hint().unwrap().contains("kikitori setup"));
    }

    #[test]
    fn python_generated_golden_replacements_match() {
        let Some(dir) = std::env::var_os("KIKITORI_TEST_UNIDIC") else {
            eprintln!("skipping UniDic golden test: KIKITORI_TEST_UNIDIC is unset");
            return;
        };
        let pairs = vec![
            ("負荷試験".into(), norm("ふかしけん")),
            ("東京大学".into(), norm("とうきょうだいがく")),
        ];
        let replacer = Replacer::new(pairs, dir).unwrap().python_compat(true);
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../tests/fixtures/golden.json")).unwrap();
        assert_eq!(rows.len(), 30);
        for row in rows {
            let input = row["input"].as_str().unwrap();
            let expected = row["expected"].as_str().unwrap();
            assert_eq!(replacer.apply(input).unwrap(), expected, "input: {input}");
        }
        assert_eq!(
            reading(
                "東京大学",
                std::env::var_os("KIKITORI_TEST_UNIDIC").unwrap()
            )
            .unwrap(),
            "トウキョウダイガク"
        );
    }
}
