use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::types::{ChannelType, NormalizedMessage};
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::Channel;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::requests::Requester;
use tokio::sync::oneshot;

/// Format a tool call event for Telegram display (plain text for reliability)
fn format_tool_call_for_telegram(tool_name: &str, parameters: &serde_json::Value) -> String {
    let params_str = serde_json::to_string_pretty(parameters)
        .unwrap_or_else(|_| parameters.to_string());
    // Truncate params if too long for Telegram
    let params_display = if params_str.len() > 500 {
        format!("{}...", &params_str[..500])
    } else {
        params_str
    };
    format!("🔧 Tool Call: {}\n{}", tool_name, params_display)
}

/// Format a tool result event for Telegram display (plain text for reliability)
fn format_tool_result_for_telegram(tool_name: &str, success: bool, duration_ms: i64, content: &str) -> String {
    let status = if success { "✅" } else { "❌" };
    // Truncate content if too long
    let content_display = if content.len() > 1000 {
        format!("{}...", &content[..1000])
    } else {
        content.to_string()
    };
    format!(
        "{} Tool Result: {} ({} ms)\n{}",
        status, tool_name, duration_ms, content_display
    )
}

/// Format an agent mode change for Telegram display
fn format_mode_change_for_telegram(mode: &str, label: &str, reason: Option<&str>) -> String {
    let emoji = match mode {
        "explore" => "🔍",
        "plan" => "📋",
        "perform" => "⚡",
        _ => "🔄",
    };
    match reason {
        Some(r) => format!("{} Mode: {} - {}", emoji, label, r),
        None => format!("{} Mode: {}", emoji, label),
    }
}

/// Start a Telegram bot listener
pub async fn start_telegram_listener(
    channel: Channel,
    dispatcher: Arc<MessageDispatcher>,
    broadcaster: Arc<EventBroadcaster>,
    shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let channel_id = channel.id;
    let channel_name = channel.name.clone();
    let bot_token = channel.bot_token.clone();

    log::info!("Starting Telegram listener for channel: {}", channel_name);
    log::info!("Telegram: Token length = {}", bot_token.len());

    // Create the bot
    let bot = Bot::new(&bot_token);

    // Validate token by calling getMe
    log::info!("Telegram: Validating bot token...");
    match bot.get_me().await {
        Ok(me) => {
            log::info!(
                "Telegram: Bot validated - username: @{}, id: {}",
                me.username(),
                me.id
            );
        }
        Err(e) => {
            let error = format!("Invalid Telegram bot token: {}", e);
            log::error!("Telegram: {}", error);
            return Err(error);
        }
    }

    // Emit started event
    broadcaster.broadcast(GatewayEvent::channel_started(
        channel_id,
        ChannelType::Telegram.as_str(),
        &channel_name,
    ));

    // Clone broadcaster for use in handler
    let broadcaster_for_handler = broadcaster.clone();

    // Create message handler
    let handler = Update::filter_message().endpoint(
        move |bot: Bot, msg: teloxide::types::Message, dispatcher: Arc<MessageDispatcher>| {
            let channel_id = channel_id;
            let broadcaster = broadcaster_for_handler.clone();
            async move {
                log::info!("Telegram: Received update from chat {}", msg.chat.id);

                // Only handle text messages
                if let Some(text) = msg.text() {
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
                        if text.len() > 50 { &text[..50] } else { text }
                    );

                    let normalized = NormalizedMessage {
                        channel_id,
                        channel_type: ChannelType::Telegram.to_string(),
                        chat_id: msg.chat.id.to_string(),
                        user_id,
                        user_name: user_name.clone(),
                        text: text.to_string(),
                        message_id: Some(msg.id.to_string()),
                        session_mode: None,
                        selected_network: None,
                        force_safe_mode: false,
                    };

                    // Subscribe to events for real-time tool call forwarding
                    let (client_id, mut event_rx) = broadcaster.subscribe();
                    log::info!("Telegram: Subscribed to events as client {}", client_id);

                    // Clone bot and chat_id for the event forwarder task
                    let bot_for_events = bot.clone();
                    let telegram_chat_id = msg.chat.id;
                    let channel_id_for_events = channel_id;
                    // Convert Telegram chat ID to string for event filtering
                    let chat_id_str_for_events = telegram_chat_id.to_string();

                    // Spawn task to forward events to Telegram in real-time
                    let event_task = tokio::spawn(async move {
                        while let Some(event) = event_rx.recv().await {
                            // Only forward events for this specific channel AND chat session
                            let event_channel_id = event.data.get("channel_id").and_then(|v| v.as_i64());
                            let event_chat_id = event.data.get("chat_id").and_then(|v| v.as_str());

                            match (event_channel_id, event_chat_id) {
                                (Some(ch_id), Some(chat_id)) => {
                                    // Both IDs present - must match both
                                    if ch_id != channel_id_for_events || chat_id != chat_id_str_for_events {
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
                                    Some(format_tool_call_for_telegram(tool_name, &params))
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

                                    // say_to_user messages are displayed directly without tool result formatting
                                    if tool_name == "say_to_user" && success && !content.is_empty() {
                                        // Truncate if too long for Telegram (4096 char limit)
                                        let display_content = if content.len() > 4000 {
                                            format!("{}...", &content[..4000])
                                        } else {
                                            content.to_string()
                                        };
                                        Some(display_content)
                                    } else {
                                        Some(format_tool_result_for_telegram(tool_name, success, duration_ms, content))
                                    }
                                }
                                "agent.mode_change" => {
                                    let mode = event.data.get("mode")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    let label = event.data.get("label")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("Unknown");
                                    let reason = event.data.get("reason")
                                        .and_then(|v| v.as_str());
                                    Some(format_mode_change_for_telegram(mode, label, reason))
                                }
                                "execution.task_started" => {
                                    let task_type = event.data.get("type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("task");
                                    let name = event.data.get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("Unknown task");
                                    Some(format!("▶️ *{}:* {}", task_type, name))
                                }
                                "execution.task_completed" => {
                                    let status = event.data.get("status")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("completed");
                                    let emoji = if status == "completed" { "✅" } else { "❌" };
                                    Some(format!("{} Task {}", emoji, status))
                                }
                                _ => None,
                            };

                            if let Some(text) = message_text {
                                // Send as plain text for maximum reliability
                                if let Err(e) = bot_for_events
                                    .send_message(telegram_chat_id, &text)
                                    .await
                                {
                                    log::warn!("Telegram: Failed to send event message: {}", e);
                                }
                            }
                        }
                    });

                    // Dispatch to AI
                    log::info!("Telegram: Dispatching message to AI for user {}", user_name);
                    let result = dispatcher.dispatch(normalized).await;
                    log::info!("Telegram: Dispatch complete, error={:?}", result.error);

                    // Unsubscribe and stop event forwarding
                    broadcaster.unsubscribe(&client_id);
                    event_task.abort();
                    log::info!("Telegram: Unsubscribed from events, client {}", client_id);

                    // Send final response
                    if result.error.is_none() && !result.response.is_empty() {
                        if let Err(e) = bot
                            .send_message(msg.chat.id, &result.response)
                            .reply_to_message_id(msg.id)
                            .await
                        {
                            log::error!("Failed to send Telegram message: {}", e);
                        }
                    } else if let Some(error) = result.error {
                        // Send error message
                        let error_msg = format!("Sorry, I encountered an error: {}", error);
                        let _ = bot
                            .send_message(msg.chat.id, &error_msg)
                            .reply_to_message_id(msg.id)
                            .await;
                    }
                }

                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        },
    );

    // Create dispatcher
    let mut tg_dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![dispatcher])
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
