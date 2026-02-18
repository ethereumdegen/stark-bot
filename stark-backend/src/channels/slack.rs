use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::safe_mode_rate_limiter::SafeModeChannelRateLimiter;
use crate::channels::types::{ChannelType, NormalizedMessage};
use crate::channels::util;
use crate::db::Database;
use crate::discord_hooks::db as user_db;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::channel_settings::ChannelSettingKey;
use crate::models::{Channel, ToolOutputVerbosity};
use rand::seq::SliceRandom;
use slack_morphism::prelude::*;
use std::sync::Arc;
use tokio::sync::oneshot;

// ---------------------------------------------------------------------------
// Shared state passed through SlackClientEventsUserStateStorage
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct SlackAppState {
    channel_id: i64,
    dispatcher: Arc<MessageDispatcher>,
    broadcaster: Arc<EventBroadcaster>,
    db: Arc<Database>,
    bot_token: SlackApiToken,
    bot_user_id: String,
    admin_user_ids: Option<String>,
    safe_mode_rate_limiter: SafeModeChannelRateLimiter,
}

// ---------------------------------------------------------------------------
// Slack API helpers
// ---------------------------------------------------------------------------

async fn send_slack_message(
    client: &SlackHyperClient,
    token: &SlackApiToken,
    channel: &SlackChannelId,
    text: &str,
    thread_ts: Option<&SlackTs>,
) -> Result<SlackTs, String> {
    let session = client.open_session(token);
    let content = SlackMessageContent::new().with_text(text.to_string());
    let mut req = SlackApiChatPostMessageRequest::new(channel.clone(), content);
    if let Some(ts) = thread_ts {
        req = req.with_thread_ts(ts.clone());
    }
    let resp = session
        .chat_post_message(&req)
        .await
        .map_err(|e| format!("chat.postMessage failed: {}", e))?;
    Ok(resp.ts)
}

async fn update_slack_message(
    client: &SlackHyperClient,
    token: &SlackApiToken,
    channel: &SlackChannelId,
    ts: &SlackTs,
    text: &str,
) -> Result<(), String> {
    let session = client.open_session(token);
    let content = SlackMessageContent::new().with_text(text.to_string());
    let req = SlackApiChatUpdateRequest::new(channel.clone(), content, ts.clone());
    session
        .chat_update(&req)
        .await
        .map_err(|e| format!("chat.update failed: {}", e))?;
    Ok(())
}

