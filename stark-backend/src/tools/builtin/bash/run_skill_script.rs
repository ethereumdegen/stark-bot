use crate::controllers::api_keys::ApiKeyId;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
    ToolSafetyLevel,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::str::FromStr;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Tool for executing scripts bundled with skills.
///
/// Hidden by default — only available when a skill declares `requires_tools: [run_skill_script]`.
/// Scripts are resolved from the filesystem first (skills_dir/{skill}/scripts/{script}),
/// then from the database (skill_scripts table). Supports Python, Bash, and Node.js.
pub struct RunSkillScriptTool {
    definition: ToolDefinition,
}

impl RunSkillScriptTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "script".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Script filename (e.g. 'polymarket.py', 'helper.sh')".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action/function to call (passed as 1st CLI argument)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "args".to_string(),
            PropertySchema {
                schema_type: "object".to_string(),
                description: "JSON arguments (serialized as 2nd CLI argument)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "skill_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Skill that owns the script (defaults to the currently active skill)"
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );
        properties.insert(
            "timeout".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Timeout in seconds (default: 60, max: 300)".to_string(),
                default: Some(json!(60)),
                items: None,
                enum_values: None,
            },
        );

        RunSkillScriptTool {
            definition: ToolDefinition {
                name: "run_skill_script".to_string(),
                description: "Execute a script bundled with a skill. Scripts are located in the skill's scripts/ directory. Supports Python (.py), Bash (.sh), and Node.js (.js). The action and args are passed as CLI arguments. Environment variables (API keys) are automatically injected.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["script".to_string()],
                },
                group: ToolGroup::Exec,
                hidden: true,
            },
        }
    }

    /// Validate script filename — reject path traversal and unsafe characters
    fn validate_script_name(name: &str) -> Result<(), String> {
        if name.is_empty() {
            return Err("Script name cannot be empty".to_string());
        }
        if name.contains("..") {
            return Err("Script name cannot contain '..'".to_string());
        }
        if name.contains('/') || name.contains('\\') {
            return Err("Script name cannot contain path separators".to_string());
        }
        if name.contains('\0') {
            return Err("Script name cannot contain null bytes".to_string());
        }
        // Allow alphanumeric, underscore, hyphen, dot
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            return Err(
                "Script name can only contain alphanumeric characters, '_', '-', and '.'"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// Detect interpreter from file extension.
    /// Returns a slice of command args — e.g. `["uv", "run"]` for Python, `["bash"]` for shell.
    fn interpreter_for_extension(name: &str) -> Result<&'static [&'static str], String> {
        if name.ends_with(".py") {
            Ok(&["uv", "run"])
        } else if name.ends_with(".sh") {
            Ok(&["bash"])
        } else if name.ends_with(".js") {
            Ok(&["node"])
        } else {
            Err(format!(
                "Unsupported script extension. Use .py, .sh, or .js. Got: {}",
                name
            ))
        }
    }

    /// Resolve the active skill name from the session's agent context via DB
    fn resolve_active_skill_from_db(context: &ToolContext) -> Option<String> {
        let db = context.database.as_ref()?;
        let session_id = context.session_id?;
        let agent_ctx = db.get_agent_context(session_id).ok()??;
        agent_ctx.active_skill.map(|s| s.name)
    }
}

impl Default for RunSkillScriptTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct RunSkillScriptParams {
    script: String,
    action: Option<String>,
    args: Option<Value>,
    skill_name: Option<String>,
    timeout: Option<u64>,
}

