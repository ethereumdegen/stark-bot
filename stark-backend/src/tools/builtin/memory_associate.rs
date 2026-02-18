//! Memory Association Tool
//!
//! Create, list, or delete typed associations between memories.
//! Associations form a knowledge graph connecting related memories.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for managing typed associations between memories
pub struct MemoryAssociateTool {
    definition: ToolDefinition,
}

impl MemoryAssociateTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action to perform: \"create\" a new association, \"list\" associations for a memory, or \"delete\" an association.".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "create".to_string(),
                    "list".to_string(),
                    "delete".to_string(),
                ]),
            },
        );

        properties.insert(
            "source_memory_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Source memory ID for the association (required for create).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "target_memory_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Target memory ID for the association (required for create).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "association_type".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Type of association between memories. Default: \"related\".".to_string(),
                default: Some(json!("related")),
                items: None,
                enum_values: Some(vec![
                    "related".to_string(),
                    "caused_by".to_string(),
                    "contradicts".to_string(),
                    "supersedes".to_string(),
                    "part_of".to_string(),
                    "references".to_string(),
                    "temporal".to_string(),
                ]),
            },
        );

        properties.insert(
            "strength".to_string(),
            PropertySchema {
                schema_type: "number".to_string(),
                description: "Association strength from 0.0 (weak) to 1.0 (strong). Default: 0.5.".to_string(),
                default: Some(json!(0.5)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "memory_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Memory ID to list associations for (required for list).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "association_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Association ID to delete (required for delete).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        Self {
            definition: ToolDefinition {
                name: "memory_associate".to_string(),
                description: "Create, list, or delete typed associations between memories. Associations form a knowledge graph connecting related memories.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string()],
                },
                group: ToolGroup::Memory,
                hidden: false,
            },
        }
    }
}

impl Default for MemoryAssociateTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct AssociateParams {
    action: String,
    source_memory_id: Option<i64>,
    target_memory_id: Option<i64>,
    association_type: Option<String>,
    strength: Option<f64>,
    memory_id: Option<i64>,
    association_id: Option<i64>,
}

#[async_trait]
impl Tool for MemoryAssociateTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: AssociateParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let db = match &context.database {
            Some(db) => db,
            None => {
                return ToolResult::error(
                    "Database not available. Memory associations require the database to be initialized.",
                );
            }
        };

        match params.action.as_str() {
            "create" => {
                let source = match params.source_memory_id {
                    Some(id) => id,
                    None => return ToolResult::error("source_memory_id is required for create action."),
                };
                let target = match params.target_memory_id {
                    Some(id) => id,
                    None => return ToolResult::error("target_memory_id is required for create action."),
                };

                let assoc_type = params.association_type.unwrap_or_else(|| "related".to_string());
                let strength = params.strength.unwrap_or(0.5);

                // Validate strength range
                if !(0.0..=1.0).contains(&strength) {
                    return ToolResult::error("strength must be between 0.0 and 1.0.");
                }

                match db.create_memory_association(source, target, &assoc_type, strength, None) {
                    Ok(association_id) => {
                        let output = format!(
                            "## Association Created\n\n\
                            **ID:** {}\n\
                            **Source Memory:** {}\n\
                            **Target Memory:** {}\n\
                            **Type:** {}\n\
                            **Strength:** {:.2}",
                            association_id, source, target, assoc_type, strength
                        );
                        ToolResult::success(output).with_metadata(json!({
                            "association_id": association_id,
                            "source_memory_id": source,
                            "target_memory_id": target,
                            "association_type": assoc_type,
                            "strength": strength
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to create association: {}", e)),
                }
            }

            "list" => {
                let memory_id = match params.memory_id {
                    Some(id) => id,
                    None => return ToolResult::error("memory_id is required for list action."),
                };

                match db.get_memory_associations(memory_id) {
                    Ok(associations) => {
                        if associations.is_empty() {
                            return ToolResult::success(format!(
                                "No associations found for memory {}.",
                                memory_id
                            ));
                        }

                        let mut output = format!(
                            "## Associations for Memory {}\n**Count:** {}\n\n\
                            | ID | Source | Target | Type | Strength |\n\
                            |----|--------|--------|------|----------|\n",
                            memory_id,
                            associations.len()
                        );

                        for assoc in &associations {
                            output.push_str(&format!(
                                "| {} | {} | {} | {} | {:.2} |\n",
                                assoc.id, assoc.source_memory_id, assoc.target_memory_id,
                                assoc.association_type, assoc.strength
                            ));
                        }

                        ToolResult::success(output).with_metadata(json!({
                            "memory_id": memory_id,
                            "count": associations.len()
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to list associations: {}", e)),
                }
            }

            "delete" => {
                let association_id = match params.association_id {
                    Some(id) => id,
                    None => return ToolResult::error("association_id is required for delete action."),
                };

                match db.delete_memory_association(association_id) {
                    Ok(deleted) => {
                        if deleted {
                            ToolResult::success(format!(
                                "## Association Deleted\n\nAssociation {} has been removed.",
                                association_id
                            )).with_metadata(json!({
                                "association_id": association_id,
                                "deleted": true
                            }))
                        } else {
                            ToolResult::success(format!(
                                "Association {} not found.",
                                association_id
                            )).with_metadata(json!({
                                "association_id": association_id,
                                "deleted": false
                            }))
                        }
                    }
                    Err(e) => ToolResult::error(format!("Failed to delete association: {}", e)),
                }
            }

            _ => ToolResult::error(format!(
                "Unknown action: \"{}\". Use \"create\", \"list\", or \"delete\".",
                params.action
            )),
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
    fn test_memory_associate_definition() {
        let tool = MemoryAssociateTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "memory_associate");
        assert_eq!(def.group, ToolGroup::Memory);
        assert!(def.input_schema.required.contains(&"action".to_string()));
    }
}
