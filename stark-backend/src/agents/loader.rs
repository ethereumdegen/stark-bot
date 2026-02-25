//! Agent folder loader and writer.
//!
//! Agents are stored as `agents/{key}/agent.md` with YAML frontmatter + prompt body.
//! This module provides functions to parse, load, write, and delete agent folders.

use crate::ai::multi_agent::types::AgentSubtypeConfig;
use std::path::Path;

/// Parse an agent.md file content into an AgentSubtypeConfig.
/// Format: YAML frontmatter between `---` delimiters, followed by prompt body.
pub fn parse_agent_file(content: &str) -> Result<AgentSubtypeConfig, String> {
    let content = content.trim();

    if !content.starts_with("---") {
        return Err("agent.md must start with YAML frontmatter (---)".to_string());
    }

    let rest = &content[3..];
    let end_idx = rest.find("---").ok_or("Missing closing --- for frontmatter")?;

    let frontmatter = rest[..end_idx].trim();
    let prompt = rest[end_idx + 3..].trim().to_string();

    let mut config = AgentSubtypeConfig {
        key: String::new(),
        version: String::new(),
        label: String::new(),
        emoji: String::new(),
        description: String::new(),
        tool_groups: Vec::new(),
        skill_tags: Vec::new(),
        additional_tools: Vec::new(),
        prompt,
        sort_order: 0,
        enabled: true,
        max_iterations: 90,
        skip_task_planner: false,
        aliases: Vec::new(),
        hidden: false,
        preferred_ai_model: None,
        hooks: Vec::new(),
    };

    // Hand-rolled YAML parser (no serde_yaml crate)
    let mut current_list_key = String::new();
    // State for skipping legacy hooks block in frontmatter (hooks are now auto-detected from files)
    let mut in_hooks_block = false;

    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Skip legacy hooks block in frontmatter (hooks are auto-detected from hooks/ directory)
        if in_hooks_block {
            if indent >= 2 {
                continue; // still inside hooks block
            }
            in_hooks_block = false;
            // Fall through to normal parsing for this line
        }

        // List item continuation
        if indent > 0 && trimmed.starts_with("- ") {
            let value = trimmed[2..].trim().to_string();
            let value = unquote_yaml(&value);
            match current_list_key.as_str() {
                "tool_groups" => config.tool_groups.push(value),
                "skill_tags" => config.skill_tags.push(value),
                "additional_tools" => config.additional_tools.push(value),
                "aliases" => config.aliases.push(value),
                _ => {}
            }
            continue;
        }

        // Top-level key: value
        if let Some((key, value)) = split_yaml_kv(trimmed) {
            current_list_key.clear();
            match key {
                "key" => config.key = unquote_yaml(value),
                "version" => config.version = unquote_yaml(value),
                "label" => config.label = unquote_yaml(value),
                "emoji" => config.emoji = parse_emoji_value(value),
                "description" => config.description = unquote_yaml(value),
                "sort_order" => config.sort_order = value.parse().unwrap_or(0),
                "enabled" => config.enabled = value == "true",
                "max_iterations" => config.max_iterations = value.parse().unwrap_or(90),
                "skip_task_planner" => config.skip_task_planner = value == "true",
                "hidden" => config.hidden = value == "true",
                "preferred_ai_model" => {
                    let v = unquote_yaml(value);
                    config.preferred_ai_model = if v.is_empty() || v == "none" { None } else { Some(v) };
                }
                "hooks" => {
                    // Legacy: skip hooks block in frontmatter (auto-detected from hooks/ dir)
                    if value.is_empty() {
                        in_hooks_block = true;
                    }
                }
                "tool_groups" | "skill_tags" | "additional_tools" | "aliases" => {
                    // Inline array or block list
                    if value.starts_with('[') {
                        let items = parse_inline_yaml_array(value);
                        match key {
                            "tool_groups" => config.tool_groups = items,
                            "skill_tags" => config.skill_tags = items,
                            "additional_tools" => config.additional_tools = items,
                            "aliases" => config.aliases = items,
                            _ => {}
                        }
                    } else if value.is_empty() {
                        // Block list — items follow on indented lines
                        current_list_key = key.to_string();
                    }
                }
                _ => {} // ignore unknown keys
            }
        }
    }

    if config.key.is_empty() {
        return Err("Agent key is required in frontmatter".to_string());
    }

    Ok(config)
}

