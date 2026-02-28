//! Persona Hooks — event-driven agent triggers.
//!
//! When a matching event occurs (e.g. a Discord message), this module finds
//! all agent subtypes with hooks for that event, renders the prompt template
//! with event variables, and dispatches an isolated session for each.
//!
//! Supported events (matched by filename in hooks/ directory):
//! - `discord_message`     — Discord gateway message
//! - `discord_mention`     — Bot is @mentioned or replied-to in Discord
//! - `discord_member_join` — New member joins a Discord guild
//! - `telegram_message`    — Telegram message
//! - `telegram_mention`    — Bot is @mentioned or replied-to in Telegram
//! - `twitter_mentioned`   — Bot is @mentioned on Twitter
//! - `heartbeat`           — Fired each heartbeat tick for every agent with the hook
//! - (custom)              — Any event name fired via `fire_custom_hooks()` / internal API

use crate::ai::multi_agent::types::{all_subtype_configs_unfiltered, AgentSubtypeConfig, PersonaHook};
use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::types::NormalizedMessage;
use crate::config::runtime_agents_dir;
use crate::gateway::protocol::{EventType, GatewayEvent};
use std::collections::HashMap;
use std::sync::Arc;

/// Timeout for each hook session (seconds).
const HOOK_TIMEOUT_SECS: u64 = 120;

/// Get all (config, hook) pairs matching a given event name.
pub fn get_hooks_for_event(event: &str) -> Vec<(AgentSubtypeConfig, PersonaHook)> {
    let configs = all_subtype_configs_unfiltered();
    let mut results = Vec::new();
    for config in configs {
        if !config.enabled {
            continue;
        }
        for hook in &config.hooks {
            if hook.event == event && !hook.prompt_template.is_empty() {
                results.push((config.clone(), hook.clone()));
            }
        }
    }
    results
}

/// Simple template renderer: replaces `{key}` placeholders with values.
pub fn render_template(template: &str, vars: &HashMap<&str, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("{{{}}}", key), value);
    }
    result
}

// =====================================================
// Shared dispatch helper
// =====================================================

