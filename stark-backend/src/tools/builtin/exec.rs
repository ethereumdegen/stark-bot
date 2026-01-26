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
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Command execution tool with configurable security
pub struct ExecTool {
    definition: ToolDefinition,
    /// Maximum execution time in seconds
    max_timeout: u64,
    /// Security mode: "full" (shell allowed), "restricted" (no shell), "sandbox" (future)
    security_mode: String,
}

impl ExecTool {
    pub fn new() -> Self {
        Self::with_config(300, "full".to_string())
    }

    pub fn with_config(max_timeout: u64, security_mode: String) -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "command".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The shell command to execute. Can include pipes, redirects, and shell features.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "workdir".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Working directory for command execution (defaults to workspace)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "timeout".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: format!(
                    "Timeout in seconds (default: 60, max: {})",
                    max_timeout
                ),
                default: Some(json!(60)),
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "env".to_string(),
            PropertySchema {
                schema_type: "object".to_string(),
                description: "Environment variables to set for the command".to_string(),
                default: Some(json!({})),
                items: None,
                enum_values: None,
            },
        );

        ExecTool {
            definition: ToolDefinition {
                name: "exec".to_string(),
                description: "Execute a shell command in the workspace. Supports full shell syntax including pipes, redirects, and command chaining. Use for running CLI tools, scripts, and system commands.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["command".to_string()],
                },
                group: ToolGroup::Exec,
            },
            max_timeout,
            security_mode,
        }
    }

    /// Check if a command should be blocked for security
    fn is_dangerous_command(&self, command: &str) -> Option<String> {
        let lower = command.to_lowercase();

        // Block commands that could damage the system
        let dangerous_patterns = [
            ("rm -rf /", "Attempted to delete root filesystem"),
            ("rm -rf /*", "Attempted to delete root filesystem"),
            ("mkfs", "Filesystem formatting not allowed"),
            ("dd if=", "Raw disk operations not allowed"),
            (":(){:|:&};:", "Fork bomb detected"),
            ("chmod -R 777 /", "Dangerous permission change"),
            ("shutdown", "System shutdown not allowed"),
            ("reboot", "System reboot not allowed"),
            ("init 0", "System halt not allowed"),
            ("init 6", "System reboot not allowed"),
        ];

        for (pattern, msg) in dangerous_patterns {
            if lower.contains(pattern) {
                return Some(msg.to_string());
            }
        }

        // In restricted mode, block shell metacharacters
        if self.security_mode == "restricted" {
            let dangerous_chars = ['|', ';', '&', '$', '`', '(', ')', '<', '>'];
            if command.chars().any(|c| dangerous_chars.contains(&c)) {
                return Some("Shell metacharacters not allowed in restricted mode".to_string());
            }
        }

        None
    }
}

impl Default for ExecTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ExecParams {
    command: String,
    workdir: Option<String>,
    timeout: Option<u64>,
    env: Option<HashMap<String, String>>,
}