/// Load all agents from a directory (scans for `{name}/agent.md` patterns).
pub fn load_agents_from_directory(dir: &Path) -> Result<Vec<AgentSubtypeConfig>, String> {
    let mut configs = Vec::new();

    if !dir.exists() {
        return Ok(configs);
    }

    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read agents directory {}: {}", dir.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with('_') {
            continue;
        }

        let agent_md = path.join("agent.md");
        if !agent_md.is_file() {
            continue;
        }

        match std::fs::read_to_string(&agent_md) {
            Ok(content) => match parse_agent_file(&content) {
                Ok(mut config) => {
                    // Auto-detect hooks from hooks/ directory
                    load_hooks_from_directory(&path, &mut config);
                    log::info!("[AGENTS] Loaded agent '{}' from {} ({} hooks)", config.key, agent_md.display(), config.hooks.len());
                    configs.push(config);
                }
                Err(e) => {
                    log::warn!("[AGENTS] Failed to parse {}: {}", agent_md.display(), e);
                }
            },
            Err(e) => {
                log::warn!("[AGENTS] Failed to read {}: {}", agent_md.display(), e);
            }
        }
    }

    configs.sort_by_key(|c| c.sort_order);
    log::info!("[AGENTS] Loaded {} agent subtypes from {}", configs.len(), dir.display());
    Ok(configs)
}

/// Write an agent config as `{agents_dir}/{key}/agent.md`.
pub fn write_agent_folder(agents_dir: &Path, config: &AgentSubtypeConfig) -> Result<(), String> {
    let folder = agents_dir.join(&config.key);
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("Failed to create agent folder '{}': {}", config.key, e))?;

    let content = serialize_agent_md(config);
    let agent_md = folder.join("agent.md");
    std::fs::write(&agent_md, content)
        .map_err(|e| format!("Failed to write {}: {}", agent_md.display(), e))?;

    Ok(())
}

/// Delete an agent folder from disk.
pub fn delete_agent_folder(agents_dir: &Path, key: &str) -> Result<(), String> {
    let folder = agents_dir.join(key);
    if folder.is_dir() {
        std::fs::remove_dir_all(&folder)
            .map_err(|e| format!("Failed to delete agent folder '{}': {}", key, e))?;
    }
    Ok(())
}

/// Reload the global registry from disk.
pub fn reload_registry_from_disk() {
    let agents_dir = crate::config::runtime_agents_dir();
    let configs = load_agents_from_directory(&agents_dir)
        .unwrap_or_else(|e| {
            log::error!("[AGENTS] Failed to reload agents: {}", e);
            vec![]
        });
    crate::ai::multi_agent::types::load_subtype_registry(configs);
}

/// Auto-detect hooks from the agent's hooks/ subdirectory.
/// Each `.md` file becomes a hook with the filename stem as the event name.
/// e.g. `hooks/heartbeat.md` → event "heartbeat", `hooks/discord_message.md` → event "discord_message"
fn load_hooks_from_directory(agent_dir: &Path, config: &mut AgentSubtypeConfig) {
    use crate::ai::multi_agent::types::PersonaHook;

    let hooks_dir = agent_dir.join("hooks");
    let entries = match std::fs::read_dir(&hooks_dir) {
        Ok(e) => e,
        Err(_) => return, // No hooks/ directory — no hooks
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("md") {
            continue;
        }
        let event = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c.trim().to_string(),
            Err(e) => {
                log::warn!("[AGENTS] Failed to read hook template {}: {}", path.display(), e);
                continue;
            }
        };
        if content.is_empty() {
            continue;
        }

        // Parse optional YAML frontmatter (--- delimited) for hook settings
        let mut safe_mode = false;
        let prompt_template = if content.starts_with("---") {
            if let Some(end) = content[3..].find("---") {
                let frontmatter = &content[3..3 + end];
                // Simple key: value parsing for safe_mode
                for line in frontmatter.lines() {
                    let line = line.trim();
                    if let Some(val) = line.strip_prefix("safe_mode:") {
                        safe_mode = val.trim().eq_ignore_ascii_case("true");
                    }
                }
                content[3 + end + 3..].trim().to_string()
            } else {
                content
            }
        } else {
            content
        };

        log::info!(
            "[AGENTS] Loaded hook '{}' for agent '{}' from {} (safe_mode={})",
            event, config.key, path.display(), safe_mode
        );
        config.hooks.push(PersonaHook {
            event,
            prompt_template,
            safe_mode,
        });
    }
}

