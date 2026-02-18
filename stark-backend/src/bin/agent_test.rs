//! Agent Test Fixture
//!
//! Tests the agent loop via the external channel gateway (default) or with
//! direct LLM calls using REAL tool implementations.
//!
//! ## Gateway Mode (default when EXT_CHANNEL_API_TOKEN is set)
//!
//!   EXT_CHANNEL_API_TOKEN="your-token" \
//!   TEST_QUERY="tell me the price of bitcoin" \
//!   cargo run --bin agent_test
//!
//! ## Direct Mode (legacy, requires TEST_AGENT_ENDPOINT)
//!
//!   TEST_AGENT_ENDPOINT="https://api.openai.com/v1/chat/completions" \
//!   TEST_AGENT_SECRET="your-api-key" \
//!   TEST_QUERY="build a simple todo app" \
//!   cargo run --bin agent_test
//!
//! Environment variables:
//!   --- Gateway mode ---
//!   EXT_CHANNEL_API_TOKEN - External channel API token (enables gateway mode)
//!   EXT_CHANNEL_URL       - Gateway base URL (default: http://localhost:8080)
//!   TEST_QUERY            - The user query to test
//!   TEST_SESSION           - Session ID for persistent conversations
//!   TEST_STREAM            - Use SSE streaming (default: false)
//!
//!   --- Direct mode ---
//!   TEST_AGENT_ENDPOINT  - LLM API endpoint (OpenAI-compatible)
//!   TEST_AGENT_SECRET    - API key for the LLM
//!   TEST_AGENT_MODEL     - Model name (auto-detected from endpoint, or specify manually)
//!   TEST_WORKSPACE       - Workspace directory for file operations
//!   TEST_SKILLS_DIR      - Path to skills directory (default: ./skills)
//!   TEST_MAX_ITERATIONS  - Max tool loop iterations (default: 25)

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap as StdHashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand};
use std::sync::Mutex;
use std::time::Duration;

// ============================================================================
// Types for OpenAI-compatible API
// ============================================================================

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ToolSpec {
    #[serde(rename = "type")]
    tool_type: String,
    function: ToolFunction,
}

#[derive(Debug, Clone, Serialize)]
struct ToolFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallResponse {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCallResponse>>,
}

// ============================================================================
// Tool Definitions - Real CodeEngineer Tools
// ============================================================================

fn get_code_engineer_tools() -> Vec<ToolSpec> {
    vec![
        // read_file
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "read_file".to_string(),
                description: "Read the contents of a file. Use this to examine existing code or files.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to read (relative to workspace)"
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
        // write_file
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "write_file".to_string(),
                description: "Create or overwrite a file with the given content. Use this to create new files or completely replace file contents.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to write (relative to workspace)"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write to the file"
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
        },
        // list_files
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "list_files".to_string(),
                description: "List files and directories in a path.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Directory path to list (relative to workspace, default: '.')"
                        }
                    },
                    "required": []
                }),
            },
        },
        // exec
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "exec".to_string(),
                description: "Execute a shell command. Use for npm, cargo, git, and other CLI tools. Commands run in the workspace directory. Use background: true for long-running commands like servers.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        },
                        "timeout": {
                            "type": "integer",
                            "description": "Timeout in seconds (default: 60, max: 300)"
                        },
                        "background": {
                            "type": "boolean",
                            "description": "Run command in background, returns immediately with process ID. Use for servers and long-running commands."
                        }
                    },
                    "required": ["command"]
                }),
            },
        },
        // process_status
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "process_status".to_string(),
                description: "Check status, get output, or manage background processes started with exec background: true.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "operation": {
                            "type": "string",
                            "enum": ["status", "output", "kill", "list"],
                            "description": "Operation: status (check process), output (get recent output), kill (terminate), list (show all)"
                        },
                        "process_id": {
                            "type": "string",
                            "description": "The process ID (e.g., 'proc_1') from exec background mode"
                        },
                        "lines": {
                            "type": "integer",
                            "description": "Number of output lines to retrieve (default: 50)"
                        }
                    },
                    "required": ["operation"]
                }),
            },
        },
        // git
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "git".to_string(),
                description: "Execute git operations. Supports: status, diff, log, add, commit, branch, checkout, init.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "operation": {
                            "type": "string",
                            "enum": ["status", "diff", "log", "add", "commit", "branch", "checkout", "init"],
                            "description": "Git operation to perform"
                        },
                        "files": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Files to operate on (for add, diff)"
                        },
                        "message": {
                            "type": "string",
                            "description": "Commit message (for commit operation)"
                        },
                        "branch": {
                            "type": "string",
                            "description": "Branch name (for checkout, branch)"
                        },
                        "create": {
                            "type": "boolean",
                            "description": "Create new branch (for checkout)"
                        }
                    },
                    "required": ["operation"]
                }),
            },
        },
        // glob
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "glob".to_string(),
                description: "Find files matching a glob pattern.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Glob pattern like '**/*.ts' or 'src/**/*.js'"
                        }
                    },
                    "required": ["pattern"]
                }),
            },
        },
        // grep
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "grep".to_string(),
                description: "Search for a pattern in files.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Regex pattern to search for"
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory or file to search in"
                        }
                    },
                    "required": ["pattern"]
                }),
            },
        },
        // use_skill
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "use_skill".to_string(),
                description: "Load a skill's instructions. Skills provide step-by-step guidance for specific tasks. Call this BEFORE attempting tasks like moltbook operations, wallet queries, etc.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "skill_name": {
                            "type": "string",
                            "description": "Name of the skill to load (e.g., 'moltbook', 'local_wallet')"
                        }
                    },
                    "required": ["skill_name"]
                }),
            },
        },
        // api_keys_check
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "api_keys_check".to_string(),
                description: "Check if a specific API key is configured. Returns whether the key exists and is set.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "key_name": {
                            "type": "string",
                            "description": "The name of the API key to check (e.g., 'MOLTBOOK_TOKEN', 'DISCORD_BOT_TOKEN')"
                        }
                    },
                    "required": ["key_name"]
                }),
            },
        },
        // discord_lookup
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "discord_lookup".to_string(),
                description: "Look up Discord servers (guilds) and channels. Use this to find server IDs by name, list channels in a server, or search for specific channels.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["list_servers", "search_servers", "list_channels", "search_channels"],
                            "description": "The action to perform"
                        },
                        "server_id": {
                            "type": "string",
                            "description": "Discord server (guild) ID. Required for list_channels and search_channels."
                        },
                        "query": {
                            "type": "string",
                            "description": "Search query for filtering by name. Required for search_servers and search_channels."
                        }
                    },
                    "required": ["action"]
                }),
            },
        },
        // discord
        ToolSpec {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: "discord".to_string(),
                description: "Perform Discord actions like sending messages, reacting, managing threads. Use 'sendMessage' action with 'to' in format 'channel:<id>' to send messages.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["sendMessage", "react", "readMessages", "editMessage", "deleteMessage"],
                            "description": "The Discord action to perform"
                        },
                        "to": {
                            "type": "string",
                            "description": "Target for sendMessage: 'channel:<id>' or 'user:<id>'"
                        },
                        "content": {
                            "type": "string",
                            "description": "Message content to send"
                        },
                        "channelId": {
                            "type": "string",
                            "description": "Channel ID for readMessages, react, editMessage, deleteMessage"
                        },
                        "messageId": {
                            "type": "string",
                            "description": "Message ID for react, editMessage, deleteMessage"
                        },
                        "emoji": {
                            "type": "string",
                            "description": "Emoji for react action"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Number of messages to read (default: 20)"
                        }
                    },
                    "required": ["action"]
                }),
            },
        },
    ]
}

