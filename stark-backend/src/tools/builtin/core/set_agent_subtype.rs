use crate::ai::multi_agent::types;
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
            "agent_subtype".to_string(),
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
                required: vec!["agent_subtype".to_string()],
            },
            group: ToolGroup::System,
            hidden: false,
        }
    }

    /// Get a description of available tools for a subtype (from registry prompt).
    /// Replaces `{available_skills}` with a dynamically generated skill list from the DB,
    /// and `{subagent_overview}` with a summary of all subtypes and their skills.
    fn describe_subtype(key: &str, context: &ToolContext) -> String {
        let config = match types::get_subtype_config(key) {
            Some(c) => c,
            None => return "❓ No toolbox selected. Call set_agent_subtype first!".to_string(),
        };

        let mut prompt = config.prompt.clone();

        // Replace {available_skills} with dynamically generated skill list
        if prompt.contains("{available_skills}") {
            let skills_section = Self::generate_skills_section(key, context);
            prompt = prompt.replace("{available_skills}", &skills_section);
        }

        // Replace {subagent_overview} with overview of all subtypes + their skills
        if prompt.contains("{subagent_overview}") {
            let overview = Self::generate_subagent_overview(context);
            prompt = prompt.replace("{subagent_overview}", &overview);
        }

        prompt
    }

    /// Generate a formatted list of available skills for a subtype, filtered by tags.
    fn generate_skills_section(subtype_key: &str, context: &ToolContext) -> String {
        let skills = Self::load_skills_for_subtype(subtype_key, context);

        if skills.is_empty() {
            return "No skills currently installed for this mode.".to_string();
        }

        skills
            .iter()
            .map(|s| {
                let desc = Self::truncate_description(&s.description, 120);
                format!("• {} — {}", s.name, desc)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Generate an overview of all subtypes and their available skills (for Director).
    fn generate_subagent_overview(context: &ToolContext) -> String {
        let configs = types::all_subtype_configs();

        configs
            .iter()
            .filter(|c| c.key != "director" && c.enabled)
            .map(|c| {
                let skills = Self::load_skills_for_subtype(&c.key, context);
                let skill_lines = if skills.is_empty() {
                    "  (no skills installed)".to_string()
                } else {
                    skills
                        .iter()
                        .map(|s| {
                            let desc = Self::truncate_description(&s.description, 100);
                            format!("  • {} — {}", s.name, desc)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                format!(
                    "### {} {} — {}\n{}",
                    c.emoji, c.label, c.description, skill_lines
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Load enabled skills from DB filtered by a subtype's allowed tags.
    fn load_skills_for_subtype(
        subtype_key: &str,
        context: &ToolContext,
    ) -> Vec<crate::skills::types::DbSkill> {
        let allowed_tags = types::allowed_skill_tags_for_key(subtype_key);
        if allowed_tags.is_empty() {
            return vec![];
        }

        context
            .database
            .as_ref()
            .and_then(|db| db.list_enabled_skills().ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|skill| skill.tags.iter().any(|tag| allowed_tags.contains(tag)))
            .collect()
    }

    /// Truncate a description to max_len chars, appending "..." if truncated.
    fn truncate_description(desc: &str, max_len: usize) -> String {
        // Take first line only (some descriptions are multi-line)
        let first_line = desc.lines().next().unwrap_or(desc);
        if first_line.len() > max_len {
            format!("{}...", &first_line[..max_len])
        } else {
            first_line.to_string()
        }
    }
}

impl Default for SetAgentSubtypeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SetAgentSubtypeParams {
    agent_subtype: String,
}

#[async_trait]
impl Tool for SetAgentSubtypeTool {
    fn definition(&self) -> ToolDefinition {
        Self::build_definition()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: SetAgentSubtypeParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                let msg = format!("Invalid parameters: {}. The parameter name is `agent_subtype`, not `subtype`.", e);
                return ToolResult::error(msg);
            }
        };

        let key = params.agent_subtype.to_lowercase();

        // Resolve via exact key match or alias
        let resolved_key = match types::resolve_subtype_key(&key) {
            Some(k) => k,
            None => {
                let valid: Vec<String> = types::all_subtype_configs()
                    .iter()
                    .map(|c| format!("'{}'", c.key))
                    .collect();
                return ToolResult::error(format!(
                    "Invalid subtype '{}'. Valid options: {}",
                    params.agent_subtype,
                    valid.join(", ")
                ));
            }
        };

        let config = match types::get_subtype_config(&resolved_key) {
            Some(c) => c,
            None => {
                let valid: Vec<String> = types::all_subtype_configs()
                    .iter()
                    .map(|c| format!("'{}'", c.key))
                    .collect();
                return ToolResult::error(format!(
                    "Invalid subtype '{}'. Valid options: {}",
                    params.agent_subtype,
                    valid.join(", ")
                ));
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
        let description = Self::describe_subtype(&config.key, context);
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
            .execute(json!({ "agent_subtype": "finance" }), &context)
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
            .execute(json!({ "agent_subtype": "code_engineer" }), &context)
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
            .execute(json!({ "agent_subtype": "secretary" }), &context)
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
            .execute(json!({ "agent_subtype": "invalid" }), &context)
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Invalid subtype"));
    }
}
