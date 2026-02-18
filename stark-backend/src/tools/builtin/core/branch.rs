//! Branch Tool
//!
//! Spawn a context-inheriting branch that runs in the background.
//! Branches inherit the parent's last 20 messages + compaction summary,
//! are restricted to Memory+System tool groups, and inject their result
//! back as a system message.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for spawning context-inheriting branches
pub struct BranchTool {
    definition: ToolDefinition,
}

impl BranchTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "task".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The task for the branch to perform. Branches have access to Memory and System tools only.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "label".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Human-readable label for this branch (e.g., 'Memory Persistence').".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "silent".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "If true, run as a silent branch — no user-visible output, result injected as system context only. Default: false.".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        Self {
            definition: ToolDefinition {
                name: "branch".to_string(),
                description: "Spawn a context-inheriting branch that runs in the background with Memory+System tools. Branches inherit recent conversation context and inject their results back into the parent session.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["task".to_string()],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }
}

impl Default for BranchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct BranchParams {
    task: String,
    label: Option<String>,
    #[serde(default)]
    silent: bool,
}

#[async_trait]
impl Tool for BranchTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: BranchParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Validate task
        if params.task.trim().is_empty() {
            return ToolResult::error("Task cannot be empty");
        }

        let label = params.label.unwrap_or_else(|| {
            if params.silent {
                "Silent Branch".to_string()
            } else {
                "Branch".to_string()
            }
        });

        // Get subagent manager for spawning the branch
        let subagent_manager = match &context.subagent_manager {
            Some(mgr) => mgr,
            None => {
                return ToolResult::error(
                    "SubAgent manager not available. Branch requires the sub-agent system to be initialized.",
                );
            }
        };

        let channel_id = context.channel_id.unwrap_or(0);
        let session_id = context.session_id.unwrap_or(0);

        // Build parent context snapshot (last 20 messages + compaction summary)
        let parent_context_snapshot = if let Some(db) = &context.database {
            match db.get_recent_session_messages(session_id, 20) {
                Ok(messages) => {
                    if messages.is_empty() {
                        None
                    } else {
                        let formatted: Vec<String> = messages.iter().map(|m| {
                            format!("[{:?}] {}: {}", m.role, m.user_name.as_deref().unwrap_or("system"), m.content)
                        }).collect();
                        Some(formatted.join("\n"))
                    }
                }
                Err(e) => {
                    log::warn!("Failed to capture parent context for branch: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Create sub-agent context for the branch
        let branch_id = format!("branch-{}", uuid::Uuid::new_v4());
        let mut sub_context = crate::ai::multi_agent::types::SubAgentContext::new(
            branch_id.clone(),
            session_id,
            channel_id,
            label.clone(),
            params.task.clone(),
            300, // 5 minute timeout
        );
        sub_context.read_only = true; // Memory+System tools only
        sub_context.mode = if params.silent {
            crate::ai::multi_agent::types::SubAgentMode::SilentBranch
        } else {
            crate::ai::multi_agent::types::SubAgentMode::Branch
        };
        sub_context.parent_context_snapshot = parent_context_snapshot;

        // Spawn the branch via subagent manager
        match subagent_manager.spawn(sub_context).await {
            Ok(_) => {
                let mode = if params.silent { "silent" } else { "visible" };
                let output = format!(
                    "## Branch Spawned\n\n\
                    **ID:** {}\n\
                    **Label:** {}\n\
                    **Mode:** {}\n\
                    **Task:** {}",
                    branch_id, label, mode, params.task
                );

                ToolResult::success(output).with_metadata(json!({
                    "branch_id": branch_id,
                    "label": label,
                    "silent": params.silent,
                    "task": params.task
                }))
            }
            Err(e) => ToolResult::error(format!("Failed to spawn branch: {}", e)),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Standard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_branch_definition() {
        let tool = BranchTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "branch");
        assert_eq!(def.group, ToolGroup::System);
        assert!(def.input_schema.required.contains(&"task".to_string()));
    }
}
