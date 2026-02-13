use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Edit file tool for precise string replacement
/// Similar to Claude Code's edit tool - requires exact match of old_text
pub struct EditFileTool {
    definition: ToolDefinition,
}

impl EditFileTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Path to the file to edit (relative to workspace directory)"
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "old_text".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Exact text to find and replace. Must match exactly including whitespace and indentation.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "new_text".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Text to replace old_text with".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "occurrence".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Which occurrence to replace: 'first', 'last', or 'all' (default: first)".to_string(),
                default: Some(json!("first")),
                items: None,
                enum_values: Some(vec![
                    "first".to_string(),
                    "last".to_string(),
                    "all".to_string(),
                ]),
            },
        );

        EditFileTool {
            definition: ToolDefinition {
                name: "edit_file".to_string(),
                description: "Edit a file by replacing exact text. old_text must match exactly (including whitespace). Returns the edited section with context. For large changes, prefer write_file or apply_patch.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![
                        "path".to_string(),
                        "old_text".to_string(),
                        "new_text".to_string(),
                    ],
                },
                group: ToolGroup::Development,
                hidden: false,
            },
        }
    }

    /// Generate a simple diff-like preview of the change
    fn generate_diff(old_text: &str, new_text: &str, context_lines: usize) -> String {
        let old_lines: Vec<&str> = old_text.lines().collect();
        let new_lines: Vec<&str> = new_text.lines().collect();

        let mut diff = String::new();
        diff.push_str("--- before\n");
        diff.push_str("+++ after\n");
        diff.push_str("@@\n");

        // Show removed lines
        for line in &old_lines {
            diff.push_str(&format!("-{}\n", line));
        }

        // Show added lines
        for line in &new_lines {
            diff.push_str(&format!("+{}\n", line));
        }

        diff
    }

    /// Find the closest matching substring in the file content
    /// Returns (matched_text, line_number, similarity_percentage)
    fn find_closest_match(content: &str, needle: &str) -> Option<(String, usize, usize)> {
        let needle_lines: Vec<&str> = needle.lines().collect();
        if needle_lines.is_empty() {
            return None;
        }

        let content_lines: Vec<&str> = content.lines().collect();
        let needle_len = needle_lines.len();

        if content_lines.is_empty() || needle_len == 0 {
            return None;
        }

        let mut best_score = 0usize;
        let mut best_start = 0usize;
        let mut best_len = needle_len;

        // Slide a window of needle_len lines over the content
        let search_len = needle_len.min(content_lines.len());
        for window_size in [search_len, search_len + 1, search_len.saturating_sub(1)] {
            if window_size == 0 || window_size > content_lines.len() {
                continue;
            }
            for start in 0..=(content_lines.len() - window_size) {
                let window = &content_lines[start..start + window_size];
                let score = Self::line_similarity(&needle_lines, window);
                if score > best_score {
                    best_score = score;
                    best_start = start;
                    best_len = window_size;
                }
            }
        }

        // Only return if similarity is above 40%
        let max_possible = needle_lines.iter().map(|l| l.len().max(1)).sum::<usize>();
        let percentage = if max_possible > 0 {
            (best_score * 100) / max_possible
        } else {
            0
        };

        if percentage >= 40 {
            let matched = content_lines[best_start..best_start + best_len].join("\n");
            // Truncate if too long
            let display = if matched.len() > 500 {
                format!("{}...", &matched[..500])
            } else {
                matched
            };
            Some((display, best_start + 1, percentage))
        } else {
            None
        }
    }

    /// Compute similarity score between two sets of lines
    fn line_similarity(a: &[&str], b: &[&str]) -> usize {
        let mut score = 0usize;
        let pairs = a.len().min(b.len());
        for i in 0..pairs {
            let al = a[i].trim();
            let bl = b[i].trim();
            if al == bl {
                score += al.len().max(1);
            } else {
                // Partial character match
                let common = al.chars().zip(bl.chars()).take_while(|(a, b)| a == b).count();
                score += common;
            }
        }
        score
    }

    /// Show context around the edit location
    fn show_context(content: &str, edit_start: usize, new_text: &str, context_lines: usize) -> String {
        let lines: Vec<&str> = content.lines().collect();

        // Find the line number where the edit starts
        let edit_line = content[..edit_start].matches('\n').count();

        let start = edit_line.saturating_sub(context_lines);
        let new_text_lines = new_text.matches('\n').count() + 1;
        let end = (edit_line + new_text_lines + context_lines).min(lines.len());

        let mut output = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_num = start + i + 1;
            let marker = if i >= context_lines && i < context_lines + new_text_lines {
                ">"
            } else {
                " "
            };
            output.push_str(&format!("{}{:>5}│ {}\n", marker, line_num, line));
        }
        output
    }
}

