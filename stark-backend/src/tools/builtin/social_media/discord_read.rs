use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Read-only Discord tool for fetching messages, permissions, and info
/// This tool is safe for non-admin users as it cannot modify anything
pub struct DiscordReadTool {
    definition: ToolDefinition,
}

impl DiscordReadTool {
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
                    "readMessages".to_string(),
                    "searchMessages".to_string(),
                    "permissions".to_string(),
                    "memberInfo".to_string(),
                    "roleInfo".to_string(),
                    "channelInfo".to_string(),
                    "channelList".to_string(),
                ]),
            },
        );

        properties.insert(
            "channelId".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Channel ID for readMessages, permissions, channelInfo".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "limit".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Number of messages to fetch for readMessages (default: 50, max: 100)".to_string(),
                default: Some(json!(50)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "before".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Message ID cursor for pagination (get messages before this ID)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "after".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Message ID cursor for pagination (get messages after this ID)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "guildId".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Guild/Server ID for searchMessages, memberInfo, roleInfo, channelList".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "userId".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "User ID for memberInfo".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "content".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Search query for searchMessages".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        DiscordReadTool {
            definition: ToolDefinition {
                name: "discord_read".to_string(),
                description: "Read-only Discord operations: read messages, search, get permissions/member/role/channel info. Safe for all users. For write operations (send, react, edit, delete), use the 'discord' tool instead.".to_string(),
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

impl Default for DiscordReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct DiscordReadParams {
    action: String,
    #[serde(rename = "channelId")]
    channel_id: Option<String>,
    limit: Option<u32>,
    before: Option<String>,
    after: Option<String>,
    #[serde(rename = "guildId")]
    guild_id: Option<String>,
    #[serde(rename = "userId")]
    user_id: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiscordMessage {
    id: String,
    content: String,
    author: DiscordAuthor,
    timestamp: String,
    #[serde(default)]
    attachments: Vec<DiscordAttachment>,
    #[serde(default)]
    embeds: Vec<Value>,
    #[serde(default)]
    reactions: Vec<DiscordReaction>,
    #[serde(default)]
    referenced_message: Option<Box<DiscordMessage>>,
}

#[derive(Debug, Deserialize)]
struct DiscordAuthor {
    id: String,
    username: String,
    #[serde(default)]
    global_name: Option<String>,
    #[serde(default)]
    bot: bool,
}

#[derive(Debug, Deserialize)]
struct DiscordAttachment {
    id: String,
    filename: String,
    url: String,
    size: u64,
    #[serde(default)]
    content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiscordReaction {
    emoji: DiscordEmoji,
    count: u32,
    me: bool,
}

#[derive(Debug, Deserialize)]
struct DiscordEmoji {
    id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiscordChannel {
    id: String,
    name: Option<String>,
    #[serde(rename = "type")]
    channel_type: u8,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    position: Option<i32>,
    #[serde(default)]
    parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiscordMember {
    user: Option<DiscordUser>,
    nick: Option<String>,
    roles: Vec<String>,
    joined_at: Option<String>,
    #[serde(default)]
    premium_since: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiscordUser {
    id: String,
    username: String,
    #[serde(default)]
    global_name: Option<String>,
    #[serde(default)]
    avatar: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct DiscordRole {
    id: String,
    name: String,
    color: u32,
    position: i32,
    permissions: String,
    #[serde(default)]
    mentionable: bool,
    #[serde(default)]
    hoist: bool,
}

#[async_trait]
impl Tool for DiscordReadTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: DiscordReadParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        log::info!("DiscordRead tool: action='{}'", params.action);

        match params.action.as_str() {
            "readMessages" => self.read_messages(&params, context).await,
            "searchMessages" => self.search_messages(&params, context).await,
            "permissions" => self.get_permissions(&params, context).await,
            "memberInfo" => self.get_member_info(&params, context).await,
            "roleInfo" => self.get_role_info(&params, context).await,
            "channelInfo" => self.get_channel_info(&params, context).await,
            "channelList" => self.get_channel_list(&params, context).await,
            other => ToolResult::error(format!(
                "Unknown action: '{}'. Valid read actions: readMessages, searchMessages, permissions, memberInfo, roleInfo, channelInfo, channelList. For write actions (sendMessage, react, editMessage, deleteMessage), use the 'discord' tool.",
                other
            )),
        }
    }
}

impl DiscordReadTool {
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

    async fn read_messages(&self, params: &DiscordReadParams, context: &ToolContext) -> ToolResult {
        let channel_id = match &params.channel_id {
            Some(id) => id,
            None => return ToolResult::error("'channelId' is required for readMessages"),
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        let limit = params.limit.unwrap_or(50).min(100);
        let mut url = format!(
            "https://discord.com/api/v10/channels/{}/messages?limit={}",
            channel_id, limit
        );

        if let Some(before) = &params.before {
            url.push_str(&format!("&before={}", before));
        }
        if let Some(after) = &params.after {
            url.push_str(&format!("&after={}", after));
        }

        let response = match client
            .get(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to fetch messages: {}", e)),
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return ToolResult::error(Self::parse_discord_error(status, &body));
        }

        let messages: Vec<DiscordMessage> = match serde_json::from_str(&body) {
            Ok(m) => m,
            Err(e) => return ToolResult::error(format!("Failed to parse messages: {}", e)),
        };

        let formatted: Vec<Value> = messages.iter().map(|m| {
            let author_name = m.author.global_name.as_ref().unwrap_or(&m.author.username);
            json!({
                "id": m.id,
                "content": m.content,
                "author": {
                    "id": m.author.id,
                    "username": m.author.username,
                    "display_name": author_name,
                    "bot": m.author.bot
                },
                "timestamp": m.timestamp,
                "attachments": m.attachments.iter().map(|a| json!({
                    "filename": a.filename,
                    "url": a.url,
                    "content_type": a.content_type
                })).collect::<Vec<_>>(),
                "reactions": m.reactions.iter().map(|r| json!({
                    "emoji": r.emoji.name,
                    "count": r.count
                })).collect::<Vec<_>>(),
                "reply_to": m.referenced_message.as_ref().map(|rm| rm.id.clone())
            })
        }).collect();

        let summary: Vec<String> = messages.iter().map(|m| {
            let author_name = m.author.global_name.as_ref().unwrap_or(&m.author.username);
            let bot_tag = if m.author.bot { " [BOT]" } else { "" };
            let content_preview = if m.content.len() > 100 {
                format!("{}...", &m.content[..100])
            } else {
                m.content.clone()
            };
            let attachments = if !m.attachments.is_empty() {
                format!(" [+{} attachment(s)]", m.attachments.len())
            } else {
                String::new()
            };
            format!("[{}] {}{}: {}{}", m.id, author_name, bot_tag, content_preview, attachments)
        }).collect();

        let message = format!(
            "Read {} messages from channel {}:\n\n{}",
            messages.len(),
            channel_id,
            summary.join("\n")
        );

        context.set_register("discord_channel_id", json!(channel_id), "discord_read");

        ToolResult::success(message).with_metadata(json!({
            "messages": formatted,
            "count": messages.len(),
            "channel_id": channel_id
        }))
    }

    async fn search_messages(&self, params: &DiscordReadParams, context: &ToolContext) -> ToolResult {
        let guild_id = match &params.guild_id {
            Some(id) => id,
            None => return ToolResult::error("'guildId' is required for searchMessages"),
        };

        let content = match &params.content {
            Some(c) => c,
            None => return ToolResult::error("'content' search query is required for searchMessages"),
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        let limit = params.limit.unwrap_or(25).min(25);

        let url = format!(
            "https://discord.com/api/v10/guilds/{}/messages/search?content={}&limit={}",
            guild_id,
            urlencoding::encode(content),
            limit
        );

        let response = match client
            .get(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to search messages: {}", e)),
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return ToolResult::error(Self::parse_discord_error(status, &body));
        }

        let search_result: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(format!("Failed to parse search results: {}", e)),
        };

        let total = search_result.get("total_results").and_then(|v| v.as_u64()).unwrap_or(0);
        let messages = search_result.get("messages").and_then(|v| v.as_array());

        let summary = if let Some(msgs) = messages {
            msgs.iter()
                .filter_map(|m| m.get(0))
                .map(|m| {
                    let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    let msg_content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    let author = m.get("author")
                        .and_then(|a| a.get("username"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let ch_id = m.get("channel_id").and_then(|v| v.as_str()).unwrap_or("?");
                    format!("[{}] #{} - {}: {}", id, ch_id, author,
                        if msg_content.len() > 80 { format!("{}...", &msg_content[..80]) } else { msg_content.to_string() })
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            "No messages found".to_string()
        };

        ToolResult::success(format!(
            "Found {} results for '{}' in guild {}:\n\n{}",
            total, content, guild_id, summary
        )).with_metadata(json!({
            "total_results": total,
            "guild_id": guild_id,
            "query": content,
            "results": search_result
        }))
    }

    async fn get_permissions(&self, params: &DiscordReadParams, context: &ToolContext) -> ToolResult {
        let channel_id = match &params.channel_id {
            Some(id) => id,
            None => return ToolResult::error("'channelId' is required for permissions"),
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        let url = format!("https://discord.com/api/v10/channels/{}", channel_id);

        let response = match client
            .get(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to get channel: {}", e)),
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return ToolResult::error(Self::parse_discord_error(status, &body));
        }

        let channel: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(format!("Failed to parse channel: {}", e)),
        };

        let channel_name = channel.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown");
        let guild_id = channel.get("guild_id").and_then(|v| v.as_str());

        let message = format!(
            "Channel info for #{} ({}):\n\
            - Guild ID: {}\n\
            - Channel type: {}\n\n\
            Note: Full permission calculation requires checking role permissions.",
            channel_name,
            channel_id,
            guild_id.unwrap_or("N/A"),
            channel.get("type").and_then(|v| v.as_u64()).unwrap_or(0)
        );

        ToolResult::success(message).with_metadata(json!({
            "channel": channel
        }))
    }

    async fn get_member_info(&self, params: &DiscordReadParams, context: &ToolContext) -> ToolResult {
        let guild_id = match &params.guild_id {
            Some(id) => id,
            None => return ToolResult::error("'guildId' is required for memberInfo"),
        };

        let user_id = match &params.user_id {
            Some(id) => id,
            None => return ToolResult::error("'userId' is required for memberInfo"),
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        let url = format!(
            "https://discord.com/api/v10/guilds/{}/members/{}",
            guild_id, user_id
        );

        let response = match client
            .get(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to get member: {}", e)),
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return ToolResult::error(Self::parse_discord_error(status, &body));
        }

        let member: DiscordMember = match serde_json::from_str(&body) {
            Ok(m) => m,
            Err(e) => return ToolResult::error(format!("Failed to parse member: {}", e)),
        };

        let user = member.user.as_ref();
        let username = user.map(|u| u.username.as_str()).unwrap_or("Unknown");
        let display_name = member.nick.as_ref()
            .or_else(|| user.and_then(|u| u.global_name.as_ref()))
            .map(|s| s.as_str())
            .unwrap_or(username);

        let message = format!(
            "Member info for {} in guild {}:\n\
            - Username: {}\n\
            - Display name: {}\n\
            - Nickname: {}\n\
            - Roles: {} role(s)\n\
            - Joined: {}\n\
            - Boosting since: {}",
            user_id, guild_id,
            username,
            display_name,
            member.nick.as_deref().unwrap_or("None"),
            member.roles.len(),
            member.joined_at.as_deref().unwrap_or("Unknown"),
            member.premium_since.as_deref().unwrap_or("Not boosting")
        );

        ToolResult::success(message).with_metadata(json!({
            "user_id": user_id,
            "guild_id": guild_id,
            "username": username,
            "display_name": display_name,
            "nickname": member.nick,
            "roles": member.roles,
            "joined_at": member.joined_at,
            "premium_since": member.premium_since
        }))
    }

    async fn get_role_info(&self, params: &DiscordReadParams, context: &ToolContext) -> ToolResult {
        let guild_id = match &params.guild_id {
            Some(id) => id,
            None => return ToolResult::error("'guildId' is required for roleInfo"),
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        let url = format!("https://discord.com/api/v10/guilds/{}/roles", guild_id);

        let response = match client
            .get(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to get roles: {}", e)),
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return ToolResult::error(Self::parse_discord_error(status, &body));
        }

        let roles: Vec<DiscordRole> = match serde_json::from_str(&body) {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to parse roles: {}", e)),
        };

        let mut sorted_roles = roles.clone();
        sorted_roles.sort_by(|a, b| b.position.cmp(&a.position));

        let role_list: Vec<String> = sorted_roles.iter().map(|r| {
            let color_hex = format!("#{:06x}", r.color);
            format!("* {} (ID: {}, color: {}, pos: {})", r.name, r.id, color_hex, r.position)
        }).collect();

        let message = format!(
            "Found {} roles in guild {}:\n\n{}",
            roles.len(),
            guild_id,
            role_list.join("\n")
        );

        ToolResult::success(message).with_metadata(json!({
            "roles": sorted_roles.iter().map(|r| json!({
                "id": r.id,
                "name": r.name,
                "color": r.color,
                "position": r.position,
                "mentionable": r.mentionable,
                "hoist": r.hoist
            })).collect::<Vec<_>>(),
            "count": roles.len(),
            "guild_id": guild_id
        }))
    }

    async fn get_channel_info(&self, params: &DiscordReadParams, context: &ToolContext) -> ToolResult {
        let channel_id = match &params.channel_id {
            Some(id) => id,
            None => return ToolResult::error("'channelId' is required for channelInfo"),
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        let url = format!("https://discord.com/api/v10/channels/{}", channel_id);

        let response = match client
            .get(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to get channel: {}", e)),
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return ToolResult::error(Self::parse_discord_error(status, &body));
        }

        let channel: DiscordChannel = match serde_json::from_str(&body) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to parse channel: {}", e)),
        };

        let channel_type_name = match channel.channel_type {
            0 => "text",
            2 => "voice",
            4 => "category",
            5 => "announcement",
            10 | 11 | 12 => "thread",
            13 => "stage",
            15 => "forum",
            _ => "unknown",
        };

        let message = format!(
            "Channel info for {} ({}):\n\
            - Name: #{}\n\
            - Type: {} ({})\n\
            - Topic: {}\n\
            - Position: {}\n\
            - Parent category: {}",
            channel_id,
            channel.name.as_deref().unwrap_or("Unknown"),
            channel.name.as_deref().unwrap_or("Unknown"),
            channel_type_name,
            channel.channel_type,
            channel.topic.as_deref().unwrap_or("None"),
            channel.position.unwrap_or(0),
            channel.parent_id.as_deref().unwrap_or("None")
        );

        context.set_register("discord_channel_id", json!(channel_id), "discord_read");

        ToolResult::success(message).with_metadata(json!({
            "id": channel.id,
            "name": channel.name,
            "type": channel.channel_type,
            "type_name": channel_type_name,
            "topic": channel.topic,
            "position": channel.position,
            "parent_id": channel.parent_id
        }))
    }

    async fn get_channel_list(&self, params: &DiscordReadParams, context: &ToolContext) -> ToolResult {
        let guild_id = match &params.guild_id {
            Some(id) => id,
            None => return ToolResult::error("'guildId' is required for channelList"),
        };

        let bot_token = match Self::get_bot_token(context) {
            Ok(t) => t,
            Err(e) => return e,
        };
        let client = context.http_client();

        let url = format!("https://discord.com/api/v10/guilds/{}/channels", guild_id);

        let response = match client
            .get(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to get channels: {}", e)),
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return ToolResult::error(Self::parse_discord_error(status, &body));
        }

        let channels: Vec<DiscordChannel> = match serde_json::from_str(&body) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to parse channels: {}", e)),
        };

        let text_channels: Vec<_> = channels.iter().filter(|c| c.channel_type == 0 || c.channel_type == 5).collect();
        let voice_channels: Vec<_> = channels.iter().filter(|c| c.channel_type == 2).collect();
        let categories: Vec<_> = channels.iter().filter(|c| c.channel_type == 4).collect();

        let format_channel = |c: &&DiscordChannel| {
            format!("* #{} (ID: {})", c.name.as_deref().unwrap_or("unnamed"), c.id)
        };

        let mut sections = Vec::new();

        if !text_channels.is_empty() {
            sections.push(format!("**Text Channels ({}):**\n{}",
                text_channels.len(),
                text_channels.iter().map(format_channel).collect::<Vec<_>>().join("\n")
            ));
        }

        if !voice_channels.is_empty() {
            sections.push(format!("**Voice Channels ({}):**\n{}",
                voice_channels.len(),
                voice_channels.iter().map(format_channel).collect::<Vec<_>>().join("\n")
            ));
        }

        if !categories.is_empty() {
            sections.push(format!("**Categories ({}):**\n{}",
                categories.len(),
                categories.iter().map(format_channel).collect::<Vec<_>>().join("\n")
            ));
        }

        let message = format!(
            "Found {} channels in guild {}:\n\n{}",
            channels.len(),
            guild_id,
            sections.join("\n\n")
        );

        context.set_register("discord_server_id", json!(guild_id), "discord_read");

        ToolResult::success(message).with_metadata(json!({
            "channels": channels.iter().map(|c| json!({
                "id": c.id,
                "name": c.name,
                "type": c.channel_type
            })).collect::<Vec<_>>(),
            "count": channels.len(),
            "guild_id": guild_id
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition() {
        let tool = DiscordReadTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "discord_read");
        assert_eq!(def.group, ToolGroup::Messaging);
        assert!(def.input_schema.required.contains(&"action".to_string()));
    }
}
