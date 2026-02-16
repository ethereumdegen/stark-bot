use crate::ai::multi_agent::types::{self, AgentSubtype};
use crate::gateway::protocol::GatewayEvent;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool to switch between agent subtypes (dynamic, config-driven toolboxes).
/// This controls which tools and skills are available to the agent.
///
/// IMPORTANT: This tool MUST be called FIRST before any other tools can be used.
/// The agent starts with no subtype selected and must choose based on the user's request.
pub struct SetAgentSubtypeTool;

impl SetAgentSubtypeTool {
    pub fn new() -> Self {
        SetAgentSubtypeTool
    }

    /// Build the tool definition dynamically from the registry.
    fn build_definition() -> ToolDefinition {
        let configs = types::all_subtype_configs();

        // Build enum_values and description dynamically
        let enum_values: Vec<String> = configs.iter().map(|c| c.key.clone()).collect();
        let desc_lines: Vec<String> = configs
            .iter()
            .map(|c| format!("• '{}' - {}", c.key, c.description))
            .collect();

        let param_desc = format!(
            "The agent subtype/toolbox to activate:\n{}",
            desc_lines.join("\n")
        );

        let tool_desc_lines: Vec<String> = configs
            .iter()
            .map(|c| format!("• '{}' - {}", c.key, c.description))
            .collect();

        let tool_desc = format!(
            "⚡ REQUIRED FIRST TOOL: Select your toolbox before doing anything else!\n\n\
             You MUST call this tool FIRST based on what the user wants:\n\
             {}\n\n\
             Choose based on the user's request, then proceed with the appropriate tools.\n\n\
             Note: Agent identity/registration (EIP-8004) skills are available in ALL subtypes.",
            tool_desc_lines.join("\n")
        );

        let mut properties = HashMap::new();
        properties.insert(
            "subtype".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: param_desc,
                default: None,
                items: None,
                enum_values: Some(enum_values),
            },
        );

        ToolDefinition {
            name: "set_agent_subtype".to_string(),
            description: tool_desc,
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties,
                required: vec!["subtype".to_string()],
            },
            group: ToolGroup::System,
            hidden: false,
        }
    }

    /// Get a description of available tools for a subtype (from registry prompt).
    fn describe_subtype(key: &str) -> String {
        if let Some(config) = types::get_subtype_config(key) {
            return config.prompt;
        }
        // Fallback for None
        "❓ No toolbox selected. Call set_agent_subtype first!".to_string()
    }
}

impl Default for SetAgentSubtypeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SetAgentSubtypeParams {
    subtype: String,
}

#[async_trait]
impl Tool for SetAgentSubtypeTool {
    fn definition(&self) -> ToolDefinition {
        Self::build_definition()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: SetAgentSubtypeParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let key = params.subtype.to_lowercase();

        // Validate against registry
        let config = match types::get_subtype_config(&key) {
            Some(c) => c,
            None => {
                // Try AgentSubtype::from_str for alias resolution
                match AgentSubtype::from_str(&key) {
                    Some(s) => {
                        // Resolved via alias — get config for the canonical key
                        match types::get_subtype_config(s.as_str()) {
                            Some(c) => c,
                            None => {
                                let valid: Vec<String> = types::all_subtype_configs()
                                    .iter()
                                    .map(|c| format!("'{}'", c.key))
                                    .collect();
                                return ToolResult::error(format!(
                                    "Invalid subtype '{}'. Valid options: {}",
                                    params.subtype,
                                    valid.join(", ")
                                ));
                            }
                        }
                    }
                    None => {
                        let valid: Vec<String> = types::all_subtype_configs()
                            .iter()
                            .map(|c| format!("'{}'", c.key))
                            .collect();
                        return ToolResult::error(format!(
                            "Invalid subtype '{}'. Valid options: {}",
                            params.subtype,
                            valid.join(", ")
                        ));
                    }
                }
            }
        };

        // Broadcast the subtype change event
        if let (Some(broadcaster), Some(channel_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::agent_subtype_change(
                channel_id,
                &config.key,
                &config.label,
            ));
        }

        // Build tool groups from config
        let tool_groups: Vec<&str> = {
            let mut groups = vec!["system", "web", "filesystem"];
            for g in &config.tool_groups {
                let gs = g.as_str();
                if !groups.contains(&gs) {
                    groups.push(gs);
                }
            }
            groups
        };

        // Return success with description of available tools
        let description = Self::describe_subtype(&config.key);
        ToolResult::success(description).with_metadata(json!({
            "subtype": config.key,
            "label": config.label,
            "emoji": config.emoji,
            "allowed_tool_groups": tool_groups,
            "allowed_skill_tags": config.skill_tags,
        }))
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::SafeMode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_set_subtype_finance() {
        // Load built-in defaults so registry is populated
        types::load_subtype_registry(types::load_test_subtypes());

        let tool = SetAgentSubtypeTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "subtype": "finance" }), &context)
            .await;

        assert!(result.success);
        assert!(result.content.contains("Finance toolbox"));
    }

    #[tokio::test]
    async fn test_set_subtype_code_engineer() {
        types::load_subtype_registry(types::load_test_subtypes());

        let tool = SetAgentSubtypeTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "subtype": "code_engineer" }), &context)
            .await;

        assert!(result.success);
        assert!(result.content.contains("CodeEngineer toolbox"));
    }

    #[tokio::test]
    async fn test_set_subtype_secretary() {
        types::load_subtype_registry(types::load_test_subtypes());

        let tool = SetAgentSubtypeTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "subtype": "secretary" }), &context)
            .await;

        assert!(result.success);
        assert!(result.content.contains("Secretary toolbox"));
    }

    #[tokio::test]
    async fn test_invalid_subtype() {
        types::load_subtype_registry(types::load_test_subtypes());

        let tool = SetAgentSubtypeTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "subtype": "invalid" }), &context)
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Invalid subtype"));
    }
}
