use crate::config::journal_dir;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Write file tool - writes contents to files within a sandboxed directory
pub struct WriteFileTool {
    definition: ToolDefinition,
}

impl WriteFileTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Path to the file to write (relative to workspace directory)"
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
                description: "Content to write to the file".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "append".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "If true, append to file instead of overwriting (default: false)"
                    .to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "create_dirs".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "If true, create parent directories if they don't exist (default: true)".to_string(),
                default: Some(json!(true)),
                items: None,
                enum_values: None,
            },
        );

        WriteFileTool {
            definition: ToolDefinition {
                name: "write_file".to_string(),
                description: "Write content to a file. The path must be within the allowed workspace directory. Can create new files or overwrite existing ones.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["path".to_string(), "content".to_string()],
                },
                group: ToolGroup::Development,
                hidden: false,
            },
        }
    }
}

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct WriteFileParams {
    path: String,
    content: String,
    append: Option<bool>,
    create_dirs: Option<bool>,
}

#[async_trait]
impl Tool for WriteFileTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: WriteFileParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let append = params.append.unwrap_or(false);
        let create_dirs = params.create_dirs.unwrap_or(true);

        // Per-write size cap (5MB)
        if params.content.len() > crate::disk_quota::MAX_WRITE_BYTES {
            return ToolResult::error(format!(
                "Write rejected: content size ({} bytes) exceeds the per-write limit of 5MB. \
                 Split into smaller writes or remove unnecessary content.",
                params.content.len()
            ));
        }

        // Check disk quota before writing
        if let Err(e) = context.check_disk_quota(params.content.len()) {
            return ToolResult::error(e);
        }

        // Get workspace directory from context or use current directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Get journal directory
        let journal = PathBuf::from(journal_dir());

        // Resolve the path - check if it starts with "journal/" to use journal dir
        let requested_path = Path::new(&params.path);
        let (full_path, base_dir) = if params.path.starts_with("journal/") || params.path == "journal" {
            // Strip "journal/" prefix and use journal directory
            let relative = params.path.strip_prefix("journal/").unwrap_or(&params.path);
            (journal.join(relative), journal.clone())
        } else if requested_path.is_absolute() {
            (requested_path.to_path_buf(), workspace.clone())
        } else {
            (workspace.join(requested_path), workspace.clone())
        };

        // Canonicalize base directory for comparison
        // For journal, create it if it doesn't exist
        if params.path.starts_with("journal") && !base_dir.exists() {
            if let Err(e) = tokio::fs::create_dir_all(&base_dir).await {
                return ToolResult::error(format!("Cannot create journal directory: {}", e));
            }
        }

        let canonical_base = match base_dir.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(format!("Cannot resolve base directory: {}", e))
            }
        };

        // For new files, we need to check the parent directory
        let parent = match full_path.parent() {
            Some(p) => p.to_path_buf(),
            None => return ToolResult::error("Invalid file path: no parent directory"),
        };

        // Create parent directories if needed and allowed
        if create_dirs && !parent.exists() {
            if let Err(e) = tokio::fs::create_dir_all(&parent).await {
                return ToolResult::error(format!("Failed to create directories: {}", e));
            }
        }

        // Now canonicalize the parent to verify it's within allowed directory
        let canonical_parent = match parent.canonicalize() {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Cannot resolve parent directory: {}", e)),
        };

        // Security check: ensure parent is within allowed directory (workspace or journal)
        if !canonical_parent.starts_with(&canonical_base) {
            return ToolResult::error(format!(
                "Access denied: path '{}' is outside the allowed directory",
                params.path
            ));
        }

        // Construct the final path
        let file_name = match full_path.file_name() {
            Some(n) => n,
            None => return ToolResult::error("Invalid file path: no file name"),
        };
        let final_path = canonical_parent.join(file_name);

        // Additional check if file exists
        if final_path.exists() {
            let canonical_path = match final_path.canonicalize() {
                Ok(p) => p,
                Err(e) => return ToolResult::error(format!("Cannot resolve file path: {}", e)),
            };

            if !canonical_path.starts_with(&canonical_base) {
                return ToolResult::error(format!(
                    "Access denied: path '{}' is outside the allowed directory",
                    params.path
                ));
            }

            if !canonical_path.is_file() {
                return ToolResult::error(format!("Path exists but is not a file: {}", params.path));
            }
        }

        // Write the file
        let result = if append {
            use tokio::io::AsyncWriteExt;
            let mut file = match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&final_path)
                .await
            {
                Ok(f) => f,
                Err(e) => return ToolResult::error(format!("Failed to open file: {}", e)),
            };
            file.write_all(params.content.as_bytes()).await
        } else {
            tokio::fs::write(&final_path, &params.content).await
        };

        match result {
            Ok(_) => {
                let bytes_written = params.content.len();
                context.record_disk_write(bytes_written);
                let lines_written = params.content.lines().count();
                let mode = if append { "appended to" } else { "written to" };

                ToolResult::success(format!(
                    "Successfully {} '{}' ({} bytes, {} lines)",
                    mode, params.path, bytes_written, lines_written
                ))
                .with_metadata(json!({
                    "path": params.path,
                    "bytes_written": bytes_written,
                    "lines_written": lines_written,
                    "append": append
                }))
            }
            Err(e) => ToolResult::error(format!("Failed to write file: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_write_file_basic() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_string_lossy().to_string();

        let tool = WriteFileTool::new();
        let context = ToolContext::new().with_workspace(workspace.clone());

        let result = tool
            .execute(
                json!({
                    "path": "test.txt",
                    "content": "Hello, World!"
                }),
                &context,
            )
            .await;

        assert!(result.success);
        assert!(temp_dir.path().join("test.txt").exists());
    }

    #[tokio::test]
    async fn test_write_file_outside_workspace() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_string_lossy().to_string();

        let tool = WriteFileTool::new();
        let context = ToolContext::new().with_workspace(workspace);

        let result = tool
            .execute(
                json!({
                    "path": "/etc/test.txt",
                    "content": "Should not write"
                }),
                &context,
            )
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("outside the allowed directory"));
    }
}
