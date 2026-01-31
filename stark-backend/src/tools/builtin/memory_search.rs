//! Memory search tool for semantic and keyword-based memory retrieval
//!
//! This tool allows the agent to search through stored memories using:
//! - Full-text search (BM25 ranking)
//! - Filtering by memory type, importance, and identity

use crate::models::MemoryType;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for searching memories using semantic and keyword queries
pub struct MemorySearchTool {
    definition: ToolDefinition,
}

impl MemorySearchTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "query".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The search query (keywords or semantic phrase). Uses full-text search with BM25 ranking.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "memory_type".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional filter by memory type.".to_string(),
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
            "min_importance".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Minimum importance level (1-10). Only return memories with importance >= this value.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "category".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional filter by category.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "limit".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum number of results to return (default: 10, max: 50).".to_string(),
                default: Some(json!(10)),
                items: None,
                enum_values: None,
            },
        );

        MemorySearchTool {
            definition: ToolDefinition {
                name: "memory_search".to_string(),
                description: "Search stored memories for previously learned information. Use ONCE per topic - if no results, don't retry with variations. Best for: user preferences, past conversations, known facts. Not for: new topics, external data (use web_fetch instead).".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["query".to_string()],
                },
                group: ToolGroup::System,
            },
        }
    }
}

impl Default for MemorySearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct MemorySearchParams {
    query: String,
    memory_type: Option<String>,
    min_importance: Option<i32>,
    category: Option<String>,
    limit: Option<i32>,
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: MemorySearchParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get database from context (using typed field)
        let db = match &context.database {
            Some(db) => db,
            None => {
                return ToolResult::error(
                    "Database not available. Memory search requires database access.",
                );
            }
        };

        // Parse memory type filter
        let memory_type = params.memory_type.as_ref().and_then(|t| MemoryType::from_str(t));

        // Get identity from context
        let identity_id = context.identity_id.as_deref();

        // Limit results (max 50)
        let limit = params.limit.unwrap_or(10).min(50);

        // Escape the query for FTS5
        let escaped_query = escape_fts5_query(&params.query);

        // Execute search
        match db.search_memories(
            &escaped_query,
            memory_type,
            identity_id,
            params.category.as_deref(),
            params.min_importance,
            limit,
        ) {
            Ok(results) => {
                if results.is_empty() {
                    return ToolResult::success(format!(
                        "No memories found matching query: '{}'",
                        params.query
                    ));
                }

                // Format results as markdown
                let mut output = format!(
                    "## Memory Search Results\n\
                     Query: '{}'\n\
                     Found: {} memories\n\n",
                    params.query,
                    results.len()
                );

                for (i, result) in results.iter().enumerate() {
                    let memory = &result.memory;

                    output.push_str(&format!(
                        "### {}. {} (ID: {})\n",
                        i + 1,
                        memory.memory_type.as_str(),
                        memory.id
                    ));

                    // Add metadata
                    output.push_str(&format!(
                        "**Importance:** {} | **Relevance Score:** {:.2}\n",
                        memory.importance,
                        -result.rank // BM25 scores are negative, lower is better
                    ));

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

                    output.push_str(&format!(
                        "**Created:** {}\n\n",
                        memory.created_at.format("%Y-%m-%d %H:%M UTC")
                    ));

                    // Add content (truncate if too long)
                    let content = if memory.content.len() > 500 {
                        format!("{}...", &memory.content[..500])
                    } else {
                        memory.content.clone()
                    };
                    output.push_str(&format!("{}\n\n---\n\n", content));
                }

                ToolResult::success(output).with_metadata(json!({
                    "query": params.query,
                    "count": results.len(),
                    "memory_ids": results.iter().map(|r| r.memory.id).collect::<Vec<_>>()
                }))
            }
            Err(e) => ToolResult::error(format!("Memory search failed: {}", e)),
        }
    }
}

/// Escape special characters for FTS5 query syntax
fn escape_fts5_query(query: &str) -> String {
    // FTS5 uses double quotes for phrase matching and * for prefix matching
    // We'll wrap the query in quotes for exact phrase matching, or just clean it
    let cleaned: String = query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
        .collect();

    // If the query has multiple words, use OR for more flexible matching
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.len() > 1 {
        words.join(" OR ")
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_search_definition() {
        let tool = MemorySearchTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "memory_search");
        assert_eq!(def.group, ToolGroup::System);
        assert!(def.input_schema.required.contains(&"query".to_string()));
    }

    #[test]
    fn test_fts5_escape() {
        assert_eq!(escape_fts5_query("hello world"), "hello OR world");
        assert_eq!(escape_fts5_query("single"), "single");
        assert_eq!(escape_fts5_query("test-query_123"), "test-query_123");
    }
}
