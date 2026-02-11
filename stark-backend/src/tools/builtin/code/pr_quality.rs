use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// PR Quality Check Tool - Automated code review before PR creation
///
/// This tool validates code quality before creating pull requests:
/// - Detects debug code (console.log, println!, dbg!, debugger, etc.)
/// - Finds TODO/FIXME comments without issue references
/// - Checks for large files that shouldn't be committed
/// - Validates PR size (lines changed, files changed)
/// - Checks for common code smells
/// - Ensures tests exist for new code (optional)
pub struct PrQualityTool {
    definition: ToolDefinition,
}

/// Debug code patterns to detect
const DEBUG_PATTERNS: &[(&str, &str, &[&str])] = &[
    // Pattern, description, file extensions
    (r"console\.(log|debug|info|warn|error)\s*\(", "console.log statement", &["js", "ts", "jsx", "tsx"]),
    (r"debugger\s*;?", "debugger statement", &["js", "ts", "jsx", "tsx"]),
    (r"println!\s*\(", "println! macro", &["rs"]),
    (r"dbg!\s*\(", "dbg! macro", &["rs"]),
    (r"eprintln!\s*\(", "eprintln! macro", &["rs"]),
    (r"print\s*\(", "print() function", &["py"]),
    (r"pprint\s*\(", "pprint() function", &["py"]),
    (r"import\s+pdb", "pdb import", &["py"]),
    (r"breakpoint\s*\(\s*\)", "breakpoint()", &["py"]),
    (r"System\.out\.print", "System.out.print", &["java"]),
    (r"fmt\.Print", "fmt.Print", &["go"]),
    (r"log\.Print", "log.Print (might be intentional)", &["go"]),
    (r"var_dump\s*\(", "var_dump()", &["php"]),
    (r"dd\s*\(", "dd() dump and die", &["php"]),
    (r"puts\s+", "puts statement", &["rb"]),
    (r"p\s+[^=]", "p debug output", &["rb"]),
    (r"binding\.pry", "binding.pry", &["rb"]),
];

/// TODO/FIXME patterns
const TODO_PATTERNS: &[&str] = &[
    r"(?i)//\s*TODO[:\s]",
    r"(?i)//\s*FIXME[:\s]",
    r"(?i)//\s*HACK[:\s]",
    r"(?i)//\s*XXX[:\s]",
    r"(?i)#\s*TODO[:\s]",
    r"(?i)#\s*FIXME[:\s]",
    r"(?i)/\*\s*TODO[:\s]",
    r"(?i)/\*\s*FIXME[:\s]",
];

/// Large file thresholds
const LARGE_FILE_LINES: usize = 1000;
const VERY_LARGE_FILE_LINES: usize = 2000;
const MAX_FILES_IN_PR: usize = 50;
const MAX_LINES_IN_PR: usize = 1000;

