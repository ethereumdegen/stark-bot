//! Agent tool for reading and updating `bot_config.ron`.
//!
//! Actions:
//! - `read`   — load and return the current config as formatted RON
//! - `update` — merge optional fields into the existing config and save

use crate::models::BotConfig;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct ModifyBotConfigTool {
    definition: ToolDefinition,
}

impl ModifyBotConfigTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action to perform: 'read' to view current config, 'update' to change settings".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec!["read".to_string(), "update".to_string()]),
            },
        );

        properties.insert(
            "bot_name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "New bot name (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "operating_mode".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Operating mode: 'Rogue' for autonomous actions, 'Partner' for user-confirmed operations (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec!["Rogue".to_string(), "Partner".to_string()]),
            },
        );

        properties.insert(
            "heartbeat_enabled".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Enable/disable heartbeat (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "heartbeat_interval_minutes".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Heartbeat interval in minutes (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "heartbeat_active_hours_start".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Start of active hours in HH:MM format (for 'update' action). Use empty string to clear.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "heartbeat_active_hours_end".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "End of active hours in HH:MM format (for 'update' action). Use empty string to clear.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "heartbeat_active_days".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Comma-separated active days e.g. 'mon,tue,wed,thu,fri' (for 'update' action). Use empty string to clear.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "max_tool_iterations".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum tool call iterations per request (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "max_response_tokens".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum tokens for AI response output (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "max_context_tokens".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Context window limit for conversation history (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "safe_mode_max_queries_per_10min".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Max safe-mode queries per user per 10 minutes (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "guest_dashboard_enabled".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Enable/disable guest dashboard access (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "session_memory_log".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "Enable/disable session memory logging (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "compaction_background_threshold".to_string(),
            PropertySchema {
                schema_type: "number".to_string(),
                description: "Context compaction background threshold 0.0-1.0 (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "compaction_aggressive_threshold".to_string(),
            PropertySchema {
                schema_type: "number".to_string(),
                description: "Context compaction aggressive threshold 0.0-1.0 (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "compaction_emergency_threshold".to_string(),
            PropertySchema {
                schema_type: "number".to_string(),
                description: "Context compaction emergency threshold 0.0-1.0 (for 'update' action)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "whisper_server_url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Whisper speech-to-text server URL (for 'update' action). Use empty string to clear.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "embeddings_server_url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Embeddings server URL (for 'update' action). Use empty string to clear.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "http_proxy_url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "HTTP proxy URL for tool requests (for 'update' action). Use empty string to clear.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "keystore_server_url".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Keystore server URL for cloud backups (for 'update' action). Use empty string to clear.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ModifyBotConfigTool {
            definition: ToolDefinition {
                name: "modify_bot_config".to_string(),
                description: "Read or update the bot configuration (bot_config.ron). Controls bot name, operating mode, heartbeat scheduling, compaction thresholds, service URLs, rate limits, and more.".to_string(),
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

impl Default for ModifyBotConfigTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ModifyBotConfigParams {
    action: String,
    bot_name: Option<String>,
    operating_mode: Option<String>,
    heartbeat_enabled: Option<bool>,
    heartbeat_interval_minutes: Option<i32>,
    heartbeat_active_hours_start: Option<String>,
    heartbeat_active_hours_end: Option<String>,
    heartbeat_active_days: Option<String>,
    max_tool_iterations: Option<i32>,
    max_response_tokens: Option<i32>,
    max_context_tokens: Option<i32>,
    safe_mode_max_queries_per_10min: Option<i32>,
    guest_dashboard_enabled: Option<bool>,
    session_memory_log: Option<bool>,
    compaction_background_threshold: Option<f64>,
    compaction_aggressive_threshold: Option<f64>,
    compaction_emergency_threshold: Option<f64>,
    whisper_server_url: Option<String>,
    embeddings_server_url: Option<String>,
    http_proxy_url: Option<String>,
    keystore_server_url: Option<String>,
}

fn format_config_ron(config: &BotConfig) -> String {
    let pretty = ron::ser::PrettyConfig::default();
    ron::ser::to_string_pretty(config, pretty).unwrap_or_else(|e| format!("(serialization error: {})", e))
}

#[async_trait]
impl Tool for ModifyBotConfigTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, _context: &ToolContext) -> ToolResult {
        let params: ModifyBotConfigParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        match params.action.as_str() {
            "read" => {
                let config = BotConfig::load();
                let ron_text = format_config_ron(&config);
                ToolResult::success(ron_text)
            }

            "update" => {
                let mut config = BotConfig::load();

                if let Some(name) = params.bot_name {
                    config.bot_name = name;
                }
                if let Some(mode) = params.operating_mode {
                    use crate::models::bot_config::OperatingMode;
                    config.operating_mode = match mode.as_str() {
                        "Rogue" => OperatingMode::Rogue,
                        "Partner" => OperatingMode::Partner,
                        _ => return ToolResult::error(format!(
                            "Invalid operating_mode: '{}'. Must be 'Rogue' or 'Partner'.", mode
                        )),
                    };
                }
                if let Some(enabled) = params.heartbeat_enabled {
                    config.heartbeat.enabled = enabled;
                }
                if let Some(interval) = params.heartbeat_interval_minutes {
                    if interval < 1 {
                        return ToolResult::error("heartbeat_interval_minutes must be >= 1");
                    }
                    config.heartbeat.interval_minutes = interval;
                }
                if let Some(start) = params.heartbeat_active_hours_start {
                    config.heartbeat.active_hours_start = if start.is_empty() { None } else { Some(start) };
                }
                if let Some(end) = params.heartbeat_active_hours_end {
                    config.heartbeat.active_hours_end = if end.is_empty() { None } else { Some(end) };
                }
                if let Some(days) = params.heartbeat_active_days {
                    config.heartbeat.active_days = if days.is_empty() { None } else { Some(days) };
                }
                if let Some(v) = params.max_tool_iterations {
                    if v < 1 { return ToolResult::error("max_tool_iterations must be >= 1"); }
                    config.max_tool_iterations = v;
                }
                if let Some(v) = params.max_response_tokens {
                    if v < 1000 { return ToolResult::error("max_response_tokens must be >= 1000"); }
                    config.max_response_tokens = v;
                }
                if let Some(v) = params.max_context_tokens {
                    if v < 80000 { return ToolResult::error("max_context_tokens must be >= 80000"); }
                    config.max_context_tokens = v;
                }
                if let Some(v) = params.safe_mode_max_queries_per_10min {
                    if v < 1 { return ToolResult::error("safe_mode_max_queries_per_10min must be >= 1"); }
                    config.safe_mode_max_queries_per_10min = v;
                }
                if let Some(v) = params.guest_dashboard_enabled {
                    config.guest_dashboard_enabled = v;
                }
                if let Some(v) = params.session_memory_log {
                    config.session_memory_log = v;
                }
                if let Some(v) = params.compaction_background_threshold {
                    config.compaction.background_threshold = v;
                }
                if let Some(v) = params.compaction_aggressive_threshold {
                    config.compaction.aggressive_threshold = v;
                }
                if let Some(v) = params.compaction_emergency_threshold {
                    config.compaction.emergency_threshold = v;
                }
                if let Some(v) = params.whisper_server_url {
                    config.services.whisper_server_url = if v.is_empty() { None } else { Some(v) };
                }
                if let Some(v) = params.embeddings_server_url {
                    config.services.embeddings_server_url = if v.is_empty() { None } else { Some(v) };
                }
                if let Some(v) = params.http_proxy_url {
                    config.services.http_proxy_url = if v.is_empty() { None } else { Some(v) };
                }
                if let Some(v) = params.keystore_server_url {
                    config.services.keystore_server_url = if v.is_empty() { None } else { Some(v) };
                }

                match config.save() {
                    Ok(()) => {
                        let ron_text = format_config_ron(&config);
                        ToolResult::success(format!("Bot config updated:\n\n{}", ron_text))
                    }
                    Err(e) => ToolResult::error(format!("Failed to save config: {}", e)),
                }
            }

            _ => ToolResult::error(format!(
                "Unknown action: '{}'. Valid actions: read, update",
                params.action
            )),
        }
    }
}
