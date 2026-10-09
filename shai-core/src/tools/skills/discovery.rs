use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Metadata extracted from a SKILL.md file's frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    /// Skill name (from frontmatter `name` or directory name)
    pub name: String,
    /// Short description (from frontmatter `description`)
    pub description: String,
    /// Full path to the SKILL.md file (empty for built-in skills)
    pub path: PathBuf,
    /// Embedded content for built-in skills; `None` for skills read from disk.
    #[serde(skip)]
    pub content: Option<&'static str>,
}

impl SkillInfo {
    /// Human-readable tier this skill was discovered from:
    /// `"project"`, `"global"`, or `"built-in"`.
    pub fn source(&self) -> &'static str {
        if self.content.is_some() {
            return "built-in";
        }
        if let Some(root) = crate::runners::coder::env::find_git_root() {
            if self.path.starts_with(root.join(".shai").join("skills")) {
                return "project";
            }
        }
        "global"
    }
}

/// Parse a SKILL.md file content to extract name and description from frontmatter.
///
/// Frontmatter format:
/// ```
/// ---
/// name: My Skill
/// description: A short description
/// ---
/// # Rest of the file...
/// ```
pub(crate) fn parse_skill_frontmatter(content: &str) -> Option<(String, String)> {
    crate::tools::frontmatter::parse_frontmatter(content)
}

/// Discover all skills in a given directory.
/// Each subdirectory containing a `SKILL.md` is considered a skill.
fn discover_skills_in_dir(dir: &Path) -> Vec<SkillInfo> {
    let mut skills = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return skills,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_md = path.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }

        let content = match std::fs::read_to_string(&skill_md) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let (name, description) = match parse_skill_frontmatter(&content) {
            Some(parsed) => parsed,
            None => {
                // Fall back to directory name if no frontmatter
                let dir_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                (dir_name, String::new())
            }
        };

        skills.push(SkillInfo {
            name,
            description,
            path: skill_md,
            content: None,
        });
    }

    skills
}

/// Discover all skills from project-local (`.shai/skills/`), global
/// (`~/.config/shai/skills/`), and built-in (embedded in the binary) tiers.
///
/// Precedence: project-local > global > built-in. A skill name discovered in a
/// higher-priority tier shadows the same name in lower tiers.
pub fn discover_skills() -> Vec<SkillInfo> {
    let mut project_skills = Vec::new();
    if let Some(root) = crate::runners::coder::env::find_git_root() {
        let project_skills_dir = root.join(".shai").join("skills");
        project_skills = discover_skills_in_dir(&project_skills_dir);
    }

    let mut global_skills = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let global_skills_dir = PathBuf::from(home)
            .join(".config")
            .join("shai")
            .join("skills");
        global_skills = discover_skills_in_dir(&global_skills_dir);
    }

    merge_skill_tiers(&[
        project_skills,
        global_skills,
        super::builtin::builtin_skills(),
    ])
}

/// Merge pre-discovered skill tiers, higher-priority tiers first.
/// A skill name seen in an earlier tier shadows the same name in later tiers.
pub(crate) fn merge_skill_tiers(tiers: &[Vec<SkillInfo>]) -> Vec<SkillInfo> {
    let mut skills = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    for tier in tiers {
        for skill in tier {
            if seen_names.insert(skill.name.clone()) {
                skills.push(skill.clone());
            }
        }
    }
    skills
}

