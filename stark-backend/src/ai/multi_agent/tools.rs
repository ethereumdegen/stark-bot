//! Multi-agent specific tools for task management
//!
//! These tools are designed for OpenAI-compatible APIs (Kimi, etc.)

use crate::tools::{PropertySchema, ToolDefinition, ToolGroup, ToolInputSchema};
use std::collections::HashMap;

// =============================================================================
// TASK PLANNER TOOLS
// =============================================================================

/// Create the `define_tasks` tool for the task planner mode
pub fn define_tasks_tool() -> ToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "tasks".to_string(),
        PropertySchema {
            schema_type: "array".to_string(),
            description: "List of task descriptions to execute in order".to_string(),
            default: None,
            items: Some(Box::new(PropertySchema {
                schema_type: "string".to_string(),
                description: "A specific, actionable task description".to_string(),
                default: None,
                items: None,
                enum_values: None,
            })),
            enum_values: None,
        },
    );

    ToolDefinition {
        name: "define_tasks".to_string(),
        description: "Define the list of tasks to accomplish the user's request. Each task should be specific and actionable. Tasks will be executed in order.".to_string(),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties,
            required: vec!["tasks".to_string()],
        },
        group: ToolGroup::System,
                hidden: false,
    }
}

/// Get tools available in task planner mode
pub fn get_planner_tools() -> Vec<ToolDefinition> {
    vec![define_tasks_tool()]
}

// =============================================================================
// TOOL SETS
// =============================================================================

/// Get tools for the specified mode
/// Note: define_tasks is now a registered tool in the main registry,
/// so TaskPlanner no longer needs to add it here (that caused duplication errors with Kimi).
pub fn get_tools_for_mode(mode: super::types::AgentMode) -> Vec<ToolDefinition> {
    match mode {
        super::types::AgentMode::TaskPlanner => vec![], // define_tasks is in the registry
        super::types::AgentMode::Assistant => vec![],
    }
}

/// Get all multi-agent tools (for reference)
pub fn get_all_tools() -> Vec<ToolDefinition> {
    vec![define_tasks_tool()]
}
