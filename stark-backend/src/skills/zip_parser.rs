use crate::skills::types::{SkillArgument, SkillMetadata};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Parsed skill from ZIP file
#[derive(Debug, Clone)]
pub struct ParsedSkill {
    pub name: String,
    pub description: String,
    pub body: String,           // Prompt template
    pub version: String,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub metadata: Option<String>,
    pub requires_tools: Vec<String>,
    pub requires_binaries: Vec<String>,
    pub arguments: HashMap<String, SkillArgument>,
    pub tags: Vec<String>,
    pub subagent_type: Option<String>,
    pub scripts: Vec<ParsedScript>,
}

/// Parsed script from ZIP file
#[derive(Debug, Clone)]
pub struct ParsedScript {
    pub name: String,
    pub code: String,
    pub language: String,
}

impl ParsedScript {
    /// Determine language from file extension
    pub fn detect_language(filename: &str) -> String {
        let ext = filename.rsplit('.').next().unwrap_or("");
        match ext.to_lowercase().as_str() {
            "py" => "python".to_string(),
            "sh" | "bash" => "bash".to_string(),
            "js" => "javascript".to_string(),
            "ts" => "typescript".to_string(),
            "rb" => "ruby".to_string(),
            _ => "unknown".to_string(),
        }
    }
}

/// Parse a ZIP file containing a skill package
pub fn parse_skill_zip(data: &[u8]) -> Result<ParsedSkill, String> {
    // ZIP bomb protection: reject compressed data > 10MB
    const MAX_ZIP_BYTES: usize = crate::disk_quota::MAX_SKILL_ZIP_BYTES;

    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| format!("Failed to read ZIP file: {}", e))?;

    // Pre-check: sum of uncompressed sizes declared in the archive
    {
        let mut total_uncompressed: u64 = 0;
        for i in 0..archive.len() {
            if let Ok(file) = archive.by_index(i) {
                total_uncompressed += file.size();
            }
        }
        if total_uncompressed > MAX_ZIP_BYTES as u64 {
            return Err(format!(
                "ZIP bomb protection: total uncompressed size ({} bytes) exceeds the 10MB limit.",
                total_uncompressed,
            ));
        }
    }

    let mut scripts: Vec<ParsedScript> = Vec::new();
    let mut skill_md_path: Option<String> = None;

    // First pass: find SKILL.md and collect info about structure
    for i in 0..archive.len() {
        let file = archive.by_index(i)
            .map_err(|e| format!("Failed to read ZIP entry: {}", e))?;

        let name = file.name().to_string();

        // Skip directories
        if name.ends_with('/') {
            continue;
        }

        // Normalize path (handle nested folder in ZIP)
        let normalized = normalize_zip_path(&name);

        if normalized.eq_ignore_ascii_case("skill.md") || normalized.ends_with("/skill.md") {
            skill_md_path = Some(name.clone());
        }
    }

    // Second pass: read SKILL.md
    let skill_md = if let Some(ref path) = skill_md_path {
        let mut file = archive.by_name(path)
            .map_err(|e| format!("Failed to read SKILL.md: {}", e))?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| format!("Failed to read SKILL.md content: {}", e))?;
        content
    } else {
        return Err("ZIP file must contain a SKILL.md file".to_string());
    };
    let (metadata, body) = parse_skill_md(&skill_md)?;

    // Third pass: collect scripts
    let base_dir = skill_md_path.as_ref()
        .and_then(|p| p.rsplit('/').nth(1))
        .unwrap_or("");

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)
            .map_err(|e| format!("Failed to read ZIP entry: {}", e))?;

        let name = file.name().to_string();

        // Skip directories
        if name.ends_with('/') {
            continue;
        }

        // Check if this is a script file in a scripts/ subdirectory
        let normalized = normalize_zip_path(&name);
        let is_script = if base_dir.is_empty() {
            normalized.starts_with("scripts/")
        } else {
            normalized.starts_with(&format!("{}/scripts/", base_dir)) ||
            normalized.starts_with("scripts/")
        };

        if is_script {
            // Extract script name (last component of path)
            let script_name = name.rsplit('/').next().unwrap_or(&name);

            // Skip non-script files
            let language = ParsedScript::detect_language(script_name);
            if language == "unknown" {
                continue;
            }

            let mut content = String::new();
            file.read_to_string(&mut content)
                .map_err(|e| format!("Failed to read script {}: {}", script_name, e))?;

            scripts.push(ParsedScript {
                name: script_name.to_string(),
                code: content,
                language,
            });
        }
    }

    Ok(ParsedSkill {
        name: metadata.name,
        description: metadata.description,
        body,
        version: metadata.version,
        author: metadata.author,
        homepage: metadata.homepage,
        metadata: metadata.metadata,
        requires_tools: metadata.requires_tools,
        requires_binaries: metadata.requires_binaries,
        arguments: metadata.arguments,
        tags: metadata.tags,
        subagent_type: metadata.subagent_type,
        scripts,
    })
}

/// Normalize ZIP path by removing leading directory if it's the only top-level entry
fn normalize_zip_path(path: &str) -> String {
    path.trim_start_matches('/').to_string()
}

