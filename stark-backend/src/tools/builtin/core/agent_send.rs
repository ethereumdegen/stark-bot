use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for sending messages to channels proactively
pub struct AgentSendTool {
    definition: ToolDefinition,
}

impl AgentSendTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "channel".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Channel identifier. Can be a channel ID, channel name, or platform-specific identifier (e.g., telegram chat ID, discord channel ID)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "message".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The message content to send to the channel".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "reply_to".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional message ID to reply to. Platform-specific format.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "platform".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional platform hint (telegram, discord, slack). Auto-detected if not specified.".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "telegram".to_string(),
                    "discord".to_string(),
                    "slack".to_string(),
                ]),
            },
        );

        AgentSendTool {
            definition: ToolDefinition {
                name: "agent_send".to_string(),
                description: "Send a message to a channel proactively. Use this to deliver messages, alerts, or notifications to configured channels.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["channel".to_string(), "message".to_string()],
                },
                group: ToolGroup::Messaging,
                hidden: false,
            },
        }
    }
}

impl Default for AgentSendTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct AgentSendParams {
    channel: String,
    message: String,
    reply_to: Option<String>,
    platform: Option<String>,
}

#[async_trait]
impl Tool for AgentSendTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: AgentSendParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        log::info!(
            "AgentSend: Sending message to channel '{}', reply_to: {:?}",
            params.channel,
            params.reply_to
        );

        // Determine the platform
        let platform = params.platform.clone().unwrap_or_else(|| {
            // Try to detect platform from channel identifier format
            if params.channel.starts_with('-') || params.channel.parse::<i64>().is_ok() {
                // Telegram chat IDs are typically numeric (can be negative for groups)
                "telegram".to_string()
            } else if params.channel.len() == 18 && params.channel.parse::<u64>().is_ok() {
                // Discord IDs are 18-digit snowflakes
                "discord".to_string()
            } else {
                // Default to context channel type if available
                context.channel_type.clone().unwrap_or_else(|| "telegram".to_string())
            }
        });

        // For now, we'll implement a simple version that uses HTTP APIs directly
        // In a full implementation, this would integrate with the ChannelManager
        match platform.as_str() {
            "telegram" => {
                self.send_telegram(&params, context).await
            }
            "discord" => {
                self.send_discord(&params, context).await
            }
            "slack" => {
                self.send_slack(&params, context).await
            }
            other => {
                ToolResult::error(format!("Unsupported platform: {}. Supported: telegram, discord, slack", other))
            }
        }
    }
}

impl AgentSendTool {
    async fn send_telegram(&self, params: &AgentSendParams, context: &ToolContext) -> ToolResult {
        // Get bot token from channel settings
        let bot_token = match context.find_channel_bot_token("telegram", "telegram_bot_token") {
            Some(token) => token,
            None => {
                return ToolResult::error(
                    "Telegram bot token not available. Configure it in your Telegram channel settings."
                );
            }
        };

        // Build the request
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            bot_token
        );

        let mut body = json!({
            "chat_id": params.channel,
            "text": params.message,
            "parse_mode": "Markdown"
        });

        // Add reply_to if specified
        if let Some(ref reply_to) = params.reply_to {
            if let Ok(msg_id) = reply_to.parse::<i64>() {
                body["reply_to_message_id"] = json!(msg_id);
            }
        }

        // Send the request
        let client = context.http_client();
        match client.post(&url).json(&body).send().await {
            Ok(response) => {
                let status = response.status();
                let body_text = response.text().await.unwrap_or_default();

                if status.is_success() {
                    ToolResult::success(format!(
                        "Message sent to Telegram chat {}",
                        params.channel
                    )).with_metadata(json!({
                        "platform": "telegram",
                        "channel": params.channel,
                        "response": body_text
                    }))
                } else {
                    ToolResult::error(format!(
                        "Telegram API error ({}): {}",
                        status, body_text
                    ))
                }
            }
            Err(e) => ToolResult::error(format!("Failed to send Telegram message: {}", e)),
        }
    }

    async fn send_discord(&self, params: &AgentSendParams, context: &ToolContext) -> ToolResult {
        // Get bot token from channel settings
        let bot_token = match context.find_channel_bot_token("discord", "discord_bot_token") {
            Some(token) => token,
            None => {
                return ToolResult::error(
                    "Discord bot token not available. Configure it in your Discord channel settings."
                );
            }
        };

        // Build the request
        let url = format!(
            "https://discord.com/api/v10/channels/{}/messages",
            params.channel
        );

        let mut body = json!({
            "content": params.message
        });

        // Add message reference for reply
        if let Some(ref reply_to) = params.reply_to {
            body["message_reference"] = json!({
                "message_id": reply_to
            });
        }

        // Send the request
        let client = context.http_client();
        match client
            .post(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let body_text = response.text().await.unwrap_or_default();

                if status.is_success() {
                    ToolResult::success(format!(
                        "Message sent to Discord channel {}",
                        params.channel
                    )).with_metadata(json!({
                        "platform": "discord",
                        "channel": params.channel,
                        "response": body_text
                    }))
                } else {
                    ToolResult::error(format!(
                        "Discord API error ({}): {}",
                        status, body_text
                    ))
                }
            }
            Err(e) => ToolResult::error(format!("Failed to send Discord message: {}", e)),
        }
    }

    async fn send_slack(&self, params: &AgentSendParams, context: &ToolContext) -> ToolResult {
        // Get bot token from channel settings
        let bot_token = match context.find_channel_bot_token("slack", "slack_bot_token") {
            Some(token) => token,
            None => {
                return ToolResult::error(
                    "Slack bot token not available. Configure it in your Slack channel settings."
                );
            }
        };

        // Build the request
        let url = "https://slack.com/api/chat.postMessage";

        let mut body = json!({
            "channel": params.channel,
            "text": params.message
        });

        // Add thread_ts for reply
        if let Some(ref reply_to) = params.reply_to {
            body["thread_ts"] = json!(reply_to);
        }

        // Send the request
        let client = context.http_client();
        match client
            .post(url)
            .header("Authorization", format!("Bearer {}", bot_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let body_text = response.text().await.unwrap_or_default();

                // Slack returns 200 even on errors, check the response body
                if status.is_success() {
                    if let Ok(json) = serde_json::from_str::<Value>(&body_text) {
                        if json.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                            return ToolResult::success(format!(
                                "Message sent to Slack channel {}",
                                params.channel
                            )).with_metadata(json!({
                                "platform": "slack",
                                "channel": params.channel,
                                "response": json
                            }));
                        } else {
                            let error = json.get("error")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown error");
                            return ToolResult::error(format!("Slack API error: {}", error));
                        }
                    }
                }
                ToolResult::error(format!("Slack API error ({}): {}", status, body_text))
            }
            Err(e) => ToolResult::error(format!("Failed to send Slack message: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition() {
        let tool = AgentSendTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "agent_send");
        assert_eq!(def.group, ToolGroup::Messaging);
        assert!(def.input_schema.required.contains(&"channel".to_string()));
        assert!(def.input_schema.required.contains(&"message".to_string()));
    }
}
