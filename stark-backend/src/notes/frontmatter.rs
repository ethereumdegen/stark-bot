//! Parse and generate YAML frontmatter for notes.
//!
//! Hand-rolled YAML (no serde_yaml) — matches skills/loader.rs pattern.
//! Also extracts [[wikilinks]] and #tags from body content via regex.

use chrono::{DateTime, Local};
use regex::Regex;
use std::sync::LazyLock;

/// Parsed note frontmatter
#[derive(Debug, Clone, Default)]
pub struct NoteFrontmatter {
    pub title: String,
    pub date: Option<String>,
    pub updated: Option<String>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub note_type: String, // note, idea, decision, log, reflection, todo
}

/// A fully parsed note (frontmatter + body)
#[derive(Debug, Clone)]
pub struct ParsedNote {
    pub frontmatter: NoteFrontmatter,
    pub body: String,
    /// [[wikilinks]] extracted from body
    pub wikilinks: Vec<String>,
    /// #tags extracted from body (merged with frontmatter tags, deduplicated)
    pub all_tags: Vec<String>,
}

// Regex patterns for extraction
static WIKILINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());
static INLINE_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)#([a-zA-Z][a-zA-Z0-9_-]*)").unwrap());

/// Parse a complete note file (frontmatter + body)
pub fn parse_note(content: &str) -> ParsedNote {
    let (frontmatter, body) = split_frontmatter(content);
    let fm = parse_frontmatter(&frontmatter);

    let wikilinks = extract_wikilinks(&body);
    let inline_tags = extract_inline_tags(&body);

    // Merge frontmatter tags with inline tags (deduplicated)
    let mut all_tags = fm.tags.clone();
    for tag in &inline_tags {
        let lower = tag.to_lowercase();
        if !all_tags.iter().any(|t| t.to_lowercase() == lower) {
            all_tags.push(tag.clone());
        }
    }

    ParsedNote {
        frontmatter: fm,
        body,
        wikilinks,
        all_tags,
    }
}

/// Split content into (frontmatter_yaml, body). Returns empty frontmatter if none found.
fn split_frontmatter(content: &str) -> (String, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (String::new(), content.to_string());
    }

    // Find the closing ---
    let after_open = &trimmed[3..];
    if let Some(close_idx) = after_open.find("\n---") {
        let yaml = after_open[..close_idx].trim().to_string();
        let body_start = close_idx + 4; // skip \n---
        let body = if body_start < after_open.len() {
            after_open[body_start..].trim_start_matches('\n').to_string()
        } else {
            String::new()
        };
        (yaml, body)
    } else {
        (String::new(), content.to_string())
    }
}

/// Parse YAML frontmatter string into NoteFrontmatter (hand-rolled, no serde_yaml)
fn parse_frontmatter(yaml: &str) -> NoteFrontmatter {
    let mut fm = NoteFrontmatter {
        note_type: "note".to_string(),
        ..Default::default()
    };

    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "title" => fm.title = unquote(value),
                "date" => fm.date = Some(unquote(value)),
                "updated" => fm.updated = Some(unquote(value)),
                "type" => fm.note_type = unquote(value),
                "tags" => {
                    if value.starts_with('[') {
                        fm.tags = parse_inline_list(value);
                    }
                }
                "aliases" => {
                    if value.starts_with('[') {
                        fm.aliases = parse_inline_list(value);
                    }
                }
                _ => {}
            }
        }
    }

    fm
}

/// Generate YAML frontmatter string from parts
pub fn generate_frontmatter(
    title: &str,
    tags: &[String],
    aliases: &[String],
    note_type: &str,
    date: Option<&str>,
) -> String {
    let now: DateTime<Local> = Local::now();
    let date_str = date.unwrap_or(&now.format("%Y-%m-%dT%H:%M:%S").to_string()).to_string();

    let mut lines = Vec::new();
    lines.push("---".to_string());
    lines.push(format!("title: \"{}\"", title.replace('"', "\\\"")));
    lines.push(format!("date: {}", date_str));
    lines.push(format!("updated: {}", date_str));

    if !tags.is_empty() {
        let tags_str: Vec<String> = tags.iter().map(|t| t.to_string()).collect();
        lines.push(format!("tags: [{}]", tags_str.join(", ")));
    } else {
        lines.push("tags: []".to_string());
    }

    if !aliases.is_empty() {
        let aliases_str: Vec<String> = aliases.iter().map(|a| format!("\"{}\"", a.replace('"', "\\\""))).collect();
        lines.push(format!("aliases: [{}]", aliases_str.join(", ")));
    }

    lines.push(format!("type: {}", note_type));
    lines.push("---".to_string());

    lines.join("\n")
}

