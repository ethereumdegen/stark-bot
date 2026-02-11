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

/// Intrinsic files that appear in all workspaces
const INTRINSIC_FILES: &[(&str, &str)] = &[
    ("SOUL.md", "Agent personality and behavior configuration"),
    ("IDENTITY.json", "Agent EIP-8004 identity registration file"),
];

/// Default maximum entries to return per page
const DEFAULT_LIMIT: usize = 20;
/// Maximum allowed limit
const MAX_LIMIT: usize = 50;
/// Maximum output size in bytes before truncation
const MAX_OUTPUT_SIZE: usize = 4000;

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
        properties.insert(
            "limit".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: format!(
                    "Maximum number of entries to return (default: {}, max: {}). Use with offset for pagination.",
                    DEFAULT_LIMIT, MAX_LIMIT
                ),
                default: Some(json!(DEFAULT_LIMIT)),
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "offset".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Number of entries to skip for pagination (default: 0). Use 'next_offset' from previous response to get next page.".to_string(),
                default: Some(json!(0)),
                items: None,
                enum_values: None,
            },
        );

        ListFilesTool {
            definition: ToolDefinition {
                name: "list_files".to_string(),
                description: format!(
                    "List files and directories with pagination. Returns up to {} entries by default. \
                     Use 'offset' parameter with 'next_offset' from response to page through large directories. \
                     Can list recursively and filter by pattern. The path must be within the allowed workspace directory.",
                    DEFAULT_LIMIT
                ),
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
    limit: Option<usize>,
    offset: Option<usize>,
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
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
        let offset = params.offset.unwrap_or(0);

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

        // Calculate total counts before pagination
        let total_entries = entries.len();
        let total_dirs: usize = entries.iter().filter(|e| e.is_dir).count();
        let total_files = total_entries - total_dirs;
        let grand_total_size: u64 = entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();

        // Apply pagination
        let paginated_entries: Vec<&FileEntry> = entries.iter().skip(offset).take(limit).collect();
        let has_more = offset + paginated_entries.len() < total_entries;
        let next_offset = if has_more { Some(offset + paginated_entries.len()) } else { None };

        // Format output
        if paginated_entries.is_empty() {
            let message = if offset > 0 {
                format!("No more entries. Total: {} entries ({} dirs, {} files)", total_entries, total_dirs, total_files)
            } else {
                "Directory is empty or no files match the pattern.".to_string()
            };
            return ToolResult::success(message)
                .with_metadata(json!({
                    "path": path,
                    "total_entries": total_entries,
                    "showing": 0,
                    "offset": offset,
                    "has_more": false
                }));
        }

        let mut page_dirs = 0;
        let mut page_files = 0;
        let mut page_size = 0u64;

        let formatted: Vec<String> = paginated_entries
            .iter()
            .map(|e| {
                let indent = "  ".repeat(e.depth);
                let type_indicator = if e.is_dir {
                    page_dirs += 1;
                    "📁"
                } else {
                    page_files += 1;
                    page_size += e.size;
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

        // Build pagination info string
        let pagination_info = if has_more {
            format!(
                "\n\n📄 Showing {}-{} of {} total ({} dirs, {} files, {})\n⏩ More entries available. Use offset={} to see next page.",
                offset + 1,
                offset + paginated_entries.len(),
                total_entries,
                total_dirs,
                total_files,
                format_size(grand_total_size),
                next_offset.unwrap()
            )
        } else if offset > 0 {
            format!(
                "\n\n📄 Showing {}-{} of {} total ({} dirs, {} files, {})\n✅ End of listing.",
                offset + 1,
                offset + paginated_entries.len(),
                total_entries,
                total_dirs,
                total_files,
                format_size(grand_total_size)
            )
        } else {
            format!(
                "\n\n📊 {} directories, {} files ({})",
                total_dirs,
                total_files,
                format_size(grand_total_size)
            )
        };

        let mut output = format!("{}{}", formatted.join("\n"), pagination_info);

        // Truncate if output is too large to prevent context bloat
        if output.len() > MAX_OUTPUT_SIZE {
            output.truncate(MAX_OUTPUT_SIZE);
            output.push_str("\n\n⚠️ [Output truncated - use pagination or filter with pattern]");
        }

        ToolResult::success(output).with_metadata(json!({
            "path": path,
            "total_entries": total_entries,
            "total_directories": total_dirs,
            "total_files": total_files,
            "total_size": grand_total_size,
            "showing": paginated_entries.len(),
            "offset": offset,
            "limit": limit,
            "has_more": has_more,
            "next_offset": next_offset
        }))
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
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
