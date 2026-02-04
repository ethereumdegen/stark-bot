//! Twitter @mention listener using polling-based approach
//!
//! Polls the Twitter API v2 mentions endpoint to detect and respond to @mentions.
//! Uses OAuth 1.0a for authentication and respects rate limits.

use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::types::{ChannelType, NormalizedMessage};
use crate::controllers::api_keys::ApiKeyId;
use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{Channel, ChannelSettingKey};
use crate::tools::builtin::social_media::{generate_oauth_header, TwitterCredentials};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::interval;

/// Pre-compiled regex for stripping leading @mentions (case-insensitive)
static LEADING_MENTION_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^\s*@\w+\s*").unwrap());

/// Minimum poll interval in seconds (Twitter rate limit protection)
const MIN_POLL_INTERVAL_SECS: u64 = 60;

/// Default poll interval in seconds
const DEFAULT_POLL_INTERVAL_SECS: u64 = 120;

/// Twitter API v2 base URL
const TWITTER_API_BASE: &str = "https://api.twitter.com/2";

/// Maximum characters per tweet
const TWITTER_MAX_CHARS: usize = 280;

/// Configuration for the Twitter listener
#[derive(Debug, Clone)]
pub struct TwitterConfig {
    pub bot_handle: String,
    pub bot_user_id: String,
    pub poll_interval_secs: u64,
    pub credentials: TwitterCredentials,
}

impl TwitterConfig {
    /// Load configuration from channel settings and API keys
    pub fn from_channel(channel: &Channel, db: &Database) -> Result<Self, String> {
        let channel_id = channel.id;

        // Load channel settings
        let bot_handle = db
            .get_channel_setting(channel_id, ChannelSettingKey::TwitterBotHandle.as_ref())
            .map_err(|e| format!("Failed to get bot handle: {}", e))?
            .ok_or_else(|| "Twitter bot handle not configured".to_string())?;

        let bot_user_id = db
            .get_channel_setting(channel_id, ChannelSettingKey::TwitterBotUserId.as_ref())
            .map_err(|e| format!("Failed to get bot user ID: {}", e))?
            .ok_or_else(|| "Twitter bot user ID not configured".to_string())?;

        let poll_interval_secs = db
            .get_channel_setting(channel_id, ChannelSettingKey::TwitterPollIntervalSecs.as_ref())
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_POLL_INTERVAL_SECS)
            .max(MIN_POLL_INTERVAL_SECS);

        // Load OAuth credentials from API keys
        let consumer_key = get_api_key(db, ApiKeyId::TwitterConsumerKey)
            .ok_or_else(|| "TWITTER_CONSUMER_KEY not configured".to_string())?;
        let consumer_secret = get_api_key(db, ApiKeyId::TwitterConsumerSecret)
            .ok_or_else(|| "TWITTER_CONSUMER_SECRET not configured".to_string())?;
        let access_token = get_api_key(db, ApiKeyId::TwitterAccessToken)
            .ok_or_else(|| "TWITTER_ACCESS_TOKEN not configured".to_string())?;
        let access_token_secret = get_api_key(db, ApiKeyId::TwitterAccessTokenSecret)
            .ok_or_else(|| "TWITTER_ACCESS_TOKEN_SECRET not configured".to_string())?;

        Ok(Self {
            bot_handle,
            bot_user_id,
            poll_interval_secs,
            credentials: TwitterCredentials::new(
                consumer_key,
                consumer_secret,
                access_token,
                access_token_secret,
            ),
        })
    }
}

