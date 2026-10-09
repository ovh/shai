use std::path::PathBuf;

use super::discovery::{parse_skill_frontmatter, SkillInfo};

/// A skill bundled inside the binary, keyed by directory name.
struct BuiltinSkill {
    fallback_name: &'static str,
    content: &'static str,
}

/// Built-in skills, embedded at compile time.
///
/// Only generic, project-agnostic skills belong here: they ship with every
/// shai binary (no install step needed) and sit at the lowest priority tier —
/// a skill with the same name in `.shai/skills/` or `~/.config/shai/skills/`
/// shadows them. Shai/Rust-specific sample skills live in
/// `shai-core/examples/skills/` instead and are NOT embedded.
const BUILTIN_SKILLS: &[BuiltinSkill] = &[BuiltinSkill {
    fallback_name: "memory",
    content: include_str!("../../../skills/memory/SKILL.md"),
}];

/// List the built-in skills with frontmatter parsed.
pub fn builtin_skills() -> Vec<SkillInfo> {
    BUILTIN_SKILLS
        .iter()
        .map(|skill| {
            let (name, description) = parse_skill_frontmatter(skill.content)
                .unwrap_or_else(|| (skill.fallback_name.to_string(), String::new()));
            SkillInfo {
                name,
                description,
                path: PathBuf::new(),
                content: Some(skill.content),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_skills_are_embedded() {
        let skills = builtin_skills();
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"memory"), "memory skill must be built-in");
        // Every built-in has embedded content and a parsed description
        for skill in &skills {
            assert!(
                skill.content.is_some(),
                "{} has no embedded content",
                skill.name
            );
            assert!(
                !skill.description.is_empty(),
                "{} has no frontmatter description",
                skill.name
            );
        }
    }

    #[test]
    fn test_shai_specific_samples_are_not_builtin() {
        let skills = builtin_skills();
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        for sample in ["code-review", "testing", "release", "rust-lint"] {
            assert!(
                !names.contains(&sample),
                "'{}' is a shai-specific sample and must stay in examples/, not built-in",
                sample
            );
        }
    }

    #[test]
    fn test_builtin_skill_names_are_unique() {
        let skills = builtin_skills();
        let mut names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "duplicate built-in skill names");
    }
}
