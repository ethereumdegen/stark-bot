//! Index Project tool - scans a workspace to build a project context map
//!
//! Produces a structured summary of the project: type, key files, module structure,
//! dependencies, and entry points. This summary gets injected into the system prompt
//! so the agent starts every task with awareness of the codebase.

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

/// Tool for scanning and indexing a project workspace
pub struct IndexProjectTool {
    definition: ToolDefinition,
}

impl IndexProjectTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Project directory to index (defaults to workspace root).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "depth".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Max directory depth to scan (default: 4).".to_string(),
                default: Some(json!(4)),
                items: None,
                enum_values: None,
            },
        );

        IndexProjectTool {
            definition: ToolDefinition {
                name: "index_project".to_string(),
                description: "Scan a project directory and produce a structured summary: project type, key files, module structure, dependencies, and entry points. Use at the start of a coding task to understand the codebase before making changes.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::Development,
            },
        }
    }

    /// Detect the project type and configuration
    fn detect_project(root: &Path) -> ProjectInfo {
        let mut info = ProjectInfo {
            project_type: "unknown".to_string(),
            name: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            language: "unknown".to_string(),
            framework: None,
            entry_points: Vec::new(),
            config_files: Vec::new(),
            dependencies_summary: None,
        };

        // Rust
        if root.join("Cargo.toml").exists() {
            info.project_type = "rust".to_string();
            info.language = "Rust".to_string();
            info.config_files.push("Cargo.toml".to_string());
            if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml")) {
                // Extract package name
                for line in content.lines() {
                    if line.starts_with("name = ") || line.starts_with("name=") {
                        let name = line.split('=').nth(1).unwrap_or("").trim().trim_matches('"');
                        info.name = name.to_string();
                        break;
                    }
                }
                // Count dependencies
                let dep_count = content.lines().filter(|l| {
                    let t = l.trim();
                    !t.starts_with('[') && !t.starts_with('#') && t.contains('=') && !t.starts_with("name") && !t.starts_with("version") && !t.starts_with("edition")
                }).count();
                info.dependencies_summary = Some(format!("{} crate dependencies", dep_count));
            }
            // Check for common entry points
            for entry in &["src/main.rs", "src/lib.rs", "src/bin/"] {
                if root.join(entry).exists() {
                    info.entry_points.push(entry.to_string());
                }
            }
            // Detect workspace
            if root.join("Cargo.toml").exists() {
                if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml")) {
                    if content.contains("[workspace]") {
                        info.project_type = "rust_workspace".to_string();
                    }
                }
            }
        }
        // Node.js / TypeScript
        else if root.join("package.json").exists() {
            info.language = if root.join("tsconfig.json").exists() {
                info.config_files.push("tsconfig.json".to_string());
                "TypeScript".to_string()
            } else {
                "JavaScript".to_string()
            };
            info.config_files.push("package.json".to_string());

            if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
                if let Ok(pkg) = serde_json::from_str::<Value>(&content) {
                    if let Some(name) = pkg.get("name").and_then(|n| n.as_str()) {
                        info.name = name.to_string();
                    }
                    // Detect framework
                    let deps_str = format!(
                        "{}{}",
                        pkg.get("dependencies").map(|d| d.to_string()).unwrap_or_default(),
                        pkg.get("devDependencies").map(|d| d.to_string()).unwrap_or_default()
                    );
                    if deps_str.contains("next") {
                        info.framework = Some("Next.js".to_string());
                        info.project_type = "nextjs".to_string();
                    } else if deps_str.contains("react") {
                        info.framework = Some("React".to_string());
                        info.project_type = "react".to_string();
                    } else if deps_str.contains("express") {
                        info.framework = Some("Express".to_string());
                        info.project_type = "node_api".to_string();
                    } else if deps_str.contains("fastify") {
                        info.framework = Some("Fastify".to_string());
                        info.project_type = "node_api".to_string();
                    } else {
                        info.project_type = "node".to_string();
                    }
                    // Count dependencies
                    let dep_count = pkg.get("dependencies").and_then(|d| d.as_object()).map(|o| o.len()).unwrap_or(0);
                    let dev_count = pkg.get("devDependencies").and_then(|d| d.as_object()).map(|o| o.len()).unwrap_or(0);
                    info.dependencies_summary = Some(format!("{} deps + {} devDeps", dep_count, dev_count));
                    // Detect entry point
                    if let Some(main) = pkg.get("main").and_then(|m| m.as_str()) {
                        info.entry_points.push(main.to_string());
                    }
                }
            }
        }
        // Python
        else if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
            info.project_type = "python".to_string();
            info.language = "Python".to_string();
            if root.join("pyproject.toml").exists() {
                info.config_files.push("pyproject.toml".to_string());
            }
            if root.join("setup.py").exists() {
                info.config_files.push("setup.py".to_string());
            }
            if root.join("requirements.txt").exists() {
                info.config_files.push("requirements.txt".to_string());
                if let Ok(content) = std::fs::read_to_string(root.join("requirements.txt")) {
                    let count = content.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).count();
                    info.dependencies_summary = Some(format!("{} Python packages", count));
                }
            }
        }
        // Go
        else if root.join("go.mod").exists() {
            info.project_type = "go".to_string();
            info.language = "Go".to_string();
            info.config_files.push("go.mod".to_string());
            if let Ok(content) = std::fs::read_to_string(root.join("go.mod")) {
                if let Some(first_line) = content.lines().next() {
                    if first_line.starts_with("module ") {
                        info.name = first_line.strip_prefix("module ").unwrap_or("").trim().to_string();
                    }
                }
            }
        }

        // Common config files
        for f in &[".env", ".gitignore", "Dockerfile", "docker-compose.yml", "Makefile", ".github/workflows"] {
            if root.join(f).exists() {
                info.config_files.push(f.to_string());
            }
        }

        info
    }

    /// Build a directory tree summary (limited depth)
    fn build_tree(root: &Path, max_depth: usize) -> Vec<TreeEntry> {
        let mut entries = Vec::new();
        Self::walk_dir(root, root, 0, max_depth, &mut entries);
        entries
    }

    fn walk_dir(root: &Path, dir: &Path, depth: usize, max_depth: usize, entries: &mut Vec<TreeEntry>) {
        if depth > max_depth {
            return;
        }

        // Skip common non-useful directories
        let skip_dirs = [
            "node_modules", ".git", "target", "dist", "build", "__pycache__",
            ".next", ".turbo", ".cache", "vendor", ".venv", "venv",
        ];

        let mut dir_entries: Vec<_> = match std::fs::read_dir(dir) {
            Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
            Err(_) => return,
        };

        // Sort for consistent output
        dir_entries.sort_by_key(|e| e.file_name());

        let mut file_count = 0;
        let mut _dir_count = 0;

        for entry in &dir_entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap_or(&path);

            if path.is_dir() {
                if skip_dirs.contains(&name.as_str()) || name.starts_with('.') {
                    continue;
                }
                _dir_count += 1;
                entries.push(TreeEntry {
                    path: format!("{}/", relative.display()),
                    is_dir: true,
                    size: None,
                });
                Self::walk_dir(root, &path, depth + 1, max_depth, entries);
            } else {
                file_count += 1;
                // Only include significant files in tree (skip at depth > 2 if too many)
                if depth <= 2 || file_count <= 20 {
                    let size = std::fs::metadata(&path).ok().map(|m| m.len());
                    entries.push(TreeEntry {
                        path: relative.display().to_string(),
                        is_dir: false,
                        size,
                    });
                }
            }
        }

        // If we truncated files, note it
        if file_count > 20 && depth > 2 {
            entries.push(TreeEntry {
                path: format!("... and {} more files in {}/", file_count - 20, dir.strip_prefix(root).unwrap_or(dir).display()),
                is_dir: false,
                size: None,
            });
        }
    }

    /// Count files by extension
    fn count_by_extension(root: &Path) -> HashMap<String, usize> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        Self::count_files_recursive(root, &mut counts);
        counts
    }

    fn count_files_recursive(dir: &Path, counts: &mut HashMap<String, usize>) {
        let skip_dirs = ["node_modules", ".git", "target", "dist", "build", "__pycache__", ".next", "vendor", ".venv"];

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if path.is_dir() {
                if !skip_dirs.contains(&name.as_str()) && !name.starts_with('.') {
                    Self::count_files_recursive(&path, counts);
                }
            } else if let Some(ext) = path.extension() {
                *counts.entry(ext.to_string_lossy().to_string()).or_insert(0) += 1;
            }
        }
    }
}

