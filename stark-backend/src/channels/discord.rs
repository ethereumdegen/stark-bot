use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::safe_mode_rate_limiter::SafeModeChannelRateLimiter;
use crate::channels::types::{ChannelType, NormalizedMessage};
use crate::channels::util;
use crate::db::Database;
use crate::discord_hooks;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{Channel, ToolOutputVerbosity};
use serenity::all::{
    Client, Context, EditMessage, EventHandler, GatewayIntents, GetMessages, Message, MessageId,
    Ready, UserId,
};
use std::sync::Arc;
use tokio::sync::oneshot;

/// Format a tool call event for Discord display based on verbosity
fn format_tool_call_for_discord(
    tool_name: &str,
    parameters: &serde_json::Value,
    verbosity: ToolOutputVerbosity,
) -> Option<String> {
    match verbosity {
        ToolOutputVerbosity::None => None,
        ToolOutputVerbosity::Minimal | ToolOutputVerbosity::MinimalThrottled => Some(format!("🔧 **Calling:** `{}`", tool_name)),
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
        ToolOutputVerbosity::Minimal | ToolOutputVerbosity::MinimalThrottled => {
            Some(format!(
                "{} **Result:** `{}` ({} ms)",
                status, tool_name, duration_ms
            ))
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

/// Check if a tool terminates the chat loop and sets the final response.

struct DiscordHandler {
    channel_id: i64,
    dispatcher: Arc<MessageDispatcher>,
    broadcaster: Arc<EventBroadcaster>,
    db: Arc<Database>,
    safe_mode_rate_limiter: SafeModeChannelRateLimiter,
    /// Cached bot user ID, set once from the Ready event to avoid
    /// calling get_current_user() (a Discord API call) on every message.
    bot_user_id: Arc<tokio::sync::OnceCell<UserId>>,
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

        // Ignore system messages (joins, pins, boosts, etc.) - only process regular messages and replies
        if msg.kind != serenity::all::MessageType::Regular
            && msg.kind != serenity::all::MessageType::InlineReply
        {
            log::debug!("Discord: Ignoring non-regular message type {:?} from {}", msg.kind, msg.author.name);
            return;
        }

        let text = msg.content.clone();
        if text.is_empty() {
            return;
        }

        // ===== Discord Hooks Integration =====
        // Get the cached bot user ID (set from Ready event).
        // Fall back to API call only if Ready hasn't fired yet (should be rare).
        let bot_user_id = match self.bot_user_id.get() {
            Some(&id) => id,
            None => {
                log::warn!("Discord: bot_user_id not cached yet (Ready event not received?), falling back to API call");
                match ctx.http.get_current_user().await {
                    Ok(user) => {
                        let id = user.id;
                        let _ = self.bot_user_id.set(id);
                        id
                    }
                    Err(e) => {
                        log::error!("Discord: Failed to get bot user ID: {}", e);
                        return;
                    }
                }
            }
        };

        // Process through discord_hooks module first (config reloaded from DB each time)
        match discord_hooks::process(&msg, &ctx, &self.db, self.channel_id, bot_user_id).await {
            Ok(result) => {
                // If module handled it with a direct response, send it and return
                if let Some(response) = result.response {
                    let chunks = util::split_message(&response, 2000);
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
                        if forward.text.len() > 50 { format!("{}...", forward.text.chars().take(50).collect::<String>()) } else { forward.text.clone() }
                    );

                    // Fetch recent channel context (last 6 messages before this one)
                    let recent_context = {
                        let mut ctx_str = String::new();

                        // If this is a reply, include what it's replying to
                        if let Some(ref replied) = msg.referenced_message {
                            let reply_author = &replied.author.name;
                            let reply_content = if replied.content.len() > 300 {
                                format!("{}...", &replied.content[..300])
                            } else {
                                replied.content.clone()
                            };
                            ctx_str.push_str(&format!(
                                "[REPLYING TO @{}:]\n{}\n\n",
                                reply_author, reply_content
                            ));
                        }

                        // Fetch last 6 messages from channel
                        match msg
                            .channel_id
                            .messages(&ctx.http, GetMessages::new().before(msg.id).limit(6))
                            .await
                        {
                            Ok(messages) if !messages.is_empty() => {
                                ctx_str.push_str("[RECENT CHAT CONTEXT - recent messages in this Discord channel:]\n");
                                // messages come newest-first, reverse for chronological order
                                let mut msgs: Vec<_> = messages.iter().collect();
                                msgs.reverse();
                                for m in msgs {
                                    let who = &m.author.name;
                                    let tag = if m.author.bot { " [you]" } else { "" };
                                    let preview = if m.content.len() > 300 {
                                        format!("{}...", &m.content[..300])
                                    } else {
                                        m.content.clone()
                                    };
                                    ctx_str.push_str(&format!("@{}{}: {}\n", who, tag, preview));
                                }
                                ctx_str.push('\n');
                            }
                            Ok(_) => {} // no messages
                            Err(e) => {
                                log::warn!("Discord: Failed to fetch channel history: {}", e);
                            }
                        }

                        ctx_str
                    };

                    let text_with_hint = if recent_context.is_empty() {
                        format!(
                            "[DISCORD MESSAGE - Use discord_tipping skill for tips.]\n\n{}",
                            forward.text
                        )
                    } else {
                        format!(
                            "[DISCORD MESSAGE - Use discord_tipping skill for tips.]\n\n{}[MESSAGE DIRECTED TO YOU:]\n{}",
                            recent_context, forward.text
                        )
                    };

                    // Get channel name for context
                    let channel_name = msg.channel_id.to_channel(&ctx.http).await.ok().and_then(|ch| {
                        ch.guild().map(|gc| gc.name().to_string())
                    });

                    let normalized = NormalizedMessage {
                        channel_id: self.channel_id,
                        channel_type: ChannelType::Discord.to_string(),
                        chat_id: msg.channel_id.to_string(),
                        chat_name: channel_name,
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

                // Safety: If we get here, result.handled is true but neither response nor forward was set.
                // This should never happen with correct ProcessResult usage, but guard against it.
                log::warn!(
                    "Discord hooks: BUG - handled=true but no response or forward for message from {}. Ignoring to prevent duplicate processing.",
                    msg.author.name
                );
                return;
            }
            Err(e) => {
                log::error!("Discord hooks error: {}", e);
                // Security: Do NOT fall through - this would bypass admin checks
                let _ = msg.channel_id.say(&ctx.http, "Sorry, I encountered an error processing your message.").await;
                return;
            }
        }
        // ===== End Discord Hooks Integration =====
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        log::info!("Discord: Bot connected as {} (id={})", ready.user.name, ready.user.id);
        let _ = self.bot_user_id.set(ready.user.id);
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
        let verbosity = ToolOutputVerbosity::Minimal;

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

            // Send an immediate "thinking" message so users see feedback right away
            match discord_channel_id.say(&http, "💭 **Thinking...**").await {
                Ok(msg) => {
                    status_message_id = Some(msg.id);
                    log::debug!("Discord: Created initial thinking message {}", msg.id);
                }
                Err(e) => {
                    log::error!("Discord: Failed to send thinking message: {}", e);
                }
            }

            while let Some(event) = event_rx.recv().await {
                if !util::event_matches_session(
                    &event.data,
                    channel_id_for_events,
                    &chat_id_for_events,
                ) {
                    continue;
                }

                let message_text = match event.event.as_str() {
                    "agent.tool_call" => {
                        let tool_name = event.data.get("tool_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let params = event.data.get("parameters")
                            .cloned()
                            .unwrap_or(serde_json::json!({}));
                        format_tool_call_for_discord(tool_name, &params, verbosity)
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
                        // Skip say_to_user in event stream — content comes through result.response
                        if tool_name == "say_to_user" {
                            None
                        } else {
                            format_tool_result_for_discord(tool_name, success, duration_ms, content, verbosity)
                        }
                    }
                    "subagent.tool_call" => {
                        let tool_name = event.data.get("tool_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let label = event.data.get("label")
                            .and_then(|v| v.as_str())
                            .unwrap_or("subagent");
                        let params = event.data.get("params_preview")
                            .cloned()
                            .unwrap_or(serde_json::json!({}));
                        format_tool_call_for_discord(tool_name, &params, verbosity)
                            .map(|s| format!("[{}] {}", label, s))
                    }
                    "subagent.tool_result" => {
                        let tool_name = event.data.get("tool_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let label = event.data.get("label")
                            .and_then(|v| v.as_str())
                            .unwrap_or("subagent");
                        let success = event.data.get("success")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let content = event.data.get("content_preview")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        format_tool_result_for_discord(tool_name, success, 0, content, verbosity)
                            .map(|s| format!("[{}] {}", label, s))
                    }
                    "agent.mode_change" => {
                        // Skip mode changes in minimal/none verbosity
                        if matches!(verbosity, ToolOutputVerbosity::Minimal | ToolOutputVerbosity::None) {
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
                        if matches!(verbosity, ToolOutputVerbosity::Minimal | ToolOutputVerbosity::None) {
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
                        if matches!(verbosity, ToolOutputVerbosity::Minimal | ToolOutputVerbosity::None) {
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

        // Wait for the event task to finish processing, then get the status message ID
        let status_message_id = match tokio::time::timeout(
            std::time::Duration::from_millis(2000),
            event_task,
        )
        .await
        {
            Ok(Ok(id)) => id,
            Ok(Err(e)) => {
                log::warn!("Discord: Event task panicked: {}", e);
                None
            }
            Err(_) => {
                log::warn!("Discord: Event task timed out — status message may not be deleted");
                None
            }
        };

        // Delete the status message now that we have the final response
        // This keeps the chat clean - users see only their message and the final answer
        if let Some(msg_id) = status_message_id {
            if let Err(e) = msg.channel_id.delete_message(&ctx.http, msg_id).await {
                log::warn!("Discord: Failed to delete status message: {}", e);
            } else {
                log::info!("Discord: Deleted status message {}", msg_id);
            }
        }

        log::info!("Discord: Unsubscribed from events, client {}", client_id);

        // Send final response
        if result.error.is_none() && !result.response.is_empty() {
            // Discord has a 2000 character limit per message
            let response = &result.response;
            let chunks = util::split_message(response, 2000);

            for chunk in chunks {
                if let Err(e) = msg.channel_id.say(&ctx.http, &chunk).await {
                    log::error!("Failed to send Discord message: {}", e);
                }
            }
        } else if let Some(error) = result.error {
            let error_msg = format!("Sorry, I encountered an error: {}", error);
            let _ = msg.channel_id.say(&ctx.http, &error_msg).await;
        } else if result.response.is_empty() {
            log::debug!("Discord: Empty final response for user {}", user_name);
        }
    }
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
        bot_user_id: Arc::new(tokio::sync::OnceCell::new()),
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
