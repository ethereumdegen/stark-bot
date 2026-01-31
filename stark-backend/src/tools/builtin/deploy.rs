use crate::controllers::api_keys::ApiKeyId;
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

/// Deploy Tool - Git push, PR creation, and CI/CD monitoring
///
/// This tool provides deployment capabilities:
/// - Push to remote repositories (with safety checks)
/// - Create pull requests via GitHub CLI
/// - Monitor CI/CD workflow runs
/// - Trigger deployments
/// - Check deployment status
pub struct DeployTool {
    definition: ToolDefinition,
}

impl DeployTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "operation".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Deploy operation: push, pull, fetch, create_pr, pr_status, workflow_status, trigger_deploy, merge_pr".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "push".to_string(),
                    "pull".to_string(),
                    "fetch".to_string(),
                    "create_pr".to_string(),
                    "pr_status".to_string(),
                    "workflow_status".to_string(),
                    "trigger_deploy".to_string(),
                    "merge_pr".to_string(),
                ]),
            },
        );

        properties.insert(
            "remote".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Remote name (default: origin)".to_string(),
                default: Some(json!("origin")),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "branch".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Branch name for push/pull operations".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "base_branch".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Base branch for PR (default: main)".to_string(),
                default: Some(json!("main")),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "title".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "PR title (for create_pr operation)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "body".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "PR body/description (for create_pr operation)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "pr_number".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "PR number (for pr_status, merge_pr operations)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "workflow_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Workflow name or ID (for workflow_status, trigger_deploy)".to_string(),
                default: None,
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
            "force".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Force push (DANGEROUS - requires explicit confirmation, never allowed on protected branches)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "draft".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Create PR as draft (default: false)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "auto_merge".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Enable auto-merge when checks pass (for merge_pr)".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        DeployTool {
            definition: ToolDefinition {
                name: "deploy".to_string(),
                description: "Deployment operations: push code, create/manage PRs, monitor CI/CD workflows. Integrates with GitHub CLI for full deployment lifecycle management.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["operation".to_string()],
                },
                group: ToolGroup::Development,
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

    /// Run a git command and return output
    async fn run_git(
        &self,
        args: &[&str],
        workspace: &PathBuf,
        context: &ToolContext,
    ) -> Result<String, String> {
        // Check if we have a GitHub token for authentication
        let github_token = context.get_api_key_by_id(ApiKeyId::GithubToken);

        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Prevent git from prompting for credentials interactively
        cmd.env("GIT_TERMINAL_PROMPT", "0");

        // Set git author from context
        let bot_name = context.get_bot_name();
        let bot_email = context.get_bot_email();
        cmd.env("GIT_AUTHOR_NAME", &bot_name);
        cmd.env("GIT_AUTHOR_EMAIL", &bot_email);
        cmd.env("GIT_COMMITTER_NAME", &bot_name);
        cmd.env("GIT_COMMITTER_EMAIL", &bot_email);

        // Configure GitHub authentication via URL rewriting
        // This is more reliable than credential helpers
        if let Some(ref token) = github_token {
            cmd.env("GH_TOKEN", token);
            cmd.env("GITHUB_TOKEN", token);
            // Use GIT_CONFIG_* env vars to rewrite GitHub URLs to include token
            cmd.env("GIT_CONFIG_COUNT", "2");
            cmd.env(
                "GIT_CONFIG_KEY_0",
                format!("url.https://x-access-token:{}@github.com/.insteadOf", token),
            );
            cmd.env("GIT_CONFIG_VALUE_0", "https://github.com/");
            cmd.env(
                "GIT_CONFIG_KEY_1",
                format!("url.https://x-access-token:{}@github.com/.insteadOf", token),
            );
            cmd.env("GIT_CONFIG_VALUE_1", "git@github.com:");
        }

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

    /// Run gh CLI command
    async fn run_gh(
        &self,
        args: &[&str],
        workspace: &PathBuf,
        context: &ToolContext,
    ) -> Result<String, String> {
        let mut cmd = Command::new("gh");
        cmd.args(args)
            .current_dir(workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set GitHub token if available
        if let Some(token) = context.get_api_key_by_id(ApiKeyId::GithubToken) {
            cmd.env("GH_TOKEN", token);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("Failed to execute gh CLI: {}. Is GitHub CLI installed?", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(format!(
                "GitHub CLI failed:\n{}{}",
                stdout,
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", stderr)
                }
            ));
        }

        Ok(stdout.to_string())
    }

    /// Get current branch name
    async fn get_current_branch(&self, workspace: &PathBuf, context: &ToolContext) -> Result<String, String> {
        self.run_git(&["branch", "--show-current"], workspace, context)
            .await
            .map(|s| s.trim().to_string())
    }

    /// Check if there are uncommitted changes
    async fn has_uncommitted_changes(&self, workspace: &PathBuf, context: &ToolContext) -> Result<bool, String> {
        let output = self.run_git(&["status", "--porcelain"], workspace, context).await?;
        Ok(!output.is_empty())
    }
}

impl Default for DeployTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct DeployParams {
    operation: String,
    remote: Option<String>,
    branch: Option<String>,
    base_branch: Option<String>,
    title: Option<String>,
    body: Option<String>,
    pr_number: Option<i64>,
    workflow_name: Option<String>,
    set_upstream: Option<bool>,
    force: Option<bool>,
    draft: Option<bool>,
    auto_merge: Option<bool>,
}

#[async_trait]
impl Tool for DeployTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: DeployParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let remote = params.remote.as_deref().unwrap_or("origin");
        let force = params.force.unwrap_or(false);
        let set_upstream = params.set_upstream.unwrap_or(true);

        // Get workspace directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        match params.operation.as_str() {
            "push" => {
                let branch = match &params.branch {
                    Some(b) => b.clone(),
                    None => match self.get_current_branch(&workspace, context).await {
                        Ok(b) => b,
                        Err(e) => return ToolResult::error(format!("Failed to get current branch: {}", e)),
                    },
                };

                // Safety check: never force push to protected branches
                if force && Self::is_protected_branch(&branch) {
                    return ToolResult::error(format!(
                        "SAFETY: Force push to protected branch '{}' is not allowed. This could destroy commit history.",
                        branch
                    ));
                }

                // Check for uncommitted changes
                if let Ok(true) = self.has_uncommitted_changes(&workspace, context).await {
                    return ToolResult::error(
                        "Uncommitted changes detected. Please commit or stash changes before pushing."
                    );
                }

                let mut args = vec!["push"];
                if set_upstream {
                    args.push("-u");
                }
                if force {
                    args.push("--force-with-lease"); // Safer than --force
                }
                args.push(remote);
                args.push(&branch);

                match self.run_git(&args, &workspace, context).await {
                    Ok(output) => {
                        let result = if output.is_empty() {
                            format!("Pushed branch '{}' to {}/{}", branch, remote, branch)
                        } else {
                            output
                        };
                        ToolResult::success(result)
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "pull" => {
                let branch = match &params.branch {
                    Some(b) => b.clone(),
                    None => match self.get_current_branch(&workspace, context).await {
                        Ok(b) => b,
                        Err(e) => return ToolResult::error(format!("Failed to get current branch: {}", e)),
                    },
                };

                match self.run_git(&["pull", remote, &branch, "--rebase"], &workspace, context).await {
                    Ok(output) => ToolResult::success(format!("Pulled {}/{} with rebase:\n{}", remote, branch, output)),
                    Err(e) => ToolResult::error(e),
                }
            }

            "fetch" => {
                let args = if let Some(branch) = &params.branch {
                    vec!["fetch", remote, branch]
                } else {
                    vec!["fetch", remote, "--prune"]
                };

                match self.run_git(&args, &workspace, context).await {
                    Ok(output) => {
                        let result = if output.is_empty() {
                            format!("Fetched from {}", remote)
                        } else {
                            output
                        };
                        ToolResult::success(result)
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "create_pr" => {
                let title = match &params.title {
                    Some(t) => t.clone(),
                    None => return ToolResult::error("PR title is required"),
                };

                let base = params.base_branch.as_deref().unwrap_or("main");
                let draft = params.draft.unwrap_or(false);

                let branch = match self.get_current_branch(&workspace, context).await {
                    Ok(b) => b,
                    Err(e) => return ToolResult::error(format!("Failed to get current branch: {}", e)),
                };

                if branch == base {
                    return ToolResult::error(format!(
                        "Cannot create PR from '{}' to itself. Please create a feature branch first.",
                        base
                    ));
                }

                // Push branch first
                if let Err(e) = self.run_git(&["push", "-u", remote, &branch], &workspace, context).await {
                    return ToolResult::error(format!("Failed to push branch before creating PR: {}", e));
                }

                let mut args = vec!["pr", "create", "--title", &title, "--base", base];

                if let Some(body) = &params.body {
                    args.push("--body");
                    args.push(body);
                }

                if draft {
                    args.push("--draft");
                }

                match self.run_gh(&args, &workspace, context).await {
                    Ok(output) => ToolResult::success(format!(
                        "Created PR: {} -> {}\n{}",
                        branch, base, output
                    )),
                    Err(e) => ToolResult::error(e),
                }
            }

            "pr_status" => {
                if let Some(pr_num) = params.pr_number {
                    let pr_num_str = pr_num.to_string();
                    match self.run_gh(
                        &["pr", "view", &pr_num_str, "--json", "number,title,state,mergeable,statusCheckRollup,reviews"],
                        &workspace,
                        context
                    ).await {
                        Ok(output) => ToolResult::success(output),
                        Err(e) => ToolResult::error(e),
                    }
                } else {
                    match self.run_gh(&["pr", "status"], &workspace, context).await {
                        Ok(output) => ToolResult::success(output),
                        Err(e) => ToolResult::error(e),
                    }
                }
            }

            "workflow_status" => {
                let args = if let Some(workflow) = &params.workflow_name {
                    vec!["run", "list", "--workflow", workflow, "--limit", "5", "--json", "databaseId,displayTitle,status,conclusion,createdAt"]
                } else {
                    vec!["run", "list", "--limit", "10", "--json", "databaseId,displayTitle,status,conclusion,createdAt,workflowName"]
                };

                match self.run_gh(&args, &workspace, context).await {
                    Ok(output) => ToolResult::success(output),
                    Err(e) => ToolResult::error(e),
                }
            }

            "trigger_deploy" => {
                let workflow = match &params.workflow_name {
                    Some(w) => w.clone(),
                    None => return ToolResult::error("workflow_name is required for trigger_deploy"),
                };

                let branch = match &params.branch {
                    Some(b) => b.clone(),
                    None => match self.get_current_branch(&workspace, context).await {
                        Ok(b) => b,
                        Err(e) => return ToolResult::error(format!("Failed to get current branch: {}", e)),
                    },
                };

                match self.run_gh(
                    &["workflow", "run", &workflow, "--ref", &branch],
                    &workspace,
                    context
                ).await {
                    Ok(output) => {
                        let result = if output.is_empty() {
                            format!("Triggered workflow '{}' on branch '{}'", workflow, branch)
                        } else {
                            output
                        };
                        ToolResult::success(result)
                    }
                    Err(e) => ToolResult::error(e),
                }
            }

            "merge_pr" => {
                let pr_num = match params.pr_number {
                    Some(n) => n,
                    None => return ToolResult::error("pr_number is required for merge_pr"),
                };

                let auto_merge = params.auto_merge.unwrap_or(false);

                if auto_merge {
                    // Enable auto-merge (waits for checks to pass)
                    match self.run_gh(
                        &["pr", "merge", &pr_num.to_string(), "--auto", "--squash"],
                        &workspace,
                        context
                    ).await {
                        Ok(output) => ToolResult::success(format!("Enabled auto-merge for PR #{}\n{}", pr_num, output)),
                        Err(e) => ToolResult::error(e),
                    }
                } else {
                    // Immediate merge (squash by default)
                    match self.run_gh(
                        &["pr", "merge", &pr_num.to_string(), "--squash", "--delete-branch"],
                        &workspace,
                        context
                    ).await {
                        Ok(output) => ToolResult::success(format!("Merged PR #{} (squash)\n{}", pr_num, output)),
                        Err(e) => ToolResult::error(e),
                    }
                }
            }

            _ => ToolResult::error(format!(
                "Unknown operation: {}. Supported: push, pull, fetch, create_pr, pr_status, workflow_status, trigger_deploy, merge_pr",
                params.operation
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protected_branch() {
        assert!(DeployTool::is_protected_branch("main"));
        assert!(DeployTool::is_protected_branch("master"));
        assert!(DeployTool::is_protected_branch("production"));
        assert!(!DeployTool::is_protected_branch("feature/test"));
    }
}
