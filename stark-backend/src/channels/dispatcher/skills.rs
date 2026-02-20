use crate::ai::multi_agent::{types as agent_types, Orchestrator};
use crate::gateway::protocol::GatewayEvent;
use crate::tools::{ToolConfig, ToolDefinition};

use super::MessageDispatcher;

impl MessageDispatcher {
    /// Auto-set the orchestrator's subtype if the skill specifies one.
    /// Returns the new subtype key if it changed, so the caller can use it for tool refresh.
    pub(super) fn apply_skill_subtype(
        &self,
        skill: &crate::skills::types::DbSkill,
        orchestrator: &mut Orchestrator,
        channel_id: i64,
    ) -> Option<String> {
        if let Some(ref subtype_str) = skill.subagent_type {
            if let Some(resolved_key) = agent_types::resolve_subtype_key(subtype_str) {
                orchestrator.set_subtype(Some(resolved_key.clone()));
                log::info!(
                    "[SKILL] Auto-set subtype to {} for skill '{}'",
                    agent_types::subtype_label(&resolved_key),
                    skill.name
                );
                self.broadcaster.broadcast(GatewayEvent::agent_subtype_change(
                    channel_id,
                    &resolved_key,
                    &agent_types::subtype_label(&resolved_key),
                ));
                return Some(resolved_key);
            }
        }
        None
    }

    /// Returns the list of skills available for the given context.
    ///
    /// Filtering layers:
    /// 1. Only enabled skills from the database
    /// 2. All enabled skills pass through (no tag filtering)
    /// 3. In safe mode with role grants, only show granted skills
    /// 4. In safe mode, only skills whose `requires_tools` are all available
    pub(super) fn available_skills_for_context(
        &self,
        _subtype_key: &str,
        tool_config: &ToolConfig,
    ) -> Vec<crate::skills::types::DbSkill> {
        use crate::tools::ToolProfile;

        let skills = match self.db.list_enabled_skills() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[SKILL] Failed to query enabled skills: {}", e);
                return vec![];
            }
        };

        // Safe mode: only show explicitly granted skills (via special role).
        // Without grants, no skills are available in safe mode.
        if tool_config.profile == ToolProfile::SafeMode {
            if tool_config.extra_skill_names.is_empty() {
                return vec![];
            }
            return skills
                .into_iter()
                .filter(|skill| {
                    // Must be explicitly granted
                    if !tool_config.extra_skill_names.contains(&skill.name) {
                        return false;
                    }
                    // And requires_tools must all be available
                    skill.requires_tools.is_empty()
                        || skill.requires_tools.iter().all(|tool_name| {
                            self.tool_registry
                                .get(tool_name)
                                .map(|tool| {
                                    tool_config.is_tool_allowed(
                                        &tool.definition().name,
                                        tool.group(),
                                    )
                                })
                                .unwrap_or(false)
                        })
                })
                .collect();
        }

        // Standard mode: all enabled skills are available (no tag filtering)
        skills
    }

    /// Build a `use_skill` definition showing ALL enabled skills (no subtype filtering).
    pub(super) fn create_skill_tool_definition_all_skills(
        &self,
        _tool_config: &ToolConfig,
    ) -> Option<ToolDefinition> {
        use crate::tools::{PropertySchema, ToolGroup, ToolInputSchema};

        let skills = match self.db.list_enabled_skills() {
            Ok(s) => s,
            Err(_) => return None,
        };

        if skills.is_empty() {
            return None;
        }

        let skill_names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();

        let mut properties = std::collections::HashMap::new();
        properties.insert(
            "skill_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: format!("The skill to execute. Options: {}", skill_names.join(", ")),
                default: None,
                items: None,
                enum_values: Some(skill_names),
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

        let formatted_skills = skills
            .iter()
            .map(|s| format!("  - {}: {}", s.name, s.description))
            .collect::<Vec<_>>()
            .join("\n");

        Some(ToolDefinition {
            name: "use_skill".to_string(),
            description: format!(
                "Execute a specialized skill. YOU MUST use this tool when a user asks for something that matches a skill.\n\nAvailable skills:\n{}",
                formatted_skills
            ),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties,
                required: vec!["skill_name".to_string(), "input".to_string()],
            },
            group: ToolGroup::System,
            hidden: false,
        })
    }

    /// Build the complete tool list for the current agent state.
    ///
    /// This centralizes tool list construction that was previously duplicated
    /// across 7+ sites. The tool list is built in layers:
    ///
    /// 1. **Subtype group filtering** -- each subtype key allows specific `ToolGroup`s.
    ///    Tools outside those groups are excluded.
    /// 2. **Skill `requires_tools` force-inclusion** -- if the active skill specifies
    ///    `requires_tools`, those tools are force-included even if their group isn't
    ///    allowed by the subtype.
    /// 3. **`use_skill` pseudo-tool** -- added if any skills are enabled in the DB.
    /// 4. **Orchestrator mode tools** -- e.g. `define_tasks` in TaskPlanner mode.
    /// 5. **`define_tasks` stripping** -- removed unless the active skill's
    ///    `requires_tools` explicitly includes it (keeps it out of Assistant mode).
    ///
    /// Note: Safe mode filtering is handled upstream by `ToolConfig`, not here.
    pub(super) fn build_tool_list(
        &self,
        tool_config: &ToolConfig,
        subtype_key: &str,
        orchestrator: &Orchestrator,
    ) -> Vec<ToolDefinition> {
        let requires_tools = orchestrator.context().active_skill
            .as_ref()
            .map(|s| s.requires_tools.clone())
            .unwrap_or_default();

        let mut tools = if !requires_tools.is_empty() {
            self.tool_registry
                .get_tool_definitions_for_subtype_with_required(
                    tool_config,
                    subtype_key,
                    &requires_tools,
                )
        } else {
            self.tool_registry
                .get_tool_definitions_for_subtype(tool_config, subtype_key)
        };

        // Patch use_skill: replace the static registered definition with a
        // context-aware one (dynamic enum_values + description), or remove it
        // entirely if no skills are available.
        // All enabled skills are shown regardless of subtype (no tag filtering).
        if let Some(patched_def) = self.create_skill_tool_definition_all_skills(tool_config) {
            if let Some(existing) = tools.iter_mut().find(|t| t.name == "use_skill") {
                *existing = patched_def;
            }
        } else {
            tools.retain(|t| t.name != "use_skill");
        }

        tools.extend(orchestrator.get_mode_tools());

        // Strip define_tasks unless a skill requires it or the subtype explicitly includes it
        let skill_requires_define_tasks = requires_tools.iter().any(|t| t == "define_tasks");
        let subtype_has_define_tasks = agent_types::get_subtype_config(subtype_key)
            .map(|c| c.additional_tools.iter().any(|t| t == "define_tasks"))
            .unwrap_or(false);
        if !skill_requires_define_tasks && !subtype_has_define_tasks {
            tools.retain(|t| t.name != "define_tasks");
        }

        // When a subtype is already active, patch set_agent_subtype description
        // so the LLM doesn't re-call it every turn
        if orchestrator.current_subtype().is_some() {
            if let Some(tool) = tools.iter_mut().find(|t| t.name == "set_agent_subtype") {
                tool.description = format!(
                    "Switch toolbox (currently: {} {}). Only call this if the user's request \
                     requires a DIFFERENT toolbox than what's already active. \
                     Do NOT call this if you're already in the right mode.",
                    agent_types::subtype_emoji(subtype_key),
                    agent_types::subtype_label(subtype_key),
                );
            }
        }

        tools
    }
}
