use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Read-only Telegram tool for fetching chat info, members, admins, and conversation history.
/// Uses Telegram Bot API for live metadata and local DB for message history.
pub struct TelegramReadTool {
    definition: ToolDefinition,
}

impl TelegramReadTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The read action to perform".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "getChatInfo".to_string(),
                    "getChatMember".to_string(),
                    "getChatAdministrators".to_string(),
                    "getChatMemberCount".to_string(),
                    "readHistory".to_string(),
                ]),
            },
        );

        properties.insert(
            "chatId".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Telegram chat ID for getChatInfo, getChatMember, getChatAdministrators, getChatMemberCount, or readHistory (to read a different chat's history)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "userId".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Telegram user ID for getChatMember".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "limit".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Number of messages to fetch for readHistory (default: 20, max: 100)".to_string(),
                default: Some(json!(20)),
                items: None,
                enum_values: None,
            },
        );

        TelegramReadTool {
            definition: ToolDefinition {
                name: "telegram_read".to_string(),
                description: "Read-only Telegram operations: get chat info, member info, list admins, member count, and read conversation history from local DB. Safe for all users.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string()],
                },
                group: ToolGroup::Messaging,
            },
        }
    }
}

impl Default for TelegramReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct TelegramReadParams {
    action: String,
    #[serde(rename = "chatId")]
    chat_id: Option<String>,
    #[serde(rename = "userId")]
    user_id: Option<String>,
    limit: Option<i32>,
}

#[async_trait]
impl Tool for TelegramReadTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: TelegramReadParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        log::info!("TelegramRead tool: action='{}'", params.action);

        match params.action.as_str() {
            "getChatInfo" => self.get_chat_info(&params, context).await,
            "getChatMember" => self.get_chat_member(&params, context).await,
            "getChatAdministrators" => self.get_chat_administrators(&params, context).await,
            "getChatMemberCount" => self.get_chat_member_count(&params, context).await,
            "readHistory" => self.read_history(&params, context).await,
            other => ToolResult::error(format!(
                "Unknown action: '{}'. Valid actions: getChatInfo, getChatMember, getChatAdministrators, getChatMemberCount, readHistory",
                other
            )),
        }
    }
}

impl TelegramReadTool {
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

    async fn telegram_api_call(token: &str, method: &str, params: &Value) -> Result<Value, ToolResult> {
        let client = reqwest::Client::new();
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

        // Telegram wraps results in {"ok": true, "result": ...}
        if response_json.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let description = response_json.get("description").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            let error_code = response_json.get("error_code").and_then(|c| c.as_u64()).unwrap_or(0);
            return Err(ToolResult::error(format!("Telegram API error: {} (code {})", description, error_code)));
        }

