use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Write-only Telegram tool for moderation: delete messages, ban/restrict users.
/// Admin-only — not available in safe mode.
pub struct TelegramWriteTool {
    definition: ToolDefinition,
}

impl TelegramWriteTool {
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
                    "deleteMessage".to_string(),
                    "banChatMember".to_string(),
                    "restrictChatMember".to_string(),
                    "sendMessage".to_string(),
                ]),
            },
        );

        properties.insert(
            "chatId".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Telegram chat ID (required for all actions)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "messageId".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Message ID for deleteMessage".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "userId".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "User ID for banChatMember or restrictChatMember".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "content".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Message text for sendMessage".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "replyTo".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Optional message ID to reply to (for sendMessage)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "revokeMessages".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "If true, delete all messages from the banned user (for banChatMember, default: true)".to_string(),
                default: Some(json!(true)),
                items: None,
                enum_values: None,
            },
        );

        TelegramWriteTool {
            definition: ToolDefinition {
                name: "telegram_write".to_string(),
                description: "Write/moderation operations for Telegram: delete messages, ban users, restrict users, send messages. Admin only. For read operations (getChatInfo, readHistory, etc.), use 'telegram_read'.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string(), "chatId".to_string()],
                },
                group: ToolGroup::Messaging,
                hidden: false,
            },
        }
    }
}

impl Default for TelegramWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct TelegramWriteParams {
    action: String,
    #[serde(rename = "chatId")]
    chat_id: String,
    #[serde(rename = "messageId")]
    message_id: Option<String>,
    #[serde(rename = "userId")]
    user_id: Option<String>,
    content: Option<String>,
    #[serde(rename = "replyTo")]
    reply_to: Option<String>,
    #[serde(rename = "revokeMessages")]
    revoke_messages: Option<bool>,
}

#[async_trait]
impl Tool for TelegramWriteTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: TelegramWriteParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        log::info!("TelegramWrite tool: action='{}' chatId='{}'", params.action, params.chat_id);

        match params.action.as_str() {
            "deleteMessage" => self.delete_message(&params, context).await,
            "banChatMember" => self.ban_chat_member(&params, context).await,
            "restrictChatMember" => self.restrict_chat_member(&params, context).await,
            "sendMessage" => self.send_message(&params, context).await,
            other => ToolResult::error(format!(
                "Unknown action: '{}'. Valid actions: deleteMessage, banChatMember, restrictChatMember, sendMessage",
                other
            )),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Standard
    }
}

impl TelegramWriteTool {
    fn get_bot_token(context: &ToolContext) -> Result<String, ToolResult> {
        context.find_channel_bot_token("telegram", "telegram_bot_token").ok_or_else(|| {
            ToolResult::error("Telegram bot token not available. Configure it in your Telegram channel settings.")
        })
    }

    fn parse_telegram_error(status: reqwest::StatusCode, body: &str) -> String {
        if let Ok(error_json) = serde_json::from_str::<Value>(body) {
            let error_code = error_json.get("error_code").and_then(|c| c.as_u64()).unwrap_or(0);
            let description = error_json.get("description").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            format!("Telegram API error: {} (code {})", description, error_code)
        } else {
            format!("Telegram API error ({}): {}", status, body)
        }
    }

    async fn telegram_api_call(token: &str, method: &str, params: &Value, client: &reqwest::Client) -> Result<Value, ToolResult> {
        let url = format!("https://api.telegram.org/bot{}/{}", token, method);

        let response = client
            .post(&url)
            .json(params)
            .send()
            .await
            .map_err(|e| ToolResult::error(format!("Failed to call Telegram API {}: {}", method, e)))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(ToolResult::error(Self::parse_telegram_error(status, &body)));
        }

        let response_json: Value = serde_json::from_str(&body)
            .map_err(|e| ToolResult::error(format!("Failed to parse Telegram response: {}", e)))?;