impl Default for IndexProjectTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct ProjectInfo {
    project_type: String,
    name: String,
    language: String,
    framework: Option<String>,
    entry_points: Vec<String>,
    config_files: Vec<String>,
    dependencies_summary: Option<String>,
}

#[derive(Debug)]
struct TreeEntry {
    path: String,
    is_dir: bool,
    #[allow(dead_code)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct IndexProjectParams {
    path: Option<String>,
    depth: Option<usize>,
}

#[async_trait]
impl Tool for IndexProjectTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: IndexProjectParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let root = if let Some(ref p) = params.path {
            let path = PathBuf::from(p);
            if path.is_absolute() { path } else { workspace.join(path) }
        } else {
            workspace
        };

        if !root.exists() || !root.is_dir() {
            return ToolResult::error(format!("Directory not found: {}", root.display()));
        }

        let max_depth = params.depth.unwrap_or(4);

        // Detect project info
        let info = Self::detect_project(&root);

        // Build tree
        let tree = Self::build_tree(&root, max_depth);

        // Count by extension
        let ext_counts = Self::count_by_extension(&root);
        let mut ext_sorted: Vec<_> = ext_counts.into_iter().collect();
        ext_sorted.sort_by(|a, b| b.1.cmp(&a.1));

        // Build output
        let mut output = String::new();
        output.push_str(&format!("## Project: {}\n\n", info.name));
        output.push_str(&format!("**Type**: {} | **Language**: {}", info.project_type, info.language));
        if let Some(ref fw) = info.framework {
            output.push_str(&format!(" | **Framework**: {}", fw));
        }
        output.push('\n');
        if let Some(ref deps) = info.dependencies_summary {
            output.push_str(&format!("**Dependencies**: {}\n", deps));
        }

        if !info.entry_points.is_empty() {
            output.push_str(&format!("**Entry points**: {}\n", info.entry_points.join(", ")));
        }

        if !info.config_files.is_empty() {
            output.push_str(&format!("**Config files**: {}\n", info.config_files.join(", ")));
        }

        // File type breakdown
        output.push_str("\n### File Types\n");
        for (ext, count) in ext_sorted.iter().take(10) {
            output.push_str(&format!("- .{}: {} files\n", ext, count));
        }

        // Directory tree
        output.push_str("\n### Structure\n```\n");
        for entry in &tree {
            if tree.len() > 200 {
                // Compact mode for large projects
                if entry.is_dir {
                    output.push_str(&format!("{}\n", entry.path));
                }
            } else {
                output.push_str(&format!("{}\n", entry.path));
            }
        }
        output.push_str("```\n");

        ToolResult::success(output).with_metadata(json!({
            "project_type": info.project_type,
            "language": info.language,
            "framework": info.framework,
            "name": info.name,
        }))
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_rust_project() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("Cargo.toml"), "[package]\nname = \"my-app\"\nversion = \"0.1.0\"").unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/main.rs"), "fn main() {}").unwrap();

        let info = IndexProjectTool::detect_project(temp.path());
        assert_eq!(info.project_type, "rust");
        assert_eq!(info.name, "my-app");
        assert!(info.entry_points.contains(&"src/main.rs".to_string()));
    }

    #[test]
    fn test_detect_nextjs_project() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("package.json"), r#"{"name":"my-site","dependencies":{"next":"14.0.0","react":"18.0.0"}}"#).unwrap();
        std::fs::write(temp.path().join("tsconfig.json"), "{}").unwrap();

        let info = IndexProjectTool::detect_project(temp.path());
        assert_eq!(info.project_type, "nextjs");
        assert_eq!(info.language, "TypeScript");
        assert_eq!(info.framework, Some("Next.js".to_string()));
    }

    #[test]
    fn test_count_by_extension() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/main.rs"), "").unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "").unwrap();
        std::fs::write(temp.path().join("README.md"), "").unwrap();

        let counts = IndexProjectTool::count_by_extension(temp.path());
        assert_eq!(counts.get("rs"), Some(&2));
        assert_eq!(counts.get("md"), Some(&1));
    }
}