impl Default for EditFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct EditFileParams {
    path: String,
    old_text: String,
    new_text: String,
    occurrence: Option<String>,
}

#[async_trait]
impl Tool for EditFileTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: EditFileParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Validate inputs
        if params.old_text.is_empty() {
            return ToolResult::error("old_text cannot be empty");
        }

        if params.old_text == params.new_text {
            return ToolResult::error("old_text and new_text are identical - no change needed");
        }

        // Get workspace directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Resolve the path
        let requested_path = Path::new(&params.path);
        let full_path = if requested_path.is_absolute() {
            requested_path.to_path_buf()
        } else {
            workspace.join(requested_path)
        };

        // Canonicalize paths for comparison
        let canonical_workspace = match workspace.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(format!("Cannot resolve workspace directory: {}", e))
            }
        };

        let canonical_path = match full_path.canonicalize() {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Cannot resolve file path: {}", e)),
        };

        // Security check: ensure path is within workspace
        if !canonical_path.starts_with(&canonical_workspace) {
            return ToolResult::error(format!(
                "Access denied: path '{}' is outside the workspace directory",
                params.path
            ));
        }

        // Check if file exists
        if !canonical_path.exists() {
            return ToolResult::error(format!("File not found: {}", params.path));
        }

        if !canonical_path.is_file() {
            return ToolResult::error(format!("Path is not a file: {}", params.path));
        }

        // Read the file
        let content = match tokio::fs::read_to_string(&canonical_path).await {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read file: {}", e)),
        };

        // Check for exact match
        if !content.contains(&params.old_text) {
            let mut msg = String::from("old_text not found in file. The text must match exactly (including whitespace and indentation).\n");

            // Fuzzy matching: find the closest match in the file
            let best_match = Self::find_closest_match(&content, &params.old_text);
            if let Some((match_text, line_num, similarity)) = best_match {
                msg.push_str(&format!(
                    "\n**Closest match** ({}% similar) at line {}:\n```\n{}\n```\n",
                    similarity, line_num, match_text
                ));
                msg.push_str("\nCompare carefully with your old_text for whitespace/indentation differences.");
            } else {
                // No fuzzy match — show the first lines of the file
                let lines: Vec<&str> = content.lines().take(20).collect();
                msg.push_str(&format!("\nFirst 20 lines of file:\n{}", lines.join("\n")));
            }

            // Check common mistakes
            let old_trimmed = params.old_text.trim();
            let content_trimmed: String = content.lines().map(|l| l.trim()).collect::<Vec<_>>().join("\n");
            if content_trimmed.contains(old_trimmed) {
                msg.push_str("\n\n**Hint**: The text exists but with different indentation. Check leading spaces/tabs.");
            }

            // Check for tab vs space mismatch
            if params.old_text.contains('\t') && !content.contains('\t') {
                msg.push_str("\n**Hint**: Your old_text uses tabs but the file uses spaces.");
            } else if !params.old_text.contains('\t') && content.contains('\t') && params.old_text.contains("    ") {
                msg.push_str("\n**Hint**: Your old_text uses spaces but the file uses tabs.");
            }

            return ToolResult::error(msg);
        }

        // Count occurrences
        let count = content.matches(&params.old_text).count();
        let occurrence = params.occurrence.as_deref().unwrap_or("first");

        // Perform the replacement
        let (new_content, replaced_count, edit_position) = match occurrence {
            "all" => {
                let new_content = content.replace(&params.old_text, &params.new_text);
                let pos = content.find(&params.old_text).unwrap_or(0);
                (new_content, count, pos)
            }
            "last" => {
                if let Some(pos) = content.rfind(&params.old_text) {
                    let mut new_content = content.clone();
                    new_content.replace_range(pos..pos + params.old_text.len(), &params.new_text);
                    (new_content, 1, pos)
                } else {
                    return ToolResult::error("old_text not found");
                }
            }
            _ => {
                // "first" (default)
                if let Some(pos) = content.find(&params.old_text) {
                    let mut new_content = content.clone();
                    new_content.replace_range(pos..pos + params.old_text.len(), &params.new_text);
                    (new_content, 1, pos)
                } else {
                    return ToolResult::error("old_text not found");
                }
            }
        };

        // Check disk quota for net size increase
        let size_increase = new_content.len().saturating_sub(content.len());
        if size_increase > 0 {
            if let Err(e) = context.check_disk_quota(size_increase) {
                return ToolResult::error(e);
            }
        }

        // Write the file
        if let Err(e) = tokio::fs::write(&canonical_path, &new_content).await {
            return ToolResult::error(format!("Failed to write file: {}", e));
        }

        // Record the net size increase with disk quota manager
        if size_increase > 0 {
            context.record_disk_write(size_increase);
        }

        // Generate output
        let diff = Self::generate_diff(&params.old_text, &params.new_text, 3);
        let context_view = Self::show_context(&new_content, edit_position, &params.new_text, 3);

        let message = if count > 1 && occurrence != "all" {
            format!(
                "Replaced {} of {} occurrences ({} mode).\n\n{}\n\nContext after edit:\n{}",
                replaced_count, count, occurrence, diff, context_view
            )
        } else {
            format!(
                "Replaced {} occurrence(s).\n\n{}\n\nContext after edit:\n{}",
                replaced_count, diff, context_view
            )
        };

        ToolResult::success(message).with_metadata(json!({
            "path": params.path,
            "occurrences_found": count,
            "occurrences_replaced": replaced_count,
            "mode": occurrence
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_edit_file_basic() {
        let tool = EditFileTool::new();
        let temp_dir = TempDir::new().unwrap();

        let test_file = temp_dir.path().join("test.txt");
        std::fs::write(&test_file, "Hello World").unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(
                json!({
                    "path": "test.txt",
                    "old_text": "World",
                    "new_text": "Rust"
                }),
                &context,
            )
            .await;

        assert!(result.success);
        let content = std::fs::read_to_string(&test_file).unwrap();
        assert_eq!(content, "Hello Rust");
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let tool = EditFileTool::new();
        let temp_dir = TempDir::new().unwrap();

        let test_file = temp_dir.path().join("test.txt");
        std::fs::write(&test_file, "Hello World").unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(
                json!({
                    "path": "test.txt",
                    "old_text": "Foo",
                    "new_text": "Bar"
                }),
                &context,
            )
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("not found in file"));
    }

    #[tokio::test]
    async fn test_edit_file_all_occurrences() {
        let tool = EditFileTool::new();
        let temp_dir = TempDir::new().unwrap();

        let test_file = temp_dir.path().join("test.txt");
        std::fs::write(&test_file, "foo bar foo baz foo").unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(
                json!({
                    "path": "test.txt",
                    "old_text": "foo",
                    "new_text": "qux",
                    "occurrence": "all"
                }),
                &context,
            )
            .await;

        assert!(result.success);
        let content = std::fs::read_to_string(&test_file).unwrap();
        assert_eq!(content, "qux bar qux baz qux");
    }

    #[tokio::test]
    async fn test_edit_file_outside_workspace() {
        let tool = EditFileTool::new();
        let temp_dir = TempDir::new().unwrap();
        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(
                json!({
                    "path": "/etc/passwd",
                    "old_text": "root",
                    "new_text": "admin"
                }),
                &context,
            )
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("outside the workspace"));
    }
}
