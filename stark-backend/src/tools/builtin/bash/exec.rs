use crate::controllers::api_keys::ApiKeyId;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::str::FromStr;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Deserialize a u64 from either a number or a string
fn deserialize_u64_lenient<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<Value> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(Value::Number(n)) => Ok(n.as_u64()),
        Some(Value::String(s)) => Ok(s.parse().ok()),
        _ => Ok(None),
    }
}

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
        properties.insert(
            "background".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Run command in background. Returns immediately with process ID. Use for long-running commands like servers.".to_string(),
                default: Some(json!(false)),
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
                hidden: false,
            },
            max_timeout,
            security_mode,
        }
    }

    /// Check if a command looks like a server/long-running process
    fn is_server_command(command: &str) -> bool {
        let lower = command.to_lowercase();
        let server_patterns = [
            "npm start",
            "npm run dev",
            "npm run serve",
            "npm run server",
            "yarn start",
            "yarn dev",
            "pnpm start",
            "pnpm dev",
            "node index.js",
            "node server.js",
            "node app.js",
            "node src/index",
            "python -m http.server",
            "python manage.py runserver",
            "python -m flask run",
            "flask run",
            "uvicorn",
            "gunicorn",
            "cargo run",
            "cargo watch",
            "go run",
            "rails server",
            "rails s",
            "php artisan serve",
            "php -S",
            "dotnet run",
            "java -jar",
            "gradle bootRun",
            "mvn spring-boot:run",
        ];
        server_patterns.iter().any(|p| lower.contains(p))
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

        // Block interactive commands that require user input
        if let Some(msg) = Self::is_interactive_command(&lower) {
            return Some(msg);
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

    /// Check if a command is interactive (requires user input)
    fn is_interactive_command(command: &str) -> Option<String> {
        // Helper to check if a pattern appears as an actual command (not inside quotes)
        fn is_command_pattern(command: &str, pattern: &str) -> bool {
            // Check if pattern appears at start of command or after command separators
            // This avoids false positives when the pattern appears inside quoted strings
            let cmd = command.trim();

            // Check if command starts with the pattern
            if cmd.starts_with(pattern) {
                return true;
            }

            // Check if pattern appears after command separators (|, ;, &&, ||)
            // but NOT inside quoted strings
            let separators = [" | ", " || ", " && ", "; "];
            for sep in separators {
                for part in cmd.split(sep) {
                    if part.trim().starts_with(pattern) {
                        return true;
                    }
                }
            }

            false
        }

        // Commands that prompt for interactive input
        // Format: (pattern, message, check_as_command)
        // check_as_command=true means only match if it's actually a command, not inside strings
        let interactive_patterns: &[(&str, &str, bool)] = &[
            ("gh auth login", "gh auth login is interactive. Configure GITHUB_TOKEN in Settings > API Keys instead.", true),
            ("gh auth", "gh auth commands are interactive. Configure GITHUB_TOKEN in Settings > API Keys instead.", true),
            ("ssh-keygen", "ssh-keygen is interactive. Provide all options via flags or use pre-generated keys.", true),
            ("passwd", "passwd is interactive and not allowed.", true),
            ("sudo -S", "Interactive sudo not allowed.", false),
            ("read -p", "Interactive read not allowed.", false),
            ("read -n", "Interactive read not allowed.", false),
            ("| read ", "Interactive read piped input not allowed.", false),
            ("; read ", "Interactive read not allowed in scripts.", false),
            ("&& read ", "Interactive read not allowed in scripts.", false),
            ("vim ", "vim is interactive. Use file editing tools instead.", true),
            ("vi ", "vi is interactive. Use file editing tools instead.", true),
            ("nano ", "nano is interactive. Use file editing tools instead.", true),
            ("emacs ", "emacs is interactive. Use file editing tools instead.", true),
            ("less ", "less is interactive. Use cat or head/tail instead.", true),
            ("more ", "more is interactive. Use cat or head/tail instead.", true),
            ("mysql -u", "Interactive mysql not allowed. Use mysql -e for queries.", true),
            ("psql ", "Interactive psql not allowed. Use psql -c for queries.", true),
            ("python3 -i", "Interactive Python not allowed.", true),
            ("python -i", "Interactive Python not allowed.", true),
            ("node --inspect", "Interactive Node debugging not allowed.", true),
            ("gdb ", "Interactive gdb not allowed.", true),
            ("lldb ", "Interactive lldb not allowed.", true),
        ];

        for (pattern, msg, check_as_command) in interactive_patterns {
            if *check_as_command {
                if is_command_pattern(command, pattern) {
                    return Some(msg.to_string());
                }
            } else if command.contains(pattern) {
                return Some(msg.to_string());
            }
        }

        None
    }

    /// Execute a command in background mode using ProcessManager
    async fn execute_background(&self, params: &ExecParams, context: &ToolContext) -> ToolResult {
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
            workspace
        };

        // Ensure working directory exists
        if !working_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&working_dir) {
                return ToolResult::error(format!("Cannot create working directory: {}", e));
            }
        }

        // Get channel ID from context (default to 0 if not set)
        let channel_id = context.channel_id.unwrap_or(0);

        // Check if ProcessManager is available in context
        let process_manager = match context.process_manager.as_ref() {
            Some(pm) => pm,
            None => {
                // Fallback: spawn without ProcessManager (fire-and-forget)
                log::warn!("ProcessManager not available, using fire-and-forget background execution");

                let shell = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
                let shell_arg = if cfg!(target_os = "windows") { "/C" } else { "-c" };

                match Command::new(shell)
                    .arg(shell_arg)
                    .arg(&params.command)
                    .current_dir(&working_dir)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(child) => {
                        let pid = child.id().unwrap_or(0);
                        return ToolResult::success(format!(
                            "Started background process (PID: {})\n\
                            Command: {}\n\
                            Working directory: {}\n\n\
                            Note: ProcessManager not available. Process output is not captured.",
                            pid,
                            params.command,
                            working_dir.display()
                        )).with_metadata(json!({
                            "pid": pid,
                            "command": params.command,
                            "background": true,
                            "working_dir": working_dir.to_string_lossy()
                        }));
                    }
                    Err(e) => {
                        return ToolResult::error(format!("Failed to start background process: {}", e));
                    }
                }
            }
        };

        // Build env vars from context
        let mut env_vars = HashMap::new();

        // Add API keys from context
        for key_id in ApiKeyId::all() {
            if let Some(value) = context.get_api_key_by_id(key_id) {
                if let Some(key_env_vars) = key_id.env_vars() {
                    for env_var in key_env_vars {
                        env_vars.insert(env_var.to_string(), value.clone());
                    }
                }
            }
        }

        // Also inject custom runtime API keys
        // Skip keys that match a built-in ApiKeyId — those are already handled above.
        for name in context.list_api_key_names() {
            if env_vars.contains_key(&name) {
                continue;
            }
            if ApiKeyId::from_str(&name).is_ok() {
                continue; // built-in key, already injected via env_vars() mapping
            }
            if let Some(value) = context.get_api_key(&name) {
                if !value.is_empty() {
                    env_vars.insert(name, value);
                }
            }
        }

        // Add custom env vars from params
        if let Some(ref param_env) = params.env {
            for (key, value) in param_env {
                env_vars.insert(key.clone(), value.clone());
            }
        }

        // Debug: log all injected environment variables (background path)
        if !env_vars.is_empty() {
            let var_names: Vec<&String> = env_vars.keys().collect();
            log::debug!("[EXEC/BG] Injected env vars for command '{}': {:?}", params.command, var_names);
            for (name, val) in &env_vars {
                let preview = if val.len() > 8 { &val[..8] } else { val.as_str() };
                log::debug!("[EXEC/BG]   {}={}... (len={})", name, preview, val.len());
            }
        } else {
            log::debug!("[EXEC/BG] No env vars injected for command '{}'", params.command);
        }

        // Spawn via ProcessManager
        match process_manager
            .spawn(
                &params.command,
                &working_dir,
                channel_id,
                Some(&env_vars),
            )
            .await
        {
            Ok(process_id) => {
                // Get process info for response
                let info = process_manager.get(&process_id);
                let pid = info.as_ref().and_then(|i| i.pid).unwrap_or(0);

                ToolResult::success(format!(
                    "Started background process\n\
                    Process ID: {}\n\
                    PID: {}\n\
                    Command: {}\n\
                    Working directory: {}\n\n\
                    Use `process_status` tool with id=\"{}\" to check status or get output.",
                    process_id,
                    pid,
                    params.command,
                    working_dir.display(),
                    process_id
                )).with_metadata(json!({
                    "process_id": process_id,
                    "pid": pid,
                    "command": params.command,
                    "background": true,
                    "working_dir": working_dir.to_string_lossy()
                }))
            }
            Err(e) => ToolResult::error(format!("Failed to start background process: {}", e)),
        }
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
    #[serde(default, deserialize_with = "deserialize_u64_lenient")]
    timeout: Option<u64>,
    env: Option<HashMap<String, String>>,
    #[serde(default)]
    background: Option<bool>,
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

        let background = params.background.unwrap_or(false);

        // Detect server commands and warn if not using background mode
        if Self::is_server_command(&params.command) && !background {
            return ToolResult::success(format!(
                "Detected server/long-running command: `{}`\n\n\
                Server commands run indefinitely and will block or timeout.\n\
                To run this command, use `background: true` to run it asynchronously.\n\n\
                Example:\n```json\n{{\n  \"command\": \"{}\",\n  \"background\": true\n}}\n```\n\n\
                After starting, use the `process_status` tool to check on it or get its output.",
                params.command,
                params.command.replace("\"", "\\\"")
            ));
        }

        // Handle background execution
        if background {
            return self.execute_background(&params, context).await;
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
        // Track which keys are available for diagnostic output
        let mut available_env_vars: Vec<String> = Vec::new();
        for key_id in ApiKeyId::all() {
            if let Some(value) = context.get_api_key_by_id(key_id) {
                // Set all configured env vars for this key
                if let Some(env_vars) = key_id.env_vars() {
                    for env_var in env_vars {
                        cmd.env(env_var, &value);
                        available_env_vars.push(env_var.to_string());
                    }
                }

                // Special git configuration for GitHub token
                if key_id.requires_git_config() {
                    // Disable git terminal prompts (would hang in non-interactive mode)
                    cmd.env("GIT_TERMINAL_PROMPT", "0");
                    // Configure git to rewrite github HTTPS URLs to include the token
                    // This allows git clone/push to authenticate automatically
                    cmd.env("GIT_CONFIG_COUNT", "2");
                    cmd.env("GIT_CONFIG_KEY_0", format!("url.https://x-access-token:{}@github.com/.insteadOf", value));
                    cmd.env("GIT_CONFIG_VALUE_0", "https://github.com/");
                    cmd.env("GIT_CONFIG_KEY_1", format!("url.https://x-access-token:{}@github.com/.insteadOf", value));
                    cmd.env("GIT_CONFIG_VALUE_1", "git@github.com:");
                    // Set git author/committer info for commits (from bot config)
                    let bot_name = context.get_bot_name();
                    let bot_email = context.get_bot_email();
                    cmd.env("GIT_AUTHOR_NAME", &bot_name);
                    cmd.env("GIT_AUTHOR_EMAIL", &bot_email);
                    cmd.env("GIT_COMMITTER_NAME", &bot_name);
                    cmd.env("GIT_COMMITTER_EMAIL", &bot_email);
                }
            }
        }

        // Also inject custom runtime API keys as env vars
        // Skip keys that match a built-in ApiKeyId — those are already handled
        // by the loop above (possibly under different env var names).
        for name in context.list_api_key_names() {
            if available_env_vars.contains(&name) {
                continue;
            }
            if ApiKeyId::from_str(&name).is_ok() {
                continue; // built-in key, already injected via env_vars() mapping
            }
            if let Some(value) = context.get_api_key(&name) {
                if !value.is_empty() {
                    cmd.env(&name, &value);
                    available_env_vars.push(name);
                }
            }
        }

        // Set custom environment variables from params
        if let Some(ref env_vars) = params.env {
            for (key, value) in env_vars {
                cmd.env(key, value);
                available_env_vars.push(key.clone());
            }
        }

        // Debug: log all injected environment variables
        if !available_env_vars.is_empty() {
            log::debug!("[EXEC] Injected env vars for command '{}': {:?}", params.command, available_env_vars);
            for var_name in &available_env_vars {
                // Log masked values: show first 8 chars + length for debugging
                if let Some(val) = context.get_api_key(var_name) {
                    let preview = if val.len() > 8 { &val[..8] } else { &val };
                    log::debug!("[EXEC]   {}={}... (len={})", var_name, preview, val.len());
                }
            }
        } else {
            log::debug!("[EXEC] No env vars injected for command '{}'", params.command);
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
            // Provide diagnostic info when there's no output
            let env_info = if available_env_vars.is_empty() {
                "None configured".to_string()
            } else {
                available_env_vars.join(", ")
            };

            result_text = if success {
                format!(
                    "Command completed successfully (exit code: {})\n\n\
                    --- Diagnostics (no stdout/stderr) ---\n\
                    Command: {}\n\
                    Working dir: {}\n\
                    Duration: {}ms\n\
                    Available env vars: {}\n\n\
                    Tip: If you expected output, check:\n\
                    - API/auth tokens are configured in Settings > API Keys\n\
                    - The command isn't using -f (silent fail) flag\n\
                    - The API didn't return an empty response",
                    exit_code,
                    params.command,
                    working_dir.display(),
                    duration_ms,
                    env_info
                )
            } else {
                format!(
                    "Command failed with exit code: {}\n\n\
                    --- Diagnostics (no stdout/stderr) ---\n\
                    Command: {}\n\
                    Working dir: {}\n\
                    Duration: {}ms\n\
                    Available env vars: {}",
                    exit_code,
                    params.command,
                    working_dir.display(),
                    duration_ms,
                    env_info
                )
            };
        }

        // Truncate if too long (keep small to avoid context bloat for smaller models)
        const MAX_OUTPUT: usize = 15000;
        if result_text.len() > MAX_OUTPUT {
            result_text = format!(
                "{}\n\n[Output truncated at {} characters]",
                &result_text[..MAX_OUTPUT],
                MAX_OUTPUT
            );
        }

        log::info!("Command completed: exit_code={}, duration={}ms, output_len={}",
            exit_code, duration_ms, result_text.len());

        // Add actionable hints for common error patterns
        if !success {
            let hints = Self::generate_error_hints(&params.command, &result_text);
            if !hints.is_empty() {
                result_text.push_str("\n\n--- Hints ---\n");
                result_text.push_str(&hints);
            }
        }

        // Best-effort disk quota warning: if usage is >90%, append warning so AI sees it
        if let Some(ref dq) = context.disk_quota {
            if dq.is_enabled() && dq.usage_percentage() > 90 {
                result_text.push_str(&format!(
                    "\n\n⚠️ DISK QUOTA WARNING: {} — consider cleaning up unused files.",
                    dq.status_line()
                ));
            }
        }

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

impl ExecTool {
    /// Generate actionable hints based on error output patterns
    fn generate_error_hints(command: &str, output: &str) -> String {
        let mut hints = Vec::new();
        let lower = output.to_lowercase();
        let cmd_lower = command.to_lowercase();

        // Rust compilation errors
        if cmd_lower.contains("cargo") {
            if lower.contains("error[e") {
                // Extract file locations from "-->  src/file.rs:line:col"
                let locations: Vec<&str> = output.lines()
                    .filter(|l| l.trim().starts_with("-->"))
                    .take(5)
                    .collect();
                if !locations.is_empty() {
                    hints.push(format!("Compilation errors found. Use `read_file` to inspect:\n{}",
                        locations.join("\n")));
                }
            }
            if lower.contains("unresolved import") || lower.contains("cannot find") {
                hints.push("Missing import or module. Check use statements and module declarations.".to_string());
            }
            if lower.contains("borrow") || lower.contains("lifetime") {
                hints.push("Borrow checker error. Consider using .clone(), references, or restructuring ownership.".to_string());
            }
        }

        // TypeScript/Node errors
        if cmd_lower.contains("tsc") || cmd_lower.contains("npm") || cmd_lower.contains("npx") {
            if lower.contains("ts2") || lower.contains("error ts") {
                hints.push("TypeScript type errors found. Use `read_file` to check the flagged files.".to_string());
            }
            if lower.contains("module not found") || lower.contains("cannot find module") {
                hints.push("Missing dependency. Run `npm install <package>` to add it.".to_string());
            }
            if lower.contains("enoent") || lower.contains("no such file") {
                hints.push("File not found. Check that paths are correct and files exist.".to_string());
            }
        }

        // Python errors
        if cmd_lower.contains("python") || cmd_lower.contains("pytest") || cmd_lower.contains("pip") {
            if lower.contains("modulenotfounderror") || lower.contains("no module named") {
                hints.push("Missing Python module. Run `pip install <module>` or check your imports.".to_string());
            }
            if lower.contains("syntaxerror") {
                hints.push("Python syntax error. Check the file and line mentioned in the traceback.".to_string());
            }
            if lower.contains("indentationerror") {
                hints.push("Indentation error. Python is whitespace-sensitive — check spaces vs tabs.".to_string());
            }
        }

        // Go errors
        if cmd_lower.contains("go ") {
            if lower.contains("undefined:") {
                hints.push("Undefined symbol. Check that the function/variable is defined and exported (capitalized).".to_string());
            }
            if lower.contains("imported and not used") {
                hints.push("Unused import. Remove the import or use the package.".to_string());
            }
        }

        // Generic patterns
        if lower.contains("permission denied") {
            hints.push("Permission denied. The command may need different file permissions.".to_string());
        }
        if lower.contains("command not found") || lower.contains("not found") && lower.contains("no such") {
            hints.push("Command not found. Check that the tool is installed and on PATH.".to_string());
        }
        if lower.contains("eaddrinuse") || lower.contains("address already in use") {
            hints.push("Port already in use. Another process is running on that port. Use `process_status` to check.".to_string());
        }

        hints.join("\n")
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