        if response_json.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let description = response_json.get("description").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            let error_code = response_json.get("error_code").and_then(|c| c.as_u64()).unwrap_or(0);
            return Err(ToolResult::error(format!("Telegram API error: {} (code {})", description, error_code)));
        }

        Ok(response_json.get("result").cloned().unwrap_or(json!(true)))
    }

    async fn delete_message(&self, params: &TelegramWriteParams, context: &ToolContext) -> ToolResult {
        let message_id = match &params.message_id {
            Some(id) => id.clone(),
            None => return ToolResult::error("'messageId' is required for deleteMessage"),
        };

        let token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };

        let client = context.http_client();
        match Self::telegram_api_call(&token, "deleteMessage", &json!({
            "chat_id": params.chat_id,
            "message_id": message_id.parse::<i64>().unwrap_or(0)
        }), &client).await {
            Ok(_) => ToolResult::success(format!(
                "Message {} deleted from chat {}",
                message_id, params.chat_id
            )).with_metadata(json!({
                "message_id": message_id,
                "chat_id": params.chat_id
            })),
            Err(e) => e,
        }
    }

    async fn ban_chat_member(&self, params: &TelegramWriteParams, context: &ToolContext) -> ToolResult {
        let user_id = match &params.user_id {
            Some(id) => id.clone(),
            None => return ToolResult::error("'userId' is required for banChatMember"),
        };

        let token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };

        let revoke = params.revoke_messages.unwrap_or(true);

        let client = context.http_client();
        match Self::telegram_api_call(&token, "banChatMember", &json!({
            "chat_id": params.chat_id,
            "user_id": user_id.parse::<i64>().unwrap_or(0),
            "revoke_messages": revoke
        }), &client).await {
            Ok(_) => ToolResult::success(format!(
                "User {} banned from chat {}{}",
                user_id, params.chat_id,
                if revoke { " (messages revoked)" } else { "" }
            )).with_metadata(json!({
                "user_id": user_id,
                "chat_id": params.chat_id,
                "revoke_messages": revoke
            })),
            Err(e) => e,
        }
    }

    async fn restrict_chat_member(&self, params: &TelegramWriteParams, context: &ToolContext) -> ToolResult {
        let user_id = match &params.user_id {
            Some(id) => id.clone(),
            None => return ToolResult::error("'userId' is required for restrictChatMember"),
        };

        let token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };

        let client = context.http_client();
        // Restrict: revoke all permissions (mute the user)
        match Self::telegram_api_call(&token, "restrictChatMember", &json!({
            "chat_id": params.chat_id,
            "user_id": user_id.parse::<i64>().unwrap_or(0),
            "permissions": {
                "can_send_messages": false,
                "can_send_audios": false,
                "can_send_documents": false,
                "can_send_photos": false,
                "can_send_videos": false,
                "can_send_video_notes": false,
                "can_send_voice_notes": false,
                "can_send_polls": false,
                "can_send_other_messages": false,
                "can_add_web_page_previews": false,
                "can_change_info": false,
                "can_invite_users": false,
                "can_pin_messages": false,
                "can_manage_topics": false
            }
        }), &client).await {
            Ok(_) => ToolResult::success(format!(
                "User {} restricted (muted) in chat {}",
                user_id, params.chat_id
            )).with_metadata(json!({
                "user_id": user_id,
                "chat_id": params.chat_id
            })),
            Err(e) => e,
        }
    }

    async fn send_message(&self, params: &TelegramWriteParams, context: &ToolContext) -> ToolResult {
        let content = match &params.content {
            Some(c) if !c.is_empty() => c.clone(),
            _ => return ToolResult::error("'content' is required for sendMessage"),
        };

        let token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };

        let client = context.http_client();

        let mut body = json!({
            "chat_id": params.chat_id,
            "text": content,
            "parse_mode": "Markdown"
        });

        if let Some(ref reply_to) = params.reply_to {
            if let Ok(msg_id) = reply_to.parse::<i64>() {
                body["reply_to_message_id"] = json!(msg_id);
            }
        }

        match Self::telegram_api_call(&token, "sendMessage", &body, &client).await {
            Ok(result) => {
                let sent_id = result.get("message_id").and_then(|v| v.as_i64()).unwrap_or(0);
                ToolResult::success(format!(
                    "Message sent to chat {} (message ID: {})",
                    params.chat_id, sent_id
                )).with_metadata(json!({
                    "message_id": sent_id,
                    "chat_id": params.chat_id
                }))
            }
            Err(e) => e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition() {
        let tool = TelegramWriteTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "telegram_write");
        assert_eq!(def.group, ToolGroup::Messaging);
        assert!(def.input_schema.required.contains(&"action".to_string()));
        assert!(def.input_schema.required.contains(&"chatId".to_string()));

        let action_prop = &def.input_schema.properties["action"];
        let actions = action_prop.enum_values.as_ref().unwrap();
        assert!(actions.contains(&"deleteMessage".to_string()));
        assert!(actions.contains(&"banChatMember".to_string()));
        assert!(actions.contains(&"restrictChatMember".to_string()));
        assert!(actions.contains(&"sendMessage".to_string()));
    }
}