// =====================================================
// Serialization helpers
// =====================================================

/// Serialize an AgentSubtypeConfig to agent.md format (YAML frontmatter + prompt body).
fn serialize_agent_md(config: &AgentSubtypeConfig) -> String {
    let mut yaml = String::from("---\n");

    yaml.push_str(&format!("key: {}\n", &config.key));
    if !config.version.is_empty() {
        yaml.push_str(&format!("version: \"{}\"\n", &config.version));
    }
    yaml.push_str(&format!("label: {}\n", quote_yaml_if_needed(&config.label)));
    yaml.push_str(&format!("emoji: {}\n", quote_yaml_value(&config.emoji)));
    yaml.push_str(&format!("description: {}\n", quote_yaml_value(&config.description)));
    yaml.push_str(&format!("aliases: {}\n", format_inline_array(&config.aliases)));
    yaml.push_str(&format!("sort_order: {}\n", config.sort_order));
    yaml.push_str(&format!("enabled: {}\n", config.enabled));
    yaml.push_str(&format!("max_iterations: {}\n", config.max_iterations));
    yaml.push_str(&format!("skip_task_planner: {}\n", config.skip_task_planner));
    yaml.push_str(&format!("hidden: {}\n", config.hidden));
    if let Some(ref model) = config.preferred_ai_model {
        yaml.push_str(&format!("preferred_ai_model: {}\n", model));
    }
    yaml.push_str(&format!("tool_groups: {}\n", format_inline_array(&config.tool_groups)));
    yaml.push_str(&format_block_array("skill_tags", &config.skill_tags));
    yaml.push_str(&format_block_array("additional_tools", &config.additional_tools));

    // Hooks are auto-detected from hooks/ directory — not serialized in frontmatter

    yaml.push_str("---\n\n");
    yaml.push_str(&config.prompt);
    yaml.push_str("\n");

    yaml
}

/// Format a vec as an inline YAML array: [a, b, c]
fn format_inline_array(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    let inner: Vec<String> = items.iter().map(|s| quote_yaml_if_needed(s)).collect();
    format!("[{}]", inner.join(", "))
}

/// Format a vec as a block YAML array (one item per line).
/// Returns empty string if vec is empty.
fn format_block_array(key: &str, items: &[String]) -> String {
    if items.is_empty() {
        return format!("{}: []\n", key);
    }
    let mut out = format!("{}:\n", key);
    for item in items {
        out.push_str(&format!("  - {}\n", quote_yaml_if_needed(item)));
    }
    out
}

/// Quote a YAML value with double quotes (always quotes).
fn quote_yaml_value(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Quote a YAML value only if it contains special characters.
fn quote_yaml_if_needed(s: &str) -> String {
    if s.is_empty() || s.contains(':') || s.contains('#') || s.contains('"')
        || s.contains('\'') || s.contains(',') || s.contains('[') || s.contains(']')
        || s.contains('{') || s.contains('}') || s.starts_with(' ') || s.ends_with(' ')
    {
        quote_yaml_value(s)
    } else {
        s.to_string()
    }
}

// =====================================================
// Parsing helpers
// =====================================================

/// Split a "key: value" line. Returns None if no colon found.
fn split_yaml_kv(line: &str) -> Option<(&str, &str)> {
    let colon_pos = line.find(':')?;
    let key = line[..colon_pos].trim();
    let value = line[colon_pos + 1..].trim();
    Some((key, value))
}

/// Unquote a YAML string value (strip surrounding quotes, handle escapes).
fn unquote_yaml(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        let inner = &s[1..s.len() - 1];
        inner.replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        s.to_string()
    }
}

/// Parse a YAML Unicode escape like \U0001F3AC into the actual character.
fn parse_emoji_value(s: &str) -> String {
    let s = unquote_yaml(s);
    // Handle \U escape sequences (e.g. \U0001F3AC)
    if s.contains("\\U") || s.contains("\\u") {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(&next) = chars.peek() {
                    if next == 'U' || next == 'u' {
                        chars.next(); // consume U/u
                        let hex: String = chars.by_ref().take(if next == 'U' { 8 } else { 4 }).collect();
                        if let Ok(code) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(code) {
                                result.push(ch);
                                continue;
                            }
                        }
                        // Fallback: put it back as-is
                        result.push('\\');
                        result.push(next);
                        result.push_str(&hex);
                        continue;
                    }
                }
                result.push(c);
            } else {
                result.push(c);
            }
        }
        result
    } else {
        s
    }
}