        response_json.get("result").cloned().ok_or_else(|| {
            ToolResult::error("Telegram API returned ok but no result field")
        })
    }

    async fn get_chat_info(&self, params: &TelegramReadParams, context: &ToolContext) -> ToolResult {
        let chat_id = match &params.chat_id {
            Some(id) => id.clone(),
            None => return ToolResult::error("'chatId' is required for getChatInfo"),
        };

        let token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };

        let result = match Self::telegram_api_call(&token, "getChat", &json!({"chat_id": chat_id})).await {
            Ok(r) => r,
            Err(e) => return e,
        };

        let title = result.get("title").and_then(|v| v.as_str()).unwrap_or("N/A");
        let chat_type = result.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
        let description = result.get("description").and_then(|v| v.as_str()).unwrap_or("None");
        let username = result.get("username").and_then(|v| v.as_str());
        let first_name = result.get("first_name").and_then(|v| v.as_str());

        let display_name = title.to_string();
        let display_name = if display_name == "N/A" {
            first_name.unwrap_or("Unknown").to_string()
        } else {
            display_name
        };

        let pinned = result.get("pinned_message")
            .and_then(|m| m.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("None");

        let message = format!(
            "Chat info for {} ({}):\n\
            - Title: {}\n\
            - Type: {}\n\
            - Username: {}\n\
            - Description: {}\n\
            - Pinned message: {}",
            chat_id,
            display_name,
            display_name,
            chat_type,
            username.unwrap_or("N/A"),
            description,
            if pinned.len() > 100 { format!("{}...", &pinned[..100]) } else { pinned.to_string() }
        );

        context.set_register("telegram_chat_id", json!(chat_id), "telegram_read");

        ToolResult::success(message).with_metadata(json!({
            "chat_id": chat_id,
            "title": display_name,
            "type": chat_type,
            "username": username,
            "description": description,
            "pinned_message": pinned,
            "raw": result
        }))
    }

    async fn get_chat_member(&self, params: &TelegramReadParams, context: &ToolContext) -> ToolResult {
        let chat_id = match &params.chat_id {
            Some(id) => id.clone(),
            None => return ToolResult::error("'chatId' is required for getChatMember"),
        };

        let user_id = match &params.user_id {
            Some(id) => id.clone(),
            None => return ToolResult::error("'userId' is required for getChatMember"),
        };

        let token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };

        let result = match Self::telegram_api_call(&token, "getChatMember", &json!({
            "chat_id": chat_id,
            "user_id": user_id
        })).await {
            Ok(r) => r,
            Err(e) => return e,
        };

        let status = result.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
        let user = result.get("user");
        let first_name = user.and_then(|u| u.get("first_name")).and_then(|v| v.as_str()).unwrap_or("Unknown");
        let last_name = user.and_then(|u| u.get("last_name")).and_then(|v| v.as_str());
        let username = user.and_then(|u| u.get("username")).and_then(|v| v.as_str());
        let is_bot = user.and_then(|u| u.get("is_bot")).and_then(|v| v.as_bool()).unwrap_or(false);
        let custom_title = result.get("custom_title").and_then(|v| v.as_str());

        let display_name = match last_name {
            Some(ln) => format!("{} {}", first_name, ln),
            None => first_name.to_string(),
        };

        let message = format!(
            "Member info for user {} in chat {}:\n\
            - Name: {}{}\n\
            - Username: {}\n\
            - Status: {}\n\
            - Custom title: {}\n\
            - Is bot: {}",
            user_id, chat_id,
            display_name,
            if is_bot { " [BOT]" } else { "" },
            username.unwrap_or("N/A"),
            status,
            custom_title.unwrap_or("None"),
            is_bot,
        );

        ToolResult::success(message).with_metadata(json!({
            "chat_id": chat_id,
            "user_id": user_id,
            "status": status,
            "first_name": first_name,
            "last_name": last_name,
            "username": username,
            "is_bot": is_bot,
            "custom_title": custom_title,
            "raw": result
        }))
    }

    async fn get_chat_administrators(&self, params: &TelegramReadParams, context: &ToolContext) -> ToolResult {
        let chat_id = match &params.chat_id {
            Some(id) => id.clone(),
            None => return ToolResult::error("'chatId' is required for getChatAdministrators"),
        };

        let token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };

        let result = match Self::telegram_api_call(&token, "getChatAdministrators", &json!({"chat_id": chat_id})).await {
            Ok(r) => r,
            Err(e) => return e,
        };

        let admins = result.as_array();

        let summary = if let Some(admins) = admins {
            admins.iter().map(|admin| {
                let status = admin.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
                let user = admin.get("user");
                let first_name = user.and_then(|u| u.get("first_name")).and_then(|v| v.as_str()).unwrap_or("Unknown");
                let username = user.and_then(|u| u.get("username")).and_then(|v| v.as_str());
                let user_id = user.and_then(|u| u.get("id")).and_then(|v| v.as_i64()).unwrap_or(0);
                let is_bot = user.and_then(|u| u.get("is_bot")).and_then(|v| v.as_bool()).unwrap_or(false);
                let custom_title = admin.get("custom_title").and_then(|v| v.as_str());

                let bot_tag = if is_bot { " [BOT]" } else { "" };
                let title_tag = custom_title.map(|t| format!(" ({})", t)).unwrap_or_default();
                let uname = username.map(|u| format!(" @{}", u)).unwrap_or_default();

                format!("* {}{}{} - {} [ID: {}]{}", first_name, uname, bot_tag, status, user_id, title_tag)
            }).collect::<Vec<_>>().join("\n")
        } else {
            "No administrators found".to_string()
        };

        let count = admins.map(|a| a.len()).unwrap_or(0);

        let message = format!(
            "Found {} administrator(s) in chat {}:\n\n{}",
            count, chat_id, summary
        );

        context.set_register("telegram_chat_id", json!(chat_id), "telegram_read");

        ToolResult::success(message).with_metadata(json!({
            "chat_id": chat_id,
            "count": count,
            "administrators": result
        }))
    }

    async fn get_chat_member_count(&self, params: &TelegramReadParams, context: &ToolContext) -> ToolResult {
        let chat_id = match &params.chat_id {
            Some(id) => id.clone(),
            None => return ToolResult::error("'chatId' is required for getChatMemberCount"),
        };

        let token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };

        let result = match Self::telegram_api_call(&token, "getChatMemberCount", &json!({"chat_id": chat_id})).await {
            Ok(r) => r,
            Err(e) => return e,
        };

        let count = result.as_i64().unwrap_or(0);

        let message = format!("Chat {} has {} member(s).", chat_id, count);

        context.set_register("telegram_chat_id", json!(chat_id), "telegram_read");

        ToolResult::success(message).with_metadata(json!({
            "chat_id": chat_id,
            "member_count": count
        }))
    }

    async fn read_history(&self, params: &TelegramReadParams, context: &ToolContext) -> ToolResult {
        let db = match &context.database {
            Some(db) => db,
            None => return ToolResult::error("Database not available for reading history"),
        };

        let limit = params.limit.unwrap_or(20).min(100);

        // If a chatId is provided, look up that specific chat's session.
        // Otherwise use the current session_id from context.
        let (session_id, chat_label) = if let Some(chat_id) = &params.chat_id {
            // Build candidate session keys. When called cross-channel (e.g. from Discord or web),
            // context.channel_id is the *calling* channel, not the Telegram channel.
            // We try the current channel first, then fall back to all Telegram channels in the DB.
            let mut candidate_channel_ids: Vec<i64> = Vec::new();

            // If we're already on a Telegram channel, try it first
            if context.channel_type.as_deref() == Some("telegram") {
                if let Some(cid) = context.channel_id {
                    candidate_channel_ids.push(cid);
                }
            }

            // Fall back: collect all Telegram channel IDs from the DB
            if let Ok(channels) = db.list_channels() {
                for ch in channels {
                    if ch.channel_type == "telegram" && !candidate_channel_ids.contains(&ch.id) {
                        candidate_channel_ids.push(ch.id);
                    }
                }
            }

            // Try each candidate until we find a matching session
            let mut found_session = None;
            for cid in &candidate_channel_ids {
                let session_key = format!("telegram:{}:{}", cid, chat_id);
                match db.get_chat_session_by_key(&session_key) {
                    Ok(Some(session)) => {
                        found_session = Some(session);
                        break;
                    }
                    Ok(None) => continue,
                    Err(e) => return ToolResult::error(format!("Database error looking up session: {}", e)),
                }
            }

            match found_session {
                Some(session) => (session.id, format!("chat {}", chat_id)),
                None => return ToolResult::error(format!(
                    "No active session found for Telegram chat {}. The bot may not have interacted in that chat yet.",
                    chat_id
                )),
            }
        } else if let Some(sid) = context.session_id {
            (sid, "current chat".to_string())
        } else {
            return ToolResult::error("No chatId provided and no active session. Provide a 'chatId' to read history.");
        };

        let messages = match db.get_recent_session_messages(session_id, limit) {
            Ok(msgs) => msgs,
            Err(e) => return ToolResult::error(format!("Failed to read message history: {}", e)),
        };

        if messages.is_empty() {
            return ToolResult::success(format!("No messages found in {} (session {}).", chat_label, session_id));
        }

        let formatted: Vec<Value> = messages.iter().map(|m| {
            json!({
                "role": m.role.as_str(),
                "content": m.content,
                "user_id": m.user_id,
                "user_name": m.user_name,
                "created_at": m.created_at.to_rfc3339(),
            })
        }).collect();

        let summary: Vec<String> = messages.iter().map(|m| {
            let who = m.user_name.as_deref()
                .unwrap_or(m.user_id.as_deref().unwrap_or(m.role.as_str()));
            let content_preview = if m.content.len() > 120 {
                format!("{}...", &m.content[..120])
            } else {
                m.content.clone()
            };
            format!("[{}] {}: {}", m.role.as_str(), who, content_preview)
        }).collect();

        let message = format!(
            "Read {} messages from {} (session {}):\n\n{}",
            messages.len(),
            chat_label,
            session_id,
            summary.join("\n")
        );

        ToolResult::success(message).with_metadata(json!({
            "messages": formatted,
            "count": messages.len(),
            "session_id": session_id
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition() {
        let tool = TelegramReadTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "telegram_read");
        assert_eq!(def.group, ToolGroup::Messaging);
        assert!(def.input_schema.required.contains(&"action".to_string()));
        assert!(def.input_schema.properties.contains_key("action"));
        assert!(def.input_schema.properties.contains_key("chatId"));
        assert!(def.input_schema.properties.contains_key("userId"));
        assert!(def.input_schema.properties.contains_key("limit"));

        let action_prop = &def.input_schema.properties["action"];
        let actions = action_prop.enum_values.as_ref().unwrap();
        assert!(actions.contains(&"getChatInfo".to_string()));
        assert!(actions.contains(&"getChatMember".to_string()));
        assert!(actions.contains(&"getChatAdministrators".to_string()));
        assert!(actions.contains(&"getChatMemberCount".to_string()));
        assert!(actions.contains(&"readHistory".to_string()));
    }
}
