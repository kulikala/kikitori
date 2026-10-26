use crate::{receipt, SkillAction};
use std::{fs, path::PathBuf};

const SKILL: &str = include_str!("../../skill/SKILL.md");
fn destination(agent: &str, dir: Option<String>) -> PathBuf {
    dir.map(PathBuf::from)
        .unwrap_or_else(|| {
            let root = if agent == "codex" {
                ".codex"
            } else {
                ".claude"
            };
            receipt::home().join(root).join("skills").join("kikitori")
        })
        .join("SKILL.md")
}
pub fn run(action: SkillAction) -> Result<(), String> {
    match action {
        SkillAction::Install { agent, dir } => {
            let path = destination(&agent, dir);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&path, SKILL).map_err(|e| e.to_string())?;
            receipt::record("skill", &path.display().to_string())?;
            println!("{}", path.display());
        }
        SkillAction::Uninstall { agent: _, dir } => {
            let target = dir.map(PathBuf::from);
            for (_, value) in receipt::entries()
                .into_iter()
                .filter(|(kind, _)| kind == "skill")
            {
                let path = PathBuf::from(value);
                if target.as_ref().is_none_or(|d| path.starts_with(d)) {
                    let _ = fs::remove_file(path);
                }
            }
        }
        SkillAction::Path { agent, dir } => println!("{}", destination(&agent, dir).display()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_skill_matches_source() {
        assert_eq!(
            include_str!("../../skill/SKILL.md"),
            include_str!("../../../../skills/kikitori/SKILL.md")
        );
    }
}
