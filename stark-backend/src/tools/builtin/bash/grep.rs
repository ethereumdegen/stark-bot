use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use walkdir::WalkDir;

/// Grep tool for content search within files
/// Uses ripgrep (rg) if available, falls back to Rust-native search
pub struct GrepTool {
    definition: ToolDefinition,
}

impl GrepTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "pattern".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Regular expression pattern to search for".to_string(),
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
                    "Directory or file to search in (relative to workspace, default: current dir)"
                        .to_string(),
                default: Some(json!(".")),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "glob".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "File pattern filter (e.g., '*.rs', '*.{ts,tsx}'). Default: all files"
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "output_mode".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Output mode: 'content' (show matching lines), 'files_with_matches' (file paths only), 'count' (match counts)".to_string(),
                default: Some(json!("content")),
                items: None,
                enum_values: Some(vec![
                    "content".to_string(),
                    "files_with_matches".to_string(),
                    "count".to_string(),
                ]),
            },
        );

        properties.insert(
            "context".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Number of context lines before and after each match (default: 0)"
                    .to_string(),
                default: Some(json!(0)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "case_insensitive".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Case-insensitive search (default: false)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "max_results".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum number of results to return (default: 100)".to_string(),
                default: Some(json!(100)),
                items: None,
                enum_values: None,
            },
        );

        GrepTool {
            definition: ToolDefinition {
                name: "grep".to_string(),
                description: "Search for patterns in file contents. Supports regex patterns, file filtering with glob patterns, and various output modes.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["pattern".to_string()],
                },
                group: ToolGroup::Development,
            },
        }
    }

    /// Check if ripgrep is available
    async fn has_ripgrep() -> bool {
        Command::new("rg")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Run search using ripgrep
    async fn search_with_ripgrep(
        &self,
        pattern: &str,
        search_path: &Path,
        params: &GrepParams,
    ) -> Result<String, String> {
        let mut cmd = Command::new("rg");

        // Add pattern and path
        cmd.arg(pattern).arg(search_path);

        // Output mode
        match params.output_mode.as_deref() {
            Some("files_with_matches") => {
                cmd.arg("-l");
            }
            Some("count") => {
                cmd.arg("-c");
            }
            _ => {
                cmd.arg("-n"); // Line numbers
                if let Some(ctx) = params.context {
                    if ctx > 0 {
                        cmd.arg("-C").arg(ctx.to_string());
                    }
                }
            }
        }

        // Case insensitivity
        if params.case_insensitive.unwrap_or(false) {
            cmd.arg("-i");
        }

        // Glob filter
        if let Some(ref glob) = params.glob {
            cmd.arg("-g").arg(glob);
        }

        // Max results
        let max_results = params.max_results.unwrap_or(100);
        cmd.arg("-m").arg(max_results.to_string());

        // Run the command
        let output = cmd
            .output()
            .await
            .map_err(|e| format!("Failed to run ripgrep: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() && stdout.is_empty() {
            if stderr.contains("No such file or directory") {
                return Err("Path not found".to_string());
            }
            // rg returns exit code 1 for "no matches" which is normal
            if output.status.code() == Some(1) {
                return Ok("No matches found.".to_string());
            }
            return Err(format!("ripgrep error: {}", stderr));
        }

        if stdout.is_empty() {
            return Ok("No matches found.".to_string());
        }

        Ok(stdout.to_string())
    }

    /// Run search using native Rust implementation (fallback)
    fn search_native(
        &self,
        pattern: &str,
        search_path: &Path,
        params: &GrepParams,
    ) -> Result<String, String> {
        let regex = if params.case_insensitive.unwrap_or(false) {
            Regex::new(&format!("(?i){}", pattern))
        } else {
            Regex::new(pattern)
        }
        .map_err(|e| format!("Invalid regex pattern: {}", e))?;

        let max_results = params.max_results.unwrap_or(100);
        let context = params.context.unwrap_or(0);
        let output_mode = params.output_mode.as_deref().unwrap_or("content");

        let glob_pattern = params.glob.as_ref().map(|g| {
            glob::Pattern::new(g).unwrap_or_else(|_| glob::Pattern::new("*").unwrap())
        });

        let mut results = Vec::new();
        let mut match_count = 0;

        for entry in WalkDir::new(search_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if match_count >= max_results {
                break;
            }

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Check glob pattern
            if let Some(ref gp) = glob_pattern {
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                if !gp.matches(&file_name) {
                    continue;
                }
            }

            // Skip binary files
            if let Ok(content) = std::fs::read_to_string(path) {
                let lines: Vec<&str> = content.lines().collect();
                let mut file_matches = Vec::new();
                let mut file_count = 0;

                for (line_num, line) in lines.iter().enumerate() {
                    if regex.is_match(line) {
                        file_count += 1;
                        match_count += 1;

                        if match_count > max_results {
                            break;
                        }

                        match output_mode {
                            "files_with_matches" => {
                                // Will handle after loop
                            }
                            "count" => {
                                // Will handle after loop
                            }
                            _ => {
                                // Content mode with context
                                let start = line_num.saturating_sub(context);
                                let end = (line_num + context + 1).min(lines.len());

                                for i in start..end {
                                    let prefix = if i == line_num { ">" } else { " " };
                                    file_matches.push(format!(
                                        "{}{}:{}: {}",
                                        prefix,
                                        path.display(),
                                        i + 1,
                                        lines[i]
                                    ));
                                }
                                if context > 0 && end < lines.len() {
                                    file_matches.push("--".to_string());
                                }
                            }
                        }
                    }
                }

                if file_count > 0 {
                    match output_mode {
                        "files_with_matches" => {
                            results.push(path.display().to_string());
                        }
                        "count" => {
                            results.push(format!("{}:{}", path.display(), file_count));
                        }
                        _ => {
                            results.extend(file_matches);
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            return Ok("No matches found.".to_string());
        }

        Ok(results.join("\n"))
    }
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct GrepParams {
    pattern: String,
    path: Option<String>,
    glob: Option<String>,
    output_mode: Option<String>,
    context: Option<usize>,
    case_insensitive: Option<bool>,
    max_results: Option<usize>,
}

#[async_trait]
impl Tool for GrepTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: GrepParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get workspace directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Resolve search path
        let search_path = if let Some(ref path) = params.path {
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

        let canonical_path = match search_path.canonicalize() {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Cannot resolve search path: {}", e)),
        };

        if !canonical_path.starts_with(&canonical_workspace) {
            return ToolResult::error(format!(
                "Access denied: path '{}' is outside the workspace directory",
                params.path.as_deref().unwrap_or(".")
            ));
        }

        // Run search
        let result = if Self::has_ripgrep().await {
            self.search_with_ripgrep(&params.pattern, &canonical_path, &params)
                .await
        } else {
            self.search_native(&params.pattern, &canonical_path, &params)
        };

        match result {
            Ok(output) => {
                // Truncate if too long (keep small to avoid context bloat)
                let max_output = 12000;
                if output.len() > max_output {
                    let truncated = &output[..max_output];
                    ToolResult::success(format!(
                        "{}\n\n[Output truncated. {} more characters not shown. Use more specific patterns.]",
                        truncated,
                        output.len() - max_output
                    ))
                } else {
                    ToolResult::success(output)
                }
            }
            Err(e) => ToolResult::error(e),
        }
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
    async fn test_grep_basic() {
        let tool = GrepTool::new();
        let temp_dir = TempDir::new().unwrap();

        // Create a test file
        let test_file = temp_dir.path().join("test.rs");
        std::fs::write(&test_file, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool.execute(json!({ "pattern": "main" }), &context).await;

        assert!(result.success);
        assert!(result.content.contains("main"));
    }

    #[tokio::test]
    async fn test_grep_outside_workspace() {
        let tool = GrepTool::new();
        let temp_dir = TempDir::new().unwrap();
        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(json!({ "pattern": "test", "path": "/etc" }), &context)
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("outside the workspace"));
    }
}