/// Parse SKILL.md content into metadata and body
pub fn parse_skill_md(content: &str) -> Result<(SkillMetadata, String), String> {
    let content = content.trim();

    // Check for frontmatter delimiters
    if !content.starts_with("---") {
        return Err("SKILL.md must start with YAML frontmatter (---)".to_string());
    }

    // Find the end of frontmatter
    let rest = &content[3..]; // Skip first ---
    let end_idx = rest.find("---").ok_or("Missing closing --- for frontmatter")?;

    let frontmatter = rest[..end_idx].trim();
    let body = rest[end_idx + 3..].trim().to_string();

    // Parse YAML frontmatter
    let metadata = parse_yaml_frontmatter(frontmatter)?;

    if metadata.name.is_empty() {
        return Err("Skill name is required in frontmatter".to_string());
    }

    if metadata.description.is_empty() {
        return Err("Skill description is required in frontmatter".to_string());
    }

    Ok((metadata, body))
}

/// Parse YAML frontmatter into SkillMetadata
fn parse_yaml_frontmatter(yaml: &str) -> Result<SkillMetadata, String> {
    let mut metadata = SkillMetadata::default();
    let mut current_key = String::new();
    let mut in_arguments = false;
    let mut current_arg_name = String::new();
    let mut current_arg = SkillArgument {
        description: String::new(),
        required: false,
        default: None,
    };

    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Check indentation level
        let indent = line.len() - line.trim_start().len();

        if indent == 0 {
            // Top-level key
            if let Some((key, value)) = trimmed.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                current_key = key.to_string();
                in_arguments = key == "arguments";

                match key {
                    "name" => metadata.name = unquote(value),
                    "description" => metadata.description = unquote(value),
                    "version" => metadata.version = unquote(value),
                    "author" => metadata.author = Some(unquote(value)),
                    "homepage" => metadata.homepage = Some(unquote(value)),
                    "metadata" => {
                        let value_str = unquote(value);
                        if !value_str.is_empty() {
                            metadata.metadata = Some(value_str);
                        }
                    }
                    "requires_tools" => {
                        if value.starts_with('[') {
                            metadata.requires_tools = parse_inline_list(value);
                        }
                    }
                    "requires_binaries" => {
                        if value.starts_with('[') {
                            metadata.requires_binaries = parse_inline_list(value);
                        }
                    }
                    "tags" => {
                        if value.starts_with('[') {
                            metadata.tags = parse_inline_list(value);
                        }
                    }
                    _ => {}
                }
            }
        } else if indent == 2 {
            // Second-level (list items or argument names)
            if trimmed.starts_with("- ") {
                let value = trimmed[2..].trim();
                match current_key.as_str() {
                    "requires_tools" => metadata.requires_tools.push(unquote(value)),
                    "requires_binaries" => metadata.requires_binaries.push(unquote(value)),
                    "tags" => metadata.tags.push(unquote(value)),
                    _ => {}
                }
            } else if in_arguments {
                // Argument name
                if let Some((arg_name, _)) = trimmed.split_once(':') {
                    if !current_arg_name.is_empty() {
                        metadata
                            .arguments
                            .insert(current_arg_name.clone(), current_arg.clone());
                    }
                    current_arg_name = arg_name.trim().to_string();
                    current_arg = SkillArgument {
                        description: String::new(),
                        required: false,
                        default: None,
                    };
                }
            }
        } else if indent >= 4 && in_arguments {
            // Argument properties
            if let Some((key, value)) = trimmed.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "description" => current_arg.description = unquote(value),
                    "required" => current_arg.required = value == "true",
                    "default" => current_arg.default = Some(unquote(value)),
                    _ => {}
                }
            }
        }
    }

    // Don't forget the last argument
    if in_arguments && !current_arg_name.is_empty() {
        metadata.arguments.insert(current_arg_name, current_arg);
    }

    Ok(metadata)
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn parse_inline_list(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.starts_with('[') && s.ends_with(']') {
        s[1..s.len() - 1]
            .split(',')
            .map(|item| unquote(item.trim()))
            .filter(|item| !item.is_empty())
            .collect()
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_md() {
        let content = r#"---
name: code-review
description: Review code and provide feedback
version: 1.0.0
requires_tools: [read_file, exec]
arguments:
  path:
    description: "Path to review"
    default: "."
---
You are a code reviewer. Review the code at {{path}} and provide feedback.
"#;

        let (metadata, body) = parse_skill_md(content).unwrap();
        assert_eq!(metadata.name, "code-review");
        assert_eq!(metadata.description, "Review code and provide feedback");
        assert_eq!(metadata.version, "1.0.0");
        assert_eq!(metadata.requires_tools, vec!["read_file", "exec"]);
        assert!(metadata.arguments.contains_key("path"));
        assert!(body.contains("You are a code reviewer"));
    }

    #[test]
    fn test_parse_skill_md_missing_frontmatter() {
        let content = "Just some text without frontmatter";
        let result = parse_skill_md(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(ParsedScript::detect_language("test.py"), "python");
        assert_eq!(ParsedScript::detect_language("test.sh"), "bash");
        assert_eq!(ParsedScript::detect_language("test.js"), "javascript");
        assert_eq!(ParsedScript::detect_language("test.txt"), "unknown");
    }
}
