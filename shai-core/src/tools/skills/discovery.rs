use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Metadata extracted from a SKILL.md file's frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    /// Skill name (from frontmatter `name` or directory name)
    pub name: String,
    /// Short description (from frontmatter `description`)
    pub description: String,
    /// Full path to the SKILL.md file
    pub path: PathBuf,
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
fn parse_skill_frontmatter(content: &str) -> Option<(String, String)> {
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
        });
    }

    skills
}

/// Discover all skills from both global (`~/.config/shai/skills/`) and
/// project-local (`.shai/skills/`) directories.
///
/// Project-local skills take precedence (they shadow global skills with the same name).
pub fn discover_skills() -> Vec<SkillInfo> {
    let mut skills = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    // Project-local skills first (higher priority)
    let git_root = crate::runners::coder::env::find_git_root();
    if let Some(root) = git_root {
        let project_skills_dir = root.join(".shai").join("skills");
        for skill in discover_skills_in_dir(&project_skills_dir) {
            seen_names.insert(skill.name.clone());
            skills.push(skill);
        }
    }

    // Global skills
    let home = std::env::var("HOME").ok();
    if let Some(home) = home {
        let global_skills_dir = PathBuf::from(home)
            .join(".config")
            .join("shai")
            .join("skills");
        for skill in discover_skills_in_dir(&global_skills_dir) {
            if !seen_names.contains(&skill.name) {
                seen_names.insert(skill.name.clone());
                skills.push(skill);
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
            },
            SkillInfo {
                name: "test".to_string(),
                description: String::new(),
                path: PathBuf::from("/tmp/skills/test/SKILL.md"),
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
}
