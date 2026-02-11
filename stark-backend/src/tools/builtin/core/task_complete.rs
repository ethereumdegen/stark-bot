//! Task fully completed tool - allows agent to signal it has finished the current task
//!
//! This tool terminates the agentic loop, preventing unnecessary continued iteration.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for signaling task completion
pub struct TaskFullyCompletedTool {
    definition: ToolDefinition,
}

impl TaskFullyCompletedTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "summary".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "A brief summary of what was accomplished (internal log only — NOT shown to the user on social channels). Use say_to_user if you need the user to see your response.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        TaskFullyCompletedTool {
            definition: ToolDefinition {
                name: "task_fully_completed".to_string(),
                description: "Signal that the current task is FULLY complete and no more tool calls are needed. Use this when you have gathered all necessary information and are ready to respond to the user. This stops the agentic loop. Only call this when you are truly done.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["summary".to_string()],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }
}

impl Default for TaskFullyCompletedTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct TaskFullyCompletedParams {
    summary: String,
}

#[async_trait]
impl Tool for TaskFullyCompletedTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, _context: &ToolContext) -> ToolResult {
        let params: TaskFullyCompletedParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Return success with metadata indicating task is complete
        // The orchestrator will handle this specially
        ToolResult::success(format!("Task fully completed: {}", params.summary))
            .with_metadata(json!({
                "task_fully_completed": true,
                "summary": params.summary
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_fully_completed_definition() {
        let tool = TaskFullyCompletedTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "task_fully_completed");
        assert_eq!(def.group, ToolGroup::System);
        assert!(def.input_schema.required.contains(&"summary".to_string()));
    }
}
