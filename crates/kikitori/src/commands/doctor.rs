use serde::Serialize;
use std::{path::Path, process::Command, time::Duration};
use sysinfo::System;

#[derive(Serialize)]
struct Doctor {
    schema_version: u8,
    os: String,
    arch: String,
    cpu: String,
    memory: Memory,
    candidates: Vec<Candidate>,
    /// 既定（--device auto）で実際に使う計算機。文字起こしは常に CPU、補正の B・K は使える GPU から選ぶ
    selected: Selected,
    ffmpeg: Tool,
    ollama: Ollama,
    caches: Vec<Cache>,
    models: Vec<ModelRow>,
    refine: RefineOutlook,
    path_kikitori: bool,
    version: &'static str,
}
#[derive(Serialize)]
struct ModelRow {
    name: String,
    /// asr・vad・diarizer・dict・refine-b・refine-k
    role: String,
    present: bool,
}
#[derive(Serialize)]
struct RefineOutlook {
    /// 既定の指定で `kikitori refine` を実行したときの直し方（merge・fix・rule・plain）
    route: String,
    model: Option<String>,
    notes: Vec<String>,
}
#[derive(Serialize)]
struct Memory {
    total_bytes: u64,
    available_bytes: u64,
    /// kikitori refine の B・K に要るメモリの見込み（CPU の fp32、Metal か fp16）
    refine_need_fp32_bytes: u64,
    refine_need_fp16_bytes: u64,
}
#[derive(Serialize)]
struct Candidate {
    name: String,
    status: &'static str,
}
#[derive(Serialize)]
struct Selected {
    transcribe: &'static str,
    refine: &'static str,
}
#[derive(Serialize)]
struct Tool {
    present: bool,
    version: Option<String>,
}
#[derive(Serialize)]
struct Ollama {
    present: bool,
    models: Vec<String>,
}
#[derive(Serialize)]
struct Cache {
    path: String,
    exists: bool,
    size_bytes: u64,
}

pub fn run(json: bool) -> Result<(), String> {
    let mut system = System::new();
    system.refresh_memory();
    system.refresh_cpu();
    let memory = Memory {
        total_bytes: system.total_memory(),
        available_bytes: system.available_memory(),
        refine_need_fp32_bytes: 9_700_000_000,
        refine_need_fp16_bytes: 5_600_000_000,
    };
    let cpu = system
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());
    let mut candidates = vec![Candidate {
        name: "cpu".into(),
        status: "built",
    }];
    candidates.push(Candidate {
        name: "metal".into(),
        status: probe(cfg!(feature = "metal"), kikitori_core::DeviceChoice::Metal),
    });
    candidates.push(Candidate {
        name: "cuda".into(),
        status: probe(cfg!(feature = "cuda"), kikitori_core::DeviceChoice::Cuda(0)),
    });
    let ffmpeg = Command::new("ffmpeg")
        .arg("-version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| Tool {
            present: true,
            version: String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .map(str::to_owned),
        })
        .unwrap_or(Tool {
            present: false,
            version: None,
        });
    let ollama_models = kikitori_fetch::http_agent(Duration::from_secs(1))
        .get(format!("{}/api/tags", kikitori_refine::ollama::base(None)))
        .call()
        .ok()
        .and_then(|r| r.into_body().read_to_string().ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("models").and_then(|m| m.as_array()).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(str::to_owned))
        .collect::<Vec<_>>();
    let cache_paths = [
        kikitori_core::paths::models_root(),
        kikitori_core::paths::dictionary_root(),
        kikitori_core::paths::hf_cache_root(),
    ];
    let caches = cache_paths
        .into_iter()
        .map(|p| Cache {
            exists: p.exists(),
            size_bytes: dir_size(&p),
            path: p.to_string_lossy().into_owned(),
        })
        .collect();
    let path_kikitori = first_on_path_is_running_executable();
    let report = Doctor {
        schema_version: 1,
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        cpu,
        memory,
        selected: Selected {
            transcribe: "cpu",
            refine: candidates
                .iter()
                .find(|c| c.name != "cpu" && c.status == "built")
                .map_or("cpu", |c| if c.name == "metal" { "metal" } else { "cuda" }),
        },
        candidates,
        ffmpeg,
        ollama: Ollama {
            present: !ollama_models.is_empty() || ollama_responds(),
            models: ollama_models,
        },
        caches,
        models: model_rows(),
        refine: match super::refine::outlook() {
            Ok(p) => RefineOutlook {
                route: p.route.into(),
                model: p.model,
                notes: p.notes,
            },
            Err(e) => RefineOutlook {
                route: "unknown".into(),
                model: None,
                notes: vec![e.message],
            },
        },
        path_kikitori,
        version: env!("CARGO_PKG_VERSION"),
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "kikitori {} — OS: {} {}",
            report.version, report.os, report.arch
        );
        println!(
            "CPU: {} | メモリ: 合計 {}、使用可能 {}",
            report.cpu,
            gb(report.memory.total_bytes),
            gb(report.memory.available_bytes)
        );
        println!(
            "kikitori refine の B・K に要るメモリの見込み: CPU（fp32）{}、Metal か --dtype fp16 で {}",
            gb(report.memory.refine_need_fp32_bytes),
            gb(report.memory.refine_need_fp16_bytes)
        );
        println!(
            "使う計算機（既定）: 文字起こし {}、補正の B・K {}",
            report.selected.transcribe, report.selected.refine
        );
        for c in &report.candidates {
            println!(
                "{}: {}",
                c.name,
                match c.status {
                    "built" => "利用可能",
                    "unavailable" => "このビルドに入っているが、この PC では使えない",
                    _ => "このビルドには入っていない",
                }
            );
        }
        println!(
            "ffmpeg: {}",
            report.ffmpeg.version.as_deref().unwrap_or("見つかりません")
        );
        println!(
            "Ollama: {}{}",
            if report.ollama.present {
                "利用可能"
            } else {
                "見つかりません"
            },
            if report.ollama.models.is_empty() {
                String::new()
            } else {
                format!(" ({})", report.ollama.models.join(", "))
            }
        );
        for c in &report.caches {
            println!(
                "置き場 {}: {}{}",
                c.path,
                gb(c.size_bytes),
                if c.exists {
                    ""
                } else {
                    "（ありません）"
                }
            );
        }
        for m in &report.models {
            println!(
                "モデル {}（{}）: {}",
                m.name,
                m.role,
                if m.present {
                    "取得済み"
                } else {
                    "未取得"
                }
            );
        }
        println!(
            "kikitori refine の直し方の見込み: {}{}",
            report.refine.route,
            report
                .refine
                .model
                .as_deref()
                .map(|m| format!("（{m}）"))
                .unwrap_or_default()
        );
        for n in &report.refine.notes {
            println!("  {n}");
        }
        println!(
            "PATH 上の kikitori が実行中のものと一致: {}",
            report.path_kikitori
        );
    }
    Ok(())
}

