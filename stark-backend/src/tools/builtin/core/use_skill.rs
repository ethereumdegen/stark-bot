use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool to execute a specialized skill by name.
///
/// The static definition has no enum_values — the dispatcher patches the
/// definition at build_tool_list() time with the context-aware list of
/// allowed skills (filtered by subtype tags, special-role grants, and
/// safe-mode tool availability).
///
/// Post-execution side effects (skill activation, subtype switching,
/// tool list refresh) are handled by the dispatcher's post-execution
/// hook, same pattern as `set_agent_subtype`.
pub struct UseSkillTool;

impl UseSkillTool {
    pub fn new() -> Self {
        UseSkillTool
    }
}

impl Default for UseSkillTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct UseSkillParams {
    #[serde(alias = "name")]
    skill_name: String,
    #[serde(default, alias = "inputs")]
    input: String,
}

#[async_trait]
impl Tool for UseSkillTool {
    fn definition(&self) -> ToolDefinition {
        let mut properties = HashMap::new();
        properties.insert(
            "skill_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The name of the skill to execute".to_string(),
                default: None,
                items: None,
                enum_values: None, // Patched dynamically by dispatcher's build_tool_list()
            },
        );
        properties.insert(
            "input".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Input or query for the skill".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ToolDefinition {
            name: "use_skill".to_string(),
            description:
                "Execute a specialized skill. YOU MUST use this tool when a user asks for \
                 something that matches an available skill."
                    .to_string(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties,
                required: vec!["skill_name".to_string(), "input".to_string()],
            },
            group: ToolGroup::System,
            hidden: false,
        }
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: UseSkillParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let skill_name = &params.skill_name;
        let input = &params.input;

        log::info!("[SKILL] Executing skill '{}' with input: {}", skill_name, input);

        let db = match &context.database {
            Some(db) => db,
            None => return ToolResult::error("Database not available"),
        };

        // Look up the skill by name
        let skill = match db.get_enabled_skill_by_name(skill_name) {
            Ok(Some(s)) => s,
            Ok(None) => {
                let available = db
                    .list_enabled_skills()
                    .map(|skills| {
                        skills
                            .iter()
                            .map(|s| s.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|_| "unknown".to_string());
                return ToolResult::error(format!(
                    "Skill '{}' not found or not enabled. Available skills: {}",
                    skill_name, available
                ));
            }
            Err(e) => {
                return ToolResult::error(format!("Failed to load skill: {}", e));
            }
        };

        // Pre-flight: check required binaries are installed
        let missing_bins: Vec<&String> = skill
            .requires_binaries
            .iter()
            .filter(|bin| which::which(bin).is_err())
            .collect();
        if !missing_bins.is_empty() {
            let names: Vec<&str> = missing_bins.iter().map(|s| s.as_str()).collect();
            return ToolResult::error(format!(
                "Skill '{}' requires binaries not installed on this system: {}\n\n\
                 Install them and try again.",
                skill.name,
                names.join(", ")
            ));
        }

        // Pre-flight: check required API keys are configured
        if !skill.requires_api_keys.is_empty() {
            let configured_keys: Vec<String> = db
                .list_api_keys()
                .unwrap_or_default()
                .into_iter()
                .map(|k| k.service_name)
                .collect();
            let missing_keys: Vec<&String> = skill
                .requires_api_keys
                .keys()
                .filter(|key| !configured_keys.contains(key))
                .collect();
            if !missing_keys.is_empty() {
                let names: Vec<&str> = missing_keys.iter().map(|s| s.as_str()).collect();
                return ToolResult::error(format!(
                    "Skill '{}' requires API keys that are not configured: {}\n\n\
                     Please go to Settings > API Keys and add these keys first.",
                    skill.name,
                    names.join(", ")
                ));
            }
        }

        // Replace {baseDir} placeholder with actual skill directory
        let skills_dir = crate::config::runtime_skills_dir();
        let skill_base_dir = format!("{}/{}", skills_dir, skill.name);
        let instructions = if !skill.body.is_empty() {
            skill.body.replace("{baseDir}", &skill_base_dir)
        } else {
            String::new()
        };

        // Save active skill to agent context for persistence
        if let Some(session_id) = context.session_id {
            use crate::ai::multi_agent::types::ActiveSkill;

            if let Ok(Some(mut agent_ctx)) = db.get_agent_context(session_id) {
                agent_ctx.active_skill = Some(ActiveSkill {
                    name: skill.name.clone(),
                    instructions: instructions.clone(),
                    activated_at: chrono::Utc::now().to_rfc3339(),
                    tool_calls_made: 0,
                    requires_tools: skill.requires_tools.clone(),
                });
                if let Err(e) = db.save_agent_context(session_id, &agent_ctx) {
                    log::warn!("[SKILL] Failed to save active skill to context: {}", e);
                } else {
                    log::info!(
                        "[SKILL] Saved active skill '{}' to session {} (tool_calls_made=0, requires_tools={:?})",
                        skill.name, session_id, skill.requires_tools
                    );
                }
            }
        }

        // Build the result content
        let mut result = format!("## Skill: {}\n\n", skill.name);
        result.push_str(&format!("Description: {}\n\n", skill.description));

        if !instructions.is_empty() {
            result.push_str("### Instructions:\n");
            result.push_str(&instructions);
            result.push_str("\n\n");
        }

        result.push_str(&format!("### User Query:\n{}\n\n", input));
        result.push_str(
            "**IMPORTANT:** Now call the actual tools mentioned in the instructions above. \
             Do NOT call use_skill again.",
        );

        ToolResult::success(&result).with_metadata(json!({
            "skill_name": skill.name,
            "requires_tools": skill.requires_tools,
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
    async fn test_use_skill_no_database() {
        let tool = UseSkillTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "skill_name": "test", "input": "hello" }), &context)
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Database not available"));
    }

    #[test]
    fn test_definition_is_system_group() {
        let tool = UseSkillTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "use_skill");
        assert_eq!(def.group, ToolGroup::System);
    }

    #[test]
    fn test_safety_level_is_safe_mode() {
        let tool = UseSkillTool::new();
        assert_eq!(tool.safety_level(), ToolSafetyLevel::SafeMode);
    }
}
