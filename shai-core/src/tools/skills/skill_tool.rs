use std::fs;

use crate::tools::tool;
use crate::tools::types::ToolResult;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::discovery::discover_skills;

/// Parameters for loading a skill by name.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkillToolParams {
    /// The name of the skill to load.
    pub name: String,
}

/// SkillTool — load a skill's full SKILL.md body on demand.
///
/// Only the skill catalog (name + description) is injected into the system
/// prompt. This tool loads the full body when the model decides it needs
/// the detailed instructions.
#[derive(Clone)]
pub struct SkillTool;

#[tool(
    name = "skill",
    description = r#"Load and display the full instructions of a named skill. Use this when you need the detailed procedure described by a skill from the catalog. Returns the full SKILL.md content.

**Examples:**
```json
{"name": "my-skill-name"}
```
"#
)]
impl SkillTool {
    pub fn new() -> Self {
        Self
    }

    async fn execute(&self, params: SkillToolParams) -> ToolResult {
        if params.name.trim().is_empty() {
            return ToolResult::error("Skill name cannot be empty".to_string());
        }

        let skills = discover_skills();
        let skill = skills.iter().find(|s| s.name == params.name.trim());

        match skill {
            Some(skill_info) => {
                let content = match fs::read_to_string(&skill_info.path) {
                    Ok(c) => c,
                    Err(e) => {
                        return ToolResult::error(format!(
                            "Failed to read skill file '{}': {}",
                            skill_info.path.display(),
                            e
                        ));
                    }
                };

                // Strip the frontmatter — return only the body after the closing `---`
                let body = strip_frontmatter(&content);
                ToolResult::success(body)
            }
            None => {
                let available: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
                ToolResult::error(format!(
                    "Skill '{}' not found. Available skills: {}",
                    params.name,
                    if available.is_empty() {
                        "(none)".to_string()
                    } else {
                        available.join(", ")
                    }
                ))
            }
        }
    }
}

impl Default for SkillTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Remove YAML frontmatter (``---\n...\n---``) from the beginning of a SKILL.md.
fn strip_frontmatter(content: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content.to_string();
    }

    let rest = &trimmed[3..];
    if let Some(end) = rest.find("\n---") {
        let after_frontmatter = &rest[end + 4..];
        // Skip the trailing newline(s) after the closing ---
        return after_frontmatter.trim_start().to_string();
    }

    // Malformed frontmate — return as-is
    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_frontmatter_with_metadata() {
        let content = "---\nname: my-skill\ndescription: test\n---\n# My Skill\n\nBody content";
        let result = strip_frontmatter(content);
        assert!(result.contains("# My Skill"));
        assert!(result.contains("Body content"));
        assert!(!result.contains("name:"));
    }

    #[test]
    fn test_strip_frontmatter_without_frontmatter() {
        let content = "# Just a heading\n\nNo frontmatter";
        let result = strip_frontmatter(content);
        assert_eq!(result, "# Just a heading\n\nNo frontmatter");
    }

    #[tokio::test]
    async fn test_skill_tool_empty_name_rejected() {
        let tool = SkillTool::new();
        let result = tool
            .execute(SkillToolParams {
                name: "   ".to_string(),
            })
            .await;
        assert!(result.is_error());
    }

    #[tokio::test]
    async fn test_skill_tool_nonexistent_skill() {
        let tool = SkillTool::new();
        let result = tool
            .execute(SkillToolParams {
                name: "nonexistent-skill-xyz".to_string(),
            })
            .await;
        assert!(result.is_error());
    }
}
