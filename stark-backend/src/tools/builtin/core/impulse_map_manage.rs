//! Impulse Map management tool
//!
//! Allows the agent to manage the impulse map:
//! - list: Show all nodes
//! - get: Get a specific node
//! - create: Add a new node (optionally connected to a parent)
//! - update: Edit a node's body
//! - delete: Remove a node
//! - connect: Create a connection between two nodes
//! - disconnect: Remove a connection between two nodes

use crate::db::tables::impulse_nodes::{CreateImpulseNodeRequest, UpdateImpulseNodeRequest};
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct ImpulseMapManageTool {
    definition: ToolDefinition,
}

impl ImpulseMapManageTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The action to perform: 'list' (show all nodes), 'get' (get node by ID), 'create' (add node), 'update' (edit node body), 'delete' (remove node), 'connect' (link two nodes), 'disconnect' (unlink two nodes)".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "list".to_string(),
                    "get".to_string(),
                    "create".to_string(),
                    "update".to_string(),
                    "delete".to_string(),
                    "connect".to_string(),
                    "disconnect".to_string(),
                ]),
            },
        );

        properties.insert(
            "node_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Node ID (required for get/update/delete, and as parent for connect/disconnect)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "body".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Text content of the node (for create/update)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "parent_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Parent node ID to connect to when creating a node, or the parent in connect/disconnect actions".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "child_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Child node ID for connect/disconnect actions".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ImpulseMapManageTool {
            definition: ToolDefinition {
                name: "impulse_map_manage".to_string(),
                description: "Manage the impulse map: list nodes, create/edit/delete nodes, and connect or disconnect them. The impulse_map is a knowledge graph of ideas, topics, and goals that the heartbeat system uses for automated reflection.".to_string(),
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

impl Default for ImpulseMapManageTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ImpulseMapParams {
    action: String,
    node_id: Option<i64>,
    body: Option<String>,
    parent_id: Option<i64>,
    child_id: Option<i64>,
}

fn format_node(node: &crate::db::tables::impulse_nodes::ImpulseNode) -> String {
    let trunk_label = if node.is_trunk { " [TRUNK]" } else { "" };
    let body_preview = if node.body.is_empty() {
        "(empty)".to_string()
    } else if node.body.len() > 100 {
        format!("{}...", &node.body[..100])
    } else {
        node.body.clone()
    };

    format!("#{}{} — {}", node.id, trunk_label, body_preview)
}

fn format_node_detail(node: &crate::db::tables::impulse_nodes::ImpulseNode) -> String {
    let trunk_label = if node.is_trunk { " [TRUNK]" } else { "" };
    format!(
        "Node #{}{}\n  Body: {}\n  Created: {}\n  Updated: {}",
        node.id,
        trunk_label,
        if node.body.is_empty() { "(empty)" } else { &node.body },
        node.created_at.format("%Y-%m-%d %H:%M"),
        node.updated_at.format("%Y-%m-%d %H:%M"),
    )
}

#[async_trait]
impl Tool for ImpulseMapManageTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ImpulseMapParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let db = match &context.database {
            Some(db) => db,
            None => return ToolResult::error("Database not available"),
        };

        match params.action.as_str() {
            "list" => {
                match db.get_impulse_graph() {
                    Ok(graph) => {
                        if graph.nodes.is_empty() {
                            return ToolResult::success("Impulse map is empty. The trunk node will be created automatically.");
                        }

                        let mut output = format!("Impulse Map — {} nodes, {} connections\n\n", graph.nodes.len(), graph.connections.len());

                        for node in &graph.nodes {
                            output.push_str(&format!("{}\n", format_node(node)));
                        }

                        if !graph.connections.is_empty() {
                            output.push_str("\nConnections:\n");
                            for conn in &graph.connections {
                                output.push_str(&format!("  #{} → #{}\n", conn.parent_id, conn.child_id));
                            }
                        }

                        ToolResult::success(output)
                            .with_metadata(json!({
                                "node_count": graph.nodes.len(),
                                "connection_count": graph.connections.len(),
                            }))
                    }
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "get" => {
                let node_id = match params.node_id {
                    Some(id) => id,
                    None => return ToolResult::error("'node_id' is required for 'get' action"),
                };

                match db.get_impulse_node(node_id) {
                    Ok(Some(node)) => {
                        let mut output = format_node_detail(&node);

                        // Also show neighbors
                        if let Ok(neighbors) = db.get_impulse_node_neighbors(node_id) {
                            if !neighbors.is_empty() {
                                output.push_str(&format!("\n  Neighbors ({}):", neighbors.len()));
                                for n in &neighbors {
                                    output.push_str(&format!("\n    {}", format_node(n)));
                                }
                            }
                        }

                        ToolResult::success(output)
                    }
                    Ok(None) => ToolResult::error(format!("Node #{} not found", node_id)),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "create" => {
                let body = params.body.unwrap_or_default();

                let request = CreateImpulseNodeRequest {
                    body: Some(body.clone()),
                    position_x: None,
                    position_y: None,
                    parent_id: params.parent_id,
                };

                match db.create_impulse_node(&request) {
                    Ok(node) => {
                        let parent_msg = params.parent_id
                            .map(|pid| format!(" (connected to parent #{})", pid))
                            .unwrap_or_default();

                        ToolResult::success(format!(
                            "Created node #{}{}: {}",
                            node.id, parent_msg, if body.is_empty() { "(empty)" } else { &body }
                        )).with_metadata(json!({
                            "node_id": node.id,
                            "parent_id": params.parent_id,
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "update" => {
                let node_id = match params.node_id {
                    Some(id) => id,
                    None => return ToolResult::error("'node_id' is required for 'update' action"),
                };

                let request = UpdateImpulseNodeRequest {
                    body: params.body.clone(),
                    position_x: None,
                    position_y: None,
                };

                match db.update_impulse_node(node_id, &request) {
                    Ok(Some(node)) => {
                        ToolResult::success(format!("Updated node:\n{}", format_node_detail(&node)))
                    }
                    Ok(None) => ToolResult::error(format!("Node #{} not found", node_id)),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "delete" => {
                let node_id = match params.node_id {
                    Some(id) => id,
                    None => return ToolResult::error("'node_id' is required for 'delete' action"),
                };

                match db.delete_impulse_node(node_id) {
                    Ok(true) => ToolResult::success(format!("Deleted node #{}", node_id)),
                    Ok(false) => ToolResult::error("Cannot delete trunk node, or node not found"),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "connect" => {
                let parent_id = match params.parent_id {
                    Some(id) => id,
                    None => return ToolResult::error("'parent_id' is required for 'connect' action"),
                };
                let child_id = match params.child_id {
                    Some(id) => id,
                    None => return ToolResult::error("'child_id' is required for 'connect' action"),
                };

                match db.create_impulse_node_connection(parent_id, child_id) {
                    Ok(conn) => ToolResult::success(format!(
                        "Connected #{} → #{} (connection #{})", conn.parent_id, conn.child_id, conn.id
                    )),
                    Err(e) => ToolResult::error(format!("Failed to connect: {}", e)),
                }
            }

            "disconnect" => {
                let parent_id = match params.parent_id {
                    Some(id) => id,
                    None => return ToolResult::error("'parent_id' is required for 'disconnect' action"),
                };
                let child_id = match params.child_id {
                    Some(id) => id,
                    None => return ToolResult::error("'child_id' is required for 'disconnect' action"),
                };

                match db.delete_impulse_node_connection(parent_id, child_id) {
                    Ok(true) => ToolResult::success(format!("Disconnected #{} → #{}", parent_id, child_id)),
                    Ok(false) => ToolResult::error("Connection not found"),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            _ => ToolResult::error(format!(
                "Unknown action: '{}'. Valid actions: list, get, create, update, delete, connect, disconnect",
                params.action
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition() {
        let tool = ImpulseMapManageTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "impulse_map_manage");
        assert_eq!(def.group, ToolGroup::System);
        assert!(def.input_schema.required.contains(&"action".to_string()));
    }
}