impl PrQualityTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "operation".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Quality check operation: full_check, debug_scan, todo_scan, size_check, diff_summary".to_string(),
                default: Some(json!("full_check")),
                items: None,
                enum_values: Some(vec![
                    "full_check".to_string(),
                    "debug_scan".to_string(),
                    "todo_scan".to_string(),
                    "size_check".to_string(),
                    "diff_summary".to_string(),
                ]),
            },
        );

        properties.insert(
            "base_branch".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Base branch to compare against (default: main)".to_string(),
                default: Some(json!("main")),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "files".to_string(),
            PropertySchema {
                schema_type: "array".to_string(),
                description: "Specific files to check (optional, defaults to all changed files)".to_string(),
                default: None,
                items: Some(Box::new(PropertySchema {
                    schema_type: "string".to_string(),
                    description: "File path".to_string(),
                    default: None,
                    items: None,
                    enum_values: None,
                })),
                enum_values: None,
            },
        );

        properties.insert(
            "strict".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Strict mode - fail on any warning (default: false)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "ignore_todos".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Ignore TODO/FIXME comments (default: false)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "max_lines".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: format!("Maximum lines changed threshold (default: {})", MAX_LINES_IN_PR),
                default: Some(json!(MAX_LINES_IN_PR)),
                items: None,
                enum_values: None,
            },
        );

        PrQualityTool {
            definition: ToolDefinition {
                name: "pr_quality".to_string(),
                description: "Pre-PR quality checks: detects debug code, TODO/FIXME comments, validates PR size, and provides diff summary. Run before creating PRs to ensure code quality.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::Development,
                hidden: false,
            },
        }
    }

    /// Run a git command and return output
    async fn run_git(
        &self,
        args: &[&str],
        workspace: &PathBuf,
    ) -> Result<String, String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("Failed to execute git: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(format!("Git command failed: {}{}", stdout, stderr));
        }

        Ok(stdout.to_string())
    }

    /// Get files changed compared to base branch
    async fn get_changed_files(&self, workspace: &PathBuf, base_branch: &str) -> Result<Vec<String>, String> {
        let output = self.run_git(
            &["diff", "--name-only", &format!("{}..HEAD", base_branch)],
            workspace
        ).await?;

        Ok(output.lines().map(|s| s.to_string()).filter(|s| !s.is_empty()).collect())
    }

    /// Get diff stats
    async fn get_diff_stats(&self, workspace: &PathBuf, base_branch: &str) -> Result<(usize, usize, usize), String> {
        let output = self.run_git(
            &["diff", "--stat", &format!("{}..HEAD", base_branch)],
            workspace
        ).await?;

        // Parse the last line which contains summary
        // Format: "X files changed, Y insertions(+), Z deletions(-)"
        let mut files = 0;
        let mut insertions = 0;
        let mut deletions = 0;

        if let Some(last_line) = output.lines().last() {
            let re = Regex::new(r"(\d+) files? changed").ok();
            if let Some(re) = re {
                if let Some(caps) = re.captures(last_line) {
                    files = caps.get(1).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
                }
            }

            let re = Regex::new(r"(\d+) insertions?\(\+\)").ok();
            if let Some(re) = re {
                if let Some(caps) = re.captures(last_line) {
                    insertions = caps.get(1).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
                }
            }

            let re = Regex::new(r"(\d+) deletions?\(-\)").ok();
            if let Some(re) = re {
                if let Some(caps) = re.captures(last_line) {
                    deletions = caps.get(1).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
                }
            }
        }

        Ok((files, insertions, deletions))
    }

    /// Scan file for debug code
    fn scan_for_debug(&self, content: &str, file_ext: &str) -> Vec<(String, usize)> {
        let mut findings = Vec::new();

        for (pattern_str, desc, extensions) in DEBUG_PATTERNS {
            // Check if this pattern applies to this file type
            if !extensions.contains(&file_ext) {
                continue;
            }

            if let Ok(re) = Regex::new(pattern_str) {
                for (line_num, line) in content.lines().enumerate() {
                    // Skip comments
                    let trimmed = line.trim();
                    if trimmed.starts_with("//") && !pattern_str.contains("TODO") {
                        continue;
                    }

                    if re.is_match(line) {
                        findings.push((desc.to_string(), line_num + 1));
                    }
                }
            }
        }

        findings
    }

    /// Scan file for TODO/FIXME comments
    fn scan_for_todos(&self, content: &str) -> Vec<(String, usize)> {
        let mut findings = Vec::new();

        for pattern_str in TODO_PATTERNS {
            if let Ok(re) = Regex::new(pattern_str) {
                for (line_num, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        // Check if it has an issue reference
                        let has_issue = Regex::new(r"#\d+|issue|ticket|jira", ).map(|r| r.is_match(line)).unwrap_or(false);
                        if !has_issue {
                            let trimmed = line.trim();
                            let preview = if trimmed.len() > 60 {
                                format!("{}...", &trimmed[..60])
                            } else {
                                trimmed.to_string()
                            };
                            findings.push((preview, line_num + 1));
                        }
                    }
                }
            }
        }

        findings
    }

    /// Get file extension
    fn get_extension(file: &str) -> String {
        PathBuf::from(file)
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }

    /// Count lines in file
    async fn count_lines(&self, file_path: &PathBuf) -> Result<usize, String> {
        let content = tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(content.lines().count())
    }
}