/// Check whether PATH would resolve `kikitori` to the running executable.
/// This helper is the seam for more exact platform-specific executable probes.
fn first_on_path_is_running_executable() -> bool {
    let Ok(running) = std::env::current_exe().and_then(std::fs::canonicalize) else {
        return false;
    };
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        let candidate = dir.join("kikitori.exe");
        #[cfg(not(windows))]
        let candidate = dir.join("kikitori");
        if candidate.is_file() {
            return std::fs::canonicalize(candidate).is_ok_and(|first| first == running);
        }
    }
    false
}

/// Runtime initialization probes can be added here when engine APIs are available.
/// built: 初期化できた、unavailable: ビルドには入っているが初期化できない、not built: ビルドに入っていない
fn probe(built: bool, choice: kikitori_core::DeviceChoice) -> &'static str {
    if !built {
        "not built"
    } else if kikitori_qwen3asr::device_works(choice) {
        "built"
    } else {
        "unavailable"
    }
}

fn model_rows() -> Vec<ModelRow> {
    let root = kikitori_core::paths::models_root();
    let mut rows: Vec<ModelRow> = kikitori_fetch::CORE_ASSETS
        .iter()
        .map(|a| ModelRow {
            name: a.name.into(),
            role: a.role.into(),
            present: a.is_present(&root),
        })
        .collect();
    rows.push(ModelRow {
        name: "unidic-lite-1.0.8".into(),
        role: "dict".into(),
        present: kikitori_core::paths::dictionary_root()
            .join("unidic-lite-1.0.8/sys.dic")
            .is_file(),
    });
    for (name, role) in [("qwen3-asr", "refine-b"), ("kana-whisper", "refine-k")] {
        let items = kikitori_fetch::refine_pull_plan();
        let mine: Vec<_> = items.iter().filter(|i| i.name.starts_with(name)).collect();
        rows.push(ModelRow {
            name: name.into(),
            role: role.into(),
            present: !mine.is_empty() && mine.iter().all(|i| i.present),
        });
    }
    rows
}

fn gb(bytes: u64) -> String {
    format!(
        "{:.1}GB",
        bytes as f64 / kikitori_refine::catalog::GB as f64
    )
}

fn ollama_responds() -> bool {
    kikitori_fetch::http_agent(Duration::from_secs(1))
        .get(format!("{}/api/tags", kikitori_refine::ollama::base(None)))
        .call()
        .is_ok()
}
fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let Ok(m) = e.metadata() else { return 0 };
            if m.is_dir() {
                dir_size(&e.path())
            } else {
                m.len()
            }
        })
        .sum()
}