/// Get an API key from the database with env var fallback
fn get_api_key(db: &Database, key_id: ApiKeyId) -> Option<String> {
    // Try database first
    if let Ok(Some(api_key)) = db.get_api_key(key_id.as_str()) {
        if !api_key.api_key.is_empty() {
            return Some(api_key.api_key);
        }
    }

    // Fallback to env vars
    if let Some(env_vars) = key_id.env_vars() {
        for var in env_vars {
            if let Ok(val) = std::env::var(var) {
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
    }

    None
}

/// Twitter API v2 mentions response
#[derive(Debug, Deserialize)]
struct MentionsResponse {
    data: Option<Vec<Tweet>>,
    meta: Option<MentionsMeta>,
    errors: Option<Vec<TwitterApiError>>,
}

#[derive(Debug, Deserialize)]
struct Tweet {
    id: String,
    text: String,
    author_id: String,
    conversation_id: Option<String>,
    in_reply_to_user_id: Option<String>,
    referenced_tweets: Option<Vec<ReferencedTweet>>,
}

#[derive(Debug, Deserialize)]
struct ReferencedTweet {
    #[serde(rename = "type")]
    ref_type: String,
    id: String,
}

#[derive(Debug, Deserialize)]
struct MentionsMeta {
    result_count: i64,
    newest_id: Option<String>,
    oldest_id: Option<String>,
    next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TwitterApiError {
    message: String,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

/// Rate limit information from Twitter API response headers
#[derive(Debug, Clone, Default)]
struct RateLimitInfo {
    /// Remaining requests in current window
    remaining: Option<u32>,
    /// Unix timestamp when the rate limit resets
    reset_at: Option<u64>,
}

impl RateLimitInfo {
    /// Parse rate limit headers from a response
    fn from_response(response: &reqwest::Response) -> Self {
        let remaining = response
            .headers()
            .get("x-rate-limit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());

        let reset_at = response
            .headers()
            .get("x-rate-limit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());

        Self { remaining, reset_at }
    }

    /// Calculate how long to wait until rate limit resets (in seconds)
    fn seconds_until_reset(&self) -> Option<u64> {
        self.reset_at.map(|reset| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            reset.saturating_sub(now)
        })
    }

    /// Returns true if we're rate limited (remaining == 0)
    fn is_rate_limited(&self) -> bool {
        self.remaining == Some(0)
    }
}

/// Result of polling mentions, includes rate limit info
struct PollResult {
    tweets: Vec<Tweet>,
    rate_limit: RateLimitInfo,
}

/// Twitter API v2 users response (for looking up usernames)
#[derive(Debug, Deserialize)]
struct UsersResponse {
    data: Option<Vec<TwitterUser>>,
}

#[derive(Debug, Deserialize)]
struct TwitterUser {
    id: String,
    username: String,
    name: String,
}

/// Twitter API v2 tweet post response
#[derive(Debug, Deserialize)]
struct PostTweetResponse {
    data: Option<PostedTweet>,
    errors: Option<Vec<TwitterApiError>>,
}

#[derive(Debug, Deserialize)]
struct PostedTweet {
    id: String,
    text: String,
}

/// Start the Twitter mention listener
pub async fn start_twitter_listener(
    channel: Channel,
    dispatcher: Arc<MessageDispatcher>,
    broadcaster: Arc<EventBroadcaster>,
    db: Arc<Database>,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let channel_id = channel.id;
    let channel_name = channel.name.clone();

    log::info!("Starting Twitter listener for channel: {}", channel_name);

    // SECURITY: Ensure safe_mode is enabled for Twitter channels
    // Twitter input is untrusted and could contain prompt injection attacks
    if !channel.safe_mode {
        log::warn!(
            "Twitter channel {} does not have safe_mode enabled - enabling now for security",
            channel_id
        );
        // Enable safe_mode on the channel
        if let Err(e) = db.set_channel_safe_mode(channel_id, true) {
            log::error!("Failed to enable safe_mode on Twitter channel {}: {}", channel_id, e);
            // Continue anyway - the dispatcher will check safe_mode per-message
        }
    }
    log::info!("Twitter: Safe mode ENABLED - tool access restricted to Web only");

    // Load configuration
    let config = TwitterConfig::from_channel(&channel, &db)?;
    log::info!(
        "Twitter: Bot handle=@{}, user_id={}, poll_interval={}s",
        config.bot_handle,
        config.bot_user_id,
        config.poll_interval_secs
    );

    // Validate credentials by fetching user info
    let client = reqwest::Client::new();
    match verify_credentials(&client, &config).await {
        Ok(username) => {
            log::info!("Twitter: Credentials validated for @{}", username);
        }
        Err(e) => {
            let error = format!("Twitter: Invalid credentials: {}", e);
            log::error!("{}", error);
            return Err(error);
        }
    }

    // Emit started event
    broadcaster.broadcast(GatewayEvent::channel_started(
        channel_id,
        ChannelType::Twitter.as_str(),
        &channel_name,
    ));

    // Get the last processed tweet ID to avoid reprocessing
    let mut since_id = db
        .get_last_processed_tweet_id(channel_id)
        .ok()
        .flatten();

    log::info!(
        "Twitter: Starting poll loop, since_id={:?}",
        since_id
    );

    // Create poll interval
    let mut poll_interval = interval(Duration::from_secs(config.poll_interval_secs));

    // Main polling loop
    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                log::info!("Twitter listener {} received shutdown signal", channel_name);
                break;
            }
            _ = poll_interval.tick() => {
                // Poll for new mentions
                match poll_mentions(&client, &config, since_id.as_deref()).await {
                    Ok(poll_result) => {
                        // Log rate limit status if getting low
                        if let Some(remaining) = poll_result.rate_limit.remaining {
                            if remaining <= 3 {
                                log::warn!(
                                    "Twitter: Rate limit low ({} remaining), reset in {:?}s",
                                    remaining,
                                    poll_result.rate_limit.seconds_until_reset()
                                );
                            }
                        }

                        // Proactive backoff if we're about to hit rate limit
                        if poll_result.rate_limit.is_rate_limited() {
                            let wait_secs = poll_result.rate_limit
                                .seconds_until_reset()
                                .unwrap_or(300)
                                .max(60); // Wait at least 60 seconds
                            log::warn!(
                                "Twitter: Rate limit exhausted, backing off for {} seconds",
                                wait_secs
                            );
                            tokio::time::sleep(Duration::from_secs(wait_secs)).await;
                            continue;
                        }

                        if !poll_result.tweets.is_empty() {
                            log::info!("Twitter: Found {} new mention(s)", poll_result.tweets.len());

                            // Process mentions in chronological order (oldest first)
                            for mention in poll_result.tweets.into_iter().rev() {
                                // Skip if already processed (safety check)
                                if db.is_tweet_processed(&mention.id).unwrap_or(false) {
                                    log::debug!("Twitter: Skipping already processed tweet {}", mention.id);
                                    continue;
                                }

                                // Skip retweets and quote tweets (only respond to direct mentions)
                                if is_retweet_or_quote(&mention) {
                                    log::debug!("Twitter: Skipping retweet/quote tweet {}", mention.id);
                                    // Still mark as processed to avoid checking again
                                    let _ = db.mark_tweet_processed(
                                        &mention.id,
                                        channel_id,
                                        &mention.author_id,
                                        "unknown",
                                        &mention.text,
                                    );
                                    continue;
                                }

                                // Look up author username
                                let author_username = match lookup_user(&client, &config, &mention.author_id).await {
                                    Ok(user) => user.username,
                                    Err(e) => {
                                        log::warn!("Twitter: Failed to lookup user {}: {}", mention.author_id, e);
                                        format!("user_{}", mention.author_id)
                                    }
                                };

                                log::info!(
                                    "Twitter: Processing mention from @{}: {}",
                                    author_username,
                                    if mention.text.len() > 50 {
                                        format!("{}...", &mention.text[..50])
                                    } else {
                                        mention.text.clone()
                                    }
                                );

                                // Process the mention
                                let response = process_mention(
                                    &mention,
                                    &author_username,
                                    &config,
                                    channel_id,
                                    &dispatcher,
                                    &broadcaster,
                                ).await;

                                // Mark as processed before replying (to avoid double-processing on errors)
                                if let Err(e) = db.mark_tweet_processed(
                                    &mention.id,
                                    channel_id,
                                    &mention.author_id,
                                    &author_username,
                                    &mention.text,
                                ) {
                                    log::error!("Twitter: Failed to mark tweet {} as processed: {}", mention.id, e);
                                }

                                // Post reply if we have a response
                                if let Some(response_text) = response {
                                    if let Err(e) = post_reply(
                                        &client,
                                        &config,
                                        &mention.id,
                                        &response_text,
                                    ).await {
                                        log::error!("Twitter: Failed to post reply: {}", e);
                                    }
                                }

                                // Update since_id to the most recent tweet
                                since_id = Some(mention.id.clone());
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Twitter: Error polling mentions: {}", e);
                        // On rate limit (429), back off with default wait time
                        // (we couldn't parse headers in error case)
                        if e.contains("429") || e.contains("rate limit") {
                            log::warn!("Twitter: Rate limited, backing off for 5 minutes");
                            tokio::time::sleep(Duration::from_secs(300)).await;
                        }
                    }
                }
            }
        }
    }

    // Emit stopped event
    broadcaster.broadcast(GatewayEvent::channel_stopped(
        channel_id,
        ChannelType::Twitter.as_str(),
        &channel_name,
    ));

    Ok(())
}

/// Verify credentials by fetching the authenticated user
async fn verify_credentials(
    client: &reqwest::Client,
    config: &TwitterConfig,
) -> Result<String, String> {
    let url = format!("{}/users/me", TWITTER_API_BASE);
    let auth_header = generate_oauth_header("GET", &url, &config.credentials, None);

    let response = client
        .get(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("API error ({}): {}", status, body));
    }

    let data: UsersResponse =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {}", e))?;

    data.data
        .and_then(|users| users.into_iter().next())
        .map(|user| user.username)
        .ok_or_else(|| "No user data returned".to_string())
}

/// Poll for new mentions, returning tweets and rate limit info
async fn poll_mentions(
    client: &reqwest::Client,
    config: &TwitterConfig,
    since_id: Option<&str>,
) -> Result<PollResult, String> {
    let url = format!(
        "{}/users/{}/mentions",
        TWITTER_API_BASE, config.bot_user_id
    );

    // Build query parameters
    let mut params: Vec<(&str, &str)> = vec![
        ("tweet.fields", "author_id,conversation_id,in_reply_to_user_id,referenced_tweets"),
        ("max_results", "10"),
    ];

    let since_id_owned: String;
    if let Some(id) = since_id {
        since_id_owned = id.to_string();
        params.push(("since_id", &since_id_owned));
    }

    // Build full URL with query string
    let query_string: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");
    let full_url = format!("{}?{}", url, query_string);

    // Generate OAuth header (params must be included in signature)
    let auth_header = generate_oauth_header(
        "GET",
        &url,
        &config.credentials,
        Some(&params),
    );

    let response = client
        .get(&full_url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    // Parse rate limit headers before consuming response body
    let rate_limit = RateLimitInfo::from_response(&response);
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("API error ({}): {}", status, body));
    }

    let data: MentionsResponse =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(errors) = data.errors {
        let error_msg = errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("Twitter API errors: {}", error_msg));
    }

    Ok(PollResult {
        tweets: data.data.unwrap_or_default(),
        rate_limit,
    })
}

/// Check if a tweet is a retweet or quote tweet
fn is_retweet_or_quote(tweet: &Tweet) -> bool {
    if let Some(refs) = &tweet.referenced_tweets {
        for ref_tweet in refs {
            if ref_tweet.ref_type == "retweeted" || ref_tweet.ref_type == "quoted" {
                return true;
            }
        }
    }
    false
}

/// Look up a user by ID
async fn lookup_user(
    client: &reqwest::Client,
    config: &TwitterConfig,
    user_id: &str,
) -> Result<TwitterUser, String> {
    let url = format!("{}/users/{}", TWITTER_API_BASE, user_id);
    let auth_header = generate_oauth_header("GET", &url, &config.credentials, None);

    let response = client
        .get(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("API error ({}): {}", status, body));
    }

    let data: UsersResponse =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {}", e))?;

    data.data
        .and_then(|users| users.into_iter().next())
        .ok_or_else(|| "User not found".to_string())
}

