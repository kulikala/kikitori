//! Choose the refinement route from environment facts (Python `refine/route.py`).

use crate::catalog::{self, Hardware};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// Qwen3-ASR and kana-whisper can run for this transcript.
    pub evidence: bool,
    /// off / bundle (an outer LLM fixes) / claude (the claude CLI).
    pub external: String,
    /// Models currently available on the Ollama endpoint: {name: bytes}.
    pub ollama: Option<std::collections::BTreeMap<String, u64>>,
    /// The endpoint knows `/v1/systemone`.
    pub systemone: bool,
    /// Hardware of the local machine, `None` for a remote endpoint.
    pub hardware: Option<Hardware>,
    pub glossary: bool,
    /// User-supplied merge model.
    pub merge_model: Option<String>,
    pub no_ollama: bool,
}

impl Default for Facts {
    fn default() -> Self {
        Self {
            evidence: false,
            external: "off".to_owned(),
            ollama: None,
            systemone: false,
            hardware: None,
            glossary: false,
            merge_model: None,
            no_ollama: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub route: &'static str,
    pub model: Option<String>,
    pub notes: Vec<String>,
}

fn candidates(f: &Facts, role: &str) -> Vec<String> {
    if role == "merge" {
        if let Some(model) = &f.merge_model {
            return vec![model.clone()];
        }
    }
    match &f.hardware {
        None => catalog::all_candidates(role)
            .into_iter()
            .map(String::from)
            .collect(),
        Some(hw) => catalog::candidates_for(hw.kind, role)
            .into_iter()
            .filter(|m| catalog::fits(m, hw))
            .map(String::from)
            .collect(),
    }
}

impl Facts {
    fn has(&self, model: &str) -> bool {
        self.ollama
            .as_ref()
            .is_some_and(|tags| tags.contains_key(model))
    }
}

pub fn choose(f: &Facts) -> Plan {
    if !f.evidence {
        return Plan {
            route: "plain",
            model: None,
            notes: vec![
                "Qwen3-ASR と kana-whisper のモデルが無いため、kikitori の本文のまま出します"
                    .to_owned()
                    + "（kikitori models pull --refine で取得します）",
            ],
        };
    }
    if f.external != "off" {
        return Plan {
            route: "external",
            model: None,
            notes: Vec::new(),
        };
    }
    let mut notes = Vec::new();
    if f.no_ollama || f.ollama.is_none() {
        if !f.no_ollama {
            notes.push(
                "Ollama が答えないため、LLM を使わず長さの規則だけで直します".to_owned()
                    + "（kikitori refine --setup で起こせます）",
            );
        }
        return Plan {
            route: "rule",
            model: None,
            notes,
        };
    }
    for m in candidates(f, "merge") {
        if f.has(&m) {
            return Plan {
                route: "merge",
                model: Some(m),
                notes,
            };
        }
    }
    if let Some(model) = &f.merge_model {
        notes.push(format!(
            "{model} が Ollama にありません（kikitori refine --setup で取得します）"
        ));
    }
    if !f.glossary {
        notes.push("用語集が無いため、用語の置き換えはしません".to_owned());
        return Plan {
            route: "rule",
            model: None,
            notes,
        };
    }
    if !f.systemone {
        notes.push(
            "Ollama の向き先に /v1/systemone が無いため（0.40.0 より古い）、用語の置き換えはしません"
                .to_owned(),
        );
        return Plan {
            route: "rule",
            model: None,
            notes,
        };
    }
    let fix = candidates(f, "fix");
    for m in &fix {
        if f.has(m) {
            return Plan {
                route: "glossary",
                model: Some(m.clone()),
                notes,
            };
        }
    }
    let listed = if fix.is_empty() {
        "用語置き換えのモデル".to_owned()
    } else {
        fix.join(" / ")
    };
    notes.push(format!(
        "{listed} が Ollama に無いため、用語の置き換えはしません（kikitori refine --setup で取得します）"
    ));
    Plan {
        route: "rule",
        model: None,
        notes,
    }
}
