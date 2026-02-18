use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::types::{ChannelType, NormalizedMessage};
use crate::channels::util;
use crate::db::Database;
use crate::discord_hooks::db as user_db;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::channel_settings::ChannelSettingKey;
use crate::models::{Channel, ToolOutputVerbosity};
use rand::seq::SliceRandom;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::requests::Requester;
use teloxide::types::MessageId;
use tokio::sync::oneshot;

/// Format a tool call event for Telegram display based on verbosity
fn format_tool_call_for_telegram(
    tool_name: &str,
    parameters: &serde_json::Value,
    verbosity: ToolOutputVerbosity,
) -> Option<String> {
    match verbosity {
        ToolOutputVerbosity::None => None,
        ToolOutputVerbosity::Minimal | ToolOutputVerbosity::MinimalThrottled => Some(format!("🔧 Calling: {}", tool_name)),
        ToolOutputVerbosity::Full => {
            let params_str = serde_json::to_string_pretty(parameters)
                .unwrap_or_else(|_| parameters.to_string());
            let params_display = if params_str.len() > 500 {
                format!("{}...", &params_str[..500])
            } else {
                params_str
            };
            Some(format!("🔧 Tool Call: {}\n{}", tool_name, params_display))
        }
    }
}

/// Format a tool result event for Telegram display based on verbosity
fn format_tool_result_for_telegram(
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
                "{} Result: {} ({} ms)",
                status, tool_name, duration_ms
            ))
        }
        ToolOutputVerbosity::Full => {
            let content_display = if content.len() > 1000 {
                format!("{}...", &content[..1000])
            } else {
                content.to_string()
            };
            Some(format!(
                "{} Tool Result: {} ({} ms)\n{}",
                status, tool_name, duration_ms, content_display
            ))
        }
    }
}

/// Check if the bot is @mentioned in the message text (case-insensitive)
fn is_bot_mentioned(text: &str, bot_username: &str) -> bool {
    text.to_lowercase()
        .contains(&format!("@{}", bot_username.to_lowercase()))
}

/// Strip bot @mention from text (case-insensitive)
fn strip_bot_mention(text: &str, bot_username: &str) -> String {
    let mention_lower = format!("@{}", bot_username.to_lowercase());
    let text_lower = text.to_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;
    for (start, _) in text_lower.match_indices(&mention_lower) {
        result.push_str(&text[last_end..start]);
        last_end = start + mention_lower.len();
    }
    result.push_str(&text[last_end..]);
    result.trim().to_string()
}

