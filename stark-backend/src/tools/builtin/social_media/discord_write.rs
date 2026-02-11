use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Write-only Discord tool for sending messages, reactions, and modifications
/// This tool is admin-only as it can modify Discord state
pub struct DiscordWriteTool {
    definition: ToolDefinition,
}

impl DiscordWriteTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The write action to perform".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "sendMessage".to_string(),
                    "react".to_string(),
                    "editMessage".to_string(),
                    "deleteMessage".to_string(),
                ]),
            },
        );

        properties.insert(
            "channelId".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Channel ID for react, editMessage, deleteMessage".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "to".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Target for sendMessage: 'channel:<id>' or 'user:<id>' for DMs".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "content".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Message content for sendMessage or editMessage".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "messageId".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Message ID for react, editMessage, deleteMessage".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "emoji".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Emoji for react action (Unicode emoji or custom emoji format)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "replyTo".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Message ID to reply to (optional, for sendMessage)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "mediaUrl".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "URL or file path for media attachment (file:///path or https://...)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        DiscordWriteTool {
            definition: ToolDefinition {
                name: "discord_write".to_string(),
                description: "Write operations for Discord: send/edit/delete messages, add reactions. Admin only. For read operations (readMessages, search, info), use 'discord_read'. For finding server/channel IDs by name, use 'discord_lookup'.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string()],
                },
                group: ToolGroup::Messaging,
                hidden: false,
            },
        }
    }
}

impl Default for DiscordWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct DiscordWriteParams {
    action: String,
    #[serde(rename = "channelId")]
    channel_id: Option<String>,
    to: Option<String>,
    content: Option<String>,
    #[serde(rename = "messageId")]
    message_id: Option<String>,
    emoji: Option<String>,
    #[serde(rename = "replyTo")]
    reply_to: Option<String>,
    #[serde(rename = "mediaUrl")]
    media_url: Option<String>,
}

#[async_trait]
impl Tool for DiscordWriteTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: DiscordWriteParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        log::info!("DiscordWrite tool: action='{}'", params.action);

        match params.action.as_str() {
            "sendMessage" => self.send_message(&params, context).await,
            "react" => self.react(&params, context).await,
            "editMessage" => self.edit_message(&params, context).await,
            "deleteMessage" => self.delete_message(&params, context).await,
            other => ToolResult::error(format!(
                "Unknown action: '{}'. Valid write actions: sendMessage, react, editMessage, deleteMessage. For read actions (readMessages, searchMessages, permissions, memberInfo, roleInfo, channelInfo, channelList), use 'discord_read'.",
                other
            )),
        }
    }
}

impl DiscordWriteTool {
    fn get_bot_token(context: &ToolContext) -> Result<String, ToolResult> {
        context.find_channel_bot_token("discord", "discord_bot_token").ok_or_else(|| {
            ToolResult::error("Discord bot token not available. Configure it in your Discord channel settings.")
        })
    }

    fn parse_discord_error(status: reqwest::StatusCode, body: &str) -> String {
        if let Ok(error_json) = serde_json::from_str::<Value>(body) {
            let code = error_json.get("code").and_then(|c| c.as_u64()).unwrap_or(0);
            let message = error_json.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            format!("Discord API error: {} (code {})", message, code)
        } else {
            format!("Discord API error ({}): {}", status, body)
        }
    }

    async fn send_message(&self, params: &DiscordWriteParams, context: &ToolContext) -> ToolResult {
        let to = match &params.to {
            Some(t) => t,
            None => return ToolResult::error("'to' is required for sendMessage (format: 'channel:<id>' or 'user:<id>')"),
        };

        let content = params.content.as_deref().unwrap_or("");
        if content.is_empty() && params.media_url.is_none() {
            return ToolResult::error("'content' or 'mediaUrl' is required for sendMessage");
        }

        // Parse the 'to' field
        let (target_type, target_id) = if let Some(id) = to.strip_prefix("channel:") {
            ("channel", id)
        } else if let Some(id) = to.strip_prefix("user:") {
            ("user", id)
        } else {
            // Assume it's a channel ID if no prefix
            ("channel", to.as_str())
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        // For DMs, we need to create a DM channel first
        let channel_id = if target_type == "user" {
            let dm_response = match client
                .post("https://discord.com/api/v10/users/@me/channels")
                .header("Authorization", format!("Bot {}", bot_token))
                .json(&json!({ "recipient_id": target_id }))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => return ToolResult::error(format!("Failed to create DM channel: {}", e)),
            };

            let dm_status = dm_response.status();
            let dm_body = dm_response.text().await.unwrap_or_default();

            if !dm_status.is_success() {
                return ToolResult::error(Self::parse_discord_error(dm_status, &dm_body));
            }

            let dm_channel: Value = match serde_json::from_str(&dm_body) {
                Ok(v) => v,
                Err(e) => return ToolResult::error(format!("Failed to parse DM channel: {}", e)),
            };

            dm_channel.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string()
        } else {
            target_id.to_string()
        };

        if channel_id.is_empty() {
            return ToolResult::error("Could not determine channel ID");
        }

        // Build the message payload
        let mut payload = json!({});
        if !content.is_empty() {
            payload["content"] = json!(content);
        }

        if let Some(reply_to) = &params.reply_to {
            payload["message_reference"] = json!({
                "message_id": reply_to
            });
        }

        // Handle media attachments
        if let Some(media_url) = &params.media_url {
            if media_url.starts_with("http://") || media_url.starts_with("https://") {
                let current_content = payload.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if current_content.is_empty() {
                    payload["content"] = json!(media_url);
                } else {
                    payload["content"] = json!(format!("{}\n{}", current_content, media_url));
                }
            } else if media_url.starts_with("file://") {
                return ToolResult::error("Local file uploads (file://) are not yet supported. Please use a remote URL (https://).");
            }
        }

        let url = format!("https://discord.com/api/v10/channels/{}/messages", channel_id);

        let response = match client
            .post(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to send message: {}", e)),
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return ToolResult::error(Self::parse_discord_error(status, &body));
        }

        let sent_message: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(format!("Failed to parse response: {}", e)),
        };