/// Extract command text from a tweet, removing @mentions
fn extract_command_text(text: &str, bot_handle: &str) -> String {
    // Remove @bot_handle (case-insensitive) and any other @mentions at the start
    let mut result = text.to_string();

    // Remove our bot's mention (case-insensitive using regex)
    let bot_mention_pattern = Regex::new(&format!(r"(?i)@{}", regex::escape(bot_handle)))
        .unwrap_or_else(|_| Regex::new(r"(?i)@\w+").unwrap());
    result = bot_mention_pattern.replace_all(&result, "").to_string();

    // Remove leading @mentions (common in replies) using pre-compiled static regex
    while LEADING_MENTION_PATTERN.is_match(&result) {
        result = LEADING_MENTION_PATTERN.replace(&result, "").to_string();
    }

    result.trim().to_string()
}

/// Process a mention and get the AI response
async fn process_mention(
    tweet: &Tweet,
    author_username: &str,
    config: &TwitterConfig,
    channel_id: i64,
    dispatcher: &Arc<MessageDispatcher>,
    broadcaster: &Arc<EventBroadcaster>,
) -> Option<String> {
    // Extract the actual command/message text
    let command_text = extract_command_text(&tweet.text, &config.bot_handle);

    if command_text.is_empty() {
        log::debug!("Twitter: Empty command after extracting text, ignoring");
        return None;
    }

    // Add source hint to help the agent understand the context
    let text_with_hint = format!(
        "[TWITTER MENTION from @{} - Keep response under 280 chars or it will be threaded]\n\n{}",
        author_username, command_text
    );

    // Create normalized message for dispatcher
    let normalized = NormalizedMessage {
        channel_id,
        channel_type: ChannelType::Twitter.to_string(),
        chat_id: tweet.conversation_id.clone().unwrap_or_else(|| tweet.id.clone()),
        user_id: tweet.author_id.clone(),
        user_name: author_username.to_string(),
        text: text_with_hint,
        message_id: Some(tweet.id.clone()),
        session_mode: None,
        selected_network: None,
        force_safe_mode: false,
    };

    // Subscribe to events (for logging, not forwarding)
    let (client_id, _event_rx) = broadcaster.subscribe();

    // Dispatch to AI
    log::info!("Twitter: Dispatching message to AI for @{}", author_username);
    let result = dispatcher.dispatch(normalized).await;

    // Unsubscribe from events
    broadcaster.unsubscribe(&client_id);

    log::info!(
        "Twitter: Dispatch complete for @{}, error={:?}",
        author_username,
        result.error
    );

    if result.error.is_none() && !result.response.is_empty() {
        Some(result.response)
    } else if let Some(error) = result.error {
        Some(format!("Sorry, I encountered an error: {}", error))
    } else {
        None
    }
}

