use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{ExecutionTask, TaskMetrics, TaskStatus, TaskType};
use dashmap::DashMap;
use std::sync::Arc;

/// Tracks execution progress for agent tasks
///
/// This service manages the hierarchical task tree for execution tracking,
/// emitting real-time events for the frontend to display progress.
pub struct ExecutionTracker {
    /// Event broadcaster for sending gateway events
    broadcaster: Arc<EventBroadcaster>,
    /// Active tasks indexed by task ID
    tasks: DashMap<String, ExecutionTask>,
    /// Maps channel_id to current execution_id
    channel_executions: DashMap<i64, String>,
    /// Channels that have been cancelled (checked by tool loops)
    cancelled_channels: DashMap<i64, bool>,
}

impl ExecutionTracker {
    /// Create a new ExecutionTracker
    pub fn new(broadcaster: Arc<EventBroadcaster>) -> Self {
        Self {
            broadcaster,
            tasks: DashMap::new(),
            channel_executions: DashMap::new(),
            cancelled_channels: DashMap::new(),
        }
    }

    /// Cancel any ongoing execution for a channel
    /// This sets a flag that tool loops should check to exit early
    pub fn cancel_execution(&self, channel_id: i64) {
        log::info!("[EXECUTION_TRACKER] Cancelling execution for channel {}", channel_id);
        self.cancelled_channels.insert(channel_id, true);

        // Also complete/abort the current execution if there is one
        self.complete_execution(channel_id);
    }

    /// Check if a channel's execution has been cancelled
    pub fn is_cancelled(&self, channel_id: i64) -> bool {
        self.cancelled_channels.get(&channel_id).map(|v| *v).unwrap_or(false)
    }

    /// Clear the cancellation flag for a channel (called when starting new execution)
    pub fn clear_cancellation(&self, channel_id: i64) {
        self.cancelled_channels.remove(&channel_id);
    }

    /// Start a new execution for a channel
    ///
    /// Returns the execution ID (which is also the root task ID)
    pub fn start_execution(&self, channel_id: i64, mode: &str, user_message: Option<&str>) -> String {
        // Clear any previous cancellation flag
        self.clear_cancellation(channel_id);

        // Create descriptive execution task based on user message
        let (description, active_form) = match user_message {
            Some(msg) => {
                let truncated = if msg.len() > 60 {
                    format!("{}...", &msg[..57])
                } else {
                    msg.to_string()
                };
                let short = if msg.len() > 30 {
                    format!("{}...", &msg[..27])
                } else {
                    msg.to_string()
                };
                (truncated, short)
            }
            None => {
                let desc = if mode == "plan" { "Planning..." } else { "Processing..." };
                (desc.to_string(), desc.to_string())
            }
        };

        let mut task = ExecutionTask::new(
            channel_id,
            TaskType::Execution,
            description,
            None,
        ).with_active_form(active_form);
        task.start();

        let execution_id = task.id.clone();

        // Track the execution
        self.channel_executions.insert(channel_id, execution_id.clone());
        self.tasks.insert(execution_id.clone(), task.clone());

        // Emit event with description
        self.broadcaster.broadcast(GatewayEvent::execution_started(
            channel_id,
            &execution_id,
            mode,
            &task.description,
            task.active_form.as_deref().unwrap_or(&task.description),
        ));
        self.broadcaster.broadcast(GatewayEvent::task_started(&task, &execution_id));

        execution_id
    }

    /// Get the current execution ID for a channel
    pub fn get_execution_id(&self, channel_id: i64) -> Option<String> {
        self.channel_executions.get(&channel_id).map(|v| v.clone())
    }

    /// Add a thinking event to the current execution
    pub fn add_thinking(&self, channel_id: i64, text: &str) {
        if let Some(execution_id) = self.get_execution_id(channel_id) {
            self.broadcaster.broadcast(GatewayEvent::execution_thinking(
                channel_id,
                &execution_id,
                text,
            ));
        }
    }

    /// Start a new task within an execution
    ///
    /// Returns the task ID
    pub fn start_task(
        &self,
        channel_id: i64,
        execution_id: &str,
        parent_id: Option<&str>,
        task_type: TaskType,
        description: impl Into<String>,
        active_form: Option<&str>,
    ) -> String {
        let description_str = description.into();
        let mut task = ExecutionTask::new(
            channel_id,
            task_type,
            description_str.clone(),
            parent_id.map(|s| s.to_string()),
        );

        if let Some(form) = active_form {
            task.active_form = Some(form.to_string());
        }

        task.start();
        let task_id = task.id.clone();

        // Update parent's child count
        if let Some(pid) = parent_id {
            if let Some(mut parent) = self.tasks.get_mut(pid) {
                parent.metrics.child_count += 1;
            }
        }

        // Store and emit
        self.tasks.insert(task_id.clone(), task.clone());
        self.broadcaster.broadcast(GatewayEvent::task_started(&task, execution_id));

        task_id
    }