// ============================================================================
// Tool Execution - REAL implementations
// ============================================================================

async fn execute_tool(name: &str, args: &Value, workspace: &Path) -> String {
    println!("\n   🔧 Executing: {}", name);
    println!("   📥 Args: {}", serde_json::to_string(args).unwrap_or_default());

    let result = match name {
        "read_file" => execute_read_file(args, workspace),
        "write_file" => execute_write_file(args, workspace),
        "list_files" => execute_list_files(args, workspace),
        "exec" => execute_exec(args, workspace),
        "process_status" => execute_process_status(args),
        "git" => execute_git(args, workspace),
        "glob" => execute_glob(args, workspace),
        "grep" => execute_grep(args, workspace),
        "use_skill" => execute_use_skill(args),
        "api_keys_check" => execute_api_keys_check(args),
        "discord_lookup" => execute_discord_lookup(args).await,
        "discord" => execute_discord(args).await,
        _ => format!("Unknown tool: {}", name),
    };

    // Truncate long output
    let display = if result.len() > 1000 {
        format!("{}...[truncated, {} chars total]", &result[..1000], result.len())
    } else {
        result.clone()
    };
    println!("   📤 Result: {}", display);

    result
}

fn execute_read_file(args: &Value, workspace: &Path) -> String {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let full_path = workspace.join(path);

    match fs::read_to_string(&full_path) {
        Ok(content) => content,
        Err(e) => format!("Error reading file: {}", e),
    }
}

fn execute_write_file(args: &Value, workspace: &Path) -> String {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let full_path = workspace.join(path);

    // Create parent directories if needed
    if let Some(parent) = full_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return format!("Error creating directories: {}", e);
        }
    }

    match fs::write(&full_path, content) {
        Ok(_) => format!("Successfully wrote {} bytes to {}", content.len(), path),
        Err(e) => format!("Error writing file: {}", e),
    }
}

fn execute_list_files(args: &Value, workspace: &Path) -> String {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let full_path = workspace.join(path);

    match fs::read_dir(&full_path) {
        Ok(entries) => {
            let mut files: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if e.path().is_dir() {
                        format!("{}/", name)
                    } else {
                        name
                    }
                })
                .collect();
            files.sort();
            files.join("\n")
        }
        Err(e) => format!("Error listing directory: {}", e),
    }
}

// Track background processes (simple in-memory store for test harness)
lazy_static::lazy_static! {
    static ref BACKGROUND_PROCESSES: Mutex<StdHashMap<String, BackgroundProcess>> = Mutex::new(StdHashMap::new());
    static ref PROCESS_COUNTER: Mutex<u32> = Mutex::new(0);
}

struct BackgroundProcess {
    id: String,
    pid: u32,
    command: String,
    #[allow(dead_code)]
    child: Option<Child>,
    output: Vec<String>,
    completed: bool,
    exit_code: Option<i32>,
}

