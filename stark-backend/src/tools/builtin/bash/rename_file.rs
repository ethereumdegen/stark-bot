use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Rename/move file tool - renames or moves files within a sandboxed directory
pub struct RenameFileTool {
    definition: ToolDefinition,
}

impl RenameFileTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "source".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Source path (relative to workspace)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "destination".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Destination path (relative to workspace)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "create_dirs".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description:
                    "If true, create parent directories for destination if needed (default: true)"
                        .to_string(),
                default: Some(json!(true)),
                items: None,
                enum_values: None,
            },
        );

        RenameFileTool {
            definition: ToolDefinition {
                name: "rename_file".to_string(),
                description: "Rename or move a file or directory. Both source and destination must be within the workspace.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["source".to_string(), "destination".to_string()],
                },
                group: ToolGroup::Development,
                hidden: false,
            },
        }
    }
}

impl Default for RenameFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct RenameFileParams {
    source: String,
    destination: String,
    create_dirs: Option<bool>,
}

#[async_trait]
impl Tool for RenameFileTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: RenameFileParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let create_dirs = params.create_dirs.unwrap_or(true);

        // Get workspace directory from context or use current directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Resolve source path
        let source_path = Path::new(&params.source);
        let full_source = if source_path.is_absolute() {
            source_path.to_path_buf()
        } else {
            workspace.join(source_path)
        };

        // Resolve destination path
        let dest_path = Path::new(&params.destination);
        let full_dest = if dest_path.is_absolute() {
            dest_path.to_path_buf()
        } else {
            workspace.join(dest_path)
        };

        // Canonicalize workspace for comparison
        let canonical_workspace = match workspace.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(format!("Cannot resolve workspace directory: {}", e))
            }
        };

        // Canonicalize source (must exist)
        let canonical_source = match full_source.canonicalize() {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Cannot resolve source path: {}", e)),
        };

        // Security check: ensure source is within workspace
        if !canonical_source.starts_with(&canonical_workspace) {
            return ToolResult::error(format!(
                "Access denied: source '{}' is outside the workspace directory",
                params.source
            ));
        }

        // Create parent directories for destination if needed
        if create_dirs {
            if let Some(parent) = full_dest.parent() {
                if !parent.exists() {
                    if let Err(e) = tokio::fs::create_dir_all(parent).await {
                        return ToolResult::error(format!(
                            "Failed to create destination directory: {}",
                            e
                        ));
                    }
                }
            }
        }

        // For the destination, we check the parent directory is within workspace
        // (the destination file itself doesn't exist yet)
        let dest_parent = full_dest.parent().unwrap_or(&full_dest);
        let canonical_dest_parent = match dest_parent.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(format!("Cannot resolve destination path: {}", e))
            }
        };

        if !canonical_dest_parent.starts_with(&canonical_workspace) {
            return ToolResult::error(format!(
                "Access denied: destination '{}' is outside the workspace directory",
                params.destination
            ));
        }

        // Check if destination already exists
        if full_dest.exists() {
            return ToolResult::error(format!(
                "Destination already exists: {}. Delete it first if you want to replace.",
                params.destination
            ));
        }

        // Perform the rename/move
        match tokio::fs::rename(&canonical_source, &full_dest).await {
            Ok(_) => {
                let action = if canonical_source.parent() == full_dest.parent() {
                    "Renamed"
                } else {
                    "Moved"
                };
                ToolResult::success(format!(
                    "{} '{}' to '{}'",
                    action, params.source, params.destination
                ))
                .with_metadata(json!({
                    "source": params.source,
                    "destination": params.destination,
                    "action": action.to_lowercase()
                }))
            }
            Err(e) => ToolResult::error(format!("Failed to rename/move: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_rename_file() {
        let tool = RenameFileTool::new();
        let temp_dir = TempDir::new().unwrap();

        let source = temp_dir.path().join("old.txt");
        std::fs::write(&source, "test content").unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(
                json!({ "source": "old.txt", "destination": "new.txt" }),
                &context,
            )
            .await;

        assert!(result.success);
        assert!(!source.exists());
        assert!(temp_dir.path().join("new.txt").exists());
    }

    #[tokio::test]
    async fn test_move_to_new_dir() {
        let tool = RenameFileTool::new();
        let temp_dir = TempDir::new().unwrap();

        let source = temp_dir.path().join("file.txt");
        std::fs::write(&source, "test content").unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(
                json!({ "source": "file.txt", "destination": "newdir/file.txt" }),
                &context,
            )
            .await;

        assert!(result.success);
        assert!(!source.exists());
        assert!(temp_dir.path().join("newdir/file.txt").exists());
    }

    #[tokio::test]
    async fn test_rename_outside_workspace() {
        let tool = RenameFileTool::new();
        let temp_dir = TempDir::new().unwrap();

        let source = temp_dir.path().join("file.txt");
        std::fs::write(&source, "test").unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(
                json!({ "source": "file.txt", "destination": "/tmp/outside.txt" }),
                &context,
            )
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("outside the workspace"));
    }
}
