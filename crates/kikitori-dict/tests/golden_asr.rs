//! Python 版（fugashi + unidic-lite）の置換結果と一致するかを、認識結果と、用語を読みに崩した文で確かめる。
//! 辞書のファイルは KIKITORI_TEST_UNIDIC（unidic-lite の dicdir）で渡す。無ければ飛ばす。
use kikitori_dict::{norm, Replacer};

#[derive(serde::Deserialize)]
struct Case {
    input: String,
    expected: String,
}

#[derive(serde::Deserialize)]
struct Golden {
    dictionary: Vec<(String, String)>,
    cases: Vec<Case>,
}

#[test]
fn matches_python_on_asr_outputs_and_misread_terms() {
    let Some(dir) = std::env::var_os("KIKITORI_TEST_UNIDIC") else {
        eprintln!("skipping: KIKITORI_TEST_UNIDIC is unset");
        return;
    };
    let g: Golden = serde_json::from_str(include_str!("fixtures/golden_asr.json")).unwrap();
    let mut pairs: Vec<(String, String)> = g
        .dictionary
        .into_iter()
        .map(|(s, r)| (s, norm(&r)))
        .collect();
    pairs.sort_by_key(|p| std::cmp::Reverse(p.1.chars().count()));
    let rep = Replacer::new(pairs, std::path::PathBuf::from(dir))
        .unwrap()
        .python_compat(true);
    let mut bad = Vec::new();
    for c in &g.cases {
        let got = rep.apply(&c.input).unwrap();
        if got != c.expected {
            bad.push(format!("{} → {} (expected {})", c.input, got, c.expected));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} differ:\n{}",
        bad.len(),
        g.cases.len(),
        bad.join("\n")
    );
}

#[test]
fn spelled_readings_match_when_pronunciation_uses_long_vowels() {
    let Some(dir) = std::env::var_os("KIKITORI_TEST_UNIDIC") else {
        eprintln!("skipping: KIKITORI_TEST_UNIDIC is unset");
        return;
    };
    // 「東京」の発音は「トーキョー」、綴りの読みは「トウキョウ」。利用者は綴りで書く
    let pairs = vec![("東京水温".to_owned(), norm("とうきょうすいおん"))];
    let dir = std::path::PathBuf::from(dir);
    let rep = Replacer::new(pairs.clone(), dir.clone()).unwrap();
    assert_eq!(
        rep.apply("ただし東京水温では").unwrap(),
        "ただし東京水温では"
    );
    let rep = Replacer::new(
        vec![("東京リージョン".to_owned(), norm("とうきょうすいおん"))],
        dir.clone(),
    )
    .unwrap();
    assert_eq!(
        rep.apply("ただし東京水温では").unwrap(),
        "ただし東京リージョンでは"
    );
    let compat = Replacer::new(
        vec![("東京リージョン".to_owned(), norm("とうきょうすいおん"))],
        dir,
    )
    .unwrap()
    .python_compat(true);
    assert_eq!(
        compat.apply("ただし東京水温では").unwrap(),
        "ただし東京水温では"
    );
}
