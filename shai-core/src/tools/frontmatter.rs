// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

/// Parse frontmatter from a markdown file content.
///
/// Frontmatter format:
/// ```
/// ---
/// name: My Name
/// description: A short description
/// ---
/// # Rest of the file...
/// ```
pub fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }

    let rest = &trimmed[3..];
    let end = rest.find("\n---")?;
    let frontmatter = &rest[..end];

    let mut name = String::new();
    let mut description = String::new();

    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("name:") {
            name = val.trim().trim_matches('"').to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').to_string();
        }
    }

    if name.is_empty() {
        return None;
    }

    Some((name, description))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_with_metadata() {
        let content = r#"---
name: my-skill
description: A test skill
---

# My Skill

Some content here.
"#;
        let result = parse_frontmatter(content);
        assert!(result.is_some());
        let (name, desc) = result.unwrap();
        assert_eq!(name, "my-skill");
        assert_eq!(desc, "A test skill");
    }

    #[test]
    fn test_parse_frontmatter_no_frontmatter() {
        let content = "# Just a heading\n\nNo frontmatter here.";
        let result = parse_frontmatter(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_frontmatter_quoted_values() {
        let content =
            "---\nname: \"quoted-name\"\ndescription: \"A quoted description\"\n---\n# Body";
        let result = parse_frontmatter(content);
        assert!(result.is_some());
        let (name, desc) = result.unwrap();
        assert_eq!(name, "quoted-name");
        assert_eq!(desc, "A quoted description");
    }

    #[test]
    fn test_parse_frontmatter_missing_name() {
        let content = "---\ndescription: No name\n---\n# Body";
        let result = parse_frontmatter(content);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_frontmatter_empty() {
        let result = parse_frontmatter("");
        assert!(result.is_none());
    }
}
