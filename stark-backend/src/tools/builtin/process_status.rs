//! Process status tool for checking and managing background processes
//!
//! This tool allows agents to:
//! - Check the status of a background process
//! - Get recent output from a process
//! - Kill a running process
//! - List all processes for the current channel

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for checking and managing background processes
pub struct ProcessStatusTool {
    definition: ToolDefinition,
}

impl ProcessStatusTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "operation".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Operation to perform: 'status' (check process), 'output' (get recent output), 'kill' (terminate process), 'list' (list all processes)".to_string(),
                default: Some(json!("status")),
                items: None,
                enum_values: Some(vec![
                    "status".to_string(),
                    "output".to_string(),
                    "kill".to_string(),
                    "list".to_string(),
                ]),
            },
        );

        properties.insert(
            "process_id".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The process ID (e.g., 'proc_1') returned from exec with background: true. Required for status/output/kill operations.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "lines".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Number of output lines to retrieve (for 'output' operation, default: 50)".to_string(),
                default: Some(json!(50)),
                items: None,
                enum_values: None,
            },
        );

        ProcessStatusTool {
            definition: ToolDefinition {
                name: "process_status".to_string(),
                description: "Check status, get output, or manage background processes started with exec. Use after starting a background process to monitor its progress.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["operation".to_string()],
                },
                group: ToolGroup::Exec,
            },
        }
    }
}

impl Default for ProcessStatusTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ProcessStatusParams {
    operation: String,
    process_id: Option<String>,
    #[serde(default)]
    lines: Option<usize>,
}

#[async_trait]
impl Tool for ProcessStatusTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ProcessStatusParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get ProcessManager from context
        let process_manager = match context.process_manager.as_ref() {
            Some(pm) => pm,
            None => {
                return ToolResult::error(
                    "ProcessManager not available. Background process tracking is not enabled.",
                );
            }
        };

        let channel_id = context.channel_id.unwrap_or(0);

        match params.operation.as_str() {
            "status" => {
                let process_id = match params.process_id {
                    Some(id) => id,
                    None => {
                        return ToolResult::error(
                            "process_id is required for 'status' operation",
                        );
                    }
                };

                match process_manager.get(&process_id) {
                    Some(info) => {
                        let status_str = info.status.to_string();
                        let result = format!(
                            "Process: {}\n\
                            Status: {}\n\
                            PID: {}\n\
                            Command: {}\n\
                            Duration: {}ms",
                            info.id,
                            status_str,
                            info.pid.map(|p| p.to_string()).unwrap_or_else(|| "N/A".to_string()),
                            info.command,
                            info.duration_ms
                        );
                        ToolResult::success(result).with_metadata(info.to_json())
                    }
                    None => ToolResult::error(format!("Process '{}' not found", process_id)),
                }
            }

            "output" => {
                let process_id = match params.process_id {
                    Some(id) => id,
                    None => {
                        return ToolResult::error(
                            "process_id is required for 'output' operation",
                        );
                    }
                };

                let lines = params.lines.unwrap_or(50);

                match process_manager.output(&process_id, lines) {
                    Some(output_lines) => {
                        if output_lines.is_empty() {
                            ToolResult::success(format!(
                                "No output captured yet for process '{}'",
                                process_id
                            ))
                        } else {
                            let output = output_lines.join("\n");
                            ToolResult::success(format!(
                                "Output from process '{}' (last {} lines):\n\n{}",
                                process_id,
                                output_lines.len(),
                                output
                            )).with_metadata(json!({
                                "process_id": process_id,
                                "line_count": output_lines.len()
                            }))
                        }
                    }
                    None => ToolResult::error(format!("Process '{}' not found", process_id)),
                }
            }

            "kill" => {
                let process_id = match params.process_id {
                    Some(id) => id,
                    None => {
                        return ToolResult::error("process_id is required for 'kill' operation");
                    }
                };

                if process_manager.kill(&process_id).await {
                    ToolResult::success(format!("Process '{}' has been killed", process_id))
                        .with_metadata(json!({
                            "process_id": process_id,
                            "killed": true
                        }))
                } else {
                    // Check if it exists but isn't running
                    match process_manager.status(&process_id) {
                        Some(status) => ToolResult::error(format!(
                            "Cannot kill process '{}': status is {}",
                            process_id, status
                        )),
                        None => ToolResult::error(format!("Process '{}' not found", process_id)),
                    }
                }
            }

            "list" => {
                let processes = process_manager.list_for_channel(channel_id);

                if processes.is_empty() {
                    return ToolResult::success("No background processes found for this channel.");
                }

                let mut result = String::from("Background processes:\n\n");
                for proc in &processes {
                    result.push_str(&format!(
                        "- {} ({}): {}\n  Command: {}\n  Duration: {}ms\n\n",
                        proc.id,
                        proc.pid.map(|p| format!("PID {}", p)).unwrap_or_else(|| "no PID".to_string()),
                        proc.status,
                        if proc.command.len() > 50 {
                            format!("{}...", &proc.command[..47])
                        } else {
                            proc.command.clone()
                        },
                        proc.duration_ms
                    ));
                }

                let metadata: Vec<Value> = processes.iter().map(|p| p.to_json()).collect();
                ToolResult::success(result).with_metadata(json!({
                    "processes": metadata,
                    "count": processes.len()
                }))
            }

            _ => ToolResult::error(format!(
                "Unknown operation '{}'. Use: status, output, kill, or list",
                params.operation
            )),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let tool = ProcessStatusTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "process_status");
        assert!(def.input_schema.properties.contains_key("operation"));
        assert!(def.input_schema.properties.contains_key("process_id"));
        assert!(def.input_schema.properties.contains_key("lines"));
    }

    #[tokio::test]
    async fn test_no_process_manager() {
        let tool = ProcessStatusTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({"operation": "list"}), &context)
            .await;

        assert!(!result.success);
        assert!(result.content.contains("ProcessManager not available"));
    }

    #[tokio::test]
    async fn test_missing_process_id() {
        let tool = ProcessStatusTool::new();
        let context = ToolContext::new();

        // Even without ProcessManager, we should get a proper error for missing process_id
        let result = tool
            .execute(json!({"operation": "status"}), &context)
            .await;

        // This will fail at ProcessManager check first
        assert!(!result.success);
    }
}
