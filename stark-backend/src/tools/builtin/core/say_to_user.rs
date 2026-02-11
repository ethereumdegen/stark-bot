//! Say to user tool for agent communication
//!
//! This tool allows the agent to send messages to the user.
//! Required when tool_choice is set to "any" so the agent always has
//! a way to communicate without performing other actions.
//!
//! When `finished_task` is true, this also terminates the orchestrator loop,
//! acting as both a communication and completion signal.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// Say to user tool for agent communication
pub struct SayToUserTool {
    definition: ToolDefinition,
}

impl SayToUserTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "message".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The message to send to the user. Include ALL relevant details - this is what the user will see.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "finished_task".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Set to true if this message completes the current task and no more tool calls are needed. This will end the agentic loop.".to_string(),
                default: Some(serde_json::Value::Bool(false)),
                items: None,
                enum_values: None,
            },
        );

        SayToUserTool {
            definition: ToolDefinition {
                name: "say_to_user".to_string(),
                description: "Send a message to the user. Use this to communicate results, answers, or status updates. Set finished_task=true when this is your final response.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["message".to_string()],
                },
                group: ToolGroup::System,
            },
        }
    }
}

impl Default for SayToUserTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SayToUserParams {
    message: String,
    #[serde(default)]
    finished_task: bool,
}

#[async_trait]
impl Tool for SayToUserTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, _context: &ToolContext) -> ToolResult {
        let params: SayToUserParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let mut result = ToolResult::success(params.message);

        // Signal to the orchestrator that this completes the task
        if params.finished_task {
            let mut metadata = serde_json::Map::new();
            metadata.insert("finished_task".to_string(), serde_json::Value::Bool(true));
            result.metadata = Some(serde_json::Value::Object(metadata));
        }

        result
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::SafeMode
    }
}
