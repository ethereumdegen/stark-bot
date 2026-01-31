use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Intrinsic files that appear in all workspaces
const INTRINSIC_FILES: &[(&str, &str)] = &[
    ("SOUL.md", "Agent personality and behavior configuration"),
];

/// List files tool - lists directory contents within a sandboxed directory
pub struct ListFilesTool {
    definition: ToolDefinition,
}

impl ListFilesTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Path to the directory to list (relative to workspace, default: '.')"
                    .to_string(),
                default: Some(json!(".")),
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "recursive".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "If true, list files recursively (default: false)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "max_depth".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum depth for recursive listing (default: 3)".to_string(),
                default: Some(json!(3)),
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "include_hidden".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "If true, include hidden files (starting with '.') (default: false)"
                    .to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "pattern".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Optional glob pattern to filter files (e.g., '*.rs', '*.txt')".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ListFilesTool {
            definition: ToolDefinition {
                name: "list_files".to_string(),
                description: "List files and directories. Can list recursively and filter by pattern. The path must be within the allowed workspace directory.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::Filesystem,
            },
        }
    }
}

impl Default for ListFilesTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ListFilesParams {
    path: Option<String>,
    recursive: Option<bool>,
    max_depth: Option<usize>,
    include_hidden: Option<bool>,
    pattern: Option<String>,
}

#[derive(Debug, Clone)]
struct FileEntry {
    path: String,
    is_dir: bool,
    size: u64,
    depth: usize,
}

/// Work item for iterative directory traversal
struct DirWorkItem {
    path: PathBuf,
    depth: usize,
}

#[async_trait]
impl Tool for ListFilesTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ListFilesParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let path = params.path.unwrap_or_else(|| ".".to_string());
        let recursive = params.recursive.unwrap_or(false);
        let max_depth = params.max_depth.unwrap_or(3);
        let include_hidden = params.include_hidden.unwrap_or(false);
        let pattern = params.pattern;

        // Get workspace directory from context or use current directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Resolve the path
        let requested_path = Path::new(&path);
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
                path
            ));
        }

        // Check if path exists and is a directory
        if !canonical_path.exists() {
            return ToolResult::error(format!("Path not found: {}", path));
        }

        if !canonical_path.is_dir() {
            return ToolResult::error(format!("Path is not a directory: {}", path));
        }

        // Collect files using iterative approach
        let mut entries = Vec::new();
        let mut work_stack = vec![DirWorkItem {
            path: canonical_path.clone(),
            depth: 0,
        }];

        while let Some(work_item) = work_stack.pop() {
            let mut read_dir = match tokio::fs::read_dir(&work_item.path).await {
                Ok(rd) => rd,
                Err(e) => {
                    log::warn!("Failed to read directory {:?}: {}", work_item.path, e);
                    continue;
                }
            };

            while let Ok(Some(entry)) = read_dir.next_entry().await {
                let entry_path = entry.path();
                let file_name = match entry.file_name().to_str() {
                    Some(n) => n.to_string(),
                    None => continue,
                };

                // Skip hidden files unless requested
                if !include_hidden && file_name.starts_with('.') {
                    continue;
                }

                // Get metadata
                let metadata = match entry.metadata().await {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let is_dir = metadata.is_dir();

                // Apply pattern filter for files
                if let Some(ref pat) = pattern {
                    if !is_dir && !matches_glob(&file_name, pat) {
                        continue;
                    }
                }

                // Get relative path from workspace
                let relative_path = entry_path
                    .strip_prefix(&canonical_workspace)
                    .unwrap_or(&entry_path)
                    .to_string_lossy()
                    .to_string();

                entries.push(FileEntry {
                    path: if work_item.depth == 0 {
                        file_name.clone()
                    } else {
                        relative_path.clone()
                    },
                    is_dir,
                    size: if is_dir { 0 } else { metadata.len() },
                    depth: work_item.depth,
                });

                // Add directories to work stack for recursive processing
                if is_dir && recursive && work_item.depth < max_depth {
                    work_stack.push(DirWorkItem {
                        path: entry_path,
                        depth: work_item.depth + 1,
                    });
                }
            }
        }

        // Format output
        if entries.is_empty() {
            return ToolResult::success("Directory is empty or no files match the pattern.")
                .with_metadata(json!({
                    "path": path,
                    "total_entries": 0
                }));
        }

        // Add intrinsic files when listing root directory
        let is_root = path == "." || path == "/" || path.is_empty();
        if is_root {
            for (name, _desc) in INTRINSIC_FILES {
                // Check if file already exists (don't duplicate)
                if !entries.iter().any(|e| e.path == *name) {
                    entries.push(FileEntry {
                        path: name.to_string(),
                        is_dir: false,
                        size: 0, // Virtual file, size unknown
                        depth: 0,
                    });
                }
            }
        }

        // Sort entries: directories first, then by name
        entries.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                b.is_dir.cmp(&a.is_dir)
            } else {
                a.path.cmp(&b.path)
            }
        });

        let mut dirs_count = 0;
        let mut files_count = 0;
        let mut total_size = 0u64;

        let formatted: Vec<String> = entries
            .iter()
            .map(|e| {
                let indent = "  ".repeat(e.depth);
                let type_indicator = if e.is_dir {
                    dirs_count += 1;
                    "📁"
                } else {
                    files_count += 1;
                    total_size += e.size;
                    "📄"
                };
                let size_str = if e.is_dir {
                    String::new()
                } else {
                    format!(" ({})", format_size(e.size))
                };
                format!("{}{} {}{}", indent, type_indicator, e.path, size_str)
            })
            .collect();

        let output = format!(
            "{}\n\n📊 {} directories, {} files ({})",
            formatted.join("\n"),
            dirs_count,
            files_count,
            format_size(total_size)
        );

        ToolResult::success(output).with_metadata(json!({
            "path": path,
            "total_entries": entries.len(),
            "directories": dirs_count,
            "files": files_count,
            "total_size": total_size
        }))
    }
}

/// Simple glob pattern matching
fn matches_glob(name: &str, pattern: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let name_chars: Vec<char> = name.chars().collect();

    matches_glob_helper(&pattern_chars, &name_chars)
}

fn matches_glob_helper(pattern: &[char], name: &[char]) -> bool {
    if pattern.is_empty() {
        return name.is_empty();
    }

    if pattern[0] == '*' {
        // Try matching zero or more characters
        for i in 0..=name.len() {
            if matches_glob_helper(&pattern[1..], &name[i..]) {
                return true;
            }
        }
        return false;
    }

    if name.is_empty() {
        return false;
    }

    if pattern[0] == '?' || pattern[0] == name[0] {
        return matches_glob_helper(&pattern[1..], &name[1..]);
    }

    false
}

/// Format file size in human-readable format
fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.1} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_matching() {
        assert!(matches_glob("test.rs", "*.rs"));
        assert!(matches_glob("test.rs", "test.*"));
        assert!(matches_glob("test.rs", "*.*"));
        assert!(matches_glob("test.rs", "????.*"));
        assert!(!matches_glob("test.rs", "*.txt"));
        assert!(!matches_glob("test.rs", "foo.*"));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1073741824), "1.0 GB");
    }
}