async fn delete_slack_message(
    client: &SlackHyperClient,
    token: &SlackApiToken,
    channel: &SlackChannelId,
    ts: &SlackTs,
) -> Result<(), String> {
    let session = client.open_session(token);
    let req = SlackApiChatDeleteRequest::new(channel.clone(), ts.clone());
    session
        .chat_delete(&req)
        .await
        .map_err(|e| format!("chat.delete failed: {}", e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Chat context via conversations.history
// ---------------------------------------------------------------------------

async fn fetch_chat_context(
    client: &SlackHyperClient,
    token: &SlackApiToken,
    channel: &SlackChannelId,
    current_ts: &SlackTs,
    bot_user_id: &str,
) -> Option<String> {
    let session = client.open_session(token);
    let req = SlackApiConversationsHistoryRequest::new()
        .with_channel(channel.clone())
        .with_limit(7);

    let resp = match session.conversations_history(&req).await {
        Ok(r) => r,
        Err(e) => {
            log::warn!("Slack: Failed to fetch chat context: {}", e);
            return None;
        }
    };

    let context_msgs: Vec<_> = resp
        .messages
        .iter()
        .filter(|m| m.origin.ts != *current_ts)
        .take(6)
        .collect();

    if context_msgs.is_empty() {
        return None;
    }

    let mut ctx =
        String::from("[RECENT CHAT CONTEXT - recent messages in this Slack channel:]\n");
    for m in &context_msgs {
        let who = m
            .sender
            .user
            .as_ref()
            .map(|u| u.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let is_bot = m.sender.bot_id.is_some()
            || m.sender.user.as_ref().map(|u| u.to_string()).as_deref() == Some(bot_user_id);
        let tag = if is_bot { " [you]" } else { "" };
        let text = m
            .content
            .text
            .as_deref()
            .unwrap_or("");
        let preview = if text.len() > 300 {
            format!("{}...", &text[..300])
        } else {
            text.to_string()
        };
        ctx.push_str(&format!("@{}{}: {}\n", who, tag, preview));
    }

    Some(ctx)
}

// ---------------------------------------------------------------------------
// User display name resolution via users.info API
// ---------------------------------------------------------------------------

async fn resolve_user_name(
    client: &SlackHyperClient,
    token: &SlackApiToken,
    user_id: &str,
) -> String {
    let session = client.open_session(token);
    let req = SlackApiUsersInfoRequest::new(SlackUserId::new(user_id.to_string()));
    match session.users_info(&req).await {
        Ok(resp) => {
            // Prefer display_name from profile, fall back to real_name, then user.name
            resp.user
                .profile
                .as_ref()
                .and_then(|p| p.display_name.as_deref().filter(|s| !s.is_empty()))
                .or(resp.user.real_name.as_deref())
                .or(resp.user.name.as_deref())
                .unwrap_or(user_id)
                .to_string()
        }
        Err(e) => {
            log::warn!("Slack: Failed to resolve display name for {}: {}", user_id, e);
            user_id.to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Mention stripping
// ---------------------------------------------------------------------------

fn strip_slack_mention(text: &str, bot_user_id: &str) -> String {
    text.replace(&format!("<@{}>", bot_user_id), "")
        .trim()
        .to_string()
}

// ---------------------------------------------------------------------------
// Tool event formatting (same as Telegram)
// ---------------------------------------------------------------------------

fn format_tool_call(
    tool_name: &str,
    _parameters: &serde_json::Value,
    verbosity: ToolOutputVerbosity,
) -> Option<String> {
    match verbosity {
        ToolOutputVerbosity::None => None,
        ToolOutputVerbosity::Minimal | ToolOutputVerbosity::MinimalThrottled => {
            Some(format!("🔧 Calling: {}", tool_name))
        }
        ToolOutputVerbosity::Full => Some(format!("🔧 Calling: {}", tool_name)),
    }
}

fn format_tool_result(
    tool_name: &str,
    success: bool,
    duration_ms: i64,
    _content: &str,
    verbosity: ToolOutputVerbosity,
) -> Option<String> {
    let status = if success { "✅" } else { "❌" };
    match verbosity {
        ToolOutputVerbosity::None => None,
        _ => Some(format!(
            "{} Result: {} ({} ms)",
            status, tool_name, duration_ms
        )),
    }
}

// ---------------------------------------------------------------------------
// Shortcircuit commands (mirrors Telegram)
// ---------------------------------------------------------------------------

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

    let slack_user_id = format!("slack:{}", user_id);

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

            if let Err(e) =
                user_db::get_or_create_profile(db, &slack_user_id, user_name).await
            {
                log::error!(
                    "Slack: Failed to create profile for {}: {}",
                    user_id,
                    e
                );
                return Some("Sorry, failed to create your profile.".to_string());
            }

            if let Ok(Some(existing)) = user_db::get_profile_by_address(db, addr).await {
                if existing.discord_user_id == slack_user_id {
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

            match user_db::register_address(db, &slack_user_id, addr).await {
                Ok(()) => Some(format!(
                    "Successfully registered your address: {}\nYou can now receive tips. 🚀",
                    addr
                )),
                Err(e) => {
                    log::error!("Slack: Failed to register address: {}", e);
                    Some("Sorry, failed to register your address.".to_string())
                }
            }
        }
        "status" | "whoami" | "me" => {
            match user_db::get_profile(db, &slack_user_id).await {
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
                    log::error!("Slack: Failed to get profile: {}", e);
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
            if let Err(e) =
                user_db::get_or_create_profile(db, &slack_user_id, user_name).await
            {
                log::error!(
                    "Slack: Failed to get profile for {}: {}",
                    user_id,
                    e
                );
                return Some("Sorry, failed to process your request.".to_string());
            }

            match user_db::unregister_address(db, &slack_user_id).await {
                Ok(()) => Some("Your address has been unregistered.".to_string()),
                Err(e) => {
                    log::error!("Slack: Failed to unregister: {}", e);
                    Some("Sorry, failed to unregister your address.".to_string())
                }
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Core message processing
// ---------------------------------------------------------------------------

async fn process_slack_message(
    client: Arc<SlackHyperClient>,
    state: SlackAppState,
    slack_channel: SlackChannelId,
    user_id: String,
    raw_text: String,
    message_ts: SlackTs,
    thread_ts: Option<SlackTs>,
) {
    let channel_id = state.channel_id;
    let bot_user_id = state.bot_user_id.clone();

    // Resolve display name (non-blocking, falls back to user_id)
    let user_name = resolve_user_name(&client, &state.bot_token, &user_id).await;

    // Strip bot mention from text
    let clean_text = strip_slack_mention(&raw_text, &bot_user_id);
    let clean_text = if clean_text.is_empty() {
        "hello".to_string()
    } else {
        clean_text
    };

    log::info!(
        "Slack: Message from {} ({}): {}",
        user_name,
        user_id,
        if clean_text.len() > 50 {
            format!("{}...", clean_text.chars().take(50).collect::<String>())
        } else {
            clean_text.clone()
        }
    );

    // Reply thread_ts: if already in a thread, use that; otherwise start a new thread under the user's message
    let reply_thread_ts = thread_ts.unwrap_or_else(|| message_ts.clone());

    // Check shortcircuit commands
    if let Some(response) =
        handle_shortcircuit_command(&clean_text, &user_id, &user_name, &state.db).await
    {
        if let Err(e) = send_slack_message(
            &client,
            &state.bot_token,
            &slack_channel,
            &response,
            Some(&reply_thread_ts),
        )
        .await
        {
            log::error!("Slack: Failed to send shortcircuit response: {}", e);
        }
        return;
    }

    // Determine safe mode from admin setting
    let force_safe_mode = match &state.admin_user_ids {
        Some(admin_ids) => {
            let is_admin = admin_ids
                .split(',')
                .map(|s| s.trim())
                .any(|id| id == user_id);
            if is_admin {
                log::info!("Slack: User {} ({}) is admin — full access", user_name, user_id);
                false
            } else {
                log::info!(
                    "Slack: User {} ({}) is not admin — using safe mode",
                    user_name,
                    user_id
                );
                true
            }
        }
        None => false,
    };

    // Check safe mode rate limit for non-admin queries
    if force_safe_mode {
        if let Err(rate_limit_msg) = state.safe_mode_rate_limiter.check_and_record_query(&user_id, "slack") {
            log::info!("Slack: Rate limiting user {} - {}", user_id, rate_limit_msg);
            let _ = send_slack_message(
                &client,
                &state.bot_token,
                &slack_channel,
                &format!("\u{231b} {}", rate_limit_msg),
                Some(&reply_thread_ts),
            )
            .await;
            return;
        }
    }

    // Fetch chat context via conversations.history
    let message_text = match fetch_chat_context(
        &client,
        &state.bot_token,
        &slack_channel,
        &message_ts,
        &bot_user_id,
    )
    .await
    {
        Some(mut ctx) => {
            ctx.push_str(&format!("\n[MESSAGE DIRECTED TO YOU:]\n{}", clean_text));
            format!("[SLACK MESSAGE]\n\n{}", ctx)
        }
        None => format!("[SLACK MESSAGE]\n\n{}", clean_text),
    };

    let normalized = NormalizedMessage {
        channel_id,
        channel_type: ChannelType::Slack.to_string(),
        chat_id: slack_channel.to_string(),
        chat_name: None,
        user_id: user_id.clone(),
        user_name: user_name.clone(),
        text: message_text,
        message_id: Some(message_ts.to_string()),
        session_mode: None,
        selected_network: None,
        force_safe_mode,
        preferred_subtype: None,
    };

    // Subscribe to events for real-time tool call forwarding
    let (client_id, mut event_rx) = state.broadcaster.subscribe();
    log::info!("Slack: Subscribed to events as client {}", client_id);

    // Spawn task to forward events as status message edits
    let client_for_events = client.clone();
    let token_for_events = state.bot_token.clone();
    let channel_for_events = slack_channel.clone();
    let thread_for_events = reply_thread_ts.clone();
    let channel_id_for_events = channel_id;
    let chat_id_for_events = slack_channel.to_string();

    let event_task = tokio::spawn(async move {
        let mut status_ts: Option<SlackTs> = None;
        let verbosity = ToolOutputVerbosity::MinimalThrottled;
        let mut throttler = util::StatusThrottler::default_for_gateway();

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
                    format_tool_call(tool_name, &params, verbosity.display_verbosity())
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

                    if tool_name == "say_to_user" {
                        None
                    } else {
                        format_tool_result(
                            tool_name,
                            success,
                            duration_ms,
                            content,
                            verbosity.display_verbosity(),
                        )
                    }
                }
                "agent.mode_change"
                | "execution.task_started"
                | "execution.task_completed" => None,
                _ => None,
            };

            if let Some(text) = message_text {
                let is_first = status_ts.is_none();
                if verbosity.is_throttled() && !throttler.should_send(is_first) {
                    continue;
                }

                let display_text = if text.len() > 4000 {
                    format!("{}...", &text[..3997])
                } else {
                    text
                };

                match &status_ts {
                    Some(ts) => {
                        match update_slack_message(
                            &client_for_events,
                            &token_for_events,
                            &channel_for_events,
                            ts,
                            &display_text,
                        )
                        .await
                        {
                            Ok(()) => {
                                throttler.record_success();
                            }
                            Err(e) => {
                                if !throttler.record_error(&e) {
                                    log::warn!(
                                        "Slack: Failed to edit status message, recreating: {}",
                                        e
                                    );
                                    let _ = delete_slack_message(
                                        &client_for_events,
                                        &token_for_events,
                                        &channel_for_events,
                                        ts,
                                    )
                                    .await;
                                    match send_slack_message(
                                        &client_for_events,
                                        &token_for_events,
                                        &channel_for_events,
                                        &display_text,
                                        Some(&thread_for_events),
                                    )
                                    .await
                                    {
                                        Ok(new_ts) => {
                                            status_ts = Some(new_ts);
                                            throttler.record_success();
                                        }
                                        Err(e2) => {
                                            log::error!(
                                                "Slack: Failed to send new status message: {}",
                                                e2
                                            );
                                            throttler.record_error(&e2);
                                            status_ts = None;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        match send_slack_message(
                            &client_for_events,
                            &token_for_events,
                            &channel_for_events,
                            &display_text,
                            Some(&thread_for_events),
                        )
                        .await
                        {
                            Ok(ts) => {
                                status_ts = Some(ts);
                                throttler.record_success();
                                log::debug!("Slack: Created status message");
                            }
                            Err(e) => {
                                if !throttler.record_error(&e) {
                                    log::error!(
                                        "Slack: Failed to send initial status message: {}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        status_ts
    });

    // Dispatch to AI
    log::info!("Slack: Dispatching message to AI for user {}", user_name);
    let result = state.dispatcher.dispatch_safe(normalized).await;
    log::info!("Slack: Dispatch complete, error={:?}", result.error);

    // Unsubscribe from events
    state.broadcaster.unsubscribe(&client_id);

    // Wait for event task to finish, then clean up status message
    let status_ts = match tokio::time::timeout(
        std::time::Duration::from_millis(2000),
        event_task,
    )
    .await
    {
        Ok(Ok(ts)) => ts,
        Ok(Err(e)) => {
            log::warn!("Slack: Event task panicked: {}", e);
            None
        }
        Err(_) => {
            log::warn!("Slack: Event task timed out — status message may not be deleted");
            None
        }
    };

    // Delete status message to keep chat clean
    if let Some(ts) = status_ts {
        if let Err(e) = delete_slack_message(
            &client,
            &state.bot_token,
            &slack_channel,
            &ts,
        )
        .await
        {
            log::warn!("Slack: Failed to delete status message: {}", e);
        } else {
            log::info!("Slack: Deleted status message");
        }
    }

    log::info!("Slack: Unsubscribed from events, client {}", client_id);

    // Send final response in thread
    if result.error.is_none() && !result.response.is_empty() {
        let chunks = util::split_message(&result.response, 4000);
        for chunk in chunks {
            if let Err(e) = send_slack_message(
                &client,
                &state.bot_token,
                &slack_channel,
                &chunk,
                Some(&reply_thread_ts),
            )
            .await
            {
                log::error!("Slack: Failed to send response: {}", e);
            }
        }
    } else if let Some(error) = result.error {
        let error_msg = format!("Sorry, I encountered an error: {}", error);
        let _ = send_slack_message(
            &client,
            &state.bot_token,
            &slack_channel,
            &error_msg,
            Some(&reply_thread_ts),
        )
        .await;
    } else if result.response.is_empty() {
        log::debug!("Slack: Empty final response for user {}", user_name);
    }
}

// ---------------------------------------------------------------------------
// Socket Mode event handler
// ---------------------------------------------------------------------------

fn handle_push_event(
    event: SlackPushEventCallback,
    client: Arc<SlackHyperClient>,
    user_state: SlackClientEventsUserState,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Result<(), Box<dyn std::error::Error + Send + Sync>>,
            > + Send,
    >,
> {
    Box::pin(async move {
        // Retrieve our app state
        let state = {
            let guard = user_state.read().await;
            match guard.get_user_state::<SlackAppState>() {
                Some(s) => s.clone(),
                None => {
                    log::error!("Slack: No SlackAppState in user_state — cannot process event");
                    return Ok(());
                }
            }
        };

        match event.event {
            // App mention: bot was @mentioned in a channel
            SlackEventCallbackBody::AppMention(mention) => {
                let text = mention
                    .content
                    .text
                    .as_deref()
                    .unwrap_or("")
                    .to_string();
                if text.is_empty() {
                    return Ok(());
                }

                let user_id = mention.user.to_string();
                let slack_channel = mention.channel;
                let message_ts = mention.origin.ts;
                let thread_ts = mention.origin.thread_ts;

                log::info!(
                    "Slack: AppMention from {} in {}: {}",
                    user_id,
                    slack_channel,
                    if text.len() > 50 {
                        format!("{}...", &text[..50])
                    } else {
                        text.clone()
                    }
                );

                tokio::spawn(process_slack_message(
                    client,
                    state,
                    slack_channel,
                    user_id,
                    text,
                    message_ts,
                    thread_ts,
                ));
            }

            // DM messages: only process direct messages, not channel messages
            SlackEventCallbackBody::Message(msg_event) => {
                // Skip bot messages
                if msg_event.sender.bot_id.is_some() {
                    return Ok(());
                }
                // Skip message subtypes (edits, deletes, etc.)
                if msg_event.subtype.is_some() {
                    return Ok(());
                }

                // Only process DMs (channel_type == "im")
                let is_dm = msg_event
                    .origin
                    .channel_type
                    .as_ref()
                    .map(|ct| ct.0 == "im")
                    .unwrap_or(false);
                if !is_dm {
                    return Ok(());
                }

                // Skip messages from the bot itself
                let sender_id = msg_event
                    .sender
                    .user
                    .as_ref()
                    .map(|u| u.to_string())
                    .unwrap_or_default();
                if sender_id == state.bot_user_id {
                    return Ok(());
                }

                let text = msg_event
                    .content
                    .as_ref()
                    .and_then(|c| c.text.clone())
                    .unwrap_or_default();
                if text.is_empty() {
                    return Ok(());
                }

                let slack_channel = match msg_event.origin.channel {
                    Some(ch) => ch,
                    None => return Ok(()),
                };
                let message_ts = msg_event.origin.ts;
                let thread_ts = msg_event.origin.thread_ts;

                log::info!(
                    "Slack: DM from {} in {}: {}",
                    sender_id,
                    slack_channel,
                    if text.len() > 50 {
                        format!("{}...", &text[..50])
                    } else {
                        text.clone()
                    }
                );

                tokio::spawn(process_slack_message(
                    client,
                    state,
                    slack_channel,
                    sender_id,
                    text,
                    message_ts,
                    thread_ts,
                ));
            }

            _ => {
                // Ignore other event types
            }
        }

        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Start a Slack bot listener using Socket Mode with full AI dispatch
pub async fn start_slack_listener(
    channel: Channel,
    dispatcher: Arc<MessageDispatcher>,
    broadcaster: Arc<EventBroadcaster>,
    db: Arc<Database>,
    safe_mode_rate_limiter: SafeModeChannelRateLimiter,
    shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let channel_id = channel.id;
    let channel_name = channel.name.clone();
    let bot_token_str = channel.bot_token.clone();
    let app_token_str = channel
        .app_token
        .clone()
        .ok_or_else(|| "Slack channels require an app_token for Socket Mode".to_string())?;

    log::info!("Starting Slack listener for channel: {}", channel_name);

    // Create Slack client
    let client = Arc::new(
        SlackClient::new(SlackClientHyperConnector::new().map_err(|e| e.to_string())?),
    );

    // Create token values
    let bot_token = SlackApiToken::new(bot_token_str.into());
    let socket_token = SlackApiToken::new(app_token_str.into());

    // Validate bot token and get bot user ID via auth.test
    log::info!("Slack: Validating bot token via auth.test...");
    let session = client.open_session(&bot_token);
    let auth_resp = session
        .auth_test()
        .await
        .map_err(|e| format!("Slack auth.test failed — invalid bot token: {}", e))?;
    let bot_user_id = auth_resp.user_id.to_string();
    log::info!(
        "Slack: Bot validated — user_id: {}, team: {}",
        bot_user_id,
        auth_resp.team
    );

    // Load admin user IDs setting
    let admin_user_ids: Option<String> = db
        .get_channel_setting(channel_id, ChannelSettingKey::SlackAdminUserIds.as_ref())
        .ok()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(ref ids) = admin_user_ids {
        log::info!(
            "Slack [{}]: Admin user IDs configured: {} — non-admin users will use safe mode",
            channel_name,
            ids
        );
    } else {
        log::info!(
            "Slack [{}]: No admin user IDs configured — all users get full access",
            channel_name
        );
    }

    // Emit started event
    broadcaster.broadcast(GatewayEvent::channel_started(
        channel_id,
        ChannelType::Slack.as_str(),
        &channel_name,
    ));

    // Build shared app state
    let app_state = SlackAppState {
        channel_id,
        dispatcher,
        broadcaster: broadcaster.clone(),
        db,
        bot_token,
        bot_user_id,
        admin_user_ids,
        safe_mode_rate_limiter,
    };

    // Create listener environment with user state
    let listener_environment = Arc::new(
        SlackClientEventsListenerEnvironment::new(client.clone()).with_user_state(app_state),
    );

    // Create Socket Mode callbacks
    let socket_mode_callbacks =
        SlackSocketModeListenerCallbacks::new().with_push_events(handle_push_event);

    // Create socket mode listener
    let socket_mode_listener = SlackClientSocketModeListener::new(
        &SlackClientSocketModeConfig::new(),
        listener_environment,
        socket_mode_callbacks,
    );

    // Run with shutdown signal
    let channel_name_for_shutdown = channel_name.clone();
    tokio::select! {
        _ = shutdown_rx => {
            log::info!("Slack listener {} received shutdown signal", channel_name_for_shutdown);
        }
        result = socket_mode_listener.listen_for(&socket_token) => {
            if let Err(e) = result {
                log::error!("Slack listener error: {}", e);
                broadcaster.broadcast(GatewayEvent::channel_error(
                    channel_id,
                    &format!("Slack error: {}", e),
                ));
            }
        }
    }

    // Emit stopped event
    broadcaster.broadcast(GatewayEvent::channel_stopped(
        channel_id,
        ChannelType::Slack.as_str(),
        &channel_name,
    ));

    Ok(())
}
