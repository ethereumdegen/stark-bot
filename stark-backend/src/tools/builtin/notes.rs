//! Notes Tool — Obsidian-compatible note management
//!
//! Single tool with `action` parameter: create, edit, read, search, list, tag, link.
//! Auto-generates YAML frontmatter. All note mutations go through this tool.

use crate::notes::frontmatter;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
    ToolSafetyLevel,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct NotesTool {
    definition: ToolDefinition,
}

impl NotesTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action to perform on notes.".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "create".to_string(),
                    "edit".to_string(),
                    "read".to_string(),
                    "search".to_string(),
                    "list".to_string(),
                    "tag".to_string(),
                    "link".to_string(),
                ]),
            },
        );

        properties.insert(
            "title".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Note title (required for create). Used to generate the filename slug."
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "content".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Note body content (for create/edit). Supports [[wikilinks]] and #tags."
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Relative file path within notes/ (for read/edit). E.g. 'ideas/my-idea.md'.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "query".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Search query (for search action). Full-text search across all notes."
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "tags".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Comma-separated tags (for create/tag actions). E.g. 'crypto, payments, protocol'."
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "note_type".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Type of note (for create). Default: 'note'.".to_string(),
                default: Some(json!("note")),
                items: None,
                enum_values: Some(vec![
                    "note".to_string(),
                    "idea".to_string(),
                    "decision".to_string(),
                    "log".to_string(),
                    "reflection".to_string(),
                    "todo".to_string(),
                ]),
            },
        );

        properties.insert(
            "subdir".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Subdirectory to place the note in (for create). E.g. 'ideas', 'decisions', 'daily', 'projects'."
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "aliases".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Comma-separated aliases (for create). Alternative names for wikilink resolution."
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "limit".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Max results for search/list (default: 20, max: 50).".to_string(),
                default: Some(json!(20)),
                items: None,
                enum_values: None,
            },
        );

        Self {
            definition: ToolDefinition {
                name: "notes".to_string(),
                description: "Create, edit, read, and search Obsidian-compatible markdown notes. Notes have YAML frontmatter, support [[wikilinks]] and #tags, and are full-text indexed.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string()],
                },
                group: ToolGroup::Memory,
                hidden: false,
            },
        }
    }
}

impl Default for NotesTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct NotesParams {
    action: String,
    title: Option<String>,
    content: Option<String>,
    path: Option<String>,
    query: Option<String>,
    tags: Option<String>,
    note_type: Option<String>,
    subdir: Option<String>,
    aliases: Option<String>,
    limit: Option<i32>,
}

/// Parse comma-separated list into Vec<String>
fn parse_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