/// Parse an inline YAML array like `[a, b, c]` or `["a", "b"]`.
fn parse_inline_yaml_array(s: &str) -> Vec<String> {
    let s = s.trim();
    if s == "[]" {
        return Vec::new();
    }
    if !s.starts_with('[') || !s.ends_with(']') {
        return Vec::new();
    }
    let inner = &s[1..s.len() - 1];
    inner
        .split(',')
        .map(|item| unquote_yaml(item.trim()))
        .filter(|item| !item.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agent_file_basic() {
        let content = r#"---
key: finance
label: Finance
emoji: "\U0001F4B0"
description: "Crypto swaps"
aliases: [defi, crypto]
sort_order: 0
enabled: true
max_iterations: 90
skip_task_planner: false
hidden: false
tool_groups: [system, web]
skill_tags:
  - general
  - crypto
additional_tools: []
---

Finance prompt body here.
"#;

        let config = parse_agent_file(content).unwrap();
        assert_eq!(config.key, "finance");
        assert_eq!(config.label, "Finance");
        assert_eq!(config.emoji, "\u{1F4B0}");
        assert_eq!(config.description, "Crypto swaps");
        assert_eq!(config.aliases, vec!["defi", "crypto"]);
        assert_eq!(config.sort_order, 0);
        assert!(config.enabled);
        assert_eq!(config.max_iterations, 90);
        assert!(!config.skip_task_planner);
        assert!(!config.hidden);
        assert_eq!(config.tool_groups, vec!["system", "web"]);
        assert_eq!(config.skill_tags, vec!["general", "crypto"]);
        assert!(config.additional_tools.is_empty());
        assert_eq!(config.prompt, "Finance prompt body here.");
    }

    #[test]
    fn test_parse_inline_yaml_array() {
        assert_eq!(parse_inline_yaml_array("[]"), Vec::<String>::new());
        assert_eq!(parse_inline_yaml_array("[a, b, c]"), vec!["a", "b", "c"]);
        assert_eq!(
            parse_inline_yaml_array("[\"hello\", \"world\"]"),
            vec!["hello", "world"]
        );
    }

    #[test]
    fn test_roundtrip_serialize_parse() {
        let config = AgentSubtypeConfig {
            key: "test".to_string(),
            version: "1.0.0".to_string(),
            label: "Test Agent".to_string(),
            emoji: "\u{1F680}".to_string(),
            description: "A test agent".to_string(),
            tool_groups: vec!["system".to_string(), "web".to_string()],
            skill_tags: vec!["general".to_string()],
            additional_tools: vec!["my_tool".to_string()],
            prompt: "Hello world".to_string(),
            sort_order: 5,
            enabled: true,
            max_iterations: 50,
            skip_task_planner: true,
            aliases: vec!["tester".to_string()],
            hidden: false,
            preferred_ai_model: Some("minimax".to_string()),
            hooks: Vec::new(),
        };

        let md = serialize_agent_md(&config);
        let parsed = parse_agent_file(&md).unwrap();

        assert_eq!(parsed.key, config.key);
        assert_eq!(parsed.label, config.label);
        assert_eq!(parsed.description, config.description);
        assert_eq!(parsed.tool_groups, config.tool_groups);
        assert_eq!(parsed.skill_tags, config.skill_tags);
        assert_eq!(parsed.additional_tools, config.additional_tools);
        assert_eq!(parsed.prompt, config.prompt);
        assert_eq!(parsed.sort_order, config.sort_order);
        assert_eq!(parsed.enabled, config.enabled);
        assert_eq!(parsed.max_iterations, config.max_iterations);
        assert_eq!(parsed.skip_task_planner, config.skip_task_planner);
        assert_eq!(parsed.aliases, config.aliases);
        assert_eq!(parsed.hidden, config.hidden);
        assert_eq!(parsed.preferred_ai_model, config.preferred_ai_model);
    }

    #[test]
    fn test_parse_emoji_unicode_escape() {
        assert_eq!(parse_emoji_value("\"\\U0001F3AC\""), "\u{1F3AC}");
        assert_eq!(parse_emoji_value("\"\\U0001F4B0\""), "\u{1F4B0}");
        // Already-decoded emoji passes through
        assert_eq!(parse_emoji_value("\"\u{1F680}\""), "\u{1F680}");
    }
}
