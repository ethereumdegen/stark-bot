//! Memory Graph Traversal Tool
//!
//! Traverse the memory knowledge graph. Find connected memories,
//! discover paths between memories, and get graph statistics.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

/// Tool for traversing the memory knowledge graph
pub struct MemoryGraphTool {
    definition: ToolDefinition,
}

impl MemoryGraphTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action to perform: \"neighbors\" to find connected memories, \"stats\" to get graph statistics, or \"path\" to find shortest path between two memories.".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "neighbors".to_string(),
                    "stats".to_string(),
                    "path".to_string(),
                ]),
            },
        );

        properties.insert(
            "memory_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Starting memory ID (required for neighbors and path).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "target_memory_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Target memory ID to find a path to (required for path).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "depth".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum traversal depth for neighbors. Default: 1, max: 3.".to_string(),
                default: Some(json!(1)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "association_type".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Filter results by association type (e.g., \"related\", \"caused_by\", \"contradicts\", \"supersedes\", \"part_of\", \"references\", \"temporal\").".to_string(),
                default: None,
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

        Self {
            definition: ToolDefinition {
                name: "memory_graph".to_string(),
                description: "Traverse the memory knowledge graph. Find connected memories, discover paths between memories, and get graph statistics.".to_string(),
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

impl Default for MemoryGraphTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct GraphParams {
    action: String,
    memory_id: Option<i64>,
    target_memory_id: Option<i64>,
    depth: Option<i32>,
    association_type: Option<String>,
}

#[async_trait]
impl Tool for MemoryGraphTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: GraphParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let db = match &context.database {
            Some(db) => db,
            None => {
                return ToolResult::error(
                    "Database not available. Memory graph requires the database to be initialized.",
                );
            }
        };

        match params.action.as_str() {
            "neighbors" => {
                let memory_id = match params.memory_id {
                    Some(id) => id,
                    None => return ToolResult::error("memory_id is required for neighbors action."),
                };

                let max_depth = params.depth.unwrap_or(1).min(3).max(1);
                let filter_type = params.association_type.as_deref();

                // Collect neighbors at each depth level
                let mut visited: HashSet<i64> = HashSet::new();
                visited.insert(memory_id);

                let mut current_level = vec![memory_id];
                let mut output = format!("## Memory Graph: Neighbors of {}\n**Depth:** {}\n\n", memory_id, max_depth);
                let mut total_found = 0;

                for depth in 1..=max_depth {
                    let mut next_level = Vec::new();

                    for &current_id in &current_level {
                        let associations = match db.get_memory_associations(current_id) {
                            Ok(assocs) => assocs,
                            Err(e) => {
                                return ToolResult::error(format!(
                                    "Failed to get associations for memory {}: {}",
                                    current_id, e
                                ));
                            }
                        };

                        for assoc in &associations {
                            // Filter by association type if specified
                            if let Some(ref filter) = filter_type {
                                if &assoc.association_type != filter {
                                    continue;
                                }
                            }

                            // Determine the connected memory (could be source or target)
                            let connected_id = if assoc.source_memory_id == current_id {
                                assoc.target_memory_id
                            } else {
                                assoc.source_memory_id
                            };

                            if !visited.contains(&connected_id) {
                                visited.insert(connected_id);
                                next_level.push(connected_id);

                                let indent = "  ".repeat(depth as usize);
                                let direction = if assoc.source_memory_id == current_id {
                                    "->"
                                } else {
                                    "<-"
                                };
                                output.push_str(&format!(
                                    "{}{} Memory {} {} Memory {} [{}] (strength: {:.2})\n",
                                    indent, direction, current_id, direction,
                                    connected_id, assoc.association_type, assoc.strength
                                ));
                                total_found += 1;
                            }
                        }
                    }

                    if next_level.is_empty() {
                        break;
                    }

                    current_level = next_level;
                }

                if total_found == 0 {
                    output.push_str("No connected memories found.");
                } else {
                    output.push_str(&format!("\n**Total connected:** {}", total_found));
                }

                ToolResult::success(output).with_metadata(json!({
                    "memory_id": memory_id,
                    "depth": max_depth,
                    "total_found": total_found
                }))
            }

            "stats" => {
                match db.get_memory_graph_stats() {
                    Ok(stats) => {
                        let mut output = "## Memory Graph Statistics\n\n".to_string();
                        output.push_str(&format!("| Metric | Value |\n"));
                        output.push_str(&format!("|--------|-------|\n"));
                        output.push_str(&format!("| Total Associations | {} |\n", stats.total_associations));
                        output.push_str(&format!("| Unique Memories Connected | {} |\n", stats.unique_memories));
                        output.push_str(&format!("| Average Strength | {:.2} |\n", stats.avg_strength));

                        if !stats.type_counts.is_empty() {
                            output.push_str("\n### Association Types\n\n");
                            output.push_str("| Type | Count |\n");
                            output.push_str("|------|-------|\n");
                            for (assoc_type, count) in &stats.type_counts {
                                output.push_str(&format!("| {} | {} |\n", assoc_type, count));
                            }
                        }

                        ToolResult::success(output).with_metadata(json!({
                            "total_associations": stats.total_associations,
                            "unique_memories": stats.unique_memories,
                            "avg_strength": stats.avg_strength,
                            "type_counts": stats.type_counts
                        }))
                    }
                    Err(e) => ToolResult::error(format!("Failed to get graph stats: {}", e)),
                }
            }

            "path" => {
                let start_id = match params.memory_id {
                    Some(id) => id,
                    None => return ToolResult::error("memory_id is required for path action."),
                };
                let target_id = match params.target_memory_id {
                    Some(id) => id,
                    None => return ToolResult::error("target_memory_id is required for path action."),
                };

                if start_id == target_id {
                    return ToolResult::success(format!(
                        "## Path: Memory {} -> Memory {}\n\nSource and target are the same memory.",
                        start_id, target_id
                    ));
                }

                let filter_type = params.association_type.as_deref();
                let max_depth = 5;

                // BFS to find shortest path
                let mut visited: HashSet<i64> = HashSet::new();
                visited.insert(start_id);

                // Queue stores (current_node, path_so_far)
                let mut queue: VecDeque<(i64, Vec<(i64, String, i64)>)> = VecDeque::new();
                queue.push_back((start_id, vec![]));

                let mut found_path: Option<Vec<(i64, String, i64)>> = None;

                while let Some((current_id, path)) = queue.pop_front() {
                    if path.len() >= max_depth {
                        continue;
                    }

                    let associations = match db.get_memory_associations(current_id) {
                        Ok(assocs) => assocs,
                        Err(_) => continue,
                    };

                    for assoc in &associations {
                        // Filter by association type if specified
                        if let Some(ref filter) = filter_type {
                            if &assoc.association_type != filter {
                                continue;
                            }
                        }

                        let connected_id = if assoc.source_memory_id == current_id {
                            assoc.target_memory_id
                        } else {
                            assoc.source_memory_id
                        };

                        if visited.contains(&connected_id) {
                            continue;
                        }

                        let mut new_path = path.clone();
                        new_path.push((current_id, assoc.association_type.clone(), connected_id));

                        if connected_id == target_id {
                            found_path = Some(new_path);
                            break;
                        }

                        visited.insert(connected_id);
                        queue.push_back((connected_id, new_path));
                    }

                    if found_path.is_some() {
                        break;
                    }
                }

                match found_path {
                    Some(path) => {
                        let mut output = format!(
                            "## Path: Memory {} -> Memory {}\n**Hops:** {}\n\n",
                            start_id, target_id, path.len()
                        );

                        for (i, (from, assoc_type, to)) in path.iter().enumerate() {
                            output.push_str(&format!(
                                "{}. Memory {} --[{}]--> Memory {}\n",
                                i + 1, from, assoc_type, to
                            ));
                        }

                        ToolResult::success(output).with_metadata(json!({
                            "start": start_id,
                            "target": target_id,
                            "hops": path.len(),
                            "path": path.iter().map(|(from, t, to)| json!({
                                "from": from, "type": t, "to": to
                            })).collect::<Vec<_>>()
                        }))
                    }
                    None => {
                        ToolResult::success(format!(
                            "## Path: Memory {} -> Memory {}\n\nNo path found within {} hops.",
                            start_id, target_id, max_depth
                        )).with_metadata(json!({
                            "start": start_id,
                            "target": target_id,
                            "found": false
                        }))
                    }
                }
            }

            _ => ToolResult::error(format!(
                "Unknown action: \"{}\". Use \"neighbors\", \"stats\", or \"path\".",
                params.action
            )),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_graph_definition() {
        let tool = MemoryGraphTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "memory_graph");
        assert_eq!(def.group, ToolGroup::Memory);
        assert!(def.input_schema.required.contains(&"action".to_string()));
    }
}