/// Update the `updated` timestamp in existing frontmatter content
pub fn touch_updated(content: &str) -> String {
    let now = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let mut result = String::new();
    let mut in_frontmatter = false;
    let mut found_open = false;

    for line in content.lines() {
        if line.trim() == "---" {
            if !found_open {
                found_open = true;
                in_frontmatter = true;
                result.push_str(line);
                result.push('\n');
                continue;
            } else {
                in_frontmatter = false;
                result.push_str(line);
                result.push('\n');
                continue;
            }
        }

        if in_frontmatter && line.trim().starts_with("updated:") {
            result.push_str(&format!("updated: {}", now));
            result.push('\n');
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    // Remove trailing extra newline if original didn't have one
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

/// Extract [[wikilinks]] from text
pub fn extract_wikilinks(text: &str) -> Vec<String> {
    WIKILINK_RE
        .captures_iter(text)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Extract #inline-tags from text
pub fn extract_inline_tags(text: &str) -> Vec<String> {
    INLINE_TAG_RE
        .captures_iter(text)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Remove surrounding quotes from a string
fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Parse an inline YAML list like [foo, bar, "baz qux"]
fn parse_inline_list(s: &str) -> Vec<String> {
    let s = s.trim();
    let inner = if s.starts_with('[') && s.ends_with(']') {
        &s[1..s.len() - 1]
    } else {
        s
    };

    inner
        .split(',')
        .map(|item| unquote(item.trim()))
        .filter(|item| !item.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_note_with_frontmatter() {
        let content = r#"---
title: "Test Note"
date: 2026-02-18T14:32:00
tags: [crypto, payments]
type: idea
---

# Test Note

Body with [[wikilink]] and #inline-tag.
"#;
        let parsed = parse_note(content);
        assert_eq!(parsed.frontmatter.title, "Test Note");
        assert_eq!(parsed.frontmatter.tags, vec!["crypto", "payments"]);
        assert_eq!(parsed.frontmatter.note_type, "idea");
        assert_eq!(parsed.wikilinks, vec!["wikilink"]);
        assert!(parsed.all_tags.contains(&"crypto".to_string()));
        assert!(parsed.all_tags.contains(&"inline-tag".to_string()));
    }

    #[test]
    fn test_parse_note_no_frontmatter() {
        let content = "# Just a heading\n\nSome body text.";
        let parsed = parse_note(content);
        assert_eq!(parsed.frontmatter.title, "");
        assert!(parsed.body.contains("Just a heading"));
    }

    #[test]
    fn test_generate_frontmatter() {
        let fm = generate_frontmatter(
            "My Note",
            &["tag1".to_string(), "tag2".to_string()],
            &[],
            "note",
            Some("2026-02-18T14:00:00"),
        );
        assert!(fm.contains("title: \"My Note\""));
        assert!(fm.contains("tags: [tag1, tag2]"));
        assert!(fm.contains("type: note"));
    }

    #[test]
    fn test_extract_wikilinks() {
        let text = "See [[foo]] and [[bar baz]] for details.";
        let links = extract_wikilinks(text);
        assert_eq!(links, vec!["foo", "bar baz"]);
    }

    #[test]
    fn test_extract_inline_tags() {
        let text = "This is #rust and #web3 related.\n#another tag here";
        let tags = extract_inline_tags(text);
        assert_eq!(tags, vec!["rust", "web3", "another"]);
    }

    #[test]
    fn test_touch_updated() {
        let content = "---\ntitle: \"Test\"\nupdated: 2026-01-01T00:00:00\n---\n\nBody";
        let result = touch_updated(content);
        assert!(!result.contains("2026-01-01"));
        assert!(result.contains("updated:"));
        assert!(result.contains("Body"));
    }
}