/// Spawn an isolated hook session for an agent.
/// This is the common dispatch pattern shared by all hook event types.
fn spawn_hook_session(
    config: &AgentSubtypeConfig,
    event_name: &str,
    prompt: String,
    chat_id: String,
    message_id_suffix: String,
    safe_mode: bool,
    dispatcher: &Arc<MessageDispatcher>,
) {
    let hook_channel_id = -(950 + hook_channel_offset(&config.key));

    let normalized = NormalizedMessage {
        channel_id: hook_channel_id,
        channel_type: config.key.clone(),
        chat_id,
        chat_name: None,
        user_id: format!("hook-{}", config.key),
        user_name: format!("Hook({})", config.key),
        text: prompt,
        message_id: Some(format!("hook-{}-{}", config.key, message_id_suffix)),
        session_mode: Some("isolated".to_string()),
        selected_network: None,
        force_safe_mode: safe_mode,
        platform_role_ids: vec![],
        chat_context: None,
    };

    log::info!(
        "[PERSONA_HOOK] Firing '{}' hook for {} event",
        config.key, event_name
    );

    // Broadcast hook.fired event
    dispatcher.broadcaster().broadcast(GatewayEvent::new(
        EventType::HookFired,
        serde_json::json!({
            "agent_key": config.key,
            "event": event_name,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    ));

    let dispatcher_clone = Arc::clone(dispatcher);
    let key_clone = config.key.clone();
    let event_clone = event_name.to_string();

    tokio::spawn(async move {
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(HOOK_TIMEOUT_SECS),
            dispatcher_clone.dispatch_safe(normalized),
        )
        .await;

        let (status, error_msg) = match &result {
            Ok(r) if r.error.is_some() => {
                let err = format!("{:?}", r.error);
                log::warn!("[PERSONA_HOOK:{}:{}] Failed: {}", key_clone, event_clone, err);
                ("failed", Some(err))
            }
            Err(_) => {
                log::warn!(
                    "[PERSONA_HOOK:{}:{}] Timed out after {}s",
                    key_clone, event_clone, HOOK_TIMEOUT_SECS
                );
                ("timeout", Some(format!("Timed out after {}s", HOOK_TIMEOUT_SECS)))
            }
            _ => {
                log::info!("[PERSONA_HOOK:{}:{}] Completed successfully", key_clone, event_clone);
                ("success", None)
            }
        };

        // Broadcast hook.completed event
        let mut data = serde_json::json!({
            "agent_key": key_clone,
            "event": event_clone,
            "status": status,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        if let Some(err) = error_msg {
            data["error"] = serde_json::Value::String(err);
        }
        dispatcher_clone.broadcaster().broadcast(GatewayEvent::new(
            EventType::HookCompleted,
            data,
        ));
    });
}

// =====================================================
// discord_message
// =====================================================

/// Template variables for `discord_message`:
/// `{guildId}`, `{channelId}`, `{channelName}`, `{authorId}`, `{authorName}`,
/// `{authorBot}`, `{messageId}`, `{content}`
pub async fn fire_discord_message_hooks(
    msg: &serenity::all::Message,
    dispatcher: &Arc<MessageDispatcher>,
) {
    let hooks = get_hooks_for_event("discord_message");
    if hooks.is_empty() {
        return;
    }

    let guild_id = msg.guild_id.map(|g| g.to_string()).unwrap_or_default();
    let channel_id_str = msg.channel_id.to_string();
    let author_id = msg.author.id.to_string();
    let author_name = msg.author.name.clone();
    let author_bot = msg.author.bot.to_string();
    let message_id = msg.id.to_string();
    let content = msg.content.clone();
    let channel_name = channel_id_str.clone();

    for (config, hook) in hooks {
        let mut vars: HashMap<&str, String> = HashMap::new();
        vars.insert("guildId", guild_id.clone());
        vars.insert("channelId", channel_id_str.clone());
        vars.insert("channelName", channel_name.clone());
        vars.insert("authorId", author_id.clone());
        vars.insert("authorName", author_name.clone());
        vars.insert("authorBot", author_bot.clone());
        vars.insert("messageId", message_id.clone());
        vars.insert("content", content.clone());
        vars.insert("goals", read_agent_goals(&config.key));

        let prompt = render_template(&hook.prompt_template, &vars);
        spawn_hook_session(
            &config,
            "discord_message",
            prompt,
            format!("hook:{}:{}:{}", config.key, channel_id_str, message_id),
            message_id.clone(),
            hook.safe_mode,
            dispatcher,
        );
    }
}

// =====================================================
// discord_mention
// =====================================================

/// Template variables for `discord_mention`:
/// `{guildId}`, `{channelId}`, `{channelName}`, `{authorId}`, `{authorName}`,
/// `{authorBot}`, `{messageId}`, `{content}`
pub async fn fire_discord_mention_hooks(
    msg: &serenity::all::Message,
    dispatcher: &Arc<MessageDispatcher>,
) {
    let hooks = get_hooks_for_event("discord_mention");
    if hooks.is_empty() {
        return;
    }

    let guild_id = msg.guild_id.map(|g| g.to_string()).unwrap_or_default();
    let channel_id_str = msg.channel_id.to_string();
    let author_id = msg.author.id.to_string();
    let author_name = msg.author.name.clone();
    let author_bot = msg.author.bot.to_string();
    let message_id = msg.id.to_string();
    let content = msg.content.clone();
    let channel_name = channel_id_str.clone();

    for (config, hook) in hooks {
        let mut vars: HashMap<&str, String> = HashMap::new();
        vars.insert("guildId", guild_id.clone());
        vars.insert("channelId", channel_id_str.clone());
        vars.insert("channelName", channel_name.clone());
        vars.insert("authorId", author_id.clone());
        vars.insert("authorName", author_name.clone());
        vars.insert("authorBot", author_bot.clone());
        vars.insert("messageId", message_id.clone());
        vars.insert("content", content.clone());
        vars.insert("goals", read_agent_goals(&config.key));

        let prompt = render_template(&hook.prompt_template, &vars);
        spawn_hook_session(
            &config,
            "discord_mention",
            prompt,
            format!("hook:{}:{}:{}", config.key, channel_id_str, message_id),
            message_id.clone(),
            hook.safe_mode,
            dispatcher,
        );
    }
}

// =====================================================
// discord_member_join
// =====================================================

/// Template variables for `discord_member_join`:
/// `{guildId}`, `{guildName}`, `{userId}`, `{userName}`, `{userBot}`,
/// `{joinedAt}`, `{memberCount}`
pub async fn fire_discord_member_join_hooks(
    guild_id: u64,
    guild_name: &str,
    user_id: u64,
    user_name: &str,
    user_bot: bool,
    joined_at: &str,
    member_count: u64,
    dispatcher: &Arc<MessageDispatcher>,
) {
    let hooks = get_hooks_for_event("discord_member_join");
    if hooks.is_empty() {
        return;
    }

    for (config, hook) in hooks {
        let mut vars: HashMap<&str, String> = HashMap::new();
        vars.insert("guildId", guild_id.to_string());
        vars.insert("guildName", guild_name.to_string());
        vars.insert("userId", user_id.to_string());
        vars.insert("userName", user_name.to_string());
        vars.insert("userBot", user_bot.to_string());
        vars.insert("joinedAt", joined_at.to_string());
        vars.insert("memberCount", member_count.to_string());
        vars.insert("goals", read_agent_goals(&config.key));

        let prompt = render_template(&hook.prompt_template, &vars);
        spawn_hook_session(
            &config,
            "discord_member_join",
            prompt,
            format!("hook:{}:join:{}:{}", config.key, guild_id, user_id),
            format!("join-{}-{}", guild_id, user_id),
            hook.safe_mode,
            dispatcher,
        );
    }
}

// =====================================================
// telegram_message
// =====================================================

/// Template variables for `telegram_message`:
/// `{chatId}`, `{chatName}`, `{chatType}`, `{userId}`, `{userName}`,
/// `{userBot}`, `{messageId}`, `{content}`
pub async fn fire_telegram_message_hooks(
    chat_id: i64,
    chat_name: &str,
    chat_type: &str,
    user_id: &str,
    user_name: &str,
    user_bot: bool,
    message_id: &str,
    content: &str,
    dispatcher: &Arc<MessageDispatcher>,
) {
    let hooks = get_hooks_for_event("telegram_message");
    if hooks.is_empty() {
        return;
    }

    for (config, hook) in hooks {
        let mut vars: HashMap<&str, String> = HashMap::new();
        vars.insert("chatId", chat_id.to_string());
        vars.insert("chatName", chat_name.to_string());
        vars.insert("chatType", chat_type.to_string());
        vars.insert("userId", user_id.to_string());
        vars.insert("userName", user_name.to_string());
        vars.insert("userBot", user_bot.to_string());
        vars.insert("messageId", message_id.to_string());
        vars.insert("content", content.to_string());
        vars.insert("goals", read_agent_goals(&config.key));

        let prompt = render_template(&hook.prompt_template, &vars);
        spawn_hook_session(
            &config,
            "telegram_message",
            prompt,
            format!("hook:{}:tg:{}:{}", config.key, chat_id, message_id),
            format!("tg-{}", message_id),
            hook.safe_mode,
            dispatcher,
        );
    }
}

// =====================================================
// telegram_mention
// =====================================================

/// Template variables for `telegram_mention`:
/// `{chatId}`, `{chatName}`, `{chatType}`, `{userId}`, `{userName}`,
/// `{userBot}`, `{messageId}`, `{content}`
pub async fn fire_telegram_mention_hooks(
    chat_id: i64,
    chat_name: &str,
    chat_type: &str,
    user_id: &str,
    user_name: &str,
    user_bot: bool,
    message_id: &str,
    content: &str,
    dispatcher: &Arc<MessageDispatcher>,
) {
    let hooks = get_hooks_for_event("telegram_mention");
    if hooks.is_empty() {
        return;
    }

    for (config, hook) in hooks {
        let mut vars: HashMap<&str, String> = HashMap::new();
        vars.insert("chatId", chat_id.to_string());
        vars.insert("chatName", chat_name.to_string());
        vars.insert("chatType", chat_type.to_string());
        vars.insert("userId", user_id.to_string());
        vars.insert("userName", user_name.to_string());
        vars.insert("userBot", user_bot.to_string());
        vars.insert("messageId", message_id.to_string());
        vars.insert("content", content.to_string());
        vars.insert("goals", read_agent_goals(&config.key));

        let prompt = render_template(&hook.prompt_template, &vars);
        spawn_hook_session(
            &config,
            "telegram_mention",
            prompt,
            format!("hook:{}:tg:{}:{}", config.key, chat_id, message_id),
            format!("tg-{}", message_id),
            hook.safe_mode,
            dispatcher,
        );
    }
}

// =====================================================
// twitter_mentioned
// =====================================================

/// Template variables for `twitter_mentioned`:
/// `{tweetId}`, `{authorId}`, `{authorUsername}`, `{content}`, `{conversationId}`, `{timestamp}`
pub async fn fire_twitter_mentioned_hooks(
    tweet_id: &str,
    author_id: &str,
    author_username: &str,
    content: &str,
    conversation_id: Option<&str>,
    dispatcher: &Arc<MessageDispatcher>,
) {
    let hooks = get_hooks_for_event("twitter_mentioned");
    if hooks.is_empty() {
        return;
    }

    let now = chrono::Utc::now();
    let timestamp = now.to_rfc3339();

    for (config, hook) in hooks {
        let mut vars: HashMap<&str, String> = HashMap::new();
        vars.insert("tweetId", tweet_id.to_string());
        vars.insert("authorId", author_id.to_string());
        vars.insert("authorUsername", author_username.to_string());
        vars.insert("content", content.to_string());
        vars.insert("conversationId", conversation_id.unwrap_or("").to_string());
        vars.insert("timestamp", timestamp.clone());
        vars.insert("goals", read_agent_goals(&config.key));

        let prompt = render_template(&hook.prompt_template, &vars);
        spawn_hook_session(
            &config,
            "twitter_mentioned",
            prompt,
            format!("hook:{}:twitter:{}", config.key, tweet_id),
            format!("tw-{}", tweet_id),
            hook.safe_mode,
            dispatcher,
        );
    }
}

// =====================================================
// heartbeat
// =====================================================

/// Template variables for `heartbeat`:
/// `{agentKey}`, `{timestamp}`
///
/// Unlike other hooks, this is fired per-agent during the heartbeat tick.
/// Only fires for agents that have the `heartbeat` hook declared.
pub async fn fire_heartbeat_hooks(
    agent_key: &str,
    dispatcher: &Arc<MessageDispatcher>,
) {
    let hooks = get_hooks_for_event("heartbeat");
    if hooks.is_empty() {
        return;
    }

    let now = chrono::Utc::now();
    let timestamp = now.to_rfc3339();

    for (config, hook) in hooks {
        // Only fire for the agent whose heartbeat is running
        if config.key != agent_key {
            continue;
        }

        let mut vars: HashMap<&str, String> = HashMap::new();
        vars.insert("agentKey", agent_key.to_string());
        vars.insert("timestamp", timestamp.clone());
        vars.insert("goals", read_agent_goals(&config.key));

        let prompt = render_template(&hook.prompt_template, &vars);
        spawn_hook_session(
            &config,
            "heartbeat",
            prompt,
            format!("hook:{}:heartbeat:{}", config.key, now.timestamp()),
            format!("hb-{}", now.timestamp()),
            hook.safe_mode,
            dispatcher,
        );
    }
}

// =====================================================
// custom (generic)
// =====================================================

/// Fire hooks for an arbitrary event name.
///
/// Template variables: `{event}`, `{data}`, `{timestamp}`, `{goals}`,
/// plus any top-level string/number/boolean fields from `data` (e.g.
/// `{username}`, `{tweet_text}` when the data payload includes those keys).
///
/// This is the generic entry-point used by the internal hooks API
/// (`POST /api/internal/hooks/fire`) and by module background workers.
/// Any agent with a matching hook file (e.g. `hooks/spot_trader_pulse.md`)
/// will be triggered.
pub async fn fire_custom_hooks(
    event: &str,
    data: serde_json::Value,
    dispatcher: &Arc<MessageDispatcher>,
) {
    let hooks = get_hooks_for_event(event);
    if hooks.is_empty() {
        log::debug!("[PERSONA_HOOK] No hooks found for custom event '{}'", event);
        return;
    }

    let now = chrono::Utc::now();
    let timestamp = now.to_rfc3339();
    let data_str = serde_json::to_string_pretty(&data).unwrap_or_default();

    for (config, hook) in hooks {
        let mut vars: HashMap<&str, String> = HashMap::new();
        vars.insert("event", event.to_string());
        vars.insert("data", data_str.clone());
        vars.insert("timestamp", timestamp.clone());
        vars.insert("goals", read_agent_goals(&config.key));

        // Also inject top-level scalar fields from the data payload so hook
        // templates can use e.g. {username}, {tweet_text} directly instead of
        // parsing the raw {data} JSON blob.
        if let Some(obj) = data.as_object() {
            for (key, value) in obj {
                if let Some(s) = value.as_str() {
                    vars.insert(key.as_str(), s.to_string());
                } else if value.is_number() || value.is_boolean() {
                    vars.insert(key.as_str(), value.to_string());
                }
            }
        }

        let prompt = render_template(&hook.prompt_template, &vars);
        spawn_hook_session(
            &config,
            event,
            prompt,
            format!("hook:{}:custom:{}:{}", config.key, event, now.timestamp()),
            format!("custom-{}-{}", event, now.timestamp()),
            hook.safe_mode,
            dispatcher,
        );
    }
}

// =====================================================
// Helpers
// =====================================================

/// Read the goals.md file for an agent, returning its content or empty string.
fn read_agent_goals(agent_key: &str) -> String {
    let path = runtime_agents_dir().join(agent_key).join("goals.md");
    std::fs::read_to_string(&path).unwrap_or_default()
}

/// Deterministic hash of an agent key to a channel offset (0..99).
/// Same algorithm as heartbeat but used for hook channel IDs.
fn hook_channel_offset(key: &str) -> i64 {
    let mut hash: u64 = 5381;
    for b in key.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u64);
    }
    (hash % 100) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_template_basic() {
        let template = "Guild: {guildId} | Channel: #{channelName} ({channelId})\nAuthor: {authorName}";
        let mut vars = HashMap::new();
        vars.insert("guildId", "12345".to_string());
        vars.insert("channelName", "general".to_string());
        vars.insert("channelId", "67890".to_string());
        vars.insert("authorName", "TestUser".to_string());

        let result = render_template(template, &vars);
        assert_eq!(result, "Guild: 12345 | Channel: #general (67890)\nAuthor: TestUser");
    }

    #[test]
    fn test_render_template_no_vars() {
        let template = "No variables here.";
        let vars = HashMap::new();
        assert_eq!(render_template(template, &vars), "No variables here.");
    }

    #[test]
    fn test_render_template_missing_var() {
        let template = "Hello {name}, welcome to {place}!";
        let mut vars = HashMap::new();
        vars.insert("name", "Alice".to_string());
        assert_eq!(render_template(template, &vars), "Hello Alice, welcome to {place}!");
    }

    #[test]
    fn test_hook_channel_offset_deterministic() {
        let a = hook_channel_offset("discord_moderator");
        let b = hook_channel_offset("discord_moderator");
        assert_eq!(a, b);
        assert!(a >= 0 && a < 100);
    }
}