/// Check for shortcircuit commands (register, status, help, love, unregister)
/// Returns Some(response) if handled, None to continue to AI dispatch
async fn handle_shortcircuit_command(
    text: &str,
    user_id: &str,
    user_name: &str,
    db: &Database,
) -> Option<String> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let first_word = parts.first()?.to_lowercase();
    let text_lower = text.to_lowercase();

    // Love easter egg
    if text_lower == "love"
        || text_lower.starts_with("love ")
        || text_lower.ends_with(" love")
        || text_lower.contains(" love ")
    {
        let responses = [
            "I love you too.",
            "I don't know, let me think about that.",
            "How much do you love me?",
        ];
        return Some(
            responses
                .choose(&mut rand::thread_rng())
                .unwrap_or(&responses[0])
                .to_string(),
        );
    }

    // Use tg: prefix to distinguish Telegram users from Discord users in the same DB
    let tg_user_id = format!("tg:{}", user_id);

    match first_word.as_str() {
        "register" => {
            let addr = match parts.get(1) {
                Some(a) => *a,
                None => {
                    return Some(
                        "Usage: register <address>\nExample: register 0x1234...abcd".to_string(),
                    )
                }
            };

            // Validate address format
            if !addr.starts_with("0x")
                || addr.len() < 42
                || addr.len() > 66
                || !addr[2..].chars().all(|c| c.is_ascii_hexdigit())
            {
                return Some(
                    "Invalid address format. Please provide a valid address starting with 0x."
                        .to_string(),
                );
            }

            // Ensure profile exists
            if let Err(e) = user_db::get_or_create_profile(db, &tg_user_id, user_name).await {
                log::error!("Telegram: Failed to create profile for {}: {}", user_id, e);
                return Some("Sorry, failed to create your profile.".to_string());
            }

            // Check if address already registered
            if let Ok(Some(existing)) = user_db::get_profile_by_address(db, addr).await {
                if existing.discord_user_id == tg_user_id {
                    return Some(format!(
                        "You already have this address registered: {}",
                        addr
                    ));
                } else {
                    return Some(
                        "This address is already registered to another user.".to_string(),
                    );
                }
            }

            // Register
            match user_db::register_address(db, &tg_user_id, addr).await {
                Ok(()) => Some(format!(
                    "Successfully registered your address: {}\nYou can now receive tips. 🚀",
                    addr
                )),
                Err(e) => {
                    log::error!("Telegram: Failed to register address: {}", e);
                    Some("Sorry, failed to register your address.".to_string())
                }
            }
        }
        "status" | "whoami" | "me" => {
            match user_db::get_profile(db, &tg_user_id).await {
                Ok(Some(profile)) => {
                    if let Some(addr) = profile.public_address {
                        let registered_at =
                            profile.registered_at.as_deref().unwrap_or("Unknown");
                        Some(format!(
                            "Your Profile\n\nStatus: Registered\nAddress: {}\nRegistered: {}",
                            addr, registered_at
                        ))
                    } else {
                        Some(
                            "Your Profile\n\nStatus: Not registered\n\nUse \"register <address>\" to register."
                                .to_string(),
                        )
                    }
                }
                Ok(None) => Some(
                    "Your Profile\n\nStatus: Not registered\n\nUse \"register <address>\" to register."
                        .to_string(),
                ),
                Err(e) => {
                    log::error!("Telegram: Failed to get profile: {}", e);
                    Some("Sorry, failed to retrieve your profile.".to_string())
                }
            }
        }
        "help" | "?" => Some(
            "StarkBot Commands\n\n\
            register <address> - Register your public address for tipping\n\
            status - Check your registration status\n\
            unregister - Remove your registered address\n\
            help - Show this message\n\n\
            For anything else, just @ me with your question!"
                .to_string(),
        ),
        "unregister" | "deregister" | "remove" => {
            if let Err(e) = user_db::get_or_create_profile(db, &tg_user_id, user_name).await {
                log::error!("Telegram: Failed to get profile for {}: {}", user_id, e);
                return Some("Sorry, failed to process your request.".to_string());
            }

            match user_db::unregister_address(db, &tg_user_id).await {
                Ok(()) => Some("Your address has been unregistered.".to_string()),
                Err(e) => {
                    log::error!("Telegram: Failed to unregister: {}", e);
                    Some("Sorry, failed to unregister your address.".to_string())
                }
            }
        }
        _ => None,
    }
}

