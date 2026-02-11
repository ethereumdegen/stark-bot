use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Delete file tool - removes files or directories within a sandboxed directory
pub struct DeleteFileTool {
    definition: ToolDefinition,
}

impl DeleteFileTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Path to the file or directory to delete (relative to workspace)"
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "recursive".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description:
                    "If true, delete directory and all contents recursively (default: false)"
                        .to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        DeleteFileTool {
            definition: ToolDefinition {
                name: "delete_file".to_string(),
                description: "Delete a file or directory. For directories, set recursive=true. The path must be within the workspace.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["path".to_string()],
                },
                group: ToolGroup::Development,
                hidden: false,
            },
        }
    }
}

impl Default for DeleteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct DeleteFileParams {
    path: String,
    recursive: Option<bool>,
}

#[async_trait]
impl Tool for DeleteFileTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: DeleteFileParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let recursive = params.recursive.unwrap_or(false);

        // Get workspace directory from context or use current directory
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
            Err(e) => return ToolResult::error(format!("Cannot resolve path: {}", e)),
        };

        // Security check: ensure path is within workspace
        if !canonical_path.starts_with(&canonical_workspace) {
            return ToolResult::error(format!(
                "Access denied: path '{}' is outside the workspace directory",
                params.path
            ));
        }

        // Don't allow deleting the workspace itself
        if canonical_path == canonical_workspace {
            return ToolResult::error("Cannot delete the workspace root directory");
        }

        // Check if path exists
        if !canonical_path.exists() {
            return ToolResult::error(format!("Path not found: {}", params.path));
        }

        // Handle file vs directory
        if canonical_path.is_file() {
            match tokio::fs::remove_file(&canonical_path).await {
                Ok(_) => ToolResult::success(format!("Deleted file: {}", params.path)),
                Err(e) => ToolResult::error(format!("Failed to delete file: {}", e)),
            }
        } else if canonical_path.is_dir() {
            if !recursive {
                // Check if directory is empty
                let is_empty = match std::fs::read_dir(&canonical_path) {
                    Ok(mut entries) => entries.next().is_none(),
                    Err(e) => return ToolResult::error(format!("Failed to read directory: {}", e)),
                };

                if !is_empty {
                    return ToolResult::error(
                        "Directory is not empty. Set recursive=true to delete directory and all contents.",
                    );
                }

                match tokio::fs::remove_dir(&canonical_path).await {
                    Ok(_) => ToolResult::success(format!("Deleted empty directory: {}", params.path)),
                    Err(e) => ToolResult::error(format!("Failed to delete directory: {}", e)),
                }
            } else {
                // Count items before deletion for reporting
                let count = walkdir::WalkDir::new(&canonical_path)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .count();

                match tokio::fs::remove_dir_all(&canonical_path).await {
                    Ok(_) => ToolResult::success(format!(
                        "Deleted directory and {} items: {}",
                        count, params.path
                    )),
                    Err(e) => ToolResult::error(format!("Failed to delete directory: {}", e)),
                }
            }
        } else {
            ToolResult::error(format!("Unknown path type: {}", params.path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_delete_file() {
        let tool = DeleteFileTool::new();
        let temp_dir = TempDir::new().unwrap();

        let test_file = temp_dir.path().join("test.txt");
        std::fs::write(&test_file, "test content").unwrap();
        assert!(test_file.exists());

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(json!({ "path": "test.txt" }), &context)
            .await;

        assert!(result.success);
        assert!(!test_file.exists());
    }

    #[tokio::test]
    async fn test_delete_dir_recursive() {
        let tool = DeleteFileTool::new();
        let temp_dir = TempDir::new().unwrap();

        let sub_dir = temp_dir.path().join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("file.txt"), "test").unwrap();
        assert!(sub_dir.exists());

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(json!({ "path": "subdir", "recursive": true }), &context)
            .await;

        assert!(result.success);
        assert!(!sub_dir.exists());
    }

    #[tokio::test]
    async fn test_delete_outside_workspace() {
        let tool = DeleteFileTool::new();
        let temp_dir = TempDir::new().unwrap();
        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(json!({ "path": "/etc/passwd" }), &context)
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("outside the workspace"));
    }
}