fn execute_exec(args: &Value, workspace: &Path) -> String {
    let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
    let background = args.get("background").and_then(|v| v.as_bool()).unwrap_or(false);

    // Server command detection
    let server_patterns = [
        "npm start", "npm run dev", "npm run serve", "yarn start", "yarn dev",
        "node index.js", "node server.js", "node app.js",
        "python -m http.server", "python manage.py runserver", "flask run",
        "cargo run", "go run", "rails server", "rails s",
    ];
    let lower_cmd = command.to_lowercase();
    let is_server = server_patterns.iter().any(|p| lower_cmd.contains(p));

    if is_server && !background {
        return format!(
            "Detected server/long-running command: `{}`\n\n\
            Server commands run indefinitely and will block or timeout.\n\
            To run this command, use `background: true` to run it asynchronously.\n\n\
            Example:\n```json\n{{\n  \"command\": \"{}\",\n  \"background\": true\n}}\n```",
            command,
            command.replace("\"", "\\\"")
        );
    }

    if background {
        println!("   🖥️  Starting background: {}", command);

        match ProcessCommand::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(workspace)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => {
                let pid = child.id();
                let mut counter = PROCESS_COUNTER.lock().unwrap();
                *counter += 1;
                let process_id = format!("proc_{}", *counter);

                let bg_process = BackgroundProcess {
                    id: process_id.clone(),
                    pid,
                    command: command.to_string(),
                    child: Some(child),
                    output: Vec::new(),
                    completed: false,
                    exit_code: None,
                };

                BACKGROUND_PROCESSES.lock().unwrap().insert(process_id.clone(), bg_process);

                format!(
                    "Started background process\n\
                    Process ID: {}\n\
                    PID: {}\n\
                    Command: {}\n\n\
                    Use `process_status` tool with process_id=\"{}\" to check status or get output.",
                    process_id, pid, command, process_id
                )
            }
            Err(e) => format!("Failed to start background process: {}", e),
        }
    } else {
        println!("   🖥️  Running: {}", command);

        let output = ProcessCommand::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(workspace)
            .output();

        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                let mut result = String::new();
                if !stdout.is_empty() {
                    result.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !result.is_empty() {
                        result.push_str("\n[stderr]: ");
                    }
                    result.push_str(&stderr);
                }
                if result.is_empty() {
                    result = format!("Command completed with exit code {}", exit_code);
                }
                result
            }
            Err(e) => format!("Failed to execute command: {}", e),
        }
    }
}

fn execute_process_status(args: &Value) -> String {
    let operation = args.get("operation").and_then(|v| v.as_str()).unwrap_or("list");
    let process_id = args.get("process_id").and_then(|v| v.as_str());

    match operation {
        "status" => {
            let pid = match process_id {
                Some(id) => id,
                None => return "Error: process_id is required for 'status' operation".to_string(),
            };

            let processes = BACKGROUND_PROCESSES.lock().unwrap();
            match processes.get(pid) {
                Some(proc) => {
                    let status = if proc.completed { "completed" } else { "running" };
                    format!(
                        "Process: {}\nStatus: {}\nPID: {}\nCommand: {}{}",
                        proc.id,
                        status,
                        proc.pid,
                        proc.command,
                        if let Some(code) = proc.exit_code {
                            format!("\nExit code: {}", code)
                        } else {
                            String::new()
                        }
                    )
                }
                None => format!("Process '{}' not found", pid),
            }
        }

        "output" => {
            let pid = match process_id {
                Some(id) => id,
                None => return "Error: process_id is required for 'output' operation".to_string(),
            };

            let lines = args.get("lines").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

            let processes = BACKGROUND_PROCESSES.lock().unwrap();
            match processes.get(pid) {
                Some(proc) => {
                    if proc.output.is_empty() {
                        format!("No output captured yet for process '{}'", pid)
                    } else {
                        let output: Vec<_> = proc.output.iter().rev().take(lines).collect();
                        format!(
                            "Output from process '{}' (last {} lines):\n\n{}",
                            pid,
                            output.len(),
                            output.into_iter().rev().cloned().collect::<Vec<_>>().join("\n")
                        )
                    }
                }
                None => format!("Process '{}' not found", pid),
            }
        }

        "kill" => {
            let pid = match process_id {
                Some(id) => id,
                None => return "Error: process_id is required for 'kill' operation".to_string(),
            };

            let mut processes = BACKGROUND_PROCESSES.lock().unwrap();
            match processes.get_mut(pid) {
                Some(proc) => {
                    if let Some(ref mut child) = proc.child {
                        match child.kill() {
                            Ok(_) => {
                                proc.completed = true;
                                format!("Process '{}' has been killed", pid)
                            }
                            Err(e) => format!("Failed to kill process '{}': {}", pid, e),
                        }
                    } else {
                        format!("Process '{}' has no active child handle", pid)
                    }
                }
                None => format!("Process '{}' not found", pid),
            }
        }

        "list" => {
            let processes = BACKGROUND_PROCESSES.lock().unwrap();
            if processes.is_empty() {
                return "No background processes found.".to_string();
            }

            let mut result = String::from("Background processes:\n\n");
            for proc in processes.values() {
                let status = if proc.completed { "completed" } else { "running" };
                let short_cmd = if proc.command.len() > 50 {
                    format!("{}...", &proc.command[..47])
                } else {
                    proc.command.clone()
                };
                result.push_str(&format!(
                    "- {} (PID {}): {}\n  Command: {}\n\n",
                    proc.id, proc.pid, status, short_cmd
                ));
            }
            result
        }

        _ => format!("Unknown operation '{}'. Use: status, output, kill, or list", operation),
    }
}

