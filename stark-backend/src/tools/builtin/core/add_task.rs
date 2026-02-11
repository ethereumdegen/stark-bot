//! Add task tool - allows the agent to dynamically insert tasks during execution
//!
//! When the agent discovers additional work is needed (e.g., token approval before swap),
//! it can insert a new task at the front or back of the task queue. The dispatcher
//! intercepts the metadata and modifies the queue accordingly.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for adding tasks to the queue during execution
pub struct AddTaskTool {
    definition: ToolDefinition,
}

impl AddTaskTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "description".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Description of the task to add. Should be specific and actionable."
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "position".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Where to insert the task: 'front' = next task to execute (after current), 'back' = last task in queue."
                        .to_string(),
                default: Some(json!("front")),
                items: None,
                enum_values: Some(vec!["front".to_string(), "back".to_string()]),
            },
        );

        AddTaskTool {
            definition: ToolDefinition {
                name: "add_task".to_string(),
                description: "Add a new task to the task queue. Use 'front' to make it the next task (e.g., approval before swap), or 'back' to add it after all other tasks.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["description".to_string()],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }
}

impl Default for AddTaskTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct AddTaskParams {
    description: String,
    #[serde(default = "default_position")]
    position: String,
}

fn default_position() -> String {
    "front".to_string()
}

#[async_trait]
impl Tool for AddTaskTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, _context: &ToolContext) -> ToolResult {
        let params: AddTaskParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let position = match params.position.as_str() {
            "front" | "back" => params.position.clone(),
            other => {
                return ToolResult::error(format!(
                    "Invalid position '{}'. Must be 'front' or 'back'.",
                    other
                ))
            }
        };

        if params.description.trim().is_empty() {
            return ToolResult::error("Task description cannot be empty.");
        }

        // Return metadata for the dispatcher to intercept and modify the queue
        ToolResult::success(format!(
            "Task added ({}): {}",
            position, params.description
        ))
        .with_metadata(json!({
            "add_task": true,
            "task_description": params.description,
            "task_position": position
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_task_definition() {
        let tool = AddTaskTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "add_task");
        assert_eq!(def.group, ToolGroup::System);
        assert!(def.input_schema.required.contains(&"description".to_string()));
    }

    #[tokio::test]
    async fn test_add_task_front() {
        let tool = AddTaskTool::new();
        let context = ToolContext::default();
        let result = tool
            .execute(
                json!({"description": "Approve tokens", "position": "front"}),
                &context,
            )
            .await;

        assert!(result.success);
        let metadata = result.metadata.unwrap();
        assert_eq!(metadata["add_task"], true);
        assert_eq!(metadata["task_description"], "Approve tokens");
        assert_eq!(metadata["task_position"], "front");
    }

    #[tokio::test]
    async fn test_add_task_back() {
        let tool = AddTaskTool::new();
        let context = ToolContext::default();
        let result = tool
            .execute(
                json!({"description": "Verify result", "position": "back"}),
                &context,
            )
            .await;

        assert!(result.success);
        let metadata = result.metadata.unwrap();
        assert_eq!(metadata["task_position"], "back");
    }

    #[tokio::test]
    async fn test_add_task_default_position() {
        let tool = AddTaskTool::new();
        let context = ToolContext::default();
        let result = tool
            .execute(json!({"description": "Some task"}), &context)
            .await;

        assert!(result.success);
        let metadata = result.metadata.unwrap();
        assert_eq!(metadata["task_position"], "front");
    }

    #[tokio::test]
    async fn test_add_task_invalid_position() {
        let tool = AddTaskTool::new();
        let context = ToolContext::default();
        let result = tool
            .execute(
                json!({"description": "Some task", "position": "middle"}),
                &context,
            )
            .await;

        assert!(!result.success);
        assert!(result.content.contains("Invalid position"));
    }

    #[tokio::test]
    async fn test_add_task_empty_description() {
        let tool = AddTaskTool::new();
        let context = ToolContext::default();
        let result = tool
            .execute(
                json!({"description": "  ", "position": "front"}),
                &context,
            )
            .await;

        assert!(!result.success);
        assert!(result.content.contains("cannot be empty"));
    }
}