#[async_trait]
impl Tool for RunSkillScriptTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Standard
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: RunSkillScriptParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // 1. Validate script name
        if let Err(e) = Self::validate_script_name(&params.script) {
            return ToolResult::error(format!("Invalid script name: {}", e));
        }

        // 2. Determine interpreter (may be multi-arg, e.g. ["uv", "run"])
        let interpreter_args = match Self::interpreter_for_extension(&params.script) {
            Ok(i) => i,
            Err(e) => return ToolResult::error(e),
        };

        // 3. Resolve skill name
        let skill_name = if let Some(ref name) = params.skill_name {
            name.clone()
        } else if let Some(name) = Self::resolve_active_skill_from_db(context) {
            name
        } else {
            return ToolResult::error(
                "No skill_name provided and no active skill found. Pass skill_name explicitly."
                    .to_string(),
            );
        };

        // 4. Find script on disk first, then fall back to DB
        //    Resolution order: {skill}/script.py > {skill}/scripts/script.py > DB
        let skills_dir = crate::config::skills_dir();
        let skill_root = PathBuf::from(&skills_dir).join(&skill_name);
        let disk_path_root = skill_root.join(&params.script);
        let disk_path_legacy = skill_root.join("scripts").join(&params.script);

        let disk_path = if disk_path_root.exists() {
            disk_path_root
        } else {
            disk_path_legacy
        };

        // Track whether we need to clean up a temp file
        let mut temp_file: Option<PathBuf> = None;

        let script_path = if disk_path.exists() {
            // Canonical path check to prevent symlink escape
            let canonical = match disk_path.canonicalize() {
                Ok(c) => c,
                Err(e) => {
                    return ToolResult::error(format!("Cannot resolve script path: {}", e))
                }
            };
            let skills_canonical = match PathBuf::from(&skills_dir).canonicalize() {
                Ok(c) => c,
                Err(e) => {
                    return ToolResult::error(format!("Cannot resolve skills dir: {}", e))
                }
            };
            if !canonical.starts_with(&skills_canonical) {
                return ToolResult::error(
                    "Script path escapes the skills directory (symlink escape blocked)".to_string(),
                );
            }
            canonical
        } else {
            // Fall back to DB
            let db = match context.database.as_ref() {
                Some(db) => db,
                None => {
                    return ToolResult::error(format!(
                        "Script '{}' not found on disk at {} and database not available",
                        params.script,
                        disk_path.display()
                    ))
                }
            };

            let scripts = match db.get_skill_scripts_by_name(&skill_name) {
                Ok(s) => s,
                Err(e) => {
                    return ToolResult::error(format!(
                        "Failed to query skill scripts from DB: {}",
                        e
                    ))
                }
            };

            let db_script = scripts.iter().find(|s| s.name == params.script);
            match db_script {
                Some(script) => {
                    // Write to temp file
                    let tmp_dir = std::env::temp_dir().join("stark_skill_scripts");
                    if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
                        return ToolResult::error(format!("Cannot create temp dir: {}", e));
                    }
                    let tmp_path = tmp_dir.join(format!(
                        "{}_{}_{}",
                        skill_name,
                        params.script,
                        std::process::id()
                    ));
                    if let Err(e) = std::fs::write(&tmp_path, &script.code) {
                        return ToolResult::error(format!("Cannot write temp script: {}", e));
                    }
                    temp_file = Some(tmp_path.clone());
                    tmp_path
                }
                None => {
                    return ToolResult::error(format!(
                        "Script '{}' not found for skill '{}'. Checked:\n  - disk: {}\n  - database: no matching script",
                        params.script,
                        skill_name,
                        disk_path.display()
                    ))
                }
            }
        };

        // 5. Build command
        let timeout_secs = params.timeout.unwrap_or(60).min(300);

        let mut cmd = Command::new(interpreter_args[0]);
        for extra in &interpreter_args[1..] {
            cmd.arg(extra);
        }
        cmd.arg(&script_path);

        // Pass action as 1st arg
        if let Some(ref action) = params.action {
            cmd.arg(action);
        }

        // Pass args as JSON string (2nd arg)
        if let Some(ref args) = params.args {
            cmd.arg(serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string()));
        }

        // Set working directory to the skill directory (or workspace)
        let skill_dir = PathBuf::from(&skills_dir).join(&skill_name);
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let working_dir = if skill_dir.exists() {
            &skill_dir
        } else {
            &workspace
        };

        cmd.current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // 6. Inject environment variables (API keys from context — reuse exec.rs pattern)
        for key_id in ApiKeyId::all() {
            if let Some(value) = context.get_api_key_by_id(key_id) {
                if let Some(env_vars) = key_id.env_vars() {
                    for env_var in env_vars {
                        cmd.env(env_var, &value);
                    }
                }
            }
        }

        // Also inject custom runtime API keys
        let mut injected: Vec<String> = Vec::new();
        for name in context.list_api_key_names() {
            if ApiKeyId::from_str(&name).is_ok() {
                continue;
            }
            if let Some(value) = context.get_api_key(&name) {
                if !value.is_empty() {
                    cmd.env(&name, &value);
                    injected.push(name);
                }
            }
        }

        // Extra env vars for skill context
        cmd.env("SKILL_NAME", &skill_name);
        cmd.env("SKILL_DIR", skill_dir.to_string_lossy().as_ref());
        cmd.env("WORKSPACE_DIR", workspace.to_string_lossy().as_ref());

        log::info!(
            "[run_skill_script] skill={} script={} action={:?} interpreter={} timeout={}s",
            skill_name,
            params.script,
            params.action,
            interpreter_args.join(" "),
            timeout_secs
        );

        // 7. Execute with timeout
        let start = std::time::Instant::now();
        let output = match timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                cleanup_temp(&temp_file);
                return ToolResult::error(format!("Failed to execute script: {}", e));
            }
            Err(_) => {
                cleanup_temp(&temp_file);
                return ToolResult::error(format!(
                    "Script timed out after {} seconds. Increase timeout if needed (max 300).",
                    timeout_secs
                ));
            }
        };
        let duration_ms = start.elapsed().as_millis() as i64;

        // 8. Clean up temp file
        cleanup_temp(&temp_file);

        // 9. Build result
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

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
            result_text = format!(
                "Script completed (exit code: {}, duration: {}ms) with no output.",
                exit_code, duration_ms
            );
        }

        // Truncate output
        const MAX_OUTPUT: usize = 15000;
        if result_text.len() > MAX_OUTPUT {
            result_text = format!(
                "{}\n\n[Output truncated at {} characters]",
                &result_text[..MAX_OUTPUT],
                MAX_OUTPUT
            );
        }

        log::info!(
            "[run_skill_script] completed: exit_code={}, duration={}ms, output_len={}",
            exit_code,
            duration_ms,
            result_text.len()
        );

        let result = if output.status.success() {
            ToolResult::success(result_text)
        } else {
            ToolResult::error(result_text)
        };

        result.with_metadata(json!({
            "skill_name": skill_name,
            "script": params.script,
            "action": params.action,
            "exit_code": exit_code,
            "duration_ms": duration_ms,
        }))
    }
}

fn cleanup_temp(path: &Option<PathBuf>) {
    if let Some(p) = path {
        let _ = std::fs::remove_file(p);
    }
}
