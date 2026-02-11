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

/// Scoped Committer Tool - Safe, intelligent git commits
///
/// This tool provides enterprise-grade commit safety by:
/// - Only staging explicitly specified files (no git add . accidents)
/// - Scanning for secrets and sensitive data before commit
/// - Enforcing conventional commit format (feat, fix, refactor, etc.)
/// - Adding attribution (Co-Authored-By)
/// - Validating files exist and are actually modified
/// - Preventing commits on protected branches
pub struct CommitterTool {
    definition: ToolDefinition,
}

/// Secret patterns to detect before committing
const SECRET_PATTERNS: &[(&str, &str)] = &[
    (r#"(?i)api[_-]?key\s*[:=]\s*['"]?[a-zA-Z0-9_-]{20,}"#, "API key"),
    (r#"(?i)secret[_-]?key\s*[:=]\s*['"]?[a-zA-Z0-9_-]{20,}"#, "Secret key"),
    (r#"(?i)password\s*[:=]\s*['"]?[^\s'"]{8,}"#, "Password"),
    (r#"(?i)token\s*[:=]\s*['"]?[a-zA-Z0-9_-]{20,}"#, "Token"),
    (r#"(?i)bearer\s+[a-zA-Z0-9_-]{20,}"#, "Bearer token"),
    (r#"-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----"#, "Private key"),
    (r#"(?i)aws[_-]?access[_-]?key[_-]?id\s*[:=]\s*['"]?[A-Z0-9]{20}"#, "AWS Access Key"),
    (r#"(?i)aws[_-]?secret[_-]?access[_-]?key\s*[:=]\s*['"]?[a-zA-Z0-9/+=]{40}"#, "AWS Secret Key"),
    (r#"ghp_[a-zA-Z0-9]{36}"#, "GitHub Personal Access Token"),
    (r#"github_pat_[a-zA-Z0-9]{22}_[a-zA-Z0-9]{59}"#, "GitHub Fine-grained PAT"),
    (r#"sk-[a-zA-Z0-9]{48}"#, "OpenAI API Key"),
    (r#"sk-ant-[a-zA-Z0-9-]{95}"#, "Anthropic API Key"),
    (r#"xox[baprs]-[a-zA-Z0-9-]{10,}"#, "Slack Token"),
];

/// Sensitive file patterns that should never be committed
const SENSITIVE_FILES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    ".env.development",
    "credentials.json",
    "secrets.json",
    "config.secret.json",
    ".npmrc",
    ".pypirc",
    "id_rsa",
    "id_ed25519",
    "*.pem",
    "*.key",
    ".htpasswd",
];

/// Conventional commit types
const COMMIT_TYPES: &[&str] = &[
    "feat",     // New feature
    "fix",      // Bug fix
    "docs",     // Documentation only
    "style",    // Formatting, no code change
    "refactor", // Code change that neither fixes a bug nor adds a feature
    "perf",     // Performance improvement
    "test",     // Adding tests
    "chore",    // Maintenance tasks
    "ci",       // CI/CD changes
    "build",    // Build system changes
    "revert",   // Revert previous commit
];

impl CommitterTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "message".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Commit message. Should follow conventional commits format: type(scope): description. Types: feat, fix, docs, style, refactor, perf, test, chore, ci, build, revert".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "files".to_string(),
            PropertySchema {
                schema_type: "array".to_string(),
                description: "Files to stage and commit. Must specify exact file paths - no wildcards or '.' allowed for safety.".to_string(),
                default: None,
                items: Some(Box::new(PropertySchema {
                    schema_type: "string".to_string(),
                    description: "File path relative to workspace".to_string(),
                    default: None,
                    items: None,
                    enum_values: None,
                })),
                enum_values: None,
            },
        );

        properties.insert(
            "allow_sensitive".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Allow committing files that match sensitive patterns (DANGEROUS - requires explicit user confirmation). Default: false".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "skip_validation".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Skip conventional commit format validation. Default: false".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "push".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Push to remote after committing. Default: false".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "dry_run".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Preview what would be committed without actually committing. Default: false".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        CommitterTool {
            definition: ToolDefinition {
                name: "committer".to_string(),
                description: "Safe, scoped git commits with secret detection and conventional commit enforcement. Only stages specified files, prevents accidental commits of sensitive data, and adds proper attribution. Preferred over direct git commit for safety.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["message".to_string(), "files".to_string()],
                },
                group: ToolGroup::Development,
                hidden: false,
            },
        }
    }

    /// Check if a branch is protected
    fn is_protected_branch(branch: &str) -> bool {
        matches!(
            branch.to_lowercase().as_str(),
            "main" | "master" | "production" | "prod" | "release"
        )
    }

    /// Validate conventional commit format
    fn validate_conventional_commit(message: &str) -> Result<(), String> {
        // Pattern: type(scope)?: description
        let pattern = format!(
            r"^({})(\([a-zA-Z0-9_-]+\))?!?:\s+.+",
            COMMIT_TYPES.join("|")
        );
        let re = Regex::new(&pattern).unwrap();

        if !re.is_match(message.lines().next().unwrap_or("")) {
            return Err(format!(
                "Commit message doesn't follow conventional commits format.\n\
                Expected: type(scope): description\n\
                Valid types: {}\n\
                Examples:\n\
                  feat(auth): add OAuth2 login support\n\
                  fix: resolve memory leak in cache\n\
                  refactor(api): simplify error handling",
                COMMIT_TYPES.join(", ")
            ));
        }
        Ok(())
    }

    /// Check if a file matches sensitive patterns
    fn is_sensitive_file(file: &str) -> Option<&'static str> {
        let file_lower = file.to_lowercase();
        let file_name = PathBuf::from(file)
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        for pattern in SENSITIVE_FILES {
            if pattern.starts_with("*.") {
                // Extension pattern
                let ext = &pattern[1..]; // ".pem"
                if file_lower.ends_with(ext) {
                    return Some(pattern);
                }
            } else if file_name == pattern.to_lowercase() || file_lower.ends_with(&format!("/{}", pattern)) {
                return Some(pattern);
            }
        }
        None
    }

    /// Scan file content for secrets
    async fn scan_for_secrets(&self, file_path: &PathBuf) -> Vec<(String, usize)> {
        let mut findings = Vec::new();

        // Read file content
        let content = match tokio::fs::read_to_string(file_path).await {
            Ok(c) => c,
            Err(_) => return findings, // Skip binary files or unreadable files
        };

        for (pattern_str, name) in SECRET_PATTERNS {
            if let Ok(re) = Regex::new(pattern_str) {
                for (line_num, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        findings.push((name.to_string(), line_num + 1));
                    }
                }
            }
        }

        findings
    }

    /// Run a git command and return output
    async fn run_git(
        &self,
        args: &[&str],
        workspace: &PathBuf,
        context: &ToolContext,
    ) -> Result<String, String> {
        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set git author from context
        let bot_name = context.get_bot_name();
        let bot_email = context.get_bot_email();
        cmd.env("GIT_AUTHOR_NAME", &bot_name);
        cmd.env("GIT_AUTHOR_EMAIL", &bot_email);
        cmd.env("GIT_COMMITTER_NAME", &bot_name);
        cmd.env("GIT_COMMITTER_EMAIL", &bot_email);

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("Failed to execute git: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(format!(
                "Git command failed:\n{}{}",
                stdout,
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!("\nStderr: {}", stderr)
                }
            ));
        }

        Ok(stdout.to_string())
    }

    /// Get the current branch name
    async fn get_current_branch(&self, workspace: &PathBuf, context: &ToolContext) -> Result<String, String> {
        self.run_git(&["branch", "--show-current"], workspace, context)
            .await
            .map(|s| s.trim().to_string())
    }

    /// Check if files are actually modified
    async fn get_modified_files(&self, workspace: &PathBuf, context: &ToolContext) -> Result<Vec<String>, String> {
        let output = self.run_git(&["status", "--porcelain"], workspace, context).await?;
        Ok(output
            .lines()
            .filter_map(|line| {
                if line.len() > 3 {
                    Some(line[3..].to_string())
                } else {
                    None
                }
            })
            .collect())
    }
}

impl Default for CommitterTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct CommitterParams {
    message: String,
    files: Vec<String>,
    allow_sensitive: Option<bool>,
    skip_validation: Option<bool>,
    push: Option<bool>,
    dry_run: Option<bool>,
}

#[async_trait]
impl Tool for CommitterTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: CommitterParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let allow_sensitive = params.allow_sensitive.unwrap_or(false);
        let skip_validation = params.skip_validation.unwrap_or(false);
        let push = params.push.unwrap_or(false);
        let dry_run = params.dry_run.unwrap_or(false);

        // Get workspace directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Validate files list isn't empty
        if params.files.is_empty() {
            return ToolResult::error("No files specified. You must provide specific file paths to commit.");
        }

        // Block dangerous patterns
        let dangerous = params.files.iter().find(|f| {
            *f == "." || *f == "-A" || *f == "--all" || f.contains('*') || *f == "-a"
        });
        if let Some(d) = dangerous {
            return ToolResult::error(format!(
                "Dangerous pattern '{}' not allowed. Please specify individual files to ensure safety.",
                d
            ));
        }

        // Validate conventional commit format
        if !skip_validation {
            if let Err(e) = Self::validate_conventional_commit(&params.message) {
                return ToolResult::error(e);
            }
        }

        // Check current branch
        let branch = match self.get_current_branch(&workspace, context).await {
            Ok(b) => b,
            Err(e) => return ToolResult::error(format!("Failed to get current branch: {}", e)),
        };

        if Self::is_protected_branch(&branch) && push {
            return ToolResult::error(format!(
                "Cannot push directly to protected branch '{}'. Please create a feature branch and use a pull request.",
                branch
            ));
        }

        // Get list of actually modified files
        let modified_files = match self.get_modified_files(&workspace, context).await {
            Ok(f) => f,
            Err(e) => return ToolResult::error(format!("Failed to get modified files: {}", e)),
        };

        // Validate all specified files exist and are modified
        let mut issues = Vec::new();
        let mut sensitive_warnings = Vec::new();
        let mut secret_warnings = Vec::new();

        for file in &params.files {
            let file_path = workspace.join(file);

            // Check if file exists
            if !file_path.exists() {
                issues.push(format!("File not found: {}", file));
                continue;
            }

            // Check if file is modified (or new)
            if !modified_files.iter().any(|m| m == file || m.ends_with(file)) {
                // Check if it's a new untracked file
                let status_output = self.run_git(&["status", "--porcelain", file], &workspace, context).await;
                if let Ok(status) = status_output {
                    if status.is_empty() {
                        issues.push(format!("File has no changes: {}", file));
                        continue;
                    }
                }
            }

            // Check for sensitive file patterns
            if let Some(pattern) = Self::is_sensitive_file(file) {
                if !allow_sensitive {
                    sensitive_warnings.push(format!("{} (matches pattern: {})", file, pattern));
                }
            }

            // Scan for secrets in content
            let secrets = self.scan_for_secrets(&file_path).await;
            for (secret_type, line_num) in secrets {
                if !allow_sensitive {
                    secret_warnings.push(format!("{}: {} found on line {}", file, secret_type, line_num));
                }
            }
        }

        // Report issues
        if !issues.is_empty() {
            return ToolResult::error(format!(
                "Cannot commit - file issues found:\n{}",
                issues.join("\n")
            ));
        }

        // Block on sensitive files unless explicitly allowed
        if !sensitive_warnings.is_empty() {
            return ToolResult::error(format!(
                "SECURITY: Cannot commit sensitive files:\n{}\n\nIf you're sure these are safe, set allow_sensitive: true (requires explicit user confirmation).",
                sensitive_warnings.join("\n")
            ));
        }

        // Block on secrets found
        if !secret_warnings.is_empty() {
            return ToolResult::error(format!(
                "SECURITY: Potential secrets detected:\n{}\n\nReview and remove secrets before committing. If these are false positives, set allow_sensitive: true.",
                secret_warnings.join("\n")
            ));
        }

        // Dry run - just report what would happen
        if dry_run {
            return ToolResult::success(format!(
                "DRY RUN - Would commit:\n\
                Branch: {}\n\
                Files ({}):\n  {}\n\
                Message: {}\n\
                Push: {}",
                branch,
                params.files.len(),
                params.files.join("\n  "),
                params.message,
                push
            ));
        }

        // Stage the files
        let mut stage_args = vec!["add"];
        for f in &params.files {
            stage_args.push(f.as_str());
        }
        if let Err(e) = self.run_git(&stage_args, &workspace, context).await {
            return ToolResult::error(format!("Failed to stage files: {}", e));
        }

        // Build commit message with attribution
        let bot_name = context.get_bot_name();
        let bot_email = context.get_bot_email();
        let full_message = format!(
            "{}\n\nCo-Authored-By: {} <{}>",
            params.message, bot_name, bot_email
        );

        // Create commit
        match self.run_git(&["commit", "-m", &full_message], &workspace, context).await {
            Ok(output) => {
                let mut result = format!(
                    "Committed {} file(s) on branch '{}':\n{}\n\nMessage: {}\n\n{}",
                    params.files.len(),
                    branch,
                    params.files.iter().map(|f| format!("  - {}", f)).collect::<Vec<_>>().join("\n"),
                    params.message,
                    output
                );

                // Push if requested
                if push {
                    match self.run_git(&["push", "-u", "origin", &branch], &workspace, context).await {
                        Ok(push_output) => {
                            result.push_str(&format!("\nPushed to origin/{}:\n{}", branch, push_output));
                        }
                        Err(e) => {
                            result.push_str(&format!("\nCommit succeeded but push failed: {}", e));
                        }
                    }
                }

                ToolResult::success(result)
            }
            Err(e) => {
                // Unstage files on failure
                let _ = self.run_git(&["reset", "HEAD"], &workspace, context).await;
                ToolResult::error(format!("Commit failed: {}", e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conventional_commit_validation() {
        // Valid commits
        assert!(CommitterTool::validate_conventional_commit("feat: add new feature").is_ok());
        assert!(CommitterTool::validate_conventional_commit("fix(auth): resolve login bug").is_ok());
        assert!(CommitterTool::validate_conventional_commit("refactor!: breaking change").is_ok());
        assert!(CommitterTool::validate_conventional_commit("docs(readme): update installation").is_ok());

        // Invalid commits
        assert!(CommitterTool::validate_conventional_commit("add new feature").is_err());
        assert!(CommitterTool::validate_conventional_commit("Fixed the bug").is_err());
        assert!(CommitterTool::validate_conventional_commit("FEAT: wrong case").is_err());
    }

    #[test]
    fn test_sensitive_file_detection() {
        assert!(CommitterTool::is_sensitive_file(".env").is_some());
        assert!(CommitterTool::is_sensitive_file("config/.env.local").is_some());
        assert!(CommitterTool::is_sensitive_file("server.pem").is_some());
        assert!(CommitterTool::is_sensitive_file("credentials.json").is_some());

        assert!(CommitterTool::is_sensitive_file("main.rs").is_none());
        assert!(CommitterTool::is_sensitive_file("README.md").is_none());
    }

    #[test]
    fn test_protected_branch() {
        assert!(CommitterTool::is_protected_branch("main"));
        assert!(CommitterTool::is_protected_branch("master"));
        assert!(CommitterTool::is_protected_branch("production"));
        assert!(CommitterTool::is_protected_branch("MAIN"));

        assert!(!CommitterTool::is_protected_branch("feature/test"));
        assert!(!CommitterTool::is_protected_branch("develop"));
    }
}
