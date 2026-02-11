//! QMD Memory Read Tool
//!
//! Read specific memory files or memory types.
//! In safe mode, access is sandboxed to the safemode/ memory directory only.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for reading specific memory files
pub struct QmdMemoryReadTool {
    definition: ToolDefinition,
}

impl QmdMemoryReadTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "file".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Specific file path to read (e.g., \"MEMORY.md\", \"2024-01-15.md\", \"user123/MEMORY.md\"). Use memory_search to discover available files.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "date".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Read daily log for this date (YYYY-MM-DD format). Shortcut for reading a specific day's log.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "type".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Type of memory to read: \"daily\" for today's log, \"long_term\" for MEMORY.md.".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec!["daily".to_string(), "long_term".to_string()]),
            },
        );

        properties.insert(
            "list".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "If true, list all available memory files instead of reading content.".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        Self {
            definition: ToolDefinition {
                name: "memory_read".to_string(),
                description: "Read specific memory files or list available memories. Use \"type\": \"long_term\" to read persistent facts/preferences, \"type\": \"daily\" for today's activity, or \"file\" for a specific path.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::Memory,
            },
        }
    }
}

impl Default for QmdMemoryReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ReadParams {
    file: Option<String>,
    date: Option<String>,
    #[serde(rename = "type")]
    memory_type: Option<String>,
    list: Option<bool>,
}

/// Check if tool context indicates safe mode
fn is_safe_mode(context: &ToolContext) -> bool {
    context.extra.get("safe_mode")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Safe mode identity — all reads are scoped to safemode/ directory
const SAFE_MODE_IDENTITY: &str = "safemode";

#[async_trait]
impl Tool for QmdMemoryReadTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ReadParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get memory store from context
        let memory_store = match &context.memory_store {
            Some(store) => store,
            None => {
                return ToolResult::error(
                    "Memory store not available. Memory read requires the memory system to be initialized.",
                );
            }
        };

        let safe_mode = is_safe_mode(context);

        // In safe mode, override identity to "safemode" so all reads are sandboxed
        let identity_id: Option<&str> = if safe_mode {
            Some(SAFE_MODE_IDENTITY)
        } else {
            context.identity_id.as_deref()
        };

        // Handle list request
        if params.list.unwrap_or(false) {
            return match memory_store.list_files() {
                Ok(files) => {
                    // In safe mode, filter to only safemode/ files
                    let files: Vec<_> = if safe_mode {
                        files.into_iter().filter(|f| f.starts_with("safemode/")).collect()
                    } else {
                        files
                    };

                    if files.is_empty() {
                        return ToolResult::success("No memory files found.");
                    }

                    let mut output = format!("## Available Memory Files\n**Count:** {}\n\n", files.len());
                    for file in &files {
                        output.push_str(&format!("- {}\n", file));
                    }

                    ToolResult::success(output).with_metadata(json!({
                        "file_count": files.len(),
                        "files": files
                    }))
                }
                Err(e) => ToolResult::error(format!("Failed to list files: {}", e)),
            };
        }

        // Handle specific file request
        if let Some(file_path) = params.file {
            // In safe mode, block reads outside safemode/ directory
            if safe_mode && !file_path.starts_with("safemode/") {
                return ToolResult::error("Access denied: safe mode only allows reading from safemode/ memory.");
            }

            return match memory_store.get_file(&file_path) {
                Ok(content) => {
                    if content.is_empty() {
                        return ToolResult::success(format!("File \"{}\" is empty or does not exist.", file_path));
                    }

                    let output = format!("## {}\n\n{}", file_path, content);
                    ToolResult::success(output).with_metadata(json!({
                        "file": file_path,
                        "length": content.len()
                    }))
                }
                Err(e) => ToolResult::error(format!("Failed to read file \"{}\": {}", file_path, e)),
            };
        }

        // Handle date request
        if let Some(date_str) = params.date {
            let date = match NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                Ok(d) => d,
                Err(_) => return ToolResult::error(format!("Invalid date format: \"{}\". Use YYYY-MM-DD.", date_str)),
            };

            return match memory_store.get_daily_log_for_date(date, identity_id) {
                Ok(content) => {
                    if content.is_empty() {
                        return ToolResult::success(format!("No daily log found for {}.", date_str));
                    }

                    let output = format!("## Daily Log: {}\n\n{}", date_str, content);
                    ToolResult::success(output).with_metadata(json!({
                        "date": date_str,
                        "length": content.len()
                    }))
                }
                Err(e) => ToolResult::error(format!("Failed to read daily log for {}: {}", date_str, e)),
            };
        }

        // Handle type request
        if let Some(memory_type) = params.memory_type {
            match memory_type.as_str() {
                "daily" => {
                    return match memory_store.get_daily_log(identity_id) {
                        Ok(content) => {
                            if content.is_empty() {
                                return ToolResult::success("No entries in today's daily log yet.");
                            }

                            let output = format!("## Today's Daily Log\n\n{}", content);
                            ToolResult::success(output).with_metadata(json!({
                                "type": "daily",
                                "length": content.len()
                            }))
                        }
                        Err(e) => ToolResult::error(format!("Failed to read daily log: {}", e)),
                    };
                }
                "long_term" => {
                    return match memory_store.get_long_term(identity_id) {
                        Ok(content) => {
                            if content.is_empty() {
                                return ToolResult::success("No long-term memories stored yet.");
                            }

                            let output = format!("## Long-Term Memory\n\n{}", content);
                            ToolResult::success(output).with_metadata(json!({
                                "type": "long_term",
                                "length": content.len()
                            }))
                        }
                        Err(e) => ToolResult::error(format!("Failed to read long-term memory: {}", e)),
                    };
                }
                _ => {
                    return ToolResult::error(format!("Unknown memory type: \"{}\". Use \"daily\" or \"long_term\".", memory_type));
                }
            }
        }

        // No specific request - show help
        ToolResult::success(
            "## Memory Read Usage\n\n\
            Provide one of the following parameters:\n\
            - `file`: Path to a specific memory file\n\
            - `date`: YYYY-MM-DD to read that day's log\n\
            - `type`: \"daily\" or \"long_term\"\n\
            - `list`: true to see all available files\n\n\
            Example: `{\"type\": \"long_term\"}` to read persistent memories."
        )
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::SafeMode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_read_definition() {
        let tool = QmdMemoryReadTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "memory_read");
        assert_eq!(def.group, ToolGroup::Memory);
        // No required params - all optional
        assert!(def.input_schema.required.is_empty());
    }
}
