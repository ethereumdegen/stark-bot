use crate::controllers::api_keys::ApiKeyId;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool for looking up Discord servers and channels
pub struct DiscordLookupTool {
    definition: ToolDefinition,
}

impl DiscordLookupTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The action to perform: 'list_servers' (list all servers the bot is in), 'search_servers' (search servers by name), 'list_channels' (list channels in a server), 'search_channels' (search channels by name)".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "list_servers".to_string(),
                    "search_servers".to_string(),
                    "list_channels".to_string(),
                    "search_channels".to_string(),
                ]),
            },
        );

        properties.insert(
            "server_id".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The Discord server (guild) ID. Required for 'list_channels' and 'search_channels' actions.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "query".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Search query for filtering by name (case-insensitive). Required for 'search_servers' and 'search_channels' actions.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        DiscordLookupTool {
            definition: ToolDefinition {
                name: "discord_lookup".to_string(),
                description: "Look up Discord servers (guilds) and channels. Use this to find server IDs by name, list channels in a server, or search for specific channels.".to_string(),
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

impl Default for DiscordLookupTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct DiscordLookupParams {
    action: String,
    server_id: Option<String>,
    query: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiscordGuild {
    id: String,
    name: String,
    icon: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DiscordChannel {
    id: String,
    name: Option<String>,
    #[serde(rename = "type")]
    channel_type: u8,
}

#[async_trait]
impl Tool for DiscordLookupTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: DiscordLookupParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        log::info!(
            "DiscordLookup: action='{}', server_id={:?}, query={:?}",
            params.action,
            params.server_id,
            params.query
        );

        match params.action.as_str() {
            "list_servers" => self.list_servers(context).await,
            "search_servers" => {
                let query = match &params.query {
                    Some(q) => q,
                    None => return ToolResult::error("'query' parameter is required for 'search_servers' action"),
                };
                self.search_servers(query, context).await
            }
            "list_channels" => {
                let server_id = match &params.server_id {
                    Some(id) => id,
                    None => return ToolResult::error("'server_id' parameter is required for 'list_channels' action"),
                };
                self.list_channels(server_id, context).await
            }
            "search_channels" => {
                let server_id = match &params.server_id {
                    Some(id) => id,
                    None => return ToolResult::error("'server_id' parameter is required for 'search_channels' action"),
                };
                let query = match &params.query {
                    Some(q) => q,
                    None => return ToolResult::error("'query' parameter is required for 'search_channels' action"),
                };
                self.search_channels(server_id, query, context).await
            }
            other => ToolResult::error(format!(
                "Unknown action: '{}'. Valid actions: list_servers, search_servers, list_channels, search_channels",
                other
            )),
        }
    }
}

impl DiscordLookupTool {
    /// Get an API key, trying environment variable first, then context
    fn get_api_key(key_id: ApiKeyId, context: &ToolContext) -> Option<String> {
        // Try environment variable first (preferred)
        if let Some(env_vars) = key_id.env_vars() {
            for env_var in env_vars {
                if let Ok(value) = std::env::var(env_var) {
                    if !value.is_empty() {
                        return Some(value);
                    }
                }
            }
        }
        // Fall back to context
        context.get_api_key_by_id(key_id)
    }

    fn get_bot_token(context: &ToolContext) -> Result<String, ToolResult> {
        Self::get_api_key(ApiKeyId::DiscordBotToken, context).ok_or_else(|| {
            ToolResult::error(
                "Discord bot token not configured. Add DISCORD_BOT_TOKEN in API Keys settings."
            )
        })
    }

    /// Fetch all guilds the bot is in, handling pagination
    async fn fetch_all_guilds(&self, context: &ToolContext) -> Result<Vec<DiscordGuild>, ToolResult> {
        let bot_token = Self::get_bot_token(context)?;
        let client = reqwest::Client::new();
        let mut all_guilds = Vec::new();
        let mut after: Option<String> = None;

        loop {
            let mut url = "https://discord.com/api/v10/users/@me/guilds?limit=200".to_string();
            if let Some(ref after_id) = after {
                url.push_str(&format!("&after={}", after_id));
            }

            let response = client
                .get(&url)
                .header("Authorization", format!("Bot {}", bot_token))
                .send()
                .await
                .map_err(|e| ToolResult::error(format!("Failed to fetch guilds: {}", e)))?;

            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();

            if !status.is_success() {
                return Err(ToolResult::error(format!(
                    "Discord API error ({}): {}",
                    status, body_text
                )));
            }

            let guilds: Vec<DiscordGuild> = serde_json::from_str(&body_text)
                .map_err(|e| ToolResult::error(format!("Failed to parse guilds response: {}", e)))?;

            let count = guilds.len();
            if count == 0 {
                break;
            }

            // Get the last guild ID for pagination
            after = guilds.last().map(|g| g.id.clone());
            all_guilds.extend(guilds);

            // If we got fewer than 200, we've reached the end
            if count < 200 {
                break;
            }
        }

        Ok(all_guilds)
    }

    async fn list_servers(&self, context: &ToolContext) -> ToolResult {
        let guilds = match self.fetch_all_guilds(context).await {
            Ok(g) => g,
            Err(e) => return e,
        };

        let result: Vec<Value> = guilds
            .iter()
            .map(|g| {
                json!({
                    "id": g.id,
                    "name": g.name,
                    "icon": g.icon
                })
            })
            .collect();

        let message = if result.is_empty() {
            "Bot is not in any Discord servers. Invite the bot using: \
            https://discord.com/oauth2/authorize?client_id=BOT_CLIENT_ID&scope=bot&permissions=3072 \
            (replace BOT_CLIENT_ID with your bot's client ID from Discord Developer Portal)".to_string()
        } else {
            let server_list: Vec<String> = guilds
                .iter()
                .map(|g| format!("• {} (ID: {})", g.name, g.id))
                .collect();
            format!(
                "Found {} server(s) the bot has access to:\n{}\n\nIf your server is not listed, the bot needs to be invited to it.",
                result.len(),
                server_list.join("\n")
            )
        };

        ToolResult::success(message).with_metadata(json!({
            "servers": result,
            "count": result.len(),
            "hint": "If the server you want is not listed, invite the bot to that server"
        }))
    }

    async fn search_servers(&self, query: &str, context: &ToolContext) -> ToolResult {
        let guilds = match self.fetch_all_guilds(context).await {
            Ok(g) => g,
            Err(e) => return e,
        };

        let query_lower = query.to_lowercase();
        let matching_guilds: Vec<&DiscordGuild> = guilds
            .iter()
            .filter(|g| g.name.to_lowercase().contains(&query_lower))
            .collect();

        let matching: Vec<Value> = matching_guilds
            .iter()
            .map(|g| {
                json!({
                    "id": g.id,
                    "name": g.name,
                    "icon": g.icon
                })
            })
            .collect();

        if matching.is_empty() {
            ToolResult::success(format!(
                "No servers found matching '{}'. If your server is not found, the bot needs to be invited to it.",
                query
            )).with_metadata(json!({
                "servers": [],
                "count": 0,
                "query": query
            }))
        } else {
            let server_list: Vec<String> = matching_guilds
                .iter()
                .map(|g| format!("• {} (ID: {})", g.name, g.id))
                .collect();

            let message = format!(
                "Found {} servers matching '{}':\n{}",
                matching.len(),
                query,
                server_list.join("\n")
            );

            ToolResult::success(message).with_metadata(json!({
                "servers": matching,
                "count": matching.len(),
                "query": query
            }))
        }
    }

    async fn fetch_channels(&self, server_id: &str, context: &ToolContext) -> Result<Vec<DiscordChannel>, ToolResult> {
        let bot_token = Self::get_bot_token(context)?;
        let client = reqwest::Client::new();

        let url = format!("https://discord.com/api/v10/guilds/{}/channels", server_id);

        let response = client
            .get(&url)
            .header("Authorization", format!("Bot {}", bot_token))
            .send()
            .await
            .map_err(|e| ToolResult::error(format!("Failed to fetch channels: {}", e)))?;

        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            // Parse Discord error for better messaging
            if let Ok(error_json) = serde_json::from_str::<Value>(&body_text) {
                let code = error_json.get("code").and_then(|c| c.as_u64()).unwrap_or(0);
                let message = error_json.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");

                match code {
                    10004 => {
                        // Unknown Guild
                        return Err(ToolResult::error(format!(
                            "Bot does not have access to server '{}'. The bot may not be invited to this server, or was kicked. \
                            Please invite the bot using: https://discord.com/oauth2/authorize?client_id=BOT_CLIENT_ID&scope=bot&permissions=3072 \
                            (replace BOT_CLIENT_ID with your bot's client ID from Discord Developer Portal)",
                            server_id
                        )));
                    }
                    50001 => {
                        // Missing Access
                        return Err(ToolResult::error(format!(
                            "Bot lacks permissions to view channels in server '{}'. \
                            Ensure the bot has 'View Channels' permission in the server settings.",
                            server_id
                        )));
                    }
                    50013 => {
                        // Missing Permissions
                        return Err(ToolResult::error(format!(
                            "Bot lacks required permissions in server '{}'. \
                            Check the bot's role permissions in Discord server settings.",
                            server_id
                        )));
                    }
                    _ => {
                        return Err(ToolResult::error(format!(
                            "Discord API error: {} (code {})",
                            message, code
                        )));
                    }
                }
            }

            return Err(ToolResult::error(format!(
                "Discord API error ({}): {}",
                status, body_text
            )));
        }

        let channels: Vec<DiscordChannel> = serde_json::from_str(&body_text)
            .map_err(|e| ToolResult::error(format!("Failed to parse channels response: {}", e)))?;

        Ok(channels)
    }

    fn channel_type_name(channel_type: u8) -> &'static str {
        match channel_type {
            0 => "text",
            2 => "voice",
            4 => "category",
            5 => "announcement",
            10 | 11 | 12 => "thread",
            13 => "stage",
            14 => "directory",
            15 => "forum",
            16 => "media",
            _ => "unknown",
        }
    }

    async fn list_channels(&self, server_id: &str, context: &ToolContext) -> ToolResult {
        let channels = match self.fetch_channels(server_id, context).await {
            Ok(c) => c,
            Err(e) => return e,
        };

        let result: Vec<Value> = channels
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "name": c.name,
                    "type": c.channel_type,
                    "type_name": Self::channel_type_name(c.channel_type)
                })
            })
            .collect();

        let channel_list: Vec<String> = channels
            .iter()
            .filter(|c| c.channel_type == 0 || c.channel_type == 5) // text and announcement channels
            .map(|c| format!("• #{} (ID: {}, type: {})",
                c.name.as_deref().unwrap_or("unnamed"),
                c.id,
                Self::channel_type_name(c.channel_type)
            ))
            .collect();

        let message = format!(
            "Found {} channels in server {} (showing text channels):\n{}\n\nUse the channel ID when sending messages with agent_send.",
            channel_list.len(),
            server_id,
            channel_list.join("\n")
        );

        ToolResult::success(message).with_metadata(json!({
            "channels": result,
            "count": result.len(),
            "server_id": server_id
        }))
    }

    async fn search_channels(&self, server_id: &str, query: &str, context: &ToolContext) -> ToolResult {
        let channels = match self.fetch_channels(server_id, context).await {
            Ok(c) => c,
            Err(e) => return e,
        };

        let query_lower = query.to_lowercase();
        let matching_channels: Vec<&DiscordChannel> = channels
            .iter()
            .filter(|c| {
                c.name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(&query_lower))
                    .unwrap_or(false)
            })
            .collect();

        let matching: Vec<Value> = matching_channels
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "name": c.name,
                    "type": c.channel_type,
                    "type_name": Self::channel_type_name(c.channel_type)
                })
            })
            .collect();

        if matching.is_empty() {
            ToolResult::success(format!("No channels found matching '{}' in server {}", query, server_id)).with_metadata(json!({
                "channels": [],
                "count": 0,
                "server_id": server_id,
                "query": query
            }))
        } else {
            let channel_list: Vec<String> = matching_channels
                .iter()
                .map(|c| format!("• #{} (ID: {}, type: {})",
                    c.name.as_deref().unwrap_or("unnamed"),
                    c.id,
                    Self::channel_type_name(c.channel_type)
                ))
                .collect();

            let message = format!(
                "Found {} channels matching '{}' in server {}:\n{}\n\nUse the channel ID when sending messages with agent_send.",
                matching.len(),
                query,
                server_id,
                channel_list.join("\n")
            );

            ToolResult::success(message).with_metadata(json!({
                "channels": matching,
                "count": matching.len(),
                "server_id": server_id,
                "query": query
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition() {
        let tool = DiscordLookupTool::new();
        let def = tool.definition();

        assert_eq!(def.name, "discord_lookup");
        assert_eq!(def.group, ToolGroup::Messaging);
        assert!(def.input_schema.required.contains(&"action".to_string()));
    }

    #[test]
    fn test_channel_type_names() {
        assert_eq!(DiscordLookupTool::channel_type_name(0), "text");
        assert_eq!(DiscordLookupTool::channel_type_name(2), "voice");
        assert_eq!(DiscordLookupTool::channel_type_name(4), "category");
        assert_eq!(DiscordLookupTool::channel_type_name(15), "forum");
    }
}
