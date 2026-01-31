use crate::controllers::api_keys::ApiKeyId;
use crate::tools::registry::Tool;
use crate::tools::types::{
    ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::Command;

/// GitHub User tool for getting the authenticated GitHub username
/// This is more reliable than relying on environment variables set at startup
pub struct GithubUserTool {
    definition: ToolDefinition,
}

impl GithubUserTool {
    pub fn new() -> Self {
        GithubUserTool {
            definition: ToolDefinition {
                name: "github_user".to_string(),
                description: "Get the authenticated GitHub username. Call this before GitHub operations that need your username (e.g., creating repos, setting remotes).".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: HashMap::new(),
                    required: vec![],
                },
                group: ToolGroup::Development,
            },
        }
    }
}

impl Default for GithubUserTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for GithubUserTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, _params: Value, context: &ToolContext) -> ToolResult {
        // Check if already cached in context
        if let Some(cached_user) = context.extra.get("github_user") {
            if let Some(username) = cached_user.as_str() {
                if !username.is_empty() {
                    return ToolResult::success(username);
                }
            }
        }

        // Check for GitHub token FIRST - fail fast with helpful error
        let token = match context.get_api_key_by_id(ApiKeyId::GithubToken) {
            Some(t) if !t.is_empty() => t,
            _ => {
                // Also check environment as fallback
                match std::env::var("GH_TOKEN").or_else(|_| std::env::var("GITHUB_TOKEN")) {
                    Ok(t) if !t.is_empty() => t,
                    _ => {
                        return ToolResult::error(
                            "No GitHub token configured. Please add your GitHub Personal Access Token in Settings > API Keys (GITHUB_TOKEN). \
                             You can create one at https://github.com/settings/tokens with 'repo' scope."
                        );
                    }
                }
            }
        };

        // Call gh api to get the authenticated user
        let mut cmd = Command::new("gh");
        cmd.args(["api", "user", "--jq", ".login"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("GH_TOKEN", &token);

        let output = cmd.output().await;

        match output {
            Ok(output) => {
                if output.status.success() {
                    let username = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if username.is_empty() {
                        ToolResult::error("GitHub CLI returned empty username. You may need to authenticate with 'gh auth login'.")
                    } else {
                        ToolResult::success(username)
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if stderr.contains("not logged in") || stderr.contains("auth") {
                        ToolResult::error(format!(
                            "Not authenticated with GitHub. Either run 'gh auth login' or add GITHUB_TOKEN in Settings > API Keys.\nDetails: {}",
                            stderr.trim()
                        ))
                    } else {
                        ToolResult::error(format!(
                            "Failed to get GitHub username: {}",
                            stderr.trim()
                        ))
                    }
                }
            }
            Err(e) => {
                ToolResult::error(format!(
                    "Failed to execute 'gh' command. Is GitHub CLI installed?\nError: {}",
                    e
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let tool = GithubUserTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "github_user");
        assert!(def.input_schema.required.is_empty());
    }
}