        let message_id = sent_message.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");

        ToolResult::success(format!(
            "Message sent successfully to {} (message ID: {})",
            to, message_id
        )).with_metadata(json!({
            "message_id": message_id,
            "channel_id": channel_id,
            "to": to
        }))
    }

    async fn react(&self, params: &DiscordWriteParams, context: &ToolContext) -> ToolResult {
        let channel_id = match &params.channel_id {
            Some(id) => id,
            None => return ToolResult::error("'channelId' is required for react"),
        };

        let message_id = match &params.message_id {
            Some(id) => id,
            None => return ToolResult::error("'messageId' is required for react"),
        };

        let emoji = match &params.emoji {
            Some(e) => e,
            None => return ToolResult::error("'emoji' is required for react"),
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        let encoded_emoji = urlencoding::encode(emoji);

        let url = format!(
            "https://discord.com/api/v10/channels/{}/messages/{}/reactions/{}/@me",
            channel_id, message_id, encoded_emoji
        );

        let response = match client
            .put(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .header("Content-Length", "0")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to add reaction: {}", e)),
        };

        let status = response.status();

        if status == reqwest::StatusCode::NO_CONTENT || status.is_success() {
            ToolResult::success(format!(
                "Added reaction {} to message {} in channel {}",
                emoji, message_id, channel_id
            )).with_metadata(json!({
                "emoji": emoji,
                "message_id": message_id,
                "channel_id": channel_id
            }))
        } else {
            let body = response.text().await.unwrap_or_default();
            ToolResult::error(Self::parse_discord_error(status, &body))
        }
    }

    async fn edit_message(&self, params: &DiscordWriteParams, context: &ToolContext) -> ToolResult {
        let channel_id = match &params.channel_id {
            Some(id) => id,
            None => return ToolResult::error("'channelId' is required for editMessage"),
        };

        let message_id = match &params.message_id {
            Some(id) => id,
            None => return ToolResult::error("'messageId' is required for editMessage"),
        };

        let content = match &params.content {
            Some(c) => c,
            None => return ToolResult::error("'content' is required for editMessage"),
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        let url = format!(
            "https://discord.com/api/v10/channels/{}/messages/{}",
            channel_id, message_id
        );

        let response = match client
            .patch(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .json(&json!({ "content": content }))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to edit message: {}", e)),
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return ToolResult::error(Self::parse_discord_error(status, &body));
        }

        ToolResult::success(format!(
            "Message {} edited successfully in channel {}",
            message_id, channel_id
        )).with_metadata(json!({
            "message_id": message_id,
            "channel_id": channel_id
        }))
    }

    async fn delete_message(&self, params: &DiscordWriteParams, context: &ToolContext) -> ToolResult {
        let channel_id = match &params.channel_id {
            Some(id) => id,
            None => return ToolResult::error("'channelId' is required for deleteMessage"),
        };

        let message_id = match &params.message_id {
            Some(id) => id,
            None => return ToolResult::error("'messageId' is required for deleteMessage"),
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        let url = format!(
            "https://discord.com/api/v10/channels/{}/messages/{}",
            channel_id, message_id
        );

        let response = match client
            .delete(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to delete message: {}", e)),
        };

        let status = response.status();

        if status == reqwest::StatusCode::NO_CONTENT || status.is_success() {
            ToolResult::success(format!(
                "Message {} deleted from channel {}",
                message_id, channel_id
            )).with_metadata(json!({
                "message_id": message_id,
                "channel_id": channel_id
            }))
        } else {
            let body = response.text().await.unwrap_or_default();
            ToolResult::error(Self::parse_discord_error(status, &body))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition() {
        let tool = DiscordWriteTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "discord_write");
        assert_eq!(def.group, ToolGroup::Messaging);
        assert!(def.input_schema.required.contains(&"action".to_string()));
    }
}