#[async_trait]
impl Tool for ExecTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ExecParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Check for dangerous commands
        if let Some(reason) = self.is_dangerous_command(&params.command) {
            return ToolResult::error(format!("Command blocked: {}", reason));
        }

        let timeout_secs = params.timeout.unwrap_or(60).min(self.max_timeout);

        // Determine working directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let working_dir = if let Some(ref wd) = params.workdir {
            let wd_path = PathBuf::from(wd);
            if wd_path.is_absolute() {
                wd_path
            } else {
                workspace.join(wd_path)
            }
        } else {
            workspace.clone()
        };

        // Ensure working directory exists
        if !working_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&working_dir) {
                return ToolResult::error(format!("Cannot create working directory: {}", e));
            }
        }

        // Build the command using shell
        let shell = if cfg!(target_os = "windows") {
            "cmd"
        } else {
            "sh"
        };

        let shell_arg = if cfg!(target_os = "windows") {
            "/C"
        } else {
            "-c"
        };

        let mut cmd = Command::new(shell);
        cmd.arg(shell_arg)
            .arg(&params.command)
            .current_dir(&working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set environment variables from context (API keys)
        if let Some(github_token) = context.get_api_key("github") {
            cmd.env("GH_TOKEN", &github_token);
            cmd.env("GITHUB_TOKEN", &github_token);
            // Disable git terminal prompts (would hang in non-interactive mode)
            cmd.env("GIT_TERMINAL_PROMPT", "0");
            // Configure git to rewrite github HTTPS URLs to include the token
            // This allows git clone/push to authenticate automatically
            cmd.env("GIT_CONFIG_COUNT", "2");
            cmd.env("GIT_CONFIG_KEY_0", "url.https://x-access-token:".to_string() + &github_token + "@github.com/.insteadOf");
            cmd.env("GIT_CONFIG_VALUE_0", "https://github.com/");
            cmd.env("GIT_CONFIG_KEY_1", "url.https://x-access-token:".to_string() + &github_token + "@github.com/.insteadOf");
            cmd.env("GIT_CONFIG_VALUE_1", "git@github.com:");
            // Set git author/committer info for commits (from bot config)
            let bot_name = context.get_bot_name();
            let bot_email = context.get_bot_email();
            cmd.env("GIT_AUTHOR_NAME", &bot_name);
            cmd.env("GIT_AUTHOR_EMAIL", &bot_email);
            cmd.env("GIT_COMMITTER_NAME", &bot_name);
            cmd.env("GIT_COMMITTER_EMAIL", &bot_email);
        }

        if let Some(openai_key) = context.get_api_key("openai") {
            cmd.env("OPENAI_API_KEY", &openai_key);
        }

        // Set custom environment variables from params
        if let Some(ref env_vars) = params.env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        // Execute with timeout
        let start = std::time::Instant::now();
        log::info!("Executing command: {} (timeout: {}s, workdir: {:?})",
            params.command, timeout_secs, working_dir);

        let output = match timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return ToolResult::error(format!("Failed to execute command: {}", e)),
            Err(_) => {
                return ToolResult::error(format!(
                    "Command timed out after {} seconds. Consider increasing timeout or running in background.",
                    timeout_secs
                ))
            }
        };
        let duration_ms = start.elapsed().as_millis() as i64;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        // Build response
        let success = output.status.success();
        let mut result_text = String::new();

        if !stdout.is_empty() {
            result_text.push_str(&stdout);
        }

        if !stderr.is_empty() {
            if !result_text.is_empty() {
                result_text.push_str("\n--- stderr ---\n");
            }
            result_text.push_str(&stderr);
        }

        if result_text.is_empty() {
            result_text = if success {
                format!("Command completed successfully (exit code: {})", exit_code)
            } else {
                format!("Command failed with exit code: {}", exit_code)
            };
        }

        // Truncate if too long
        const MAX_OUTPUT: usize = 50000;
        if result_text.len() > MAX_OUTPUT {
            result_text = format!(
                "{}\n\n[Output truncated at {} characters]",
                &result_text[..MAX_OUTPUT],
                MAX_OUTPUT
            );
        }

        log::info!("Command completed: exit_code={}, duration={}ms, output_len={}",
            exit_code, duration_ms, result_text.len());

        let result = if success {
            ToolResult::success(result_text)
        } else {
            ToolResult::error(result_text)
        };

        result.with_metadata(json!({
            "command": params.command,
            "exit_code": exit_code,
            "duration_ms": duration_ms,
            "working_dir": working_dir.to_string_lossy()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dangerous_command_detection() {
        let tool = ExecTool::new();

        assert!(tool.is_dangerous_command("rm -rf /").is_some());
        assert!(tool.is_dangerous_command("mkfs.ext4 /dev/sda").is_some());
        assert!(tool.is_dangerous_command(":(){:|:&};:").is_some());

        // Safe commands
        assert!(tool.is_dangerous_command("ls -la").is_none());
        assert!(tool.is_dangerous_command("curl wttr.in").is_none());
        assert!(tool.is_dangerous_command("echo hello | grep hello").is_none());
    }

    #[test]
    fn test_restricted_mode() {
        let tool = ExecTool::with_config(60, "restricted".to_string());

        // Shell metacharacters blocked in restricted mode
        assert!(tool.is_dangerous_command("echo hello | grep hello").is_some());
        assert!(tool.is_dangerous_command("ls; pwd").is_some());

        // Simple commands allowed
        assert!(tool.is_dangerous_command("ls -la").is_none());
    }

    #[tokio::test]
    async fn test_exec_simple_command() {
        let tool = ExecTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "command": "echo hello world"
                }),
                &context,
            )
            .await;

        assert!(result.success);
        assert!(result.content.contains("hello world"));
    }

    #[tokio::test]
    async fn test_exec_with_pipes() {
        let tool = ExecTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "command": "echo 'hello world' | tr 'a-z' 'A-Z'"
                }),
                &context,
            )
            .await;

        assert!(result.success);
        assert!(result.content.contains("HELLO WORLD"));
    }
}
