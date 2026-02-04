use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::safe_mode_rate_limiter::SafeModeChannelRateLimiter;
use crate::channels::types::{ChannelType, NormalizedMessage};
use crate::db::Database;
use crate::discord_hooks;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{Channel, ToolOutputVerbosity};
use serenity::all::{
    Client, Context, EditMessage, EventHandler, GatewayIntents, Message, MessageId, Ready,
};
use std::sync::Arc;
use tokio::sync::oneshot;

/// Discord channel output configuration
/// Verbosity is hard-coded to Minimal for cleaner output
#[derive(Debug, Clone)]
pub struct DiscordOutputConfig {
    pub tool_call_verbosity: ToolOutputVerbosity,
    pub tool_result_verbosity: ToolOutputVerbosity,
}

impl DiscordOutputConfig {
    /// Get the output configuration (hard-coded to minimal)
    pub fn get() -> Self {
        Self::default()
    }
}

impl Default for DiscordOutputConfig {
    fn default() -> Self {
        Self {
            tool_call_verbosity: ToolOutputVerbosity::Minimal,
            tool_result_verbosity: ToolOutputVerbosity::Minimal,
        }
    }
}

/// Format a tool call event for Discord display based on verbosity
fn format_tool_call_for_discord(
    tool_name: &str,
    parameters: &serde_json::Value,
    verbosity: ToolOutputVerbosity,
) -> Option<String> {
    match verbosity {
        ToolOutputVerbosity::None => None,
        ToolOutputVerbosity::Minimal => Some(format!("🔧 **Calling:** `{}`", tool_name)),
        ToolOutputVerbosity::Full => {
            let params_str = serde_json::to_string_pretty(parameters)
                .unwrap_or_else(|_| parameters.to_string());
            // Truncate params if too long for Discord
            let params_display = if params_str.len() > 800 {
                format!("{}...", &params_str[..800])
            } else {
                params_str
            };
            Some(format!("🔧 **Tool Call:** `{}`\n```json\n{}\n```", tool_name, params_display))
        }
    }
}

/// Format a tool result event for Discord display based on verbosity
fn format_tool_result_for_discord(
    tool_name: &str,
    success: bool,
    duration_ms: i64,
    content: &str,
    verbosity: ToolOutputVerbosity,
) -> Option<String> {
    let status = if success { "✅" } else { "❌" };
    match verbosity {
        ToolOutputVerbosity::None => None,
        ToolOutputVerbosity::Minimal => {
            // say_to_user should always show content since that's its whole purpose
            if tool_name == "say_to_user" {
                Some(format!("{} {}", status, content))
            } else {
                Some(format!(
                    "{} **Result:** `{}` ({} ms)",
                    status, tool_name, duration_ms
                ))
            }
        }
        ToolOutputVerbosity::Full => {
            // Truncate content if too long
            let content_display = if content.len() > 1200 {
                format!("{}...", &content[..1200])
            } else {
                content.to_string()
            };
            Some(format!(
                "{} **Tool Result:** `{}` ({} ms)\n```\n{}\n```",
                status, tool_name, duration_ms, content_display
            ))
        }
    }
}

/// Format an agent mode change for Discord display
fn format_mode_change_for_discord(mode: &str, label: &str, reason: Option<&str>) -> String {
    let emoji = match mode {
        "explore" => "🔍",
        "plan" => "📋",
        "perform" => "⚡",
        _ => "🔄",
    };
    match reason {
        Some(r) => format!("{} **Mode:** {} - {}", emoji, label, r),
        None => format!("{} **Mode:** {}", emoji, label),
    }
}

struct DiscordHandler {
    channel_id: i64,
    dispatcher: Arc<MessageDispatcher>,
    broadcaster: Arc<EventBroadcaster>,
    db: Arc<Database>,
    safe_mode_rate_limiter: SafeModeChannelRateLimiter,
}

#[serenity::async_trait]
impl EventHandler for DiscordHandler {
    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore messages from bots (including ourselves)
        if msg.author.bot {
            return;
        }

        // Ignore webhook messages (integrations, other bots disguised as users)
        if msg.webhook_id.is_some() {
            log::debug!("Discord: Ignoring webhook message from {}", msg.author.name);
            return;
        }

        // Ignore system messages (joins, pins, boosts, etc.) - only process regular messages
        if msg.kind != serenity::all::MessageType::Regular {
            log::debug!("Discord: Ignoring non-regular message type {:?} from {}", msg.kind, msg.author.name);
            return;
        }

        let text = msg.content.clone();
        if text.is_empty() {
            return;
        }

