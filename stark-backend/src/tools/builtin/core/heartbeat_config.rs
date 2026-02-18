//! Heartbeat configuration management tool
//!
//! Allows the agent to view and control heartbeat settings:
//! - list: Show all heartbeat configs
//! - get: Get a specific config by ID
//! - update: Change interval, active hours/days, target
//! - enable/disable: Toggle heartbeat on/off

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct HeartbeatConfigTool {
    definition: ToolDefinition,
}

impl HeartbeatConfigTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The action to perform: 'list' (show all configs), 'get' (get specific config), 'update' (change settings), 'enable' (turn on), 'disable' (turn off)".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "list".to_string(),
                    "get".to_string(),
                    "update".to_string(),
                    "enable".to_string(),
                    "disable".to_string(),
                ]),
            },
        );

        properties.insert(
            "config_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Heartbeat config ID (required for get/update/enable/disable). Use 'list' first to find IDs.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "channel_id".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Channel ID for 'get' action to get/create config for a specific channel. Omit for global config.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "interval_minutes".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Interval between heartbeats in minutes (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "target".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Heartbeat target: 'last' to continue last session, or a specific session key (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "active_hours_start".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Start of active hours in HH:MM format, e.g. '09:00' (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "active_hours_end".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "End of active hours in HH:MM format, e.g. '17:00' (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "active_days".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Comma-separated active days, e.g. 'mon,tue,wed,thu,fri' (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "impulse_evolver".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Enable/disable the impulse evolver — automatically evolves impulse map nodes after each heartbeat (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        HeartbeatConfigTool {
            definition: ToolDefinition {
                name: "heartbeat_config".to_string(),
                description: "Manage heartbeat settings: list configs, view details, update interval/schedule/impulse_evolver, enable or disable. The heartbeat system periodically wakes the agent to reflect on the impulse_map. The impulse_evolver sub-system automatically evolves map nodes after each heartbeat.".to_string(),
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

impl Default for HeartbeatConfigTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct HeartbeatConfigParams {
    action: String,
    config_id: Option<i64>,
    channel_id: Option<i64>,
    interval_minutes: Option<i32>,
    target: Option<String>,
    active_hours_start: Option<String>,
    active_hours_end: Option<String>,
    active_days: Option<String>,
    impulse_evolver: Option<bool>,
}

fn format_config(config: &crate::models::HeartbeatConfig) -> String {
    let mut out = format!(
        "Heartbeat Config #{}\n  Enabled: {}\n  Interval: {} minutes\n  Target: {}",
        config.id,
        if config.enabled { "YES" } else { "NO" },
        config.interval_minutes,
        config.target,
    );

    if let Some(ref ch) = config.channel_id {
        out.push_str(&format!("\n  Channel: {}", ch));
    } else {
        out.push_str("\n  Channel: Global");
    }

    if let Some(ref start) = config.active_hours_start {
        out.push_str(&format!("\n  Active hours: {} - {}", start, config.active_hours_end.as_deref().unwrap_or("?")));
    }

    if let Some(ref days) = config.active_days {
        out.push_str(&format!("\n  Active days: {}", days));
    }

    if let Some(ref last) = config.last_beat_at {
        out.push_str(&format!("\n  Last beat: {}", last));
    }

    if let Some(ref next) = config.next_beat_at {
        out.push_str(&format!("\n  Next beat: {}", next));
    }

    out.push_str(&format!("\n  Impulse Evolver: {}", if config.impulse_evolver { "ON" } else { "OFF" }));

    if let Some(ref node_id) = config.current_impulse_node_id {
        out.push_str(&format!("\n  Current impulse node: #{}", node_id));
    }

    out
}

#[async_trait]
impl Tool for HeartbeatConfigTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: HeartbeatConfigParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let db = match &context.database {
            Some(db) => db,
            None => return ToolResult::error("Database not available"),
        };

        match params.action.as_str() {
            "list" => {
                match db.list_heartbeat_configs() {
                    Ok(configs) => {
                        if configs.is_empty() {
                            return ToolResult::success("No heartbeat configs found. Use 'get' with a channel_id to create one, or 'get' without channel_id for the global config.");
                        }

                        let output: Vec<String> = configs.iter().map(|c| format_config(c)).collect();
                        ToolResult::success(output.join("\n\n"))
                            .with_metadata(json!({ "count": configs.len() }))
                    }
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "get" => {
                if let Some(config_id) = params.config_id {
                    match db.get_heartbeat_config_by_id(config_id) {
                        Ok(Some(config)) => ToolResult::success(format_config(&config)),
                        Ok(None) => ToolResult::error(format!("Heartbeat config #{} not found", config_id)),
                        Err(e) => ToolResult::error(format!("Database error: {}", e)),
                    }
                } else {
                    // Get or create for channel (or global)
                    match db.get_or_create_heartbeat_config(params.channel_id) {
                        Ok(config) => ToolResult::success(format_config(&config)),
                        Err(e) => ToolResult::error(format!("Database error: {}", e)),
                    }
                }
            }

            "update" => {
                let config_id = match params.config_id {
                    Some(id) => id,
                    None => return ToolResult::error("'config_id' is required for 'update' action. Use 'list' to find the config ID."),
                };

                match db.update_heartbeat_config(
                    config_id,
                    params.interval_minutes,
                    params.target.as_deref(),
                    params.active_hours_start.as_deref(),
                    params.active_hours_end.as_deref(),
                    params.active_days.as_deref(),
                    None, // don't change enabled via update — use enable/disable actions
                    params.impulse_evolver,
                ) {
                    Ok(config) => ToolResult::success(format!("Updated heartbeat config:\n\n{}", format_config(&config))),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "enable" => {
                let config_id = match params.config_id {
                    Some(id) => id,
                    None => {
                        // Try global config
                        match db.get_or_create_heartbeat_config(None) {
                            Ok(config) => config.id,
                            Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                        }
                    }
                };

                match db.update_heartbeat_config(config_id, None, None, None, None, None, Some(true), None) {
                    Ok(config) => ToolResult::success(format!("Heartbeat ENABLED:\n\n{}", format_config(&config))),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            "disable" => {
                let config_id = match params.config_id {
                    Some(id) => id,
                    None => {
                        match db.get_or_create_heartbeat_config(None) {
                            Ok(config) => config.id,
                            Err(e) => return ToolResult::error(format!("Database error: {}", e)),
                        }
                    }
                };

                match db.update_heartbeat_config(config_id, None, None, None, None, None, Some(false), None) {
                    Ok(config) => ToolResult::success(format!("Heartbeat DISABLED:\n\n{}", format_config(&config))),
                    Err(e) => ToolResult::error(format!("Database error: {}", e)),
                }
            }

            _ => ToolResult::error(format!(
                "Unknown action: '{}'. Valid actions: list, get, update, enable, disable",
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
        let tool = HeartbeatConfigTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "heartbeat_config");
        assert_eq!(def.group, ToolGroup::System);
        assert!(def.input_schema.required.contains(&"action".to_string()));
    }
}
