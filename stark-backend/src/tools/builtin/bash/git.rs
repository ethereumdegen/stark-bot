use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// Git tool for structured git operations
/// Provides safe git operations with protection against dangerous commands
pub struct GitTool {
    definition: ToolDefinition,
}

impl GitTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "operation".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Git operation: status, diff, log, add, commit, branch, checkout, stash, reset, push, pull, fetch, clone, remote".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "status".to_string(),
                    "diff".to_string(),
                    "log".to_string(),
                    "add".to_string(),
                    "commit".to_string(),
                    "branch".to_string(),
                    "checkout".to_string(),
                    "stash".to_string(),
                    "reset".to_string(),
                    "push".to_string(),
                    "pull".to_string(),
                    "fetch".to_string(),
                    "clone".to_string(),
                    "remote".to_string(),
                ]),
            },
        );

        properties.insert(
            "remote".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Remote name for push/pull/fetch (default: origin)".to_string(),
                default: Some(json!("origin")),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Repository URL for clone operation".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "force".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Force operation (DANGEROUS for push - uses --force-with-lease for safety)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "set_upstream".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Set upstream tracking on push (default: true)".to_string(),
                default: Some(json!(true)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "files".to_string(),
            PropertySchema {
                schema_type: "array".to_string(),
                description: "Files to operate on (for add, diff, checkout)".to_string(),
                default: Some(json!([])),
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
            "message".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Commit message (required for commit operation)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "branch".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Branch name (for checkout, branch operations)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "count".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Number of commits to show in log (default: 10)".to_string(),
                default: Some(json!(10)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "staged".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "For diff: show only staged changes (default: false)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "base_branch".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "For diff: base branch to compare from (e.g., 'master'). Use with 'branch' to compare two branches.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "create".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "For checkout: create new branch with -b flag (default: false)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        GitTool {
            definition: ToolDefinition {
                name: "git".to_string(),
                description: "Execute git operations safely. Supports: status, diff, log, add, commit, branch, checkout, stash, reset, push, pull, fetch, clone, remote. Protected branches (main, master) have safety restrictions - force push is forbidden. For safer commits with secret detection, use the 'committer' tool instead.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["operation".to_string()],
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
            "main" | "master" | "production" | "prod"
        )
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

        // Set git author from context if available
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
}

impl Default for GitTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct GitParams {
    operation: String,
    files: Option<Vec<String>>,
    message: Option<String>,
    branch: Option<String>,
    base_branch: Option<String>,
    count: Option<usize>,
    staged: Option<bool>,
    create: Option<bool>,
    remote: Option<String>,
    url: Option<String>,
    force: Option<bool>,
    set_upstream: Option<bool>,
}

#[async_trait]
impl Tool for GitTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: GitParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Check if user is asking about an EXTERNAL GitHub repo
        // If so, redirect them to use gh CLI instead of local git commands
        let github_urls: Vec<_> = context.context_bank.items()
            .into_iter()
            .filter(|item| item.item_type == "github_url")
            .collect();

        if !github_urls.is_empty() {
            // For read-only operations (log, status, diff without files), reject and redirect
            let is_read_only = matches!(params.operation.as_str(), "log" | "status")
                || (params.operation == "diff" && params.files.is_none());

            if is_read_only {
                let url = &github_urls[0].value;
                let owner_repo = github_urls[0].label.as_deref().unwrap_or("owner/repo");
                return ToolResult::error(format!(
                    "WRONG TOOL: You're trying to inspect an EXTERNAL GitHub repo ({}).\n\n\
                    The `git` tool only works on your LOCAL workspace, not external repos!\n\n\
                    For external repos, use the `exec` tool with `gh` CLI:\n\
                    - View repo: {{\"tool\": \"exec\", \"command\": \"gh repo view {}\"}}\n\
                    - View commits: {{\"tool\": \"exec\", \"command\": \"gh api repos/{}/commits --jq '.[].commit.message'\"}}\n\
                    - View README: {{\"tool\": \"exec\", \"command\": \"gh api repos/{}/readme --jq .content | base64 -d\"}}\n\
                    - Or use web_fetch on the URL directly",
                    url, owner_repo, owner_repo, owner_repo
                ));
            }
        }

        // Get workspace directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        match params.operation.as_str() {
            "status" => {
                match self.run_git(&["status", "--porcelain=v1"], &workspace, context).await {
                    Ok(output) => {
                        if output.is_empty() {
                            ToolResult::success("Working tree is clean - no changes to commit.")
                        } else {
                            // Also show branch info
                            let branch = self.run_git(&["branch", "--show-current"], &workspace, context).await
                                .unwrap_or_else(|_| "unknown".to_string());
                            ToolResult::success(format!(
                                "On branch: {}\n\nChanges:\n{}",
                                branch.trim(),
                                output
                            ))
                        }
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "diff" => {
                let mut args = vec!["diff"];

                // Branch comparison: git diff base_branch..branch
                let branch_range: Option<String> = match (&params.base_branch, &params.branch) {
                    (Some(base), Some(target)) => Some(format!("{}..{}", base, target)),
                    (Some(base), None) => Some(base.clone()), // Compare current HEAD to base
                    (None, Some(target)) => Some(target.clone()), // Compare working tree to target
                    (None, None) => None,
                };

                if let Some(ref range) = branch_range {
                    args.push(range.as_str());
                } else if params.staged.unwrap_or(false) {
                    args.push("--staged");
                }

                if let Some(ref files) = params.files {
                    args.push("--");
                    for f in files {
                        args.push(f.as_str());
                    }
                }
                match self.run_git(&args, &workspace, context).await {
                    Ok(output) => {
                        if output.is_empty() {
                            ToolResult::success("No differences found.")
                        } else {
                            // Truncate if too long
                            let max_output = 30000;
                            if output.len() > max_output {
                                ToolResult::success(format!(
                                    "{}\n\n[Output truncated. {} more characters not shown.]",
                                    &output[..max_output],
                                    output.len() - max_output
                                ))
                            } else {
                                ToolResult::success(output)
                            }
                        }
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "log" => {
                let count = params.count.unwrap_or(10);
                let count_str = format!("-{}", count);
                match self
                    .run_git(
                        &["log", &count_str, "--oneline", "--decorate"],
                        &workspace,
                        context,
                    )
                    .await
                {
                    Ok(output) => ToolResult::success(output),
                    Err(e) => ToolResult::error(e),
                }
            }

            "add" => {
                let files = params.files.unwrap_or_default();
                if files.is_empty() {
                    return ToolResult::error(
                        "No files specified. Use 'files' parameter with specific file paths. Avoid using '.' to prevent staging sensitive files.",
                    );
                }

                // Check for dangerous patterns
                if files.iter().any(|f| f == "." || f == "-A" || f == "--all") {
                    return ToolResult::error(
                        "Using 'git add .' or '-A' is not allowed. Please specify files individually to avoid staging sensitive files (.env, credentials, etc.).",
                    );
                }

                let mut args = vec!["add"];
                for f in &files {
                    args.push(f.as_str());
                }
                match self.run_git(&args, &workspace, context).await {
                    Ok(_) => ToolResult::success(format!("Staged {} file(s): {}", files.len(), files.join(", "))),
                    Err(e) => ToolResult::error(e),
                }
            }

            "commit" => {
                let message = match params.message {
                    Some(m) if !m.is_empty() => m,
                    _ => return ToolResult::error("Commit message is required"),
                };

                // Create commit
                match self
                    .run_git(&["commit", "-m", &message], &workspace, context)
                    .await
                {
                    Ok(output) => ToolResult::success(format!("Committed:\n{}", output)),
                    Err(e) => ToolResult::error(e),
                }
            }

            "branch" => {
                if let Some(ref branch) = params.branch {
                    // Create new branch
                    match self.run_git(&["branch", branch], &workspace, context).await {
                        Ok(_) => ToolResult::success(format!("Created branch: {}", branch)),
                        Err(e) => ToolResult::error(e),
                    }
                } else {
                    // List branches
                    match self.run_git(&["branch", "-a"], &workspace, context).await {
                        Ok(output) => ToolResult::success(format!("Branches:\n{}", output)),
                        Err(e) => ToolResult::error(e),
                    }
                }
            }

            "checkout" => {
                let branch = match params.branch {
                    Some(b) => b,
                    None => return ToolResult::error("Branch name is required for checkout"),
                };

                // Check for file checkout (restore)
                if let Some(ref files) = params.files {
                    if !files.is_empty() {
                        // Safety: don't allow checking out all files
                        if files.iter().any(|f| f == "." || f == "*") {
                            return ToolResult::error(
                                "Checking out all files with '.' is destructive. Please specify individual files.",
                            );
                        }
                        let mut args = vec!["checkout", branch.as_str(), "--"];
                        for f in files {
                            args.push(f.as_str());
                        }
                        return match self.run_git(&args, &workspace, context).await {
                            Ok(_) => ToolResult::success(format!("Restored {} file(s) from {}", files.len(), branch)),
                            Err(e) => ToolResult::error(e),
                        };
                    }
                }

                // Branch checkout
                let create = params.create.unwrap_or(false);
                let args = if create {
                    vec!["checkout", "-b", branch.as_str()]
                } else {
                    vec!["checkout", branch.as_str()]
                };
                match self.run_git(&args, &workspace, context).await {
                    Ok(_) => ToolResult::success(format!(
                        "{} branch: {}",
                        if create { "Created and switched to" } else { "Switched to" },
                        branch
                    )),
                    Err(e) => ToolResult::error(e),
                }
            }

            "stash" => {
                let action = params.branch.as_deref().unwrap_or("push");
                match action {
                    "push" | "save" => {
                        let message = params.message.as_deref().unwrap_or("WIP");
                        match self.run_git(&["stash", "push", "-m", message], &workspace, context).await {
                            Ok(_) => ToolResult::success(format!("Stashed changes: {}", message)),
                            Err(e) => ToolResult::error(e),
                        }
                    }
                    "pop" => {
                        match self.run_git(&["stash", "pop"], &workspace, context).await {
                            Ok(output) => ToolResult::success(format!("Popped stash:\n{}", output)),
                            Err(e) => ToolResult::error(e),
                        }
                    }
                    "list" => {
                        match self.run_git(&["stash", "list"], &workspace, context).await {
                            Ok(output) => {
                                if output.is_empty() {
                                    ToolResult::success("No stashed changes.")
                                } else {
                                    ToolResult::success(format!("Stash list:\n{}", output))
                                }
                            }
                            Err(e) => ToolResult::error(e),
                        }
                    }
                    _ => ToolResult::error(format!("Unknown stash action: {}. Use 'push', 'pop', or 'list'.", action)),
                }
            }

            "reset" => {
                // Safety: only allow soft reset
                let files = params.files.unwrap_or_default();
                if files.is_empty() {
                    return ToolResult::error(
                        "Reset requires specific files. Hard reset is not allowed through this tool for safety.",
                    );
                }

                // Unstage specific files
                let mut args = vec!["reset", "HEAD", "--"];
                for f in &files {
                    args.push(f.as_str());
                }
                match self.run_git(&args, &workspace, context).await {
                    Ok(_) => ToolResult::success(format!("Unstaged {} file(s)", files.len())),
                    Err(e) => ToolResult::error(e),
                }
            }

            "push" => {
                let remote = params.remote.as_deref().unwrap_or("origin");
                let force = params.force.unwrap_or(false);
                let set_upstream = params.set_upstream.unwrap_or(true);

                // Get current branch if not specified
                let branch = match &params.branch {
                    Some(b) => b.clone(),
                    None => {
                        match self.run_git(&["branch", "--show-current"], &workspace, context).await {
                            Ok(b) => b.trim().to_string(),
                            Err(e) => return ToolResult::error(format!("Failed to get current branch: {}", e)),
                        }
                    }
                };

                // SAFETY: Never allow force push to protected branches
                if force && Self::is_protected_branch(&branch) {
                    return ToolResult::error(format!(
                        "SAFETY: Force push to protected branch '{}' is FORBIDDEN. This could destroy commit history and affect other developers.",
                        branch
                    ));
                }

                // Check for uncommitted changes
                match self.run_git(&["status", "--porcelain"], &workspace, context).await {
                    Ok(output) if !output.is_empty() => {
                        return ToolResult::error(
                            "Uncommitted changes detected. Please commit or stash changes before pushing."
                        );
                    }
                    Err(e) => return ToolResult::error(format!("Failed to check status: {}", e)),
                    _ => {}
                }

                let mut args = vec!["push"];
                if set_upstream {
                    args.push("-u");
                }
                if force {
                    // Use --force-with-lease instead of --force for safety
                    // This prevents overwriting commits that others have pushed
                    args.push("--force-with-lease");
                }
                args.push(remote);
                args.push(&branch);

                match self.run_git(&args, &workspace, context).await {
                    Ok(output) => {
                        let result = if output.is_empty() {
                            format!("Pushed branch '{}' to {}/{}", branch, remote, branch)
                        } else {
                            format!("Pushed to {}/{}:\n{}", remote, branch, output)
                        };
                        ToolResult::success(result)
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "pull" => {
                let remote = params.remote.as_deref().unwrap_or("origin");
                let branch = match &params.branch {
                    Some(b) => b.clone(),
                    None => {
                        match self.run_git(&["branch", "--show-current"], &workspace, context).await {
                            Ok(b) => b.trim().to_string(),
                            Err(e) => return ToolResult::error(format!("Failed to get current branch: {}", e)),
                        }
                    }
                };

                // Use rebase to keep history clean
                match self.run_git(&["pull", "--rebase", remote, &branch], &workspace, context).await {
                    Ok(output) => {
                        if output.contains("Already up to date") {
                            ToolResult::success("Already up to date.")
                        } else {
                            ToolResult::success(format!("Pulled {}/{} with rebase:\n{}", remote, branch, output))
                        }
                    }
                    Err(e) => {
                        if e.contains("conflict") {
                            ToolResult::error(format!(
                                "Merge conflict detected. Please resolve conflicts manually:\n{}\n\nAfter resolving, run: git rebase --continue",
                                e
                            ))
                        } else {
                            ToolResult::error(e)
                        }
                    }
                }
            }

            "fetch" => {
                let remote = params.remote.as_deref().unwrap_or("origin");

                let args = if let Some(branch) = &params.branch {
                    vec!["fetch", remote, branch]
                } else {
                    vec!["fetch", remote, "--prune"]
                };

                match self.run_git(&args, &workspace, context).await {
                    Ok(output) => {
                        let result = if output.is_empty() {
                            format!("Fetched from {} (no new changes)", remote)
                        } else {
                            format!("Fetched from {}:\n{}", remote, output)
                        };
                        ToolResult::success(result)
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "clone" => {
                let url = match &params.url {
                    Some(u) => u,
                    None => return ToolResult::error("URL is required for clone operation"),
                };

                // Extract repo name from URL for the target directory
                let repo_name = url
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("repo")
                    .trim_end_matches(".git");

                let target_dir = workspace.join(repo_name);

                if target_dir.exists() {
                    return ToolResult::error(format!(
                        "Directory '{}' already exists. Please remove it or use a different location.",
                        repo_name
                    ));
                }

                // Clone with depth 1 by default for faster cloning (can be full cloned later if needed)
                match self.run_git(&["clone", "--depth", "1", url, repo_name], &workspace, context).await {
                    Ok(output) => {
                        ToolResult::success(format!(
                            "Cloned {} to {}:\n{}\n\nNote: Cloned with --depth 1. Run 'git fetch --unshallow' for full history.",
                            url, repo_name, output
                        ))
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "remote" => {
                // List or manage remotes
                if let Some(url) = &params.url {
                    // Add remote
                    let remote_name = params.remote.as_deref().unwrap_or("origin");
                    match self.run_git(&["remote", "add", remote_name, url], &workspace, context).await {
                        Ok(_) => ToolResult::success(format!("Added remote '{}' -> {}", remote_name, url)),
                        Err(e) => {
                            // Try to set-url if remote already exists
                            if e.contains("already exists") {
                                match self.run_git(&["remote", "set-url", remote_name, url], &workspace, context).await {
                                    Ok(_) => ToolResult::success(format!("Updated remote '{}' -> {}", remote_name, url)),
                                    Err(e) => ToolResult::error(e),
                                }
                            } else {
                                ToolResult::error(e)
                            }
                        }
                    }
                } else {
                    // List remotes
                    match self.run_git(&["remote", "-v"], &workspace, context).await {
                        Ok(output) => {
                            if output.is_empty() {
                                ToolResult::success("No remotes configured.")
                            } else {
                                ToolResult::success(format!("Remotes:\n{}", output))
                            }
                        }
                        Err(e) => ToolResult::error(e),
                    }
                }
            }

            _ => ToolResult::error(format!(
                "Unknown operation: {}. Supported: status, diff, log, add, commit, branch, checkout, stash, reset, push, pull, fetch, clone, remote",
                params.operation
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_git_status() {
        let tool = GitTool::new();
        let temp_dir = TempDir::new().unwrap();

        // Initialize git repo
        Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .output()
            .await
            .unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        let result = tool
            .execute(json!({ "operation": "status" }), &context)
            .await;

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_git_add_safety() {
        let tool = GitTool::new();
        let temp_dir = TempDir::new().unwrap();

        let context =
            ToolContext::new().with_workspace(temp_dir.path().to_string_lossy().to_string());

        // Should reject 'git add .'
        let result = tool
            .execute(json!({ "operation": "add", "files": ["."] }), &context)
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("not allowed"));
    }

    #[test]
    fn test_protected_branch() {
        assert!(GitTool::is_protected_branch("main"));
        assert!(GitTool::is_protected_branch("master"));
        assert!(GitTool::is_protected_branch("MAIN"));
        assert!(!GitTool::is_protected_branch("feature/test"));
    }
}