        // ===== Discord Hooks Integration =====
        // Process through discord_hooks module first (config reloaded from DB each time)
        match discord_hooks::process(&msg, &ctx, &self.db, self.channel_id).await {
            Ok(result) => {
                // If module handled it with a direct response, send it and return
                if let Some(response) = result.response {
                    let chunks = split_message(&response, 2000);
                    for chunk in chunks {
                        if let Err(e) = msg.channel_id.say(&ctx.http, &chunk).await {
                            log::error!("Discord: Failed to send hooks response: {}", e);
                        }
                    }
                    return;
                }

                // If module says forward to agent, use the forwarded text
                if let Some(forward) = result.forward_to_agent {
                    let user_name = forward.user_name;
                    let user_id = forward.user_id.clone();

                    // Check safe mode rate limit for non-admin queries
                    if forward.force_safe_mode {
                        if let Err(rate_limit_msg) = self.safe_mode_rate_limiter.check_and_record_query(&user_id, "discord") {
                            log::info!("Discord: Rate limiting user {} - {}", user_id, rate_limit_msg);
                            if let Err(e) = msg.channel_id.say(&ctx.http, format!("⏳ {}", rate_limit_msg)).await {
                                log::error!("Discord: Failed to send rate limit message to user {}: {}", user_id, e);
                            }
                            return;
                        }
                    }

                    log::info!(
                        "Discord: {} from {} ({}): {}",
                        if forward.force_safe_mode { "Safe mode query" } else { "Admin command" },
                        user_name,
                        user_id,
                        if forward.text.len() > 50 { format!("{}...", &forward.text[..50]) } else { forward.text.clone() }
                    );

                    let text_with_hint = format!(
                        "[DISCORD MESSAGE - Use discord skill for tipping/messaging. Use discord_resolve_user to resolve @mentions to addresses.]\n\n{}",
                        forward.text
                    );

                    let normalized = NormalizedMessage {
                        channel_id: self.channel_id,
                        channel_type: ChannelType::Discord.to_string(),
                        chat_id: msg.channel_id.to_string(),
                        user_id,
                        user_name: user_name.clone(),
                        text: text_with_hint,
                        message_id: Some(msg.id.to_string()),
                        session_mode: None,
                        selected_network: None,
                        force_safe_mode: forward.force_safe_mode,
                    };

                    self.dispatch_and_respond(&ctx, &msg, normalized, &user_name).await;
                    return;
                }

                // Module didn't handle it (bot not mentioned), ignore the message
                if !result.handled {
                    return;
                }
            }
            Err(e) => {
                log::error!("Discord hooks error: {}", e);
                // Security: Do NOT fall through - this would bypass admin checks
                let _ = msg.channel_id.say(&ctx.http, "Sorry, I encountered an error processing your message.").await;
                return;
            }
        }
        // ===== End Discord Hooks Integration =====

        let user_id = msg.author.id.to_string();
        // Discord moved away from discriminators, so just use the username
        // If discriminator exists and is non-zero, include it for backwards compatibility
        let user_name = match msg.author.discriminator {
            Some(disc) => format!("{}#{}", msg.author.name, disc),
            None => msg.author.name.clone(),
        };

        log::info!(
            "Discord: Message from {} ({}): {}",
            user_name,
            user_id,
            if text.len() > 50 { &text[..50] } else { &text }
        );

        let normalized = NormalizedMessage {
            channel_id: self.channel_id,
            channel_type: ChannelType::Discord.to_string(),
            chat_id: msg.channel_id.to_string(),
            user_id,
            user_name: user_name.clone(),
            text,
            message_id: Some(msg.id.to_string()),
            session_mode: None,
            selected_network: None,
            force_safe_mode: false,
        };

        self.dispatch_and_respond(&ctx, &msg, normalized, &user_name).await;
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        log::info!("Discord: Bot connected as {}", ready.user.name);
    }
}

