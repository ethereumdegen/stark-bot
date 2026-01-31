//! Memory retrieval tool for getting specific memories or browsing by filters
//!
//! This tool allows the agent to:
//! - Get a specific memory by ID
//! - List memories by entity type/name
//! - Get recent memories of a specific type

use crate::models::MemoryType;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for retrieving memories by ID or filters
pub struct MemoryGetTool {
    definition: ToolDefinition,
}

impl MemoryGetTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Get a specific memory by its ID.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "memory_type".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "List memories of this type.".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "daily_log".to_string(),
                    "long_term".to_string(),
                    "preference".to_string(),
                    "fact".to_string(),
                    "task".to_string(),
                    "entity".to_string(),
                    "session_summary".to_string(),
                ]),
            },
        );

        properties.insert(
            "entity_type".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Filter by entity type (e.g., 'person', 'project', 'place').".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "entity_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Filter by entity name (requires entity_type).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "min_importance".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Minimum importance level (1-10).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "limit".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum number of memories to return (default: 10, max: 50).".to_string(),
                default: Some(json!(10)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "include_superseded".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Include memories that have been superseded by newer ones (default: false).".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        MemoryGetTool {
            definition: ToolDefinition {
                name: "memory_get".to_string(),
                description: "Retrieve specific memories by ID or browse with filters. Only use when you know specific memories exist. If memory_search found nothing, don't follow up with memory_get - move on.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::System,
            },
        }
    }
}

impl Default for MemoryGetTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct MemoryGetParams {
    id: Option<i64>,
    memory_type: Option<String>,
    entity_type: Option<String>,
    entity_name: Option<String>,
    min_importance: Option<i32>,
    limit: Option<i32>,
    include_superseded: Option<bool>,
}

#[async_trait]
impl Tool for MemoryGetTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: MemoryGetParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get database from context (using typed field)
        let db = match &context.database {
            Some(db) => db,
            None => {
                return ToolResult::error(
                    "Database not available. Memory retrieval requires database access.",
                );
            }
        };

        // Get identity from context
        let identity_id = context.identity_id.as_deref();

        // Limit results (max 50)
        let limit = params.limit.unwrap_or(10).min(50);
        let include_superseded = params.include_superseded.unwrap_or(false);

        // If ID is provided, get specific memory
        if let Some(id) = params.id {
            match db.get_memory(id) {
                Ok(Some(memory)) => {
                    let mut output = format!(
                        "## Memory #{}\n\
                         **Type:** {}\n\
                         **Importance:** {}\n\
                         **Created:** {}\n",
                        memory.id,
                        memory.memory_type.as_str(),
                        memory.importance,
                        memory.created_at.format("%Y-%m-%d %H:%M UTC")
                    );

                    if let Some(ref category) = memory.category {
                        output.push_str(&format!("**Category:** {}\n", category));
                    }

                    if let Some(ref tags) = memory.tags {
                        output.push_str(&format!("**Tags:** {}\n", tags));
                    }

                    if let Some(ref entity_type) = memory.entity_type {
                        output.push_str(&format!(
                            "**Entity:** {} ({})\n",
                            memory.entity_name.as_deref().unwrap_or("unknown"),
                            entity_type
                        ));
                    }

                    if let Some(ref source) = memory.source_channel_type {
                        output.push_str(&format!("**Source:** {}\n", source));
                    }

                    if memory.superseded_by.is_some() {
                        output.push_str(&format!(
                            "**Superseded by:** #{}\n",
                            memory.superseded_by.unwrap()
                        ));
                    }

                    output.push_str(&format!("\n## Content\n{}\n", memory.content));

                    return ToolResult::success(output).with_metadata(json!({
                        "memory_id": memory.id,
                        "memory_type": memory.memory_type.as_str(),
                        "importance": memory.importance
                    }));
                }
                Ok(None) => {
                    return ToolResult::error(format!("Memory with ID {} not found", id));
                }
                Err(e) => {
                    return ToolResult::error(format!("Failed to retrieve memory: {}", e));
                }
            }
        }

        // If entity_type is provided, search by entity
        if let Some(ref entity_type) = params.entity_type {
            match db.get_memories_by_entity(
                entity_type,
                params.entity_name.as_deref(),
                identity_id,
                limit,
            ) {
                Ok(memories) => {
                    return format_memory_list(
                        memories,
                        &format!(
                            "Memories for entity type: {}{}",
                            entity_type,
                            params
                                .entity_name
                                .as_ref()
                                .map(|n| format!(" ({})", n))
                                .unwrap_or_default()
                        ),
                    );
                }
                Err(e) => {
                    return ToolResult::error(format!("Failed to retrieve memories: {}", e));
                }
            }
        }

        // If memory_type is provided, list by type
        let memory_type = params.memory_type.as_ref().and_then(|t| MemoryType::from_str(t));

        match db.list_memories_filtered(
            memory_type,
            identity_id,
            params.min_importance,
            include_superseded,
            limit,
            0, // offset
        ) {
            Ok(memories) => {
                let title = match &params.memory_type {
                    Some(t) => format!("Memories of type: {}", t),
                    None => "Recent Memories".to_string(),
                };
                format_memory_list(memories, &title)
            }
            Err(e) => ToolResult::error(format!("Failed to retrieve memories: {}", e)),
        }
    }
}

/// Format a list of memories as markdown
fn format_memory_list(memories: Vec<crate::models::Memory>, title: &str) -> ToolResult {
    if memories.is_empty() {
        return ToolResult::success("No memories found matching the criteria.");
    }

    let mut output = format!(
        "## {}\n\
         Found: {} memories\n\n",
        title,
        memories.len()
    );

    for (i, memory) in memories.iter().enumerate() {
        output.push_str(&format!(
            "### {}. {} (ID: {})\n",
            i + 1,
            memory.memory_type.as_str(),
            memory.id
        ));

        output.push_str(&format!(
            "**Importance:** {} | **Created:** {}\n",
            memory.importance,
            memory.created_at.format("%Y-%m-%d %H:%M UTC")
        ));

        if let Some(ref category) = memory.category {
            output.push_str(&format!("**Category:** {}\n", category));
        }

        if let Some(ref entity_type) = memory.entity_type {
            output.push_str(&format!(
                "**Entity:** {} ({})\n",
                memory.entity_name.as_deref().unwrap_or("unknown"),
                entity_type
            ));
        }

        // Truncate long content
        let content = if memory.content.len() > 300 {
            format!("{}...", &memory.content[..300])
        } else {
            memory.content.clone()
        };
        output.push_str(&format!("\n{}\n\n---\n\n", content));
    }

    ToolResult::success(output).with_metadata(json!({
        "count": memories.len(),
        "memory_ids": memories.iter().map(|m| m.id).collect::<Vec<_>>()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_get_definition() {
        let tool = MemoryGetTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "memory_get");
        assert_eq!(def.group, ToolGroup::System);
        // No required params - can call with empty object
        assert!(def.input_schema.required.is_empty());
    }
}