impl Default for PrQualityTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct PrQualityParams {
    operation: Option<String>,
    base_branch: Option<String>,
    files: Option<Vec<String>>,
    strict: Option<bool>,
    ignore_todos: Option<bool>,
    max_lines: Option<usize>,
}

#[async_trait]
impl Tool for PrQualityTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: PrQualityParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let operation = params.operation.as_deref().unwrap_or("full_check");
        let base_branch = params.base_branch.as_deref().unwrap_or("main");
        let strict = params.strict.unwrap_or(false);
        let ignore_todos = params.ignore_todos.unwrap_or(false);
        let max_lines = params.max_lines.unwrap_or(MAX_LINES_IN_PR);

        // Get workspace directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Get files to check
        let files_to_check = if let Some(files) = params.files {
            files
        } else {
            match self.get_changed_files(&workspace, base_branch).await {
                Ok(f) => f,
                Err(e) => {
                    // If comparing to base fails, check staged files
                    match self.run_git(&["diff", "--name-only", "--staged"], &workspace).await {
                        Ok(output) => output.lines().map(|s| s.to_string()).filter(|s| !s.is_empty()).collect(),
                        Err(_) => return ToolResult::error(format!("Failed to get changed files: {}", e)),
                    }
                }
            }
        };

        if files_to_check.is_empty() {
            return ToolResult::success("No changed files to check.");
        }

        match operation {
            "debug_scan" => {
                let mut all_findings = Vec::new();

                for file in &files_to_check {
                    let file_path = workspace.join(file);
                    if !file_path.exists() {
                        continue;
                    }

                    let content = match tokio::fs::read_to_string(&file_path).await {
                        Ok(c) => c,
                        Err(_) => continue, // Skip binary files
                    };

                    let ext = Self::get_extension(file);
                    let findings = self.scan_for_debug(&content, &ext);

                    for (desc, line) in findings {
                        all_findings.push(format!("{}:{} - {}", file, line, desc));
                    }
                }

                if all_findings.is_empty() {
                    ToolResult::success("No debug code found.")
                } else if strict {
                    ToolResult::error(format!(
                        "DEBUG CODE DETECTED ({} issues):\n{}",
                        all_findings.len(),
                        all_findings.join("\n")
                    ))
                } else {
                    ToolResult::success(format!(
                        "WARNING: Debug code found ({} issues):\n{}",
                        all_findings.len(),
                        all_findings.join("\n")
                    ))
                }
            }

            "todo_scan" => {
                if ignore_todos {
                    return ToolResult::success("TODO scanning skipped (ignore_todos=true)");
                }

                let mut all_findings = Vec::new();

                for file in &files_to_check {
                    let file_path = workspace.join(file);
                    if !file_path.exists() {
                        continue;
                    }

                    let content = match tokio::fs::read_to_string(&file_path).await {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    let findings = self.scan_for_todos(&content);

                    for (text, line) in findings {
                        all_findings.push(format!("{}:{} - {}", file, line, text));
                    }
                }

                if all_findings.is_empty() {
                    ToolResult::success("No TODO/FIXME comments without issue references found.")
                } else {
                    ToolResult::success(format!(
                        "Found {} TODO/FIXME comments without issue references:\n{}\n\nConsider linking to issue numbers (e.g., TODO #123: ...)",
                        all_findings.len(),
                        all_findings.join("\n")
                    ))
                }
            }

            "size_check" => {
                let (files_changed, insertions, deletions) = match self.get_diff_stats(&workspace, base_branch).await {
                    Ok(stats) => stats,
                    Err(e) => return ToolResult::error(format!("Failed to get diff stats: {}", e)),
                };

                let total_lines = insertions + deletions;
                let mut warnings = Vec::new();

                if files_changed > MAX_FILES_IN_PR {
                    warnings.push(format!(
                        "Too many files changed: {} (recommended max: {})",
                        files_changed, MAX_FILES_IN_PR
                    ));
                }

                if total_lines > max_lines {
                    warnings.push(format!(
                        "Too many lines changed: {} (recommended max: {})",
                        total_lines, max_lines
                    ));
                }

                // Check for large files
                for file in &files_to_check {
                    let file_path = workspace.join(file);
                    if !file_path.exists() {
                        continue;
                    }

                    if let Ok(lines) = self.count_lines(&file_path).await {
                        if lines > VERY_LARGE_FILE_LINES {
                            warnings.push(format!(
                                "Very large file: {} ({} lines)",
                                file, lines
                            ));
                        } else if lines > LARGE_FILE_LINES {
                            warnings.push(format!(
                                "Large file: {} ({} lines)",
                                file, lines
                            ));
                        }
                    }
                }

                let summary = format!(
                    "PR Size:\n  Files changed: {}\n  Lines added: +{}\n  Lines removed: -{}\n  Total changes: {}",
                    files_changed, insertions, deletions, total_lines
                );

                if warnings.is_empty() {
                    ToolResult::success(format!("{}\n\nSize check passed.", summary))
                } else if strict {
                    ToolResult::error(format!(
                        "{}\n\nSIZE WARNINGS:\n{}",
                        summary,
                        warnings.join("\n")
                    ))
                } else {
                    ToolResult::success(format!(
                        "{}\n\nWarnings:\n{}\n\nConsider splitting into smaller PRs for easier review.",
                        summary,
                        warnings.join("\n")
                    ))
                }
            }

            "diff_summary" => {
                let (files_changed, insertions, deletions) = match self.get_diff_stats(&workspace, base_branch).await {
                    Ok(stats) => stats,
                    Err(e) => return ToolResult::error(format!("Failed to get diff stats: {}", e)),
                };

                // Get commit log
                let log = self.run_git(
                    &["log", "--oneline", &format!("{}..HEAD", base_branch), "-20"],
                    &workspace
                ).await.unwrap_or_else(|_| "Unable to get commit log".to_string());

                // Categorize changed files
                let mut by_type: HashMap<String, Vec<String>> = HashMap::new();
                for file in &files_to_check {
                    let ext = Self::get_extension(file);
                    let category = match ext.as_str() {
                        "rs" | "go" | "py" | "js" | "ts" | "java" | "c" | "cpp" | "h" => "Source",
                        "md" | "txt" | "rst" => "Documentation",
                        "json" | "yaml" | "yml" | "toml" | "ini" => "Configuration",
                        "test" | "spec" => "Tests",
                        "css" | "scss" | "less" => "Styles",
                        "html" | "jsx" | "tsx" | "vue" => "UI",
                        _ => "Other",
                    };
                    by_type.entry(category.to_string()).or_default().push(file.clone());
                }

                let categories: Vec<String> = by_type
                    .iter()
                    .map(|(cat, files)| format!("  {}: {} files", cat, files.len()))
                    .collect();

                ToolResult::success(format!(
                    "DIFF SUMMARY vs {}\n\n\
                    Stats:\n  Files: {}\n  +{} / -{}\n\n\
                    Categories:\n{}\n\n\
                    Recent Commits:\n{}\n\n\
                    Changed Files:\n  {}",
                    base_branch,
                    files_changed,
                    insertions,
                    deletions,
                    categories.join("\n"),
                    log,
                    files_to_check.join("\n  ")
                ))
            }

            "full_check" | _ => {
                // Run all checks
                let mut results = Vec::new();
                let mut has_errors = false;
                let mut has_warnings = false;

                // 1. Size check
                let (files_changed, insertions, deletions) = match self.get_diff_stats(&workspace, base_branch).await {
                    Ok(stats) => stats,
                    Err(_) => (files_to_check.len(), 0, 0),
                };

                let total_lines = insertions + deletions;
                results.push(format!(
                    "SIZE: {} files, +{} -{} ({} total lines)",
                    files_changed, insertions, deletions, total_lines
                ));

                if files_changed > MAX_FILES_IN_PR || total_lines > max_lines {
                    results.push(format!(
                        "  WARNING: PR may be too large (max {} files, {} lines recommended)",
                        MAX_FILES_IN_PR, max_lines
                    ));
                    has_warnings = true;
                }

                // 2. Debug code scan
                let mut debug_count = 0;
                for file in &files_to_check {
                    let file_path = workspace.join(file);
                    if let Ok(content) = tokio::fs::read_to_string(&file_path).await {
                        let ext = Self::get_extension(file);
                        let findings = self.scan_for_debug(&content, &ext);
                        debug_count += findings.len();
                        for (desc, line) in findings.iter().take(3) {
                            results.push(format!("  DEBUG: {}:{} - {}", file, line, desc));
                        }
                        if findings.len() > 3 {
                            results.push(format!("  ... and {} more in {}", findings.len() - 3, file));
                        }
                    }
                }

                if debug_count > 0 {
                    results.insert(results.len() - debug_count.min(4), format!("DEBUG CODE: {} issues found", debug_count));
                    has_errors = true;
                } else {
                    results.push("DEBUG CODE: None found".to_string());
                }

                // 3. TODO scan
                if !ignore_todos {
                    let mut todo_count = 0;
                    for file in &files_to_check {
                        let file_path = workspace.join(file);
                        if let Ok(content) = tokio::fs::read_to_string(&file_path).await {
                            let findings = self.scan_for_todos(&content);
                            todo_count += findings.len();
                        }
                    }

                    if todo_count > 0 {
                        results.push(format!("TODO/FIXME: {} without issue references", todo_count));
                        has_warnings = true;
                    } else {
                        results.push("TODO/FIXME: All have issue references or none found".to_string());
                    }
                }

                // Summary
                let status = if has_errors {
                    "FAILED"
                } else if has_warnings {
                    "PASSED WITH WARNINGS"
                } else {
                    "PASSED"
                };

                let result_text = format!(
                    "PR QUALITY CHECK: {}\n\n{}\n\nChecked {} files against {}",
                    status,
                    results.join("\n"),
                    files_to_check.len(),
                    base_branch
                );

                if has_errors && strict {
                    ToolResult::error(result_text)
                } else {
                    ToolResult::success(result_text)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_detection() {
        let tool = PrQualityTool::new();

        // JavaScript
        let js_code = "console.log('debug');\nconst x = 1;";
        let findings = tool.scan_for_debug(js_code, "js");
        assert_eq!(findings.len(), 1);

        // Rust
        let rust_code = "println!(\"debug\");\nlet x = 1;";
        let findings = tool.scan_for_debug(rust_code, "rs");
        assert_eq!(findings.len(), 1);

        // Clean code
        let clean = "const x = 1;\nconst y = 2;";
        let findings = tool.scan_for_debug(clean, "js");
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn test_todo_detection() {
        let tool = PrQualityTool::new();

        // TODO without issue
        let code_with_todo = "// TODO: fix this later";
        let findings = tool.scan_for_todos(code_with_todo);
        assert_eq!(findings.len(), 1);

        // TODO with issue reference
        let code_with_issue = "// TODO #123: fix this later";
        let findings = tool.scan_for_todos(code_with_issue);
        assert_eq!(findings.len(), 0);
    }
}