/// Format the skill catalog for injection into the system prompt.
/// Returns a compact list of `name: description` pairs.
pub fn format_skill_catalog(skills: &[SkillInfo]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    lines.push("## Available Skills\n".to_string());
    for skill in skills {
        if skill.description.is_empty() {
            lines.push(format!("- **{}**", skill.name));
        } else {
            lines.push(format!("- **{}**: {}", skill.name, skill.description));
        }
    }
    lines.push(
        "\nUse the `skill` tool with a skill name to load its full instructions.".to_string(),
    );
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_format_skill_catalog_empty() {
        let catalog = format_skill_catalog(&[]);
        assert!(catalog.is_empty());
    }

    #[test]
    fn test_format_skill_catalog_nonempty() {
        let skills = vec![
            SkillInfo {
                name: "deploy".to_string(),
                description: "Deploy the app".to_string(),
                path: PathBuf::from("/tmp/skills/deploy/SKILL.md"),
                content: None,
            },
            SkillInfo {
                name: "test".to_string(),
                description: String::new(),
                path: PathBuf::from("/tmp/skills/test/SKILL.md"),
                content: None,
            },
        ];
        let catalog = format_skill_catalog(&skills);
        assert!(catalog.contains("**deploy**: Deploy the app"));
        assert!(catalog.contains("**test**"));
        assert!(catalog.contains("skill"));
    }

    #[test]
    fn test_discover_skills_in_dir() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create a skill directory with SKILL.md
        let skill_dir = temp_path.join("my-skill");
        fs::create_dir(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: A test skill\n---\n# My Skill\n",
        )
        .unwrap();

        // Create another skill without frontmatter
        let skill_dir2 = temp_path.join("another-skill");
        fs::create_dir(&skill_dir2).unwrap();
        fs::write(
            skill_dir2.join("SKILL.md"),
            "# Another Skill\n\nNo frontmatter.",
        )
        .unwrap();

        // Create a non-skill directory (no SKILL.md)
        let non_skill_dir = temp_path.join("not-a-skill");
        fs::create_dir(&non_skill_dir).unwrap();

        let skills = discover_skills_in_dir(temp_path);
        assert_eq!(skills.len(), 2);

        // Skills should be discovered
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"my-skill"));
        assert!(names.contains(&"another-skill"));
    }

    fn skill(name: &str, content: Option<&'static str>) -> SkillInfo {
        SkillInfo {
            name: name.to_string(),
            description: format!("{} desc", name),
            path: PathBuf::new(),
            content,
        }
    }

    #[test]
    fn test_merge_skill_tiers_shadows_by_priority() {
        let project = vec![skill("memory", None)];
        let global = vec![skill("memory", None), skill("deploy", None)];
        let builtin = vec![
            skill("memory", Some("built-in body")),
            skill("testing", Some("built-in testing")),
        ];

        let merged = merge_skill_tiers(&[project, global, builtin]);
        let names: Vec<&str> = merged.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["memory", "deploy", "testing"]);

        // Project tier wins: no embedded content
        assert!(merged[0].content.is_none());
        // Built-in tier provides embedded content when not shadowed
        assert!(merged[2].content.is_some());
    }

    #[test]
    fn test_discover_skills_includes_builtins() {
        let skills = discover_skills();
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"memory"),
            "built-in memory skill must be discovered"
        );
        // First occurrence of a name wins — no duplicates across tiers
        let mut deduped = names.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(deduped.len(), names.len());
    }

    #[test]
    fn test_user_skill_overrides_builtin() {
        // A user-defined skill with the same name as a built-in must win
        let temp_dir = TempDir::new().unwrap();
        let skill_dir = temp_dir.path().join("memory");
        fs::create_dir(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: memory\ndescription: my custom memory protocol\n---\n# Custom Memory\nDo it my way.\n",
        )
        .unwrap();

        let user_tier = discover_skills_in_dir(temp_dir.path());
        let merged =
            merge_skill_tiers(&[user_tier, crate::tools::skills::builtin::builtin_skills()]);

        let memories: Vec<&SkillInfo> = merged.iter().filter(|s| s.name == "memory").collect();
        assert_eq!(memories.len(), 1, "memory must appear exactly once");
        // Disk skill wins: no embedded content, description from user frontmatter
        assert!(memories[0].content.is_none());
        assert_eq!(memories[0].description, "my custom memory protocol");
        // A disk skill outside the project dir reports as global, not built-in
        assert_eq!(memories[0].source(), "global");
    }

    #[test]
    fn test_source_labels_builtin() {
        let builtin = crate::tools::skills::builtin::builtin_skills();
        assert!(!builtin.is_empty());
        for skill in &builtin {
            assert_eq!(skill.source(), "built-in");
        }
    }
}
