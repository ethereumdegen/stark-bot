use crate::models::SpecialRole;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct ModifySpecialRoleTool {
    definition: ToolDefinition,
}

impl ModifySpecialRoleTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action: 'list_roles', 'create_role', 'delete_role', 'list_assignments', 'assign_role', 'unassign_role'".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "list_roles".to_string(),
                    "create_role".to_string(),
                    "delete_role".to_string(),
                    "list_assignments".to_string(),
                    "assign_role".to_string(),
                    "unassign_role".to_string(),
                ]),
            },
        );

        properties.insert(
            "role_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Role name (for create/delete/assign/unassign, and optional filter for list_assignments)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "allowed_tools".to_string(),
            PropertySchema {
                schema_type: "array".to_string(),
                description: "Tool names to grant (for create_role)".to_string(),
                default: None,
                items: Some(Box::new(PropertySchema {
                    schema_type: "string".to_string(),
                    description: "Tool name".to_string(),
                    default: None,
                    items: None,
                    enum_values: None,
                })),
                enum_values: None,
            },
        );

        properties.insert(
            "allowed_skills".to_string(),
            PropertySchema {
                schema_type: "array".to_string(),
                description: "Skill tags to grant (for create_role)".to_string(),
                default: None,
                items: Some(Box::new(PropertySchema {
                    schema_type: "string".to_string(),
                    description: "Skill tag".to_string(),
                    default: None,
                    items: None,
                    enum_values: None,
                })),
                enum_values: None,
            },
        );

        properties.insert(
            "description".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Role description (for create_role)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "channel_type".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Channel type for assignment (discord, twitter, telegram, slack, external_channel)".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "discord".to_string(),
                    "twitter".to_string(),
                    "telegram".to_string(),
                    "slack".to_string(),
                    "external_channel".to_string(),
                ]),
            },
        );

        properties.insert(
            "user_id".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Platform-specific user ID for assignment".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ModifySpecialRoleTool {
            definition: ToolDefinition {
                name: "modify_special_role".to_string(),
                description: "Manage special roles for enriched safe mode: create/delete roles with extra tools/skills, and assign/unassign roles to users on specific channels.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string()],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }
}

impl Default for ModifySpecialRoleTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct Params {
    action: String,
    role_name: Option<String>,
    allowed_tools: Option<Vec<String>>,
    allowed_skills: Option<Vec<String>>,
    description: Option<String>,
    channel_type: Option<String>,
    user_id: Option<String>,
}

#[async_trait]
impl Tool for ModifySpecialRoleTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: Params = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let db = match &context.database {
            Some(db) => db,
            None => return ToolResult::error("Database not available"),
        };

        match params.action.as_str() {
            "list_roles" => match db.list_special_roles() {
                Ok(roles) => {
                    if roles.is_empty() {
                        return ToolResult::success("No special roles configured.");
                    }
                    let lines: Vec<String> = roles
                        .iter()
                        .map(|r| {
                            format!(
                                "- {} | tools: [{}] | skills: [{}]{}",
                                r.name,
                                r.allowed_tools.join(", "),
                                r.allowed_skills.join(", "),
                                r.description
                                    .as_deref()
                                    .map(|d| format!(" | {}", d))
                                    .unwrap_or_default()
                            )
                        })
                        .collect();
                    ToolResult::success(format!("Special roles ({}):\n{}", roles.len(), lines.join("\n")))
                        .with_metadata(json!({ "roles": roles }))
                }
                Err(e) => ToolResult::error(format!("Database error: {}", e)),
            },

            "create_role" => {
                let name = match &params.role_name {
                    Some(n) => n.trim().to_lowercase(),
                    None => return ToolResult::error("'role_name' is required for create_role"),
                };
                if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return ToolResult::error("role_name must be alphanumeric/underscore only");
                }

                // Check limit (only for genuinely new roles)
                if let Ok(None) = db.get_special_role(&name) {
                    match db.count_special_roles() {
                        Ok(count) if count >= 10 => {
                            return ToolResult::error("Maximum of 10 special roles allowed");
                        }
                        _ => {}
                    }
                }

                let role = SpecialRole {
                    name: name.clone(),
                    allowed_tools: params.allowed_tools.unwrap_or_default(),
                    allowed_skills: params.allowed_skills.unwrap_or_default(),
                    description: params.description,
                    created_at: String::new(),
                    updated_at: String::new(),
                };

                match db.upsert_special_role(&role) {
                    Ok(_) => ToolResult::success(format!(
                        "Special role '{}' created/updated. Tools: [{}], Skills: [{}]",
                        role.name,
                        role.allowed_tools.join(", "),
                        role.allowed_skills.join(", ")
                    )),
                    Err(e) => ToolResult::error(format!("Failed to create role: {}", e)),
                }
            }

            "delete_role" => {
                let name = match &params.role_name {
                    Some(n) => n.as_str(),
                    None => return ToolResult::error("'role_name' is required for delete_role"),
                };
                match db.delete_special_role(name) {
                    Ok(true) => ToolResult::success(format!("Special role '{}' deleted (and its assignments cascade-deleted).", name)),
                    Ok(false) => ToolResult::error(format!("Special role '{}' not found", name)),
                    Err(e) => ToolResult::error(format!("Failed to delete role: {}", e)),
                }
            }

            "list_assignments" => {
                match db.list_special_role_assignments(params.role_name.as_deref()) {
                    Ok(assignments) => {
                        if assignments.is_empty() {
                            return ToolResult::success("No special role assignments.");
                        }
                        let lines: Vec<String> = assignments
                            .iter()
                            .map(|a| {
                                format!(
                                    "- #{}: {} / {} -> role '{}'",
                                    a.id, a.channel_type, a.user_id, a.special_role_name
                                )
                            })
                            .collect();
                        ToolResult::success(format!(
                            "Assignments ({}):\n{}",
                            assignments.len(),
                            lines.join("\n")
                        ))
                        .with_metadata(json!({ "assignments": assignments }))
                    }
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "assign_role" => {
                let role_name = match &params.role_name {
                    Some(n) => n.as_str(),
                    None => return ToolResult::error("'role_name' is required for assign_role"),
                };
                let channel_type = match &params.channel_type {
                    Some(ct) => ct.as_str(),
                    None => return ToolResult::error("'channel_type' is required for assign_role"),
                };
                let user_id = match &params.user_id {
                    Some(uid) => uid.as_str(),
                    None => return ToolResult::error("'user_id' is required for assign_role"),
                };

                // Check assignment limit
                match db.count_special_role_assignments() {
                    Ok(count) if count >= 100 => {
                        return ToolResult::error("Maximum of 100 special role assignments allowed");
                    }
                    _ => {}
                }

                // Verify role exists
                match db.get_special_role(role_name) {
                    Ok(None) => return ToolResult::error(format!("Special role '{}' does not exist. Create it first.", role_name)),
                    Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                    Ok(Some(_)) => {}
                }

                match db.create_special_role_assignment(channel_type, user_id, role_name) {
                    Ok(a) => ToolResult::success(format!(
                        "Assigned role '{}' to user {} on {} (assignment #{})",
                        a.special_role_name, a.user_id, a.channel_type, a.id
                    )),
                    Err(e) => ToolResult::error(format!("Failed to assign role: {}", e)),
                }
            }

            "unassign_role" => {
                let role_name = match &params.role_name {
                    Some(n) => n.as_str(),
                    None => return ToolResult::error("'role_name' is required for unassign_role"),
                };
                let channel_type = match &params.channel_type {
                    Some(ct) => ct.as_str(),
                    None => return ToolResult::error("'channel_type' is required for unassign_role"),
                };
                let user_id = match &params.user_id {
                    Some(uid) => uid.as_str(),
                    None => return ToolResult::error("'user_id' is required for unassign_role"),
                };

                match db.delete_special_role_assignment_by_key(channel_type, user_id, role_name) {
                    Ok(true) => ToolResult::success(format!(
                        "Unassigned role '{}' from user {} on {}",
                        role_name, user_id, channel_type
                    )),
                    Ok(false) => ToolResult::error("Assignment not found"),
                    Err(e) => ToolResult::error(format!("Failed to unassign role: {}", e)),
                }
            }

            _ => ToolResult::error(format!(
                "Unknown action: '{}'. Valid: list_roles, create_role, delete_role, list_assignments, assign_role, unassign_role",
                params.action
            )),
        }
    }
}