/// Split a response into tweet-sized chunks for threading
fn split_for_twitter(text: &str) -> Vec<String> {
    if text.chars().count() <= TWITTER_MAX_CHARS {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    // Try to split on sentence boundaries first, then words
    for line in text.lines() {
        for word in line.split_whitespace() {
            let potential = if current_chunk.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", current_chunk, word)
            };

            // Reserve space for thread indicator (e.g., " 1/3")
            let max_chunk_chars = TWITTER_MAX_CHARS - 5;

            if potential.chars().count() > max_chunk_chars {
                if !current_chunk.is_empty() {
                    chunks.push(current_chunk);
                    current_chunk = word.to_string();
                } else {
                    // Single word exceeds limit, truncate it
                    let truncated: String = word.chars().take(max_chunk_chars - 3).collect();
                    chunks.push(format!("{}...", truncated));
                    current_chunk = String::new();
                }
            } else {
                current_chunk = potential;
            }
        }

        // Add newline between lines if we have content
        if !current_chunk.is_empty() && current_chunk.chars().count() < TWITTER_MAX_CHARS - 5 {
            current_chunk.push('\n');
        }
    }

    if !current_chunk.is_empty() {
        // Remove trailing newline
        chunks.push(current_chunk.trim_end().to_string());
    }

    // Add thread indicators if multiple chunks
    if chunks.len() > 1 {
        let total = chunks.len();
        chunks = chunks
            .into_iter()
            .enumerate()
            .map(|(i, chunk)| format!("{} {}/{}", chunk.trim_end(), i + 1, total))
            .collect();
    }

    chunks
}