fn execute_git(args: &Value, workspace: &Path) -> String {
    let operation = args.get("operation").and_then(|v| v.as_str()).unwrap_or("");

    let git_args: Vec<&str> = match operation {
        "status" => vec!["status", "--porcelain"],
        "diff" => vec!["diff"],
        "log" => vec!["log", "--oneline", "-10"],
        "init" => vec!["init"],
        "add" => {
            let files = args.get("files")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();
            if files.is_empty() {
                return "Error: No files specified for git add".to_string();
            }
            // Execute with files
            let mut cmd_args = vec!["add"];
            let output = ProcessCommand::new("git")
                .arg("add")
                .args(&files)
                .current_dir(workspace)
                .output();
            return match output {
                Ok(o) => format!("Staged {} file(s)", files.len()),
                Err(e) => format!("Git error: {}", e),
            };
        }
        "commit" => {
            let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("Update");
            let output = ProcessCommand::new("git")
                .args(["commit", "-m", message])
                .current_dir(workspace)
                .output();
            return match output {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    format!("{}{}", stdout, stderr)
                }
                Err(e) => format!("Git error: {}", e),
            };
        }
        "branch" => {
            if let Some(branch) = args.get("branch").and_then(|v| v.as_str()) {
                vec!["branch", branch]
            } else {
                vec!["branch", "-a"]
            }
        }
        "checkout" => {
            let branch = args.get("branch").and_then(|v| v.as_str()).unwrap_or("main");
            let create = args.get("create").and_then(|v| v.as_bool()).unwrap_or(false);
            if create {
                vec!["checkout", "-b", branch]
            } else {
                vec!["checkout", branch]
            }
        }
        _ => return format!("Unknown git operation: {}", operation),
    };

    let output = ProcessCommand::new("git")
        .args(&git_args)
        .current_dir(workspace)
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            if stdout.is_empty() && stderr.is_empty() {
                format!("Git {} completed successfully", operation)
            } else {
                format!("{}{}", stdout, stderr)
            }
        }
        Err(e) => format!("Git error: {}", e),
    }
}

fn execute_glob(args: &Value, workspace: &Path) -> String {
    let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("*");

    // Use find command for glob-like behavior
    let output = ProcessCommand::new("find")
        .args([".", "-name", pattern, "-type", "f"])
        .current_dir(workspace)
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.is_empty() {
                "No files found matching pattern".to_string()
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error: {}", e),
    }
}

fn execute_grep(args: &Value, workspace: &Path) -> String {
    let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    let output = ProcessCommand::new("grep")
        .args(["-rn", pattern, path])
        .current_dir(workspace)
        .output();

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.is_empty() {
                "No matches found".to_string()
            } else {
                stdout.to_string()
            }
        }
        Err(e) => format!("Error: {}", e),
    }
}

fn execute_use_skill(args: &Value) -> String {
    let skill_name = args.get("skill_name").and_then(|v| v.as_str()).unwrap_or("");

    // Try to find the skill file
    let skill_paths = [
        format!("../skills/{}.md", skill_name),
        format!("skills/{}.md", skill_name),
        format!("./skills/{}.md", skill_name),
    ];

    for skill_path in &skill_paths {
        if let Ok(content) = fs::read_to_string(skill_path) {
            println!("   📚 Loaded skill: {} from {}", skill_name, skill_path);
            return format!(
                "# Skill Loaded: {}\n\n{}\n\n---\nFollow the instructions above to complete the task.",
                skill_name, content
            );
        }
    }

    format!(
        "Skill '{}' not found. Available skills can be found in the skills/ directory.\n\
        Try listing skills with list_files on the skills directory.",
        skill_name
    )
}

fn execute_api_keys_check(args: &Value) -> String {
    let key_name = args.get("key_name").and_then(|v| v.as_str()).unwrap_or("");

    match std::env::var(key_name) {
        Ok(val) if !val.is_empty() => {
            // Mask the value for security
            let masked = if val.len() > 8 {
                format!("{}...{}", &val[..4], &val[val.len()-4..])
            } else {
                "****".to_string()
            };
            format!(
                "✅ API key '{}' is configured.\nValue: {} (masked)\nLength: {} characters",
                key_name, masked, val.len()
            )
        }
        Ok(_) => format!(
            "⚠️ API key '{}' exists but is empty.\nPlease set the value in your environment or .env file.",
            key_name
        ),
        Err(_) => format!(
            "❌ API key '{}' is NOT configured.\n\n\
            To set this key:\n\
            1. Add it to your .env file: {}=your_value_here\n\
            2. Or export it: export {}=your_value_here\n\n\
            For MOLTBOOK_TOKEN specifically:\n\
            - Get a token from https://www.moltbook.com\n\
            - Or use the self-registration API (see moltbook skill)",
            key_name, key_name, key_name
        ),
    }
}