#[async_trait]
impl Tool for NotesTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: NotesParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let notes_store = match &context.notes_store {
            Some(store) => store,
            None => {
                return ToolResult::error(
                    "Notes store not available. The notes system must be initialized.",
                );
            }
        };

        match params.action.as_str() {
            "create" => {
                let title = match &params.title {
                    Some(t) if !t.trim().is_empty() => t.trim(),
                    _ => return ToolResult::error("Title is required for create action."),
                };
                let content = params.content.as_deref().unwrap_or("");
                let tags = params.tags.as_deref().map(parse_csv).unwrap_or_default();
                let aliases = params.aliases.as_deref().map(parse_csv).unwrap_or_default();
                let note_type = params.note_type.as_deref().unwrap_or("note");
                let subdir = params.subdir.as_deref();

                match notes_store.create_note(title, content, &tags, &aliases, note_type, subdir) {
                    Ok(rel_path) => {
                        ToolResult::success(format!(
                            "Note created: `{}`\nTitle: {}\nType: {}\nTags: [{}]",
                            rel_path,
                            title,
                            note_type,
                            tags.join(", ")
                        ))
                        .with_metadata(json!({
                            "action": "create",
                            "path": rel_path,
                            "title": title,
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to create note: {}", e)),
                }
            }

            "edit" => {
                let path = match &params.path {
                    Some(p) if !p.trim().is_empty() => p.trim(),
                    _ => return ToolResult::error("Path is required for edit action."),
                };
                let content = match &params.content {
                    Some(c) => c.as_str(),
                    None => return ToolResult::error("Content is required for edit action."),
                };

                match notes_store.edit_note(path, content) {
                    Ok(()) => {
                        ToolResult::success(format!("Note updated: `{}`", path))
                            .with_metadata(json!({
                                "action": "edit",
                                "path": path,
                            }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to edit note: {}", e)),
                }
            }

            "read" => {
                let path = match &params.path {
                    Some(p) if !p.trim().is_empty() => p.trim(),
                    _ => return ToolResult::error("Path is required for read action."),
                };

                match notes_store.read_note(path) {
                    Ok(content) if content.is_empty() => {
                        ToolResult::error(format!("Note not found: {}", path))
                    }
                    Ok(content) => {
                        ToolResult::success(content).with_metadata(json!({
                            "action": "read",
                            "path": path,
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to read note: {}", e)),
                }
            }

            "search" => {
                let query = match &params.query {
                    Some(q) if !q.trim().is_empty() => q.trim().to_string(),
                    _ => return ToolResult::error("Query is required for search action."),
                };
                let limit = params.limit.unwrap_or(20).min(50).max(1);

                match notes_store.search(&query, limit) {
                    Ok(results) => {
                        if results.is_empty() {
                            return ToolResult::success(format!(
                                "No notes found matching: \"{}\"",
                                query
                            ));
                        }

                        let mut output = format!(
                            "## Notes Search Results\n**Query:** \"{}\"\n**Found:** {} result(s)\n\n",
                            query,
                            results.len()
                        );

                        for (i, r) in results.iter().enumerate() {
                            output.push_str(&format!(
                                "### {}. {}\n**File:** `{}`\n**Tags:** {}\n{}\n\n",
                                i + 1,
                                r.title,
                                r.file_path,
                                if r.tags.is_empty() { "none" } else { &r.tags },
                                r.snippet
                                    .replace(">>>", "**")
                                    .replace("<<<", "**")
                            ));
                        }

                        ToolResult::success(output).with_metadata(json!({
                            "action": "search",
                            "query": query,
                            "result_count": results.len(),
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Search failed: {}", e)),
                }
            }

            "list" => {
                let limit = params.limit.unwrap_or(50).min(100).max(1) as usize;

                match notes_store.list_files() {
                    Ok(files) => {
                        if files.is_empty() {
                            return ToolResult::success(
                                "No notes yet. Use the `create` action to add your first note.",
                            );
                        }

                        let total = files.len();
                        let shown: Vec<&String> = files.iter().take(limit).collect();

                        let mut output = format!("## Notes ({} total)\n\n", total);
                        for f in &shown {
                            output.push_str(&format!("- `{}`\n", f));
                        }
                        if total > limit {
                            output.push_str(&format!(
                                "\n... and {} more. Use search to find specific notes.",
                                total - limit
                            ));
                        }

                        ToolResult::success(output).with_metadata(json!({
                            "action": "list",
                            "total": total,
                            "shown": shown.len(),
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to list notes: {}", e)),
                }
            }

            "tag" => {
                // List all tags, or search by tag
                if let Some(query) = &params.query {
                    // Search by tag
                    let limit = params.limit.unwrap_or(20).min(50).max(1);
                    match notes_store.search_by_tag(query.trim(), limit) {
                        Ok(results) => {
                            if results.is_empty() {
                                return ToolResult::success(format!(
                                    "No notes found with tag: #{}",
                                    query.trim()
                                ));
                            }

                            let mut output = format!(
                                "## Notes tagged #{}\n**Found:** {} note(s)\n\n",
                                query.trim(),
                                results.len()
                            );

                            for (i, r) in results.iter().enumerate() {
                                output.push_str(&format!(
                                    "{}. **{}** — `{}`\n",
                                    i + 1,
                                    r.title,
                                    r.file_path,
                                ));
                            }

                            ToolResult::success(output).with_metadata(json!({
                                "action": "tag",
                                "tag": query.trim(),
                                "result_count": results.len(),
                            }))
                        }
                        Err(e) => ToolResult::error(format!("Tag search failed: {}", e)),
                    }
                } else {
                    // List all tags
                    match notes_store.list_tags() {
                        Ok(tags) => {
                            if tags.is_empty() {
                                return ToolResult::success(
                                    "No tags yet. Tags are extracted from frontmatter and inline #tags.",
                                );
                            }

                            let mut output = format!("## All Tags ({} unique)\n\n", tags.len());
                            for (tag, count) in &tags {
                                output.push_str(&format!("- **#{}** ({} note{})\n", tag, count, if *count == 1 { "" } else { "s" }));
                            }

                            ToolResult::success(output).with_metadata(json!({
                                "action": "tag",
                                "tag_count": tags.len(),
                            }))
                        }
                        Err(e) => ToolResult::error(format!("Failed to list tags: {}", e)),
                    }
                }
            }

            "link" => {
                // Resolve a wikilink to a file path, or list all wikilinks in a note
                let path = match &params.path {
                    Some(p) if !p.trim().is_empty() => p.trim(),
                    _ => {
                        // If query provided, resolve it as a wikilink
                        if let Some(query) = &params.query {
                            let resolved = crate::notes::file_ops::resolve_wikilink(
                                notes_store.notes_dir(),
                                query.trim(),
                            );
                            return match resolved {
                                Some(p) => {
                                    let rel = crate::notes::file_ops::relative_path(
                                        notes_store.notes_dir(),
                                        &p,
                                    )
                                    .unwrap_or_else(|| p.to_string_lossy().to_string());
                                    ToolResult::success(format!(
                                        "[[{}]] resolves to: `{}`",
                                        query.trim(),
                                        rel
                                    ))
                                    .with_metadata(json!({
                                        "action": "link",
                                        "link": query.trim(),
                                        "resolved_path": rel,
                                    }))
                                }
                                None => ToolResult::success(format!(
                                    "[[{}]] — no matching note found. Create one with the `create` action.",
                                    query.trim()
                                )),
                            };
                        }
                        return ToolResult::error(
                            "Path or query is required for link action.",
                        );
                    }
                };

                // List wikilinks in a note
                match notes_store.read_note(path) {
                    Ok(content) if content.is_empty() => {
                        ToolResult::error(format!("Note not found: {}", path))
                    }
                    Ok(content) => {
                        let parsed = frontmatter::parse_note(&content);
                        if parsed.wikilinks.is_empty() {
                            return ToolResult::success(format!(
                                "Note `{}` has no [[wikilinks]].",
                                path
                            ));
                        }

                        let mut output = format!(
                            "## Wikilinks in `{}`\n\n",
                            path
                        );
                        for link in &parsed.wikilinks {
                            let resolved = crate::notes::file_ops::resolve_wikilink(
                                notes_store.notes_dir(),
                                link,
                            );
                            let status = match resolved {
                                Some(p) => {
                                    let rel = crate::notes::file_ops::relative_path(
                                        notes_store.notes_dir(),
                                        &p,
                                    )
                                    .unwrap_or_default();
                                    format!("`{}`", rel)
                                }
                                None => "*(unresolved)*".to_string(),
                            };
                            output.push_str(&format!("- [[{}]] → {}\n", link, status));
                        }

                        ToolResult::success(output).with_metadata(json!({
                            "action": "link",
                            "path": path,
                            "link_count": parsed.wikilinks.len(),
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to read note: {}", e)),
                }
            }

            other => ToolResult::error(format!(
                "Unknown action: '{}'. Valid actions: create, edit, read, search, list, tag, link",
                other
            )),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Standard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notes_tool_definition() {
        let tool = NotesTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "notes");
        assert_eq!(def.group, ToolGroup::Memory);
        assert!(def.input_schema.required.contains(&"action".to_string()));
        assert!(def.input_schema.properties.contains_key("title"));
        assert!(def.input_schema.properties.contains_key("content"));
        assert!(def.input_schema.properties.contains_key("path"));
        assert!(def.input_schema.properties.contains_key("query"));
        assert!(def.input_schema.properties.contains_key("tags"));
    }

    #[test]
    fn test_parse_csv() {
        assert_eq!(parse_csv("a, b, c"), vec!["a", "b", "c"]);
        assert_eq!(parse_csv("single"), vec!["single"]);
        assert_eq!(parse_csv("  "), Vec::<String>::new());
    }
}
