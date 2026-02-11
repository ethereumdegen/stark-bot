use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use glob::glob as glob_match;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Glob tool for file pattern matching
/// Returns files matching a glob pattern, sorted by modification time
pub struct GlobTool {
    definition: ToolDefinition,
}

impl GlobTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "pattern".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Glob pattern to match files (e.g., '**/*.rs', 'src/**/*.ts', '*.md')"
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
                description:
                    "Base directory to search from (relative to workspace, default: current dir)"
                        .to_string(),
                default: Some(json!(".")),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "sort_by".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Sort results by: 'modified' (newest first), 'name', 'size' (default: modified)"
                        .to_string(),
                default: Some(json!("modified")),
                items: None,
                enum_values: Some(vec![
                    "modified".to_string(),
                    "name".to_string(),
                    "size".to_string(),
                ]),
            },
        );

        properties.insert(
            "limit".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum number of files to return (default: 100)".to_string(),
                default: Some(json!(100)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "include_hidden".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Include hidden files (starting with '.', default: false)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        GlobTool {
            definition: ToolDefinition {
                name: "glob".to_string(),
                description: "Find files matching a glob pattern. Supports recursive patterns like '**/*.rs'. Returns files sorted by modification time (newest first by default).".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["pattern".to_string()],
                },
                group: ToolGroup::Development,
            },
        }
    }
}

impl Default for GlobTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct GlobParams {
    pattern: String,
    path: Option<String>,
    sort_by: Option<String>,
    limit: Option<usize>,
    include_hidden: Option<bool>,
}

struct FileEntry {
    path: PathBuf,
    modified: SystemTime,
    size: u64,
}

#[async_trait]
impl Tool for GlobTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: GlobParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get workspace directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Resolve base path
        let base_path = if let Some(ref path) = params.path {
            let p = Path::new(path);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                workspace.join(p)
            }
        } else {
            workspace.clone()
        };

        // Security check: ensure path is within workspace
        let canonical_workspace = match workspace.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(format!("Cannot resolve workspace directory: {}", e))
            }
        };

        let canonical_base = match base_path.canonicalize() {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Cannot resolve base path: {}", e)),
        };

        if !canonical_base.starts_with(&canonical_workspace) {
            return ToolResult::error(format!(
                "Access denied: path '{}' is outside the workspace directory",
                params.path.as_deref().unwrap_or(".")
            ));
        }

        // Build full glob pattern
        let full_pattern = canonical_base.join(&params.pattern);
        let pattern_str = full_pattern.to_string_lossy();

        // Collect matching files
        let include_hidden = params.include_hidden.unwrap_or(false);
        let mut files: Vec<FileEntry> = Vec::new();

        match glob_match(&pattern_str) {
            Ok(paths) => {
                for entry in paths.filter_map(Result::ok) {
                    // Security: verify each path is within workspace
                    if let Ok(canonical) = entry.canonicalize() {
                        if !canonical.starts_with(&canonical_workspace) {
                            continue;
                        }
                    }

                    // Skip hidden files unless requested
                    if !include_hidden {
                        if let Some(name) = entry.file_name() {
                            if name.to_string_lossy().starts_with('.') {
                                continue;
                            }
                        }
                    }

                    // Get file metadata
                    if let Ok(metadata) = fs::metadata(&entry) {
                        if metadata.is_file() {
                            files.push(FileEntry {
                                path: entry,
                                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                                size: metadata.len(),
                            });
                        }
                    }
                }
            }
            Err(e) => return ToolResult::error(format!("Invalid glob pattern: {}", e)),
        }

        // Sort files
        let sort_by = params.sort_by.as_deref().unwrap_or("modified");
        match sort_by {
            "name" => files.sort_by(|a, b| a.path.cmp(&b.path)),
            "size" => files.sort_by(|a, b| b.size.cmp(&a.size)), // Largest first
            _ => files.sort_by(|a, b| b.modified.cmp(&a.modified)), // Newest first
        }

        // Limit results
        let limit = params.limit.unwrap_or(100);
        if files.len() > limit {
            files.truncate(limit);
        }

        // Format output
        if files.is_empty() {
            return ToolResult::success("No files found matching pattern.").with_metadata(json!({
                "pattern": params.pattern,
                "base_path": canonical_base.display().to_string(),
                "count": 0
            }));
        }

        let total_found = files.len();
        let output: Vec<String> = files
            .iter()
            .map(|f| {
                let relative = f
                    .path
                    .strip_prefix(&canonical_workspace)
                    .unwrap_or(&f.path);
                let size_str = if f.size < 1024 {
                    format!("{}B", f.size)
                } else if f.size < 1024 * 1024 {
                    format!("{:.1}KB", f.size as f64 / 1024.0)
                } else {
                    format!("{:.1}MB", f.size as f64 / (1024.0 * 1024.0))
                };
                format!("{} ({})", relative.display(), size_str)
            })
            .collect();

        ToolResult::success(output.join("\n")).with_metadata(json!({
            "pattern": params.pattern,
            "base_path": canonical_base.display().to_string(),
            "count": total_found,
            "sort_by": sort_by,
            "limited": total_found == limit
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
    async fn test_glob_basic() {
        let tool = GlobTool::new();
        let temp_dir = TempDir::new().unwrap();

        // Create test files
        std::fs::write(temp_dir.path().join("test1.rs"), "fn main() {}").unwrap();
        std::fs::write(temp_dir.path().join("test2.rs"), "fn test() {}").unwrap();
        std::fs::write(temp_dir.path().join("readme.md"), "# Test").unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool.execute(json!({ "pattern": "*.rs" }), &context).await;

        assert!(result.success);
        assert!(result.content.contains("test1.rs"));
        assert!(result.content.contains("test2.rs"));
        assert!(!result.content.contains("readme.md"));
    }

    #[tokio::test]
    async fn test_glob_recursive() {
        let tool = GlobTool::new();
        let temp_dir = TempDir::new().unwrap();

        // Create nested structure
        let src = temp_dir.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "pub mod test;").unwrap();
        std::fs::write(temp_dir.path().join("main.rs"), "fn main() {}").unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(json!({ "pattern": "**/*.rs" }), &context)
            .await;

        assert!(result.success);
        assert!(result.content.contains("lib.rs"));
        assert!(result.content.contains("main.rs"));
    }

    #[tokio::test]
    async fn test_glob_outside_workspace() {
        let tool = GlobTool::new();
        let temp_dir = TempDir::new().unwrap();
        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(json!({ "pattern": "*.txt", "path": "/etc" }), &context)
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("outside the workspace"));
    }
}
