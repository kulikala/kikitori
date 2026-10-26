use crate::{receipt, SkillAction};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// スキルを成すファイル。置き場からの相対パスと中身
pub const FILES: &[(&str, &str)] = &[
    ("SKILL.md", include_str!("../../skill/SKILL.md")),
    (
        "references/install.md",
        include_str!("../../skill/references/install.md"),
    ),
    (
        "references/transcribe.md",
        include_str!("../../skill/references/transcribe.md"),
    ),
    (
        "references/refine.md",
        include_str!("../../skill/references/refine.md"),
    ),
];

fn destination(agent: &str, dir: Option<String>) -> PathBuf {
    dir.map(PathBuf::from).unwrap_or_else(|| {
        let root = if agent == "codex" {
            ".codex"
        } else {
            ".claude"
        };
        receipt::home().join(root).join("skills").join("kikitori")
    })
}

/// `dir` にスキルの全ファイルを書き、受け取り記録に無いものを記録する
pub fn write_all(dir: &Path) -> Result<(), String> {
    let recorded: Vec<String> = receipt::entries()
        .into_iter()
        .filter(|(kind, _)| kind == "skill")
        .map(|(_, value)| value)
        .collect();
    for (rel, body) in FILES {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&path, body).map_err(|e| e.to_string())?;
        let shown = path.display().to_string();
        if !recorded.contains(&shown) {
            receipt::record("skill", &shown)?;
        }
    }
    Ok(())
}

/// スキルのファイルを消したあと、空になった references と置き場を片付ける
pub fn remove_empty_dirs(file: &Path) {
    let Some(dir) = file.parent() else { return };
    let _ = fs::remove_dir(dir);
    if dir.file_name().is_some_and(|n| n == "references") {
        if let Some(parent) = dir.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

pub fn run(action: SkillAction) -> Result<(), String> {
    match action {
        SkillAction::Install { agent, dir } => {
            let dir = destination(&agent, dir);
            write_all(&dir)?;
            println!("{}", dir.join("SKILL.md").display());
        }
        SkillAction::Uninstall { agent: _, dir } => {
            let target = dir.map(PathBuf::from);
            for (_, value) in receipt::entries()
                .into_iter()
                .filter(|(kind, _)| kind == "skill")
            {
                let path = PathBuf::from(value);
                if target.as_ref().is_none_or(|d| path.starts_with(d)) {
                    let _ = fs::remove_file(&path);
                    remove_empty_dirs(&path);
                }
            }
        }
        SkillAction::Path { agent, dir } => {
            println!("{}", destination(&agent, dir).join("SKILL.md").display())
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_skill_matches_source() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/kikitori");
        for (rel, body) in super::FILES {
            let on_disk = std::fs::read_to_string(source.join(rel)).unwrap();
            assert_eq!(*body, on_disk, "{rel}");
        }
    }

    #[test]
    fn every_source_file_is_bundled() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/kikitori");
        let mut found = vec!["SKILL.md".to_string()];
        for entry in std::fs::read_dir(source.join("references")).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            found.push(format!("references/{name}"));
        }
        for rel in found {
            assert!(
                super::FILES.iter().any(|(r, _)| *r == rel),
                "{rel} is not bundled"
            );
        }
    }
}