/// Start a Telegram bot listener
pub async fn start_telegram_listener(
    channel: Channel,
    dispatcher: Arc<MessageDispatcher>,
    broadcaster: Arc<EventBroadcaster>,
    db: Arc<Database>,
    shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let channel_id = channel.id;
    let channel_name = channel.name.clone();
    let bot_token = channel.bot_token.clone();

    log::info!("Starting Telegram listener for channel: {}", channel_name);
    log::info!("Telegram: Token length = {}", bot_token.len());

    // Create the bot
    let bot = Bot::new(&bot_token);

    // Validate token and get bot info for mention detection
    log::info!("Telegram: Validating bot token...");
    let me = match bot.get_me().await {
        Ok(me) => {
            log::info!(
                "Telegram: Bot validated - username: @{}, id: {}",
                me.username(),
                me.id
            );
            me
        }
        Err(e) => {
            let error = format!("Invalid Telegram bot token: {}", e);
            log::error!("Telegram: {}", error);
            return Err(error);
        }
    };

    let bot_username = me.username().to_string();
    let bot_user_id = me.id;

    // Load admin user ID setting
    let admin_user_id: Option<String> = db
        .get_channel_setting(channel_id, ChannelSettingKey::TelegramAdminUserId.as_ref())
        .ok()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(ref admin_id) = admin_user_id {
        log::info!(
            "Telegram [{}]: Admin user ID configured: {} — non-admin users will use safe mode",
            channel_name, admin_id
        );
    } else {
        log::info!(
            "Telegram [{}]: No admin user ID configured — all users get full access",
            channel_name
        );
    }

    // Emit started event
    broadcaster.broadcast(GatewayEvent::channel_started(
        channel_id,
        ChannelType::Telegram.as_str(),
        &channel_name,
    ));

    // Clone for handler closure
    let broadcaster_for_handler = broadcaster.clone();
    let bot_username_for_handler = bot_username.clone();
    let db_for_handler = db.clone();

    // Create message handler
    let handler = Update::filter_message().endpoint(
        move |bot: Bot, msg: teloxide::types::Message, dispatcher: Arc<MessageDispatcher>, db: Arc<Database>| {
            let channel_id = channel_id;
            let broadcaster = broadcaster_for_handler.clone();
            let admin_user_id = admin_user_id.clone();
            let bot_username = bot_username_for_handler.clone();
            let bot_user_id = bot_user_id;
            async move {
                log::info!("Telegram: Received update from chat {}", msg.chat.id);

                // Only handle text messages
                if let Some(text) = msg.text() {
                    // Ignore messages from the bot itself to prevent self-triggered loops
                    if msg.from().map(|u| u.id == bot_user_id).unwrap_or(false) {
                        log::debug!("Telegram: Ignoring self-message from bot user {}", bot_user_id);
                        return Ok(());
                    }

                    // Passively log ALL messages for readHistory (before mention check)
                    let passive_user = msg.from();
                    let passive_user_id = passive_user.map(|u| u.id.to_string());
                    let passive_user_name = passive_user.map(|u| {
                        u.username.clone().unwrap_or_else(|| u.first_name.clone())
                    });
                    if let Err(e) = db.store_telegram_chat_message(
                        channel_id,
                        &msg.chat.id.to_string(),
                        passive_user_id.as_deref(),
                        passive_user_name.as_deref(),
                        text,
                        Some(&msg.id.to_string()),
                        false,
                    ) {
                        log::warn!("Telegram: Failed to store passive chat message: {}", e);
                    }

                    // Only respond if bot is @mentioned or replied to (all chat types including DMs)
                    let mentioned = is_bot_mentioned(text, &bot_username);
                    let is_reply_to_bot = msg.reply_to_message()
                        .and_then(|r| r.from())
                        .map(|u| u.id == bot_user_id)
                        .unwrap_or(false);

                    log::info!(
                        "Telegram: Chat {} — mentioned={}, reply_to_bot={}, bot_username=@{}",
                        msg.chat.id, mentioned, is_reply_to_bot, bot_username
                    );

                    if !mentioned && !is_reply_to_bot {
                        log::debug!("Telegram: Ignoring message (bot not @mentioned or replied to)");
                        return Ok(());
                    }

                    // Strip bot @mention from text
                    let clean_text = strip_bot_mention(text, &bot_username);
                    let clean_text = if clean_text.is_empty() {
                        "hello".to_string()
                    } else {
                        clean_text
                    };

                    let user = msg.from();
                    let user_id = user.map(|u| u.id.to_string()).unwrap_or_default();
                    let user_name = user
                        .map(|u| {
                            u.username
                                .clone()
                                .unwrap_or_else(|| u.first_name.clone())
                        })
                        .unwrap_or_else(|| "Unknown".to_string());

                    log::info!(
                        "Telegram: Message from {} ({}): {}",
                        user_name,
                        user_id,
                        if clean_text.len() > 50 {
                            format!("{}...", clean_text.chars().take(50).collect::<String>())
                        } else {
                            clean_text.clone()
                        }
                    );

                    // Check for shortcircuit commands (register, status, help, love)
                    if let Some(response) = handle_shortcircuit_command(&clean_text, &user_id, &user_name, &db).await {
                        if let Err(e) = bot
                            .send_message(msg.chat.id, &response)
                            .reply_to_message_id(msg.id)
                            .await
                        {
                            log::error!("Telegram: Failed to send shortcircuit response: {}", e);
                        }
                        return Ok(());
                    }

                    // Determine safe mode: if admin is configured, only admin gets full access
                    let force_safe_mode = match &admin_user_id {
                        Some(admin_id) => admin_id != &user_id,
                        None => false,
                    };

                    if force_safe_mode {
                        log::info!(
                            "Telegram: User {} ({}) is not admin — using safe mode",
                            user_name, user_id
                        );
                    } else if admin_user_id.is_some() {
                        log::info!(
                            "Telegram: User {} ({}) is admin — full access",
                            user_name, user_id
                        );
                    }

                    // Prepend recent chat context (like Twitter thread context)
                    let message_text = {
                        let chat_id_str = msg.chat.id.to_string();
                        let recent = db.get_recent_telegram_chat_messages(channel_id, &chat_id_str, 6);
                        match recent {
                            Ok(msgs) if !msgs.is_empty() => {
                                // Filter out the current message
                                let context_msgs: Vec<_> = msgs.iter()
                                    .filter(|m| m.platform_message_id.as_deref() != Some(&msg.id.to_string()))
                                    .collect();
                                if !context_msgs.is_empty() {
                                    let mut ctx = String::from("[RECENT CHAT CONTEXT - recent messages in this Telegram group:]\n");
                                    for m in &context_msgs {
                                        let who = m.user_name.as_deref()
                                            .unwrap_or(m.user_id.as_deref().unwrap_or("unknown"));
                                        let tag = if m.is_bot_response { " [you]" } else { "" };
                                        let preview = if m.content.len() > 300 {
                                            format!("{}...", &m.content[..300])
                                        } else {
                                            m.content.clone()
                                        };
                                        ctx.push_str(&format!("@{}{}: {}\n", who, tag, preview));
                                    }
                                    ctx.push_str(&format!("\n[MESSAGE DIRECTED TO YOU:]\n{}", clean_text));
                                    ctx
                                } else {
                                    clean_text.clone()
                                }
                            }
                            _ => clean_text.clone(),
                        }
                    };

                    let normalized = NormalizedMessage {
                        channel_id,
                        channel_type: ChannelType::Telegram.to_string(),
                        chat_id: msg.chat.id.to_string(),
                        chat_name: msg.chat.title().map(|t| t.to_string()),
                        user_id,
                        user_name: user_name.clone(),
                        text: message_text,
                        message_id: Some(msg.id.to_string()),
                        session_mode: None,
                        selected_network: None,
                        force_safe_mode,
                    };

                    // Subscribe to events for real-time tool call forwarding
                    let (client_id, mut event_rx) = broadcaster.subscribe();
                    log::info!("Telegram: Subscribed to events as client {}", client_id);

                    // Clone for event forwarder task
                    let bot_for_events = bot.clone();
                    let telegram_chat_id = msg.chat.id;
                    let channel_id_for_events = channel_id;
                    let chat_id_str_for_events = telegram_chat_id.to_string();

                    // Spawn task to forward events to Telegram in real-time
                    // Uses a single "status message" that gets edited (like Discord minimal mode)
                    let event_task = tokio::spawn(async move {
                        let mut status_message_id: Option<MessageId> = None;
                        let verbosity = ToolOutputVerbosity::MinimalThrottled;
                        let mut throttler = util::StatusThrottler::default_for_gateway();

                        // Send an immediate "thinking" message so users see feedback right away
                        match bot_for_events
                            .send_message(telegram_chat_id, "💭 Thinking...")
                            .await
                        {
                            Ok(sent_msg) => {
                                status_message_id = Some(sent_msg.id);
                                throttler.record_success();
                                log::debug!(
                                    "Telegram: Created initial thinking message {:?}",
                                    sent_msg.id
                                );
                            }
                            Err(e) => {
                                log::error!(
                                    "Telegram: Failed to send thinking message: {}",
                                    e
                                );
                            }
                        }

                        while let Some(event) = event_rx.recv().await {
                            if !util::event_matches_session(
                                &event.data,
                                channel_id_for_events,
                                &chat_id_str_for_events,
                            ) {
                                continue;
                            }

                            let message_text = match event.event.as_str() {
                                "agent.tool_call" => {
                                    let tool_name = event
                                        .data
                                        .get("tool_name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    let params = event
                                        .data
                                        .get("parameters")
                                        .cloned()
                                        .unwrap_or(serde_json::json!({}));
                                    format_tool_call_for_telegram(
                                        tool_name,
                                        &params,
                                        verbosity.display_verbosity(),
                                    )
                                }
                                "tool.result" => {
                                    let tool_name = event
                                        .data
                                        .get("tool_name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    let success = event
                                        .data
                                        .get("success")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                    let duration_ms = event
                                        .data
                                        .get("duration_ms")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0);
                                    let content = event
                                        .data
                                        .get("content")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");

                                    // Skip say_to_user in event stream — content comes through result.response
                                    if tool_name == "say_to_user" {
                                        None
                                    } else {
                                        format_tool_result_for_telegram(
                                            tool_name,
                                            success,
                                            duration_ms,
                                            content,
                                            verbosity.display_verbosity(),
                                        )
                                    }
                                }
                                "subagent.tool_call" => {
                                    let tool_name = event
                                        .data
                                        .get("tool_name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    let label = event
                                        .data
                                        .get("label")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("subagent");
                                    let params = event
                                        .data
                                        .get("params_preview")
                                        .cloned()
                                        .unwrap_or(serde_json::json!({}));
                                    format_tool_call_for_telegram(
                                        tool_name,
                                        &params,
                                        verbosity.display_verbosity(),
                                    )
                                    .map(|s| format!("[{}] {}", label, s))
                                }
                                "subagent.tool_result" => {
                                    let tool_name = event
                                        .data
                                        .get("tool_name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    let label = event
                                        .data
                                        .get("label")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("subagent");
                                    let success = event
                                        .data
                                        .get("success")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                    let content = event
                                        .data
                                        .get("content_preview")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    format_tool_result_for_telegram(
                                        tool_name,
                                        success,
                                        0,
                                        content,
                                        verbosity.display_verbosity(),
                                    )
                                    .map(|s| format!("[{}] {}", label, s))
                                }
                                // Skip mode changes and task events in minimal mode
                                "agent.mode_change"
                                | "execution.task_started"
                                | "execution.task_completed" => None,
                                _ => None,
                            };

                            if let Some(text) = message_text {
                                // Throttle: skip status updates if too frequent or rate-limited
                                let is_first = status_message_id.is_none();
                                if verbosity.is_throttled() && !throttler.should_send(is_first) {
                                    continue;
                                }

                                let display_text = if text.len() > 4096 {
                                    format!("{}...", &text[..4093])
                                } else {
                                    text
                                };

                                match status_message_id {
                                    Some(msg_id) => {
                                        // Edit existing status message
                                        match bot_for_events
                                            .edit_message_text(
                                                telegram_chat_id,
                                                msg_id,
                                                &display_text,
                                            )
                                            .await
                                        {
                                            Ok(_) => {
                                                throttler.record_success();
                                            }
                                            Err(e) => {
                                                if !throttler.record_error(&e.to_string()) {
                                                    // Not a rate limit — try recreating
                                                    log::warn!(
                                                        "Telegram: Failed to edit status message, recreating: {}",
                                                        e
                                                    );
                                                    let _ = bot_for_events
                                                        .delete_message(telegram_chat_id, msg_id)
                                                        .await;
                                                    match bot_for_events
                                                        .send_message(telegram_chat_id, &display_text)
                                                        .await
                                                    {
                                                        Ok(new_msg) => {
                                                            status_message_id = Some(new_msg.id);
                                                            throttler.record_success();
                                                        }
                                                        Err(e2) => {
                                                            log::error!(
                                                                "Telegram: Failed to send new status message: {}",
                                                                e2
                                                            );
                                                            throttler.record_error(&e2.to_string());
                                                            status_message_id = None;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        // First status message — create it
                                        match bot_for_events
                                            .send_message(telegram_chat_id, &display_text)
                                            .await
                                        {
                                            Ok(sent_msg) => {
                                                status_message_id = Some(sent_msg.id);
                                                throttler.record_success();
                                                log::debug!(
                                                    "Telegram: Created status message {:?}",
                                                    sent_msg.id
                                                );
                                            }
                                            Err(e) => {
                                                if !throttler.record_error(&e.to_string()) {
                                                    log::error!(
                                                        "Telegram: Failed to send initial status message: {}",
                                                        e
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Return the status message ID for cleanup
                        status_message_id
                    });

                    // Dispatch to AI
                    log::info!(
                        "Telegram: Dispatching message to AI for user {}",
                        user_name
                    );
                    let result = dispatcher.dispatch(normalized).await;
                    log::info!("Telegram: Dispatch complete, error={:?}", result.error);

                    // Unsubscribe from events
                    broadcaster.unsubscribe(&client_id);

                    // Wait for event task to finish, then get status message ID
                    let status_message_id = match tokio::time::timeout(
                        std::time::Duration::from_millis(2000),
                        event_task,
                    )
                    .await
                    {
                        Ok(Ok(id)) => id,
                        Ok(Err(e)) => {
                            log::warn!("Telegram: Event task panicked: {}", e);
                            None
                        }
                        Err(_) => {
                            log::warn!("Telegram: Event task timed out — status message may not be deleted");
                            None
                        }
                    };

                    // Delete the status message to keep chat clean (minimal mode cleanup)
                    if let Some(msg_id) = status_message_id {
                        if let Err(e) = bot.delete_message(msg.chat.id, msg_id).await {
                            log::warn!("Telegram: Failed to delete status message: {}", e);
                        } else {
                            log::info!("Telegram: Deleted status message {:?}", msg_id);
                        }
                    }

                    log::info!(
                        "Telegram: Unsubscribed from events, client {}",
                        client_id
                    );

                    // Send final response
                    if result.error.is_none() && !result.response.is_empty() {
                        // Log bot response in passive chat log
                        let _ = db.store_telegram_chat_message(
                            channel_id,
                            &msg.chat.id.to_string(),
                            Some(&bot_user_id.to_string()),
                            Some(&bot_username),
                            &result.response,
                            None,
                            true,
                        );

                        let chunks = util::split_message(&result.response, 4096);
                        for chunk in chunks {
                            if let Err(e) = bot
                                .send_message(msg.chat.id, &chunk)
                                .reply_to_message_id(msg.id)
                                .await
                            {
                                log::error!("Failed to send Telegram message: {}", e);
                            }
                        }
                    } else if let Some(error) = result.error {
                        let error_msg =
                            format!("Sorry, I encountered an error: {}", error);
                        let _ = bot
                            .send_message(msg.chat.id, &error_msg)
                            .reply_to_message_id(msg.id)
                            .await;
                    } else if result.response.is_empty() {
                        log::debug!("Telegram: Empty final response for user {}", user_name);
                    }
                }

                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        },
    );

    // Create dispatcher
    let mut tg_dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![dispatcher, db_for_handler])
        .enable_ctrlc_handler()
        .build();

    // Run with shutdown signal
    tokio::select! {
        _ = shutdown_rx => {
            log::info!("Telegram listener {} received shutdown signal", channel_name);
        }
        _ = tg_dispatcher.dispatch() => {
            log::info!("Telegram listener {} stopped", channel_name);
        }
    }

    // Emit stopped event
    broadcaster.broadcast(GatewayEvent::channel_stopped(
        channel_id,
        ChannelType::Telegram.as_str(),
        &channel_name,
    ));

    Ok(())
}