/// Post a reply to a tweet (with threading for long responses)
async fn post_reply(
    client: &reqwest::Client,
    config: &TwitterConfig,
    reply_to_id: &str,
    text: &str,
) -> Result<String, String> {
    let chunks = split_for_twitter(text);
    let mut last_tweet_id = reply_to_id.to_string();

    for (i, chunk) in chunks.iter().enumerate() {
        log::info!(
            "Twitter: Posting reply chunk {}/{} ({} chars)",
            i + 1,
            chunks.len(),
            chunk.chars().count()
        );

        let tweet_id = post_single_tweet(client, config, &chunk, Some(&last_tweet_id)).await?;
        last_tweet_id = tweet_id;
    }

    Ok(last_tweet_id)
}

/// Post a single tweet
async fn post_single_tweet(
    client: &reqwest::Client,
    config: &TwitterConfig,
    text: &str,
    reply_to_id: Option<&str>,
) -> Result<String, String> {
    let url = format!("{}/tweets", TWITTER_API_BASE);
    let auth_header = generate_oauth_header("POST", &url, &config.credentials, None);

    // Build request body
    let mut body = serde_json::json!({
        "text": text
    });

    if let Some(reply_to) = reply_to_id {
        body["reply"] = serde_json::json!({
            "in_reply_to_tweet_id": reply_to
        });
    }

    let response = client
        .post(&url)
        .header("Authorization", auth_header)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = response.status();
    let response_body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("API error ({}): {}", status, response_body));
    }

    let data: PostTweetResponse = serde_json::from_str(&response_body)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    if let Some(errors) = data.errors {
        let error_msg = errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("Twitter API errors: {}", error_msg));
    }

    data.data
        .map(|tweet| {
            log::info!(
                "Twitter: Posted tweet {} - {}",
                tweet.id,
                if tweet.text.len() > 50 {
                    format!("{}...", &tweet.text[..50])
                } else {
                    tweet.text.clone()
                }
            );
            tweet.id
        })
        .ok_or_else(|| "No tweet data returned".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_command_text() {
        // Basic case
        assert_eq!(
            extract_command_text("@starkbot hello world", "starkbot"),
            "hello world"
        );
        // Mixed case - should be case-insensitive
        assert_eq!(
            extract_command_text("@StarkBot what's the price?", "starkbot"),
            "what's the price?"
        );
        // Fully uppercase
        assert_eq!(
            extract_command_text("@STARKBOT test message", "starkbot"),
            "test message"
        );
        // Weird mixed case
        assert_eq!(
            extract_command_text("@StArKbOt random case", "starkbot"),
            "random case"
        );
        // Multiple mentions with our bot
        assert_eq!(
            extract_command_text("@user1 @starkbot help me", "starkbot"),
            "help me"
        );
        // Bot mention in middle of text (should be removed)
        assert_eq!(
            extract_command_text("Hey @StarkBot can you help?", "starkbot"),
            "Hey can you help?"
        );
        // Just the mention, no text
        assert_eq!(
            extract_command_text("@starkbot", "starkbot"),
            ""
        );
    }

    #[test]
    fn test_split_for_twitter() {
        // Short message - no split
        let short = "Hello world!";
        assert_eq!(split_for_twitter(short), vec!["Hello world!"]);

        // Long message - should split
        let long = "a ".repeat(200);
        let chunks = split_for_twitter(&long);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= TWITTER_MAX_CHARS);
        }
    }

    #[test]
    fn test_is_retweet_or_quote() {
        let regular_tweet = Tweet {
            id: "123".to_string(),
            text: "Hello".to_string(),
            author_id: "456".to_string(),
            conversation_id: None,
            in_reply_to_user_id: None,
            referenced_tweets: None,
        };
        assert!(!is_retweet_or_quote(&regular_tweet));

        let retweet = Tweet {
            id: "123".to_string(),
            text: "RT: Hello".to_string(),
            author_id: "456".to_string(),
            conversation_id: None,
            in_reply_to_user_id: None,
            referenced_tweets: Some(vec![ReferencedTweet {
                ref_type: "retweeted".to_string(),
                id: "789".to_string(),
            }]),
        };
        assert!(is_retweet_or_quote(&retweet));
    }
}