impl DiscordHandler {
    /// Dispatch a message to the AI and send the response
    async fn dispatch_and_respond(
        &self,
        ctx: &Context,
        msg: &Message,
        normalized: NormalizedMessage,
        user_name: &str,
    ) {
        // Load output configuration from channel settings
        let output_config = DiscordOutputConfig::get();
        log::debug!(
            "Discord: Output config - tool_call={:?}, tool_result={:?}",
            output_config.tool_call_verbosity,
            output_config.tool_result_verbosity
        );

        // Subscribe to events for real-time tool call forwarding
        let (client_id, mut event_rx) = self.broadcaster.subscribe();
        log::info!("Discord: Subscribed to events as client {}", client_id);

        // Clone context and channel info for the event forwarder task
        let http = ctx.http.clone();
        let discord_channel_id = msg.channel_id;
        let channel_id_for_events = self.channel_id;
        // Convert Discord channel ID to string for event filtering
        let chat_id_for_events = discord_channel_id.to_string();

        // Spawn task to forward events to Discord in real-time
        // Uses a single "status message" that gets edited for each update to reduce spam
        let event_task = tokio::spawn(async move {
            // Track the status message ID - we'll edit this instead of sending new messages
            let mut status_message_id: Option<MessageId> = None;

            while let Some(event) = event_rx.recv().await {
                // Only forward events for this specific channel AND chat session
                // This ensures messages go only to the Discord channel that originated the request
                let event_channel_id = event.data.get("channel_id").and_then(|v| v.as_i64());
                let event_chat_id = event.data.get("chat_id").and_then(|v| v.as_str());

                match (event_channel_id, event_chat_id) {
                    (Some(ch_id), Some(chat_id)) => {
                        // Both IDs present - must match both
                        if ch_id != channel_id_for_events || chat_id != chat_id_for_events {
                            continue;
                        }
                    }
                    (Some(ch_id), None) => {
                        // Only channel_id present (legacy event) - check channel only
                        if ch_id != channel_id_for_events {
                            continue;
                        }
                    }
                    _ => {
                        // No channel_id - skip this event
                        continue;
                    }
                }

                let message_text = match event.event.as_str() {
                    "agent.tool_call" => {
                        let tool_name = event.data.get("tool_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let params = event.data.get("parameters")
                            .cloned()
                            .unwrap_or(serde_json::json!({}));
                        format_tool_call_for_discord(tool_name, &params, output_config.tool_call_verbosity)
                    }
                    "tool.result" => {
                        let tool_name = event.data.get("tool_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let success = event.data.get("success")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let duration_ms = event.data.get("duration_ms")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let content = event.data.get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        // say_to_user messages: skip sending via event stream for gateway channels
                        // because the final response already contains this content.
                        // Sending here would cause duplicate messages.
                        if tool_name == "say_to_user" {
                            None // Don't send - final response will handle it
                        } else {
                            format_tool_result_for_discord(tool_name, success, duration_ms, content, output_config.tool_result_verbosity)
                        }
                    }
                    "agent.mode_change" => {
                        // Skip mode changes in minimal/none verbosity
                        if matches!(output_config.tool_call_verbosity, ToolOutputVerbosity::Minimal | ToolOutputVerbosity::None) {
                            None
                        } else {
                            let mode = event.data.get("mode")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let label = event.data.get("label")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown");
                            let reason = event.data.get("reason")
                                .and_then(|v| v.as_str());
                            Some(format_mode_change_for_discord(mode, label, reason))
                        }
                    }
                    "execution.task_started" => {
                        // Skip task started in minimal/none verbosity
                        if matches!(output_config.tool_call_verbosity, ToolOutputVerbosity::Minimal | ToolOutputVerbosity::None) {
                            None
                        } else {
                            let task_type = event.data.get("type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("task");
                            let name = event.data.get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown task");
                            Some(format!("▶️ **{}:** {}", task_type, name))
                        }
                    }
                    "execution.task_completed" => {
                        // Skip task completed in minimal/none verbosity
                        if matches!(output_config.tool_call_verbosity, ToolOutputVerbosity::Minimal | ToolOutputVerbosity::None) {
                            None
                        } else {
                            let status = event.data.get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("completed");
                            let emoji = if status == "completed" { "✅" } else { "❌" };
                            Some(format!("{} Task {}", emoji, status))
                        }
                    }
                    _ => None,
                };

                if let Some(text) = message_text {
                    // Only use the first chunk if message is too long (status updates should be brief)
                    let display_text = if text.len() > 2000 {
                        format!("{}...", &text[..1997])
                    } else {
                        text
                    };

                    match status_message_id {
                        Some(msg_id) => {
                            // Try to edit the existing status message
                            let edit_result = discord_channel_id
                                .edit_message(&http, msg_id, EditMessage::new().content(&display_text))
                                .await;

                            if let Err(e) = edit_result {
                                log::warn!("Discord: Failed to edit status message, will delete and recreate: {}", e);
                                // Try to delete the old message
                                let _ = discord_channel_id.delete_message(&http, msg_id).await;
                                // Send a new message
                                match discord_channel_id.say(&http, &display_text).await {
                                    Ok(new_msg) => {
                                        status_message_id = Some(new_msg.id);
                                    }
                                    Err(e) => {
                                        log::error!("Discord: Failed to send new status message: {}", e);
                                        status_message_id = None;
                                    }
                                }
                            }
                        }
                        None => {
                            // First message - create it and store the ID
                            match discord_channel_id.say(&http, &display_text).await {
                                Ok(msg) => {
                                    status_message_id = Some(msg.id);
                                    log::debug!("Discord: Created status message {}", msg.id);
                                }
                                Err(e) => {
                                    log::error!("Discord: Failed to send initial status message: {}", e);
                                }
                            }
                        }
                    }
                }
            }

            // Return the status message ID so we can clean it up after the response
            status_message_id
        });

        // Dispatch to AI
        log::info!("Discord: Dispatching message to AI for user {}", user_name);
        let result = self.dispatcher.dispatch(normalized).await;
        log::info!("Discord: Dispatch complete, error={:?}", result.error);

        // Unsubscribe from events
        self.broadcaster.unsubscribe(&client_id);

        // Wait briefly for the event task to finish processing, then get the status message ID
        // We give it a short timeout to wrap up any pending edits
        let status_message_id = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            event_task,
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten();

        // Delete the status message now that we have the final response
        // This keeps the chat clean - users see only their message and the final answer
        if let Some(msg_id) = status_message_id {
            if let Err(e) = msg.channel_id.delete_message(&ctx.http, msg_id).await {
                log::warn!("Discord: Failed to delete status message: {}", e);
            } else {
                log::debug!("Discord: Deleted status message {}", msg_id);
            }
        }

        log::info!("Discord: Unsubscribed from events, client {}", client_id);

        // Send final response
        if result.error.is_none() && !result.response.is_empty() {
            // Discord has a 2000 character limit per message
            let response = &result.response;
            let chunks = split_message(response, 2000);

            for chunk in chunks {
                if let Err(e) = msg.channel_id.say(&ctx.http, &chunk).await {
                    log::error!("Failed to send Discord message: {}", e);
                }
            }
        } else if let Some(error) = result.error {
            let error_msg = format!("Sorry, I encountered an error: {}", error);
            let _ = msg.channel_id.say(&ctx.http, &error_msg).await;
        }
    }
}

/// Split a message into chunks respecting Discord's character limit
fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        if current.len() + line.len() + 1 > max_len {
            if !current.is_empty() {
                chunks.push(current);
                current = String::new();
            }
            // If single line is too long, split it
            if line.len() > max_len {
                let mut remaining = line;
                while remaining.len() > max_len {
                    chunks.push(remaining[..max_len].to_string());
                    remaining = &remaining[max_len..];
                }
                if !remaining.is_empty() {
                    current = remaining.to_string();
                }
            } else {
                current = line.to_string();
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

/// Start a Discord bot listener
pub async fn start_discord_listener(
    channel: Channel,
    dispatcher: Arc<MessageDispatcher>,
    broadcaster: Arc<EventBroadcaster>,
    db: Arc<Database>,
    safe_mode_rate_limiter: SafeModeChannelRateLimiter,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let channel_id = channel.id;
    let channel_name = channel.name.clone();
    let bot_token = channel.bot_token.clone();

    log::info!("Starting Discord listener for channel: {}", channel_name);
    log::info!("Discord: Token length = {}", bot_token.len());

    // Set up intents - we need message content to read messages
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let handler = DiscordHandler {
        channel_id,
        dispatcher,
        broadcaster: broadcaster.clone(),
        db,
        safe_mode_rate_limiter,
    };

    // Create client
    let mut client = Client::builder(&bot_token, intents)
        .event_handler(handler)
        .await
        .map_err(|e| format!("Failed to create Discord client: {}", e))?;

    log::info!("Discord: Client created successfully");

    // Emit started event
    broadcaster.broadcast(GatewayEvent::channel_started(
        channel_id,
        ChannelType::Discord.as_str(),
        &channel_name,
    ));

    // Get shard manager for shutdown
    let shard_manager = client.shard_manager.clone();

    // Run with shutdown signal
    tokio::select! {
        _ = &mut shutdown_rx => {
            log::info!("Discord listener {} received shutdown signal", channel_name);
            shard_manager.shutdown_all().await;
        }
        result = client.start() => {
            match result {
                Ok(()) => log::info!("Discord listener {} stopped", channel_name),
                Err(e) => {
                    let error = format!("Discord client error: {}", e);
                    log::error!("{}", error);
                    broadcaster.broadcast(GatewayEvent::channel_stopped(
                        channel_id,
                        ChannelType::Discord.as_str(),
                        &channel_name,
                    ));
                    return Err(error);
                }
            }
        }
    }

    // Emit stopped event
    broadcaster.broadcast(GatewayEvent::channel_stopped(
        channel_id,
        ChannelType::Discord.as_str(),
        &channel_name,
    ));

    Ok(())
}