async fn execute_discord_lookup(args: &Value) -> String {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let server_id = args.get("server_id").and_then(|v| v.as_str());
    let query = args.get("query").and_then(|v| v.as_str());

    let bot_token = match std::env::var("DISCORD_BOT_TOKEN") {
        Ok(t) => t,
        Err(_) => return "Error: DISCORD_BOT_TOKEN not set".to_string(),
    };

    let client = reqwest::Client::new();

    match action {
        "list_servers" | "search_servers" => {
            let url = "https://discord.com/api/v10/users/@me/guilds?limit=200";
            let response = client
                .get(url)
                .header("Authorization", format!("Bot {}", bot_token))
                .send()
                .await;

            match response {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        return format!("Discord API error: {}", resp.status());
                    }
                    let body = resp.text().await.unwrap_or_default();
                    let guilds: Vec<Value> = serde_json::from_str(&body).unwrap_or_default();

                    let filtered: Vec<&Value> = if action == "search_servers" {
                        let q = query.unwrap_or("").to_lowercase();
                        guilds.iter().filter(|g| {
                            g.get("name")
                                .and_then(|n| n.as_str())
                                .map(|n| n.to_lowercase().contains(&q))
                                .unwrap_or(false)
                        }).collect()
                    } else {
                        guilds.iter().collect()
                    };

                    let result: Vec<Value> = filtered.iter().map(|g| {
                        json!({
                            "id": g.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            "name": g.get("name").and_then(|v| v.as_str()).unwrap_or("")
                        })
                    }).collect();

                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| "[]".to_string())
                }
                Err(e) => format!("Request failed: {}", e),
            }
        }
        "list_channels" | "search_channels" => {
            let sid = match server_id {
                Some(id) => id,
                None => return "Error: server_id is required".to_string(),
            };

            let url = format!("https://discord.com/api/v10/guilds/{}/channels", sid);
            let response = client
                .get(&url)
                .header("Authorization", format!("Bot {}", bot_token))
                .send()
                .await;

            match response {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        return format!("Discord API error: {}", resp.status());
                    }
                    let body = resp.text().await.unwrap_or_default();
                    let channels: Vec<Value> = serde_json::from_str(&body).unwrap_or_default();

                    let filtered: Vec<&Value> = if action == "search_channels" {
                        let q = query.unwrap_or("").to_lowercase();
                        channels.iter().filter(|c| {
                            c.get("name")
                                .and_then(|n| n.as_str())
                                .map(|n| n.to_lowercase().contains(&q))
                                .unwrap_or(false)
                        }).collect()
                    } else {
                        channels.iter().collect()
                    };

                    let result: Vec<Value> = filtered.iter().map(|c| {
                        let channel_type = c.get("type").and_then(|v| v.as_u64()).unwrap_or(0);
                        let type_name = match channel_type {
                            0 => "text",
                            2 => "voice",
                            4 => "category",
                            5 => "announcement",
                            _ => "other",
                        };
                        json!({
                            "id": c.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            "name": c.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "type": type_name
                        })
                    }).collect();

                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| "[]".to_string())
                }
                Err(e) => format!("Request failed: {}", e),
            }
        }
        _ => format!("Unknown action: {}", action),
    }
}

async fn execute_discord(args: &Value) -> String {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");

    let bot_token = match std::env::var("DISCORD_BOT_TOKEN") {
        Ok(t) => t,
        Err(_) => return "Error: DISCORD_BOT_TOKEN not set".to_string(),
    };

    let client = reqwest::Client::new();

    match action {
        "sendMessage" => {
            let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");

            // Parse "channel:<id>" format
            let channel_id = if to.starts_with("channel:") {
                to.trim_start_matches("channel:")
            } else {
                to
            };

            if channel_id.is_empty() {
                return "Error: 'to' parameter is required (format: 'channel:<id>')".to_string();
            }
            if content.is_empty() {
                return "Error: 'content' parameter is required".to_string();
            }

            let url = format!("https://discord.com/api/v10/channels/{}/messages", channel_id);
            let body = json!({ "content": content });

            let response = client
                .post(&url)
                .header("Authorization", format!("Bot {}", bot_token))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    let body_text = resp.text().await.unwrap_or_default();
                    if status.is_success() {
                        format!("Message sent successfully to channel {}", channel_id)
                    } else {
                        format!("Discord API error ({}): {}", status, body_text)
                    }
                }
                Err(e) => format!("Request failed: {}", e),
            }
        }
        "readMessages" => {
            let channel_id = args.get("channelId").and_then(|v| v.as_str()).unwrap_or("");
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20);

            if channel_id.is_empty() {
                return "Error: 'channelId' parameter is required".to_string();
            }

            let url = format!(
                "https://discord.com/api/v10/channels/{}/messages?limit={}",
                channel_id, limit
            );

            let response = client
                .get(&url)
                .header("Authorization", format!("Bot {}", bot_token))
                .send()
                .await;

            match response {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        return format!("Discord API error: {}", resp.status());
                    }
                    let body = resp.text().await.unwrap_or_default();
                    let messages: Vec<Value> = serde_json::from_str(&body).unwrap_or_default();

                    let result: Vec<Value> = messages.iter().map(|m| {
                        json!({
                            "id": m.get("id"),
                            "content": m.get("content"),
                            "author": m.get("author").and_then(|a| a.get("username"))
                        })
                    }).collect();

                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| "[]".to_string())
                }
                Err(e) => format!("Request failed: {}", e),
            }
        }
        "react" => {
            let channel_id = args.get("channelId").and_then(|v| v.as_str()).unwrap_or("");
            let message_id = args.get("messageId").and_then(|v| v.as_str()).unwrap_or("");
            let emoji = args.get("emoji").and_then(|v| v.as_str()).unwrap_or("");

            if channel_id.is_empty() || message_id.is_empty() || emoji.is_empty() {
                return "Error: channelId, messageId, and emoji are all required".to_string();
            }

            // URL-encode the emoji
            let encoded_emoji = urlencoding::encode(emoji);
            let url = format!(
                "https://discord.com/api/v10/channels/{}/messages/{}/reactions/{}/@me",
                channel_id, message_id, encoded_emoji
            );

            let response = client
                .put(&url)
                .header("Authorization", format!("Bot {}", bot_token))
                .send()
                .await;

            match response {
                Ok(resp) => {
                    if resp.status().is_success() {
                        format!("Reacted with {} to message {}", emoji, message_id)
                    } else {
                        format!("Discord API error: {}", resp.status())
                    }
                }
                Err(e) => format!("Request failed: {}", e),
            }
        }
        _ => format!("Unknown or unsupported action: {}", action),
    }
}