    /// Start a tool execution task
    ///
    /// Convenience wrapper for starting tool executions with context from arguments
    pub fn start_tool(
        &self,
        channel_id: i64,
        execution_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> String {
        // Extract context from tool arguments for better descriptions
        let (description, active_form) = Self::describe_tool_call(tool_name, arguments);

        self.start_task(
            channel_id,
            execution_id,
            Some(execution_id),  // Parent is the execution itself
            TaskType::ToolExecution,
            description,
            Some(&active_form),
        )
    }

    /// Generate human-readable description and active form for a tool call
    fn describe_tool_call(tool_name: &str, args: &serde_json::Value) -> (String, String) {
        match tool_name {
            // File operations
            "read_file" | "read" => {
                let path = args.get("path")
                    .or_else(|| args.get("file_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("file");
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                (format!("Reading {}", filename), format!("Reading {}", filename))
            }
            "write_file" | "write" => {
                let path = args.get("path")
                    .or_else(|| args.get("file_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("file");
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                (format!("Writing {}", filename), format!("Writing {}", filename))
            }
            "list_files" | "list" => {
                let path = args.get("path")
                    .or_else(|| args.get("directory"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                (format!("Listing {}", path), "Listing files".to_string())
            }
            "apply_patch" => {
                let path = args.get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("file");
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                (format!("Patching {}", filename), format!("Patching {}", filename))
            }

            // Web operations
            "web_fetch" => {
                let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let host = url.split("://")
                    .nth(1)
                    .unwrap_or(url)
                    .split('/')
                    .next()
                    .unwrap_or(url);
                (format!("Fetching {}", host), format!("Fetching {}", host))
            }
            // Shell/exec operations
            "exec" | "shell" | "bash" => {
                let cmd = args.get("command")
                    .or_else(|| args.get("cmd"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // Extract first word of command
                let first_word = cmd.split_whitespace().next().unwrap_or("command");

                // Special handling for curl to show the URL
                if first_word == "curl" {
                    // Try to extract URL from curl command
                    let url = Self::extract_curl_url(cmd);
                    if let Some(url) = url {
                        let host = url.split("://")
                            .nth(1)
                            .unwrap_or(&url)
                            .split('/')
                            .next()
                            .unwrap_or(&url);
                        let short_url = if url.len() > 60 {
                            format!("{}...", &url[..57])
                        } else {
                            url.to_string()
                        };
                        (format!("curl {}", short_url), format!("Calling {}", host))
                    } else {
                        ("Running curl".to_string(), "Running curl".to_string())
                    }
                } else {
                    let short_cmd = if cmd.len() > 50 {
                        format!("{}...", &cmd[..47])
                    } else {
                        cmd.to_string()
                    };
                    (format!("Running: {}", short_cmd), format!("Running {}", first_word))
                }
            }

            // Skill operations
            "use_skill" => {
                let skill = args.get("skill_name")
                    .or_else(|| args.get("skill"))
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("skill");
                let input = args.get("input")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let short_input = if input.len() > 40 {
                    format!("{}...", &input[..37])
                } else {
                    input.to_string()
                };
                if input.is_empty() {
                    (format!("Using skill: {}", skill), format!("Using {}", skill))
                } else {
                    (format!("Skill {}: {}", skill, short_input), format!("Using {}", skill))
                }
            }

            // Agent/subagent operations
            "spawn_agent" | "subagent" => {
                let task = args.get("task")
                    .or_else(|| args.get("description"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("task");
                let short_task = if task.len() > 40 {
                    format!("{}...", &task[..37])
                } else {
                    task.to_string()
                };
                (format!("Agent: {}", short_task), "Running agent".to_string())
            }

            // Memory operations
            "remember" | "memory_store" => {
                ("Storing memory".to_string(), "Storing memory".to_string())
            }
            "recall" | "memory_search" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("...");
                let short = if query.len() > 30 {
                    format!("{}...", &query[..27])
                } else {
                    query.to_string()
                };
                (format!("Recalling: {}", short), "Searching memory".to_string())
            }

            // Message operations
            "send_message" => {
                let channel = args.get("channel")
                    .or_else(|| args.get("channel_id"))
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "channel".to_string());
                (format!("Sending to {}", channel), "Sending message".to_string())
            }

            // User interaction
            "ask_user" => {
                let question = args.get("question").and_then(|v| v.as_str()).unwrap_or("question");
                let short_q = if question.len() > 30 {
                    format!("{}...", &question[..30])
                } else {
                    question.to_string()
                };
                (format!("Asking: {}", short_q), "Asking user".to_string())
            }

            // RPC operations
            "x402_rpc" => {
                let method = args.get("method").and_then(|v| v.as_str()).unwrap_or("call");
                let network = args.get("network").and_then(|v| v.as_str()).unwrap_or("base");
                (format!("RPC {} on {}", method, network), format!("RPC {}", method))
            }

            // Default fallback
            _ => {
                (format!("Using {}", tool_name), format!("Running {}", tool_name))
            }
        }
    }

    /// Extract URL from a curl command string
    fn extract_curl_url(cmd: &str) -> Option<String> {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let mut i = 0;
        while i < parts.len() {
            let part = parts[i];
            // Skip curl itself
            if part == "curl" {
                i += 1;
                continue;
            }
            // Skip flags that take arguments
            if part.starts_with('-') {
                // Common curl flags that take an argument
                let flags_with_args = [
                    "-H", "--header",
                    "-d", "--data", "--data-raw", "--data-binary",
                    "-X", "--request",
                    "-o", "--output",
                    "-u", "--user",
                    "-A", "--user-agent",
                    "-e", "--referer",
                    "-b", "--cookie",
                    "-c", "--cookie-jar",
                    "-F", "--form",
                    "-T", "--upload-file",
                    "--connect-timeout",
                    "--max-time",
                    "-m",
                ];
                if flags_with_args.iter().any(|f| *f == part) {
                    i += 2; // Skip flag and its argument
                    continue;
                }
                i += 1;
                continue;
            }
            // This should be the URL (or a positional argument)
            if part.starts_with("http://") || part.starts_with("https://") {
                return Some(part.to_string());
            }
            // Could be a URL without quotes that got split, or just a path
            if part.contains("://") || part.contains('.') {
                return Some(part.to_string());
            }
            i += 1;
        }
        None
    }

    /// Update task metrics
    pub fn update_task_metrics(&self, task_id: &str, metrics: TaskMetrics) {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.metrics = metrics.clone();
            self.broadcaster.broadcast(GatewayEvent::task_updated(
                task_id,
                task.channel_id,
                &metrics,
            ));
        }
    }

    /// Add metrics to existing task
    pub fn add_to_task_metrics(&self, task_id: &str, tool_uses: u32, tokens: u32, lines: u32) {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.metrics.tool_uses += tool_uses;
            task.metrics.tokens_used += tokens;
            task.metrics.lines_read += lines;

            self.broadcaster.broadcast(GatewayEvent::task_updated(
                task_id,
                task.channel_id,
                &task.metrics.clone(),
            ));
        }
    }

    /// Update the active form (progress text) of a task
    pub fn update_task_active_form(&self, task_id: &str, active_form: &str) {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.active_form = Some(active_form.to_string());
            // Broadcast update with current metrics and active form
            self.broadcaster.broadcast(GatewayEvent::task_updated_with_active_form(
                task_id,
                task.channel_id,
                &task.metrics.clone(),
                active_form,
            ));
        }
    }

    /// Complete a task successfully
    pub fn complete_task(&self, task_id: &str) {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.complete();
            self.broadcaster.broadcast(GatewayEvent::task_completed(
                task_id,
                task.channel_id,
                "completed",
                &task.metrics,
            ));
        }
    }

    /// Complete a task with an error
    pub fn complete_task_with_error(&self, task_id: &str, error: &str) {
        if let Some(mut task) = self.tasks.get_mut(task_id) {
            task.complete_with_error(error);
            self.broadcaster.broadcast(GatewayEvent::task_completed(
                task_id,
                task.channel_id,
                &format!("error: {}", error),
                &task.metrics,
            ));
        }
    }

    /// Complete an entire execution
    ///
    /// Aggregates metrics from all child tasks
    pub fn complete_execution(&self, channel_id: i64) {
        if let Some((_, execution_id)) = self.channel_executions.remove(&channel_id) {
            // Aggregate metrics from all tasks in this execution
            let mut total_metrics = TaskMetrics::default();
            let mut task_ids_to_remove = Vec::new();

            for entry in self.tasks.iter() {
                let task = entry.value();
                if task.channel_id == channel_id {
                    total_metrics.tool_uses += task.metrics.tool_uses;
                    total_metrics.tokens_used += task.metrics.tokens_used;
                    total_metrics.lines_read += task.metrics.lines_read;
                    task_ids_to_remove.push(entry.key().clone());
                }
            }

            // Complete the root task
            if let Some(mut root_task) = self.tasks.get_mut(&execution_id) {
                root_task.complete();
                total_metrics.duration_ms = root_task.metrics.duration_ms;
            }

            // Emit completion event
            self.broadcaster.broadcast(GatewayEvent::execution_completed(
                channel_id,
                &execution_id,
                &total_metrics,
            ));

            // Clean up tasks for this execution
            for task_id in task_ids_to_remove {
                self.tasks.remove(&task_id);
            }
        }
    }

    /// Get a task by ID
    pub fn get_task(&self, task_id: &str) -> Option<ExecutionTask> {
        self.tasks.get(task_id).map(|t| t.clone())
    }

    /// Get all tasks for a channel
    pub fn get_channel_tasks(&self, channel_id: i64) -> Vec<ExecutionTask> {
        self.tasks
            .iter()
            .filter(|entry| entry.value().channel_id == channel_id)
            .map(|entry| entry.value().clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tracker() -> ExecutionTracker {
        let broadcaster = Arc::new(EventBroadcaster::new());
        ExecutionTracker::new(broadcaster)
    }

    #[test]
    fn test_execution_lifecycle() {
        let tracker = create_test_tracker();

        // Start execution
        let execution_id = tracker.start_execution(1, "execute", Some("Test execution"));
        assert!(!execution_id.is_empty());
        assert!(tracker.get_execution_id(1).is_some());

        // Start a tool task with arguments
        let args = serde_json::json!({"url": "https://example.com/api"});
        let tool_id = tracker.start_tool(1, &execution_id, "web_fetch", &args);
        assert!(!tool_id.is_empty());

        // Verify the task has a descriptive name
        let task = tracker.get_task(&tool_id).unwrap();
        assert!(task.description.contains("example.com"));

        // Complete the tool
        tracker.complete_task(&tool_id);
        let task = tracker.get_task(&tool_id).unwrap();
        assert!(matches!(task.status, TaskStatus::Completed));

        // Complete execution
        tracker.complete_execution(1);
        assert!(tracker.get_execution_id(1).is_none());
    }

    #[test]
    fn test_metrics_aggregation() {
        let tracker = create_test_tracker();

        let execution_id = tracker.start_execution(1, "execute", Some("Test execution"));

        // Start multiple tools with arguments
        let args1 = serde_json::json!({"path": "/tmp/test.txt"});
        let tool1 = tracker.start_tool(1, &execution_id, "read_file", &args1);
        tracker.add_to_task_metrics(&tool1, 1, 100, 10);
        tracker.complete_task(&tool1);

        let args2 = serde_json::json!({"url": "https://example.com"});
        let tool2 = tracker.start_tool(1, &execution_id, "web_fetch", &args2);
        tracker.add_to_task_metrics(&tool2, 1, 200, 20);
        tracker.complete_task(&tool2);

        // Check that metrics are tracked
        let task1 = tracker.get_task(&tool1).unwrap();
        assert_eq!(task1.metrics.tool_uses, 1);
        assert_eq!(task1.metrics.tokens_used, 100);
        assert!(task1.description.contains("test.txt"));

        let task2 = tracker.get_task(&tool2).unwrap();
        assert_eq!(task2.metrics.tool_uses, 1);
        assert_eq!(task2.metrics.tokens_used, 200);
        assert!(task2.description.contains("example.com"));
    }

    #[test]
    fn test_tool_descriptions() {
        // Test that various tools get nice descriptions
        let test_cases = vec![
            ("read_file", serde_json::json!({"path": "/home/user/docs/readme.md"}), "readme.md"),
            ("web_fetch", serde_json::json!({"url": "https://api.github.com/repos"}), "api.github.com"),
            ("exec", serde_json::json!({"command": "git status"}), "git status"),
            ("use_skill", serde_json::json!({"skill": "weather"}), "weather"),
            ("list_files", serde_json::json!({"path": "/home/user/projects"}), "/home/user/projects"),
        ];

        for (tool_name, args, expected_substr) in test_cases {
            let (desc, _) = ExecutionTracker::describe_tool_call(tool_name, &args);
            assert!(
                desc.contains(expected_substr),
                "Tool '{}' description '{}' should contain '{}'",
                tool_name, desc, expected_substr
            );
        }
    }
}
