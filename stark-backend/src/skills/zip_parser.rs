use crate::skills::types::{SkillApiKey, SkillArgument, SkillMetadata};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Parsed ABI from ZIP file or disk
#[derive(Debug, Clone)]
pub struct ParsedAbi {
    pub name: String,
    pub content: String,
}

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
    pub requires_api_keys: HashMap<String, SkillApiKey>,
    pub scripts: Vec<ParsedScript>,
    pub abis: Vec<ParsedAbi>,
    pub presets_content: Option<String>,
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

    // Third pass: collect scripts, ABIs, and presets
    let base_dir = skill_md_path.as_ref()
        .and_then(|p| p.rsplit('/').nth(1))
        .unwrap_or("");

    let mut abis: Vec<ParsedAbi> = Vec::new();
    let mut presets_content: Option<String> = None;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)
            .map_err(|e| format!("Failed to read ZIP entry: {}", e))?;

        let name = file.name().to_string();

        // Skip directories
        if name.ends_with('/') {
            continue;
        }

        let normalized = normalize_zip_path(&name);

        // Check if this is a script file in a scripts/ subdirectory
        let is_script = if base_dir.is_empty() {
            normalized.starts_with("scripts/")
        } else {
            normalized.starts_with(&format!("{}/scripts/", base_dir)) ||
            normalized.starts_with("scripts/")
        };

        // Check if this is an ABI file in an abis/ subdirectory
        let is_abi = if base_dir.is_empty() {
            normalized.starts_with("abis/") && normalized.ends_with(".json")
        } else {
            (normalized.starts_with(&format!("{}/abis/", base_dir)) || normalized.starts_with("abis/"))
                && normalized.ends_with(".json")
        };

        // Check if this is a presets.ron file
        let is_presets = if base_dir.is_empty() {
            normalized == "presets.ron"
        } else {
            normalized == format!("{}/presets.ron", base_dir) || normalized == "presets.ron"
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
        } else if is_abi {
            let abi_filename = name.rsplit('/').next().unwrap_or(&name);
            let abi_name = abi_filename.strip_suffix(".json").unwrap_or(abi_filename);

            let mut content = String::new();
            file.read_to_string(&mut content)
                .map_err(|e| format!("Failed to read ABI {}: {}", abi_filename, e))?;

            abis.push(ParsedAbi {
                name: abi_name.to_string(),
                content,
            });
        } else if is_presets {
            let mut content = String::new();
            file.read_to_string(&mut content)
                .map_err(|e| format!("Failed to read presets.ron: {}", e))?;

            presets_content = Some(content);
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
        requires_api_keys: metadata.requires_api_keys,
        scripts,
        abis,
        presets_content,
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

    // Parse YAML frontmatter (use shared parser from loader)
    let metadata = crate::skills::loader::serde_yaml_parse(frontmatter)?;

    if metadata.name.is_empty() {
        return Err("Skill name is required in frontmatter".to_string());
    }

    if metadata.description.is_empty() {
        return Err("Skill description is required in frontmatter".to_string());
    }

    Ok((metadata, body))
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