// ============================================================================
// System Prompt
// ============================================================================

fn get_system_prompt(workspace: &Path, skills: &[String]) -> String {
    format!(r#"You are an AI agent that can perform various tasks. Your workspace is: {}

## ⚠️ CRITICAL: You MUST Call Tools ⚠️

**ABSOLUTE RULE: NEVER respond to questions without calling tools first.**

For ANY request about external services, APIs, registrations, balances, or data:
1. **FIRST** - Call `use_skill` to load relevant instructions
2. **THEN** - Call `api_keys_check` to verify required credentials exist
3. **THEN** - Call the actual tools (e.g., `exec` with curl) to get real data
4. **FINALLY** - Report ONLY what the tools actually returned

### ❌ WRONG (will provide bad info):
- User asks "are you registered on moltbook?" → Responding "I'm not registered" without checking
- Saying "I don't have accounts" without calling tools

### ✅ CORRECT:
1. use_skill(skill_name="moltbook") - Load moltbook instructions
2. api_keys_check(key_name="MOLTBOOK_TOKEN") - Check if token exists
3. exec(command="curl ... /agents/me") - Get actual registration status
4. Report the ACTUAL result from the tool

## Available Tools

### Skill & Config Tools (USE FIRST!)
- `use_skill` - Load detailed instructions for a task (skill_name). ALWAYS call this first!
- `api_keys_check` - Check if an API key is configured (key_name)

### File Operations
- `write_file` - Create or overwrite files (path, content)
- `read_file` - Read file contents (path)
- `list_files` - List directory contents (path)
- `glob` - Find files by pattern
- `grep` - Search in files

### System
- `exec` - Run shell commands (command) - use for curl, npm, cargo, pip, etc.
  - For API calls: curl with $ENV_VAR for auth tokens
- `git` - Git operations (operation: status/diff/log/add/commit/init, files, message, branch)

### Discord Integration
- `discord_lookup` - Look up Discord servers and channels
- `discord` - Perform Discord actions (sendMessage, readMessages, react)

## Skills Available: {}

## Workflow for Service Queries

When asked about any external service (Moltbook, Discord, GitHub, etc.):

1. `use_skill(skill_name="<service>")` - Get the instructions
2. `api_keys_check(key_name="<TOKEN>")` - Verify credentials
3. If token exists: `exec(command="curl ... -H 'Authorization: Bearer $TOKEN' ...")` - Make API call
4. If token missing: Tell user how to configure it

For Moltbook specifically:
- Skill: "moltbook"
- Token: "MOLTBOOK_TOKEN"
- Check profile: curl -sf "https://www.moltbook.com/api/v1/agents/me" -H "Authorization: Bearer $MOLTBOOK_TOKEN"

Accomplish what the user asks for. Use the available tools."#,
        workspace.display(),
        if skills.is_empty() { "None".to_string() } else { skills.join(", ") }
    )
}

// ============================================================================
// Skills
// ============================================================================

fn list_available_skills(skills_dir: &str) -> Vec<String> {
    let mut skills = Vec::new();
    if let Ok(entries) = fs::read_dir(skills_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "md").unwrap_or(false) {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    skills.push(name.to_string());
                }
            }
        }
    }
    skills
}

// ============================================================================
// Main Agent Loop
// ============================================================================

