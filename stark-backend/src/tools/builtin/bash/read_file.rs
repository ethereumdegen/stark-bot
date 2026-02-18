use crate::config::{notes_dir, soul_dir};
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Intrinsic files mapping: (virtual_name, actual_path_from_repo_root)
const INTRINSIC_FILES: &[(&str, &str)] = &[
    ("SOUL.md", "SOUL.md"),
];

/// Maximum output size in characters to prevent context bloat
const MAX_OUTPUT_SIZE: usize = 12000;

/// Read file tool - reads contents of files within a sandboxed directory
pub struct ReadFileTool {
    definition: ToolDefinition,
}

impl ReadFileTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Path to the file to read (relative to workspace directory)"
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "max_lines".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum number of lines to read (default: 500)".to_string(),
                default: Some(json!(500)),
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "offset".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Line number to start reading from (0-based, default: 0)".to_string(),
                default: Some(json!(0)),
                items: None,
                enum_values: None,
            },
        );

        ReadFileTool {
            definition: ToolDefinition {
                name: "read_file".to_string(),
                description: "Read the contents of a file. The path must be within the allowed workspace directory.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["path".to_string()],
                },
                group: ToolGroup::Filesystem,
                hidden: false,
            },
        }
    }
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ReadFileParams {
    path: String,
    max_lines: Option<usize>,
    offset: Option<usize>,
}

#[async_trait]
impl Tool for ReadFileTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ReadFileParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let max_lines = params.max_lines.unwrap_or(500);
        let offset = params.offset.unwrap_or(0);

        // Check if this is an intrinsic file (e.g., SOUL.md)
        let intrinsic_match = INTRINSIC_FILES.iter().find(|(name, _)| *name == params.path);

        let content = if let Some((name, _)) = intrinsic_match {
            // Read intrinsic files from soul directory (the agent's working copy)
            // SOUL.md is copied to soul directory on startup to protect the original
            let soul = PathBuf::from(soul_dir());

            let full_path = soul.join(name);
            match tokio::fs::read_to_string(&full_path).await {
                Ok(c) => c,
                Err(e) => return ToolResult::error(format!(
                    "Failed to read '{}' from soul directory: {}. Make sure the workspace was initialized properly.",
                    name, e
                )),
            }
        } else {
            // Normal workspace file handling
            let workspace = context
                .workspace_dir
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

            // Get notes directory
            let notes = PathBuf::from(notes_dir());

            // Resolve the path - check if it starts with "notes/" to use notes dir
            let requested_path = Path::new(&params.path);
            let (full_path, base_dir) = if params.path.starts_with("notes/") || params.path == "notes" {
                // Strip "notes/" prefix and use notes directory
                let relative = params.path.strip_prefix("notes/").unwrap_or(&params.path);
                (notes.join(relative), notes.clone())
            } else if requested_path.is_absolute() {
                (requested_path.to_path_buf(), workspace.clone())
            } else {
                (workspace.join(requested_path), workspace.clone())
            };

            // Canonicalize paths for comparison
            let canonical_base = match base_dir.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    return ToolResult::error(format!("Cannot resolve base directory: {}", e))
                }
            };

            let canonical_path = match full_path.canonicalize() {
                Ok(p) => p,
                Err(e) => return ToolResult::error(format!("Cannot resolve file path: {}", e)),
            };

            // Security check: ensure path is within allowed directory (workspace or notes)
            if !canonical_path.starts_with(&canonical_base) {
                return ToolResult::error(format!(
                    "Access denied: path '{}' is outside the allowed directory",
                    params.path
                ));
            }

            // Check if file exists and is a file
            if !canonical_path.exists() {
                return ToolResult::error(format!("File not found: {}", params.path));
            }

            if !canonical_path.is_file() {
                return ToolResult::error(format!("Path is not a file: {}", params.path));
            }

            // Read the file
            match tokio::fs::read_to_string(&canonical_path).await {
                Ok(c) => c,
                Err(e) => return ToolResult::error(format!("Failed to read file: {}", e)),
            }
        };

        // Apply offset and max_lines
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        if offset >= total_lines {
            return ToolResult::success(format!(
                "[Empty: offset {} exceeds total lines {}]",
                offset, total_lines
            ))
            .with_metadata(json!({
                "path": params.path,
                "total_lines": total_lines,
                "offset": offset,
                "lines_returned": 0
            }));
        }

        let end = (offset + max_lines).min(total_lines);
        let selected_lines: Vec<String> = lines[offset..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>5}│ {}", offset + i + 1, line))
            .collect();

        let result = selected_lines.join("\n");
        let truncated = end < total_lines;

        let mut output = if truncated {
            format!(
                "{}\n\n[Showing lines {}-{} of {}. Use offset parameter to read more.]",
                result,
                offset + 1,
                end,
                total_lines
            )
        } else {
            result
        };

        // Truncate if output is too large to prevent context bloat
        let size_truncated = output.len() > MAX_OUTPUT_SIZE;
        if size_truncated {
            output.truncate(MAX_OUTPUT_SIZE);
            output.push_str("\n\n⚠️ [Output truncated due to size - use offset/max_lines for smaller chunks]");
        }

        ToolResult::success(output).with_metadata(json!({
            "path": params.path,
            "total_lines": total_lines,
            "offset": offset,
            "lines_returned": end - offset,
            "truncated": truncated
        }))
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_read_file_outside_workspace() {
        let tool = ReadFileTool::new();
        let temp_dir = TempDir::new().unwrap();
        let context = ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(json!({ "path": "/etc/passwd" }), &context)
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("outside the allowed directory"));
    }
}