async fn run_agent_loop(
    client: &Client,
    endpoint: &str,
    api_key: &str,
    model: &str,
    query: &str,
    workspace: &Path,
    skills: &[String],
    max_iterations: usize,
) -> Result<String, String> {
    let tools = get_code_engineer_tools();
    let system_prompt = get_system_prompt(workspace, skills);

    let mut messages: Vec<Message> = vec![
        Message {
            role: "system".to_string(),
            content: Some(system_prompt),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
        Message {
            role: "user".to_string(),
            content: Some(query.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        },
    ];

    let mut iteration = 0;

    loop {
        iteration += 1;
        println!("\n============================================================");
        println!("📤 ITERATION {} / {}", iteration, max_iterations);
        println!("============================================================");

        if iteration > max_iterations {
            return Err(format!("Max iterations ({}) reached", max_iterations));
        }

        let request = ChatRequest {
            model: model.to_string(),
            messages: messages.clone(),
            max_tokens: 4096,
            tools: Some(tools.clone()),
            tool_choice: Some(json!("auto")),
        };

        println!("\n📋 Sending request to {} (model: {})", endpoint, model);

        let response = client
            .post(endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        let status = response.status();
        let response_text = response.text().await.map_err(|e| format!("Failed to read response: {}", e))?;

        if !status.is_success() {
            println!("\n❌ API Error ({}): {}", status, response_text);
            return Err(format!("API error {}: {}", status, response_text));
        }

        let chat_response: ChatResponse = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse response: {} - body: {}", e, response_text))?;

        let choice = chat_response.choices.first().ok_or("No choices in response")?;

        println!("\n📊 Response:");
        println!("   finish_reason: {:?}", choice.finish_reason);
        if let Some(content) = &choice.message.content {
            let preview = if content.len() > 300 { format!("{}...", &content[..300]) } else { content.clone() };
            println!("   content: {}", preview);
        }
        println!("   tool_calls: {:?}", choice.message.tool_calls.as_ref().map(|t| t.len()));

        // Check for tool calls
        if let Some(tool_calls) = &choice.message.tool_calls {
            if !tool_calls.is_empty() {
                println!("\n🔧 Processing {} tool call(s):", tool_calls.len());

                // Add assistant message with tool calls
                messages.push(Message {
                    role: "assistant".to_string(),
                    content: choice.message.content.clone(),
                    tool_calls: Some(tool_calls.clone()),
                    tool_call_id: None,
                    name: None,
                });

                // Execute each tool
                for tc in tool_calls {
                    println!("\n   📍 Tool: {} (id: {})", tc.function.name, tc.id);

                    let args: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
                    let result = execute_tool(&tc.function.name, &args, workspace).await;

                    messages.push(Message {
                        role: "tool".to_string(),
                        content: Some(result),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                        name: Some(tc.function.name.clone()),
                    });
                }

                continue; // Next iteration
            }
        }

        // No tool calls - final response
        let final_content = choice.message.content.clone().unwrap_or_default();
        println!("\n✅ Final response:");
        println!("{}", final_content);

        return Ok(final_content);
    }
}

// ============================================================================
// Main
// ============================================================================

// ============================================================================
// Gateway Mode - test via external channel endpoint
// ============================================================================

async fn run_gateway_test(
    client: &Client,
    base_url: &str,
    token: &str,
    query: &str,
    session_id: Option<&str>,
    use_stream: bool,
) -> Result<String, String> {
    if use_stream {
        run_gateway_stream(client, base_url, token, query, session_id).await
    } else {
        run_gateway_chat(client, base_url, token, query, session_id).await
    }
}

async fn run_gateway_chat(
    client: &Client,
    base_url: &str,
    token: &str,
    query: &str,
    session_id: Option<&str>,
) -> Result<String, String> {
    let url = format!("{}/api/gateway/chat", base_url);

    let mut body = json!({ "message": query });
    if let Some(sid) = session_id {
        body["session_id"] = json!(sid);
    }

    println!("📤 POST {}", url);
    println!("   Message: {}", query);

    let start = std::time::Instant::now();
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;
    let elapsed = start.elapsed();

    println!("\n📊 Response (HTTP {} in {:.1}s):", status, elapsed.as_secs_f64());

    if !status.is_success() {
        println!("❌ Error: {}", response_text);
        return Err(format!("HTTP {}: {}", status, response_text));
    }

    let parsed: Value = serde_json::from_str(&response_text)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let success = parsed.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    let response_content = parsed.get("response").and_then(|v| v.as_str()).unwrap_or("");
    let session = parsed.get("session_id").and_then(|v| v.as_i64());
    let error = parsed.get("error").and_then(|v| v.as_str());

    if let Some(sid) = session {
        println!("   Session: {}", sid);
    }

    if success {
        println!("\n{}", response_content);
        Ok(response_content.to_string())
    } else {
        let err = error.unwrap_or("Unknown error");
        println!("❌ {}", err);
        Err(err.to_string())
    }
}

async fn run_gateway_stream(
    client: &Client,
    base_url: &str,
    token: &str,
    query: &str,
    session_id: Option<&str>,
) -> Result<String, String> {
    let url = format!("{}/api/gateway/chat/stream", base_url);

    let mut body = json!({ "message": query });
    if let Some(sid) = session_id {
        body["session_id"] = json!(sid);
    }

    println!("📤 POST {} (streaming)", url);
    println!("   Message: {}", query);

    let start = std::time::Instant::now();
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, text));
    }

    println!("\n📊 Streaming response:");

    let mut collected_text = Vec::new();
    let mut bytes_stream = response.bytes_stream();

    use futures_util::StreamExt;
    let mut buffer = String::new();
    while let Some(chunk) = bytes_stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete SSE events
        while let Some(pos) = buffer.find("\n\n") {
            let event_str = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            for line in event_str.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                        let event_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match event_type {
                            "text" => {
                                let content = parsed.get("content").and_then(|v| v.as_str()).unwrap_or("");
                                print!("{}", content);
                                collected_text.push(content.to_string());
                            }
                            "tool_call" => {
                                let tool = parsed.get("tool_name").and_then(|v| v.as_str()).unwrap_or("?");
                                println!("   🔧 Tool call: {}", tool);
                            }
                            "tool_result" => {
                                let tool = parsed.get("tool_name").and_then(|v| v.as_str()).unwrap_or("?");
                                println!("   ✅ Tool result: {}", tool);
                            }
                            "done" => {
                                println!("\n   📍 Stream complete");
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    let elapsed = start.elapsed();
    println!("\n\n⏱️  Completed in {:.1}s", elapsed.as_secs_f64());

    let full_response = collected_text.join("\n\n");
    if full_response.is_empty() {
        Err("No text response received from stream".to_string())
    } else {
        Ok(full_response)
    }
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    println!("🤖 StarkBot Agent Test");
    println!("======================\n");

    // Check for gateway mode (default when EXT_CHANNEL_API_TOKEN is set)
    let gateway_token = env::var("EXT_CHANNEL_API_TOKEN").ok();

    if let Some(token) = gateway_token {
        // ── Gateway mode ──────────────────────────────────────────
        let base_url = env::var("EXT_CHANNEL_URL")
            .unwrap_or_else(|_| "http://localhost:8080".to_string());
        let query = env::var("TEST_QUERY").unwrap_or_else(|_| {
            "tell me the price of bitcoin".to_string()
        });
        let session_id = env::var("TEST_SESSION").ok();
        let use_stream = env::var("TEST_STREAM")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        println!("📝 Mode: Gateway (External Channel)");
        println!("   URL:     {}", base_url);
        println!("   Token:   {}...{}", &token[..8.min(token.len())], &token[token.len().saturating_sub(4)..]);
        println!("   Query:   {}", query);
        println!("   Stream:  {}", use_stream);
        if let Some(ref sid) = session_id {
            println!("   Session: {}", sid);
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .expect("Failed to create HTTP client");

        println!("\n🚀 Sending to gateway...\n");

        match run_gateway_test(
            &client,
            &base_url,
            &token,
            &query,
            session_id.as_deref(),
            use_stream,
        ).await {
            Ok(response) => {
                println!("\n============================================================");
                println!("🎉 SUCCESS");
                println!("============================================================");
                println!("{}", response);
            }
            Err(e) => {
                println!("\n============================================================");
                println!("❌ ERROR");
                println!("============================================================");
                println!("{}", e);
                std::process::exit(1);
            }
        }
    } else {
        // ── Direct mode (legacy) ──────────────────────────────────
        let query = env::var("TEST_QUERY").unwrap_or_else(|_| {
            "Build a simple todo app with TypeScript. Create a basic CLI todo app with add, list, and remove commands.".to_string()
        });

        let endpoint = env::var("TEST_AGENT_ENDPOINT").unwrap_or_else(|_| {
            eprintln!("❌ Neither EXT_CHANNEL_API_TOKEN nor TEST_AGENT_ENDPOINT is set!");
            eprintln!("   Gateway mode: set EXT_CHANNEL_API_TOKEN");
            eprintln!("   Direct mode:  set TEST_AGENT_ENDPOINT and TEST_AGENT_SECRET");
            std::process::exit(1);
        });

        let secret = env::var("TEST_AGENT_SECRET").unwrap_or_else(|_| {
            eprintln!("❌ TEST_AGENT_SECRET not set!");
            std::process::exit(1);
        });

        let model = env::var("TEST_AGENT_MODEL").unwrap_or_else(|_| {
            if endpoint.contains("moonshot") {
                "moonshot-v1-128k".to_string()
            } else if endpoint.contains("anthropic") {
                "claude-sonnet-4-20250514".to_string()
            } else {
                "gpt-4o".to_string()
            }
        });

        let workspace_str = env::var("TEST_WORKSPACE").unwrap_or_else(|_| {
            "/tmp/agent-test-workspace".to_string()
        });
        let workspace = PathBuf::from(&workspace_str);

        let skills_dir = env::var("TEST_SKILLS_DIR").unwrap_or_else(|_| {
            if Path::new("skills").exists() {
                "skills".to_string()
            } else if Path::new("../skills").exists() {
                "../skills".to_string()
            } else {
                "./skills".to_string()
            }
        });

        let max_iterations: usize = env::var("TEST_MAX_ITERATIONS")
            .unwrap_or_else(|_| "25".to_string())
            .parse()
            .unwrap_or(25);

        let skills = list_available_skills(&skills_dir);

        println!("📝 Mode: Direct (LLM endpoint)");
        println!("   Query:      {}", query);
        println!("   Endpoint:   {}", endpoint);
        println!("   Model:      {}", model);
        println!("   Workspace:  {}", workspace.display());
        println!("   Skills:     {} ({} found)", skills_dir, skills.len());
        println!("   Max Iters:  {}", max_iterations);

        // Clean and create workspace
        if workspace.exists() {
            println!("\n🧹 Cleaning existing workspace...");
            let _ = fs::remove_dir_all(&workspace);
        }
        if let Err(e) = fs::create_dir_all(&workspace) {
            eprintln!("❌ Failed to create workspace: {}", e);
            std::process::exit(1);
        }
        println!("✅ Workspace ready: {}", workspace.display());

        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");

        println!("\n🚀 Starting agent loop...\n");

        match run_agent_loop(
            &client,
            &endpoint,
            &secret,
            &model,
            &query,
            &workspace,
            &skills,
            max_iterations,
        ).await {
            Ok(response) => {
                println!("\n============================================================");
                println!("🎉 SUCCESS");
                println!("============================================================");
                println!("{}", response);

                println!("\n📁 Workspace contents:");
                fn list_recursive(path: &Path, prefix: &str) {
                    if let Ok(entries) = fs::read_dir(path) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            let name = p.file_name().unwrap_or_default().to_string_lossy();
                            if p.is_dir() {
                                println!("{}📁 {}/", prefix, name);
                                list_recursive(&p, &format!("{}  ", prefix));
                            } else {
                                println!("{}📄 {}", prefix, name);
                            }
                        }
                    }
                }
                list_recursive(&workspace, "   ");
            }
            Err(e) => {
                println!("\n============================================================");
                println!("❌ ERROR");
                println!("============================================================");
                println!("{}", e);
                std::process::exit(1);
            }
        }
    }
}
