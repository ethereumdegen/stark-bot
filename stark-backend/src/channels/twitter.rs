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
use crate::tools::builtin::social_media::{
    check_subscription_tier, generate_oauth_header, percent_encode, TwitterCredentials,
    XSubscriptionTier, TWITTER_MAX_CHARS, TWITTER_PREMIUM_MAX_CHARS,
};
use once_cell::sync::Lazy;
use rand::Rng;
use regex::Regex;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
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

/// Maximum number of tweets to fetch for thread context
const MAX_THREAD_TWEETS: usize = 100;

/// Maximum characters of thread context to include in the hint
const MAX_THREAD_CONTEXT_CHARS: usize = 15_000;

/// Configuration for the Twitter listener
#[derive(Debug, Clone)]
pub struct TwitterConfig {
    pub bot_handle: String,
    pub bot_user_id: String,
    pub poll_interval_secs: u64,
    pub subscription_tier: XSubscriptionTier,
    pub reply_chance: u8,
    pub max_mentions_per_hour: u32,
    pub admin_user_id: Option<String>,
    pub credentials: TwitterCredentials,
}

impl TwitterConfig {
    /// Get the max characters per tweet based on subscription tier
    pub fn max_chars(&self) -> usize {
        self.subscription_tier.max_tweet_chars()
    }
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

        let reply_chance: u8 = db
            .get_channel_setting(channel_id, ChannelSettingKey::TwitterReplyChance.as_ref())
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100)
            .min(100);

        let max_mentions_per_hour: u32 = db
            .get_channel_setting(channel_id, ChannelSettingKey::TwitterMaxMentionsPerHour.as_ref())
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let admin_user_id = db
            .get_channel_setting(channel_id, ChannelSettingKey::TwitterAdminXAccount.as_ref())
            .ok()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()));

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
            subscription_tier: XSubscriptionTier::None, // detected at startup via API
            reply_chance,
            max_mentions_per_hour,
            admin_user_id,
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

/// Twitter API v2 users response (for looking up usernames - single user)
#[derive(Debug, Deserialize)]
struct SingleUserResponse {
    data: Option<TwitterUser>,
}

#[derive(Debug, Deserialize)]
struct TwitterUser {
    id: String,
    username: String,
    name: String,
    /// X Premium subscription type: None, Basic, Premium, PremiumPlus
    subscription_type: Option<String>,
}

/// Twitter API v2 search response for thread context (includes user expansions)
#[derive(Debug, Deserialize)]
struct ThreadSearchResponse {
    data: Option<Vec<ThreadTweet>>,
    includes: Option<ThreadIncludes>,
}

#[derive(Debug, Deserialize)]
struct ThreadTweet {
    id: String,
    text: String,
    author_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThreadIncludes {
    users: Option<Vec<ThreadUser>>,
}

#[derive(Debug, Deserialize)]
struct ThreadUser {
    id: String,
    username: String,
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

    // Load configuration
    let mut config = TwitterConfig::from_channel(&channel, &db)?;

    // SECURITY: Safe mode is always handled per-message via force_safe_mode.
    // If an admin X account is configured, admin tweets get standard mode while others get safe mode.
    // If no admin is configured, ALL tweets get safe mode.
    // We never set channel-level safe_mode, so the channel stays eligible for cloud backup.
    if config.admin_user_id.is_some() {
        log::info!(
            "Twitter: Admin user ID configured ({}) — admin tweets use standard mode, others use safe mode",
            config.admin_user_id.as_deref().unwrap_or("?")
        );
    } else {
        log::info!("Twitter: No admin configured — all tweets use safe mode (per-message)");
    }
    // Ensure channel-level safe_mode is off (per-message handling is sufficient)
    if channel.safe_mode {
        if let Err(e) = db.set_channel_safe_mode(channel_id, false) {
            log::error!("Failed to disable channel-level safe_mode: {}", e);
        }
    }

    log::info!(
        "Twitter: Bot handle=@{}, user_id={}, poll_interval={}s, reply_chance={}%, max_mentions/hr={}, admin_id={}",
        config.bot_handle,
        config.bot_user_id,
        config.poll_interval_secs,
        config.reply_chance,
        if config.max_mentions_per_hour == 0 { "unlimited".to_string() } else { config.max_mentions_per_hour.to_string() },
        config.admin_user_id.as_deref().unwrap_or("none")
    );

    // Pre-compile bot mention regex (used per-tweet in extract_command_text)
    let bot_mention_regex = Regex::new(&format!(r"(?i)@{}", regex::escape(&config.bot_handle)))
        .unwrap_or_else(|_| Regex::new(r"(?i)@\w+").unwrap());

    // Validate credentials by fetching user info
    let client = crate::http::shared_client().clone();
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

    // Auto-detect subscription tier (Premium allows long tweets)
    config.subscription_tier = check_subscription_tier(&client, &config.credentials).await;
    log::info!(
        "Twitter: Subscription tier={:?}, max_chars={}",
        config.subscription_tier,
        config.max_chars()
    );

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

    // Hourly rate limiter state
    let mut hour_start = Instant::now();
    let mut replies_this_hour: u32 = 0;

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
                let poll_result = poll_mentions(&client, &config, since_id.as_deref()).await;

                // If since_id is stale (older than ~7 days), Twitter rejects it.
                // Retry without since_id and clear the stale value.
                let poll_result = if let Err(ref e) = poll_result {
                    if since_id.is_some() && e.contains("since_id") {
                        log::warn!("Twitter: since_id is stale, dropping it and retrying");
                        since_id = None;
                        poll_mentions(&client, &config, None).await
                    } else {
                        poll_result
                    }
                } else {
                    poll_result
                };

                match poll_result {
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

                                // Skip replies to bot's tweets unless they explicitly @mention the bot.
                                // Twitter auto-prepends @bot_handle when replying, so we ignore those
                                // unless the user typed @bot_handle themselves in the reply body.
                                // This applies to everyone including admin — implicit replies should
                                // not trigger the agentic loop.
                                if is_implicit_reply_to_bot(&mention, &config, &bot_mention_regex) {
                                    log::info!(
                                        "Twitter: Skipping reply-to-bot {} (no explicit @{} in body)",
                                        mention.id, config.bot_handle
                                    );
                                    let _ = db.mark_tweet_processed(
                                        &mention.id,
                                        channel_id,
                                        &mention.author_id,
                                        "unknown",
                                        &mention.text,
                                    );
                                    continue;
                                }

                                // Reset hourly counter if an hour has elapsed
                                if hour_start.elapsed() >= Duration::from_secs(3600) {
                                    hour_start = Instant::now();
                                    replies_this_hour = 0;
                                }

                                // Check if author is the admin (bypass rate limit + reply chance)
                                let is_admin = config.admin_user_id.as_ref()
                                    .map(|admin_id| admin_id == &mention.author_id)
                                    .unwrap_or(false);

                                // Check hourly rate limit (0 = unlimited, admin always bypasses)
                                if !is_admin && config.max_mentions_per_hour > 0 && replies_this_hour >= config.max_mentions_per_hour {
                                    log::info!(
                                        "Twitter: Hourly rate limit reached ({}/{}), skipping mention {}",
                                        replies_this_hour, config.max_mentions_per_hour, mention.id
                                    );
                                    let _ = db.mark_tweet_processed(
                                        &mention.id,
                                        channel_id,
                                        &mention.author_id,
                                        "unknown",
                                        &mention.text,
                                    );
                                    continue;
                                }

                                // Reply chance roll (100 = always reply, admin always bypasses)
                                if !is_admin && config.reply_chance < 100 {
                                    let roll: u8 = rand::thread_rng().gen_range(1..=100);
                                    if roll > config.reply_chance {
                                        log::info!(
                                            "Twitter: Skipping mention {} (rolled {}, need <= {}%)",
                                            mention.id, roll, config.reply_chance
                                        );
                                        let _ = db.mark_tweet_processed(
                                            &mention.id,
                                            channel_id,
                                            &mention.author_id,
                                            "unknown",
                                            &mention.text,
                                        );
                                        continue;
                                    }
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
                                        format!("{}...", mention.text.chars().take(50).collect::<String>())
                                    } else {
                                        mention.text.clone()
                                    }
                                );

                                // Determine safe mode: admin gets standard mode, everyone else gets safe mode.
                                // When no admin is configured, all tweets are safe mode.
                                // (is_admin was computed above, before rate limit check)
                                let force_safe_mode = !is_admin;

                                if is_admin {
                                    log::info!("Twitter: @{} is admin — using standard mode", author_username);
                                } else {
                                    log::info!("Twitter: @{} is not admin — using safe mode", author_username);
                                }

                                // Process the mention
                                let response = process_mention(
                                    &mention,
                                    &author_username,
                                    &config,
                                    channel_id,
                                    force_safe_mode,
                                    &dispatcher,
                                    &broadcaster,
                                    &bot_mention_regex,
                                    &client,
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
                                    match post_reply(
                                        &client,
                                        &config,
                                        &mention.id,
                                        &response_text,
                                    ).await {
                                        Ok(_) => {
                                            replies_this_hour += 1;
                                        }
                                        Err(e) => {
                                            log::error!("Twitter: Failed to post reply: {}", e);
                                        }
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

    let data: SingleUserResponse =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {}", e))?;

    data.data
        .map(|user| user.username)
        .ok_or_else(|| "No user data returned".to_string())
}

/// Poll for new mentions, returning tweets and rate limit info
async fn poll_mentions(
    client: &reqwest::Client,
    config: &TwitterConfig,
    since_id: Option<&str>,
) -> Result<PollResult, String> {
    // Use /tweets/search/recent instead of /users/{id}/mentions
    // This endpoint is available on the pay-per-usage plan (no Basic tier needed)
    let url = format!("{}/tweets/search/recent", TWITTER_API_BASE);

    // Search for @mentions of our bot handle
    let query = format!("@{}", config.bot_handle);

    // Build query parameters
    let mut params: Vec<(&str, &str)> = vec![
        ("query", &query),
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
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
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

    log::debug!("Twitter search/recent response ({}): {}", status, body);

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

/// Check if a tweet is a reply to the bot that doesn't explicitly mention @bot_handle.
/// Twitter auto-prepends @bot_handle when replying to the bot's tweet, which causes
/// the search API to pick it up. We only want to respond if the user explicitly
/// @'d the bot in the body of their reply (not just via the auto-prepended mention).
fn is_implicit_reply_to_bot(tweet: &Tweet, config: &TwitterConfig, bot_mention_regex: &Regex) -> bool {
    // Only applies to direct replies to the bot's tweets
    if tweet.in_reply_to_user_id.as_deref() != Some(config.bot_user_id.as_str()) {
        return false;
    }

    // Count how many times @bot_handle appears in the original text.
    // Twitter auto-prepends exactly one @bot_handle when replying. If the user also
    // explicitly typed @bot_handle, there will be 2+ occurrences (or 1 if Twitter
    // deduped them, but in that case the user clearly intended to mention the bot).
    let mention_count = bot_mention_regex.find_iter(&tweet.text).count();

    // If @bot_handle appears more than once, the user explicitly added one
    if mention_count > 1 {
        return false;
    }

    // Single @bot_handle: strip it (the auto-prepended one) and all other leading
    // @mentions (e.g. @other_user in a multi-person thread). If the remaining text
    // is empty or whitespace-only, it was just the auto-prepend with no real content
    // directed at the bot — treat as implicit. But if there's actual text after
    // stripping, the user wrote something in reply to the bot, so treat it as
    // intentional even with only one @mention.
    let mut text = tweet.text.clone();
    while LEADING_MENTION_PATTERN.is_match(&text) {
        text = LEADING_MENTION_PATTERN.replace(&text, "").to_string();
    }

    // If there's real text remaining, the user intended to interact with the bot
    let remaining = text.trim();
    if !remaining.is_empty() {
        return false;
    }

    // Truly implicit: reply to bot with nothing but @mentions
    true
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

    let data: SingleUserResponse =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {}", e))?;

    data.data
        .ok_or_else(|| "User not found".to_string())
}

/// Fetch thread context for a tweet that's part of a conversation.
/// Returns formatted thread context string, or None on any error (graceful degradation).
async fn fetch_thread_context(
    client: &reqwest::Client,
    config: &TwitterConfig,
    conversation_id: &str,
    current_tweet_id: &str,
) -> Option<String> {
    let url = format!("{}/tweets/search/recent", TWITTER_API_BASE);

    let query = format!("conversation_id:{}", conversation_id);
    let max_results = MAX_THREAD_TWEETS.min(100).to_string();

    let params: Vec<(&str, &str)> = vec![
        ("query", &query),
        ("expansions", "author_id"),
        ("user.fields", "username"),
        ("max_results", &max_results),
    ];

    // Build full URL with query string
    let query_string: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let full_url = format!("{}?{}", url, query_string);

    // Generate OAuth header (params must be included in signature)
    let auth_header = generate_oauth_header("GET", &url, &config.credentials, Some(&params));

    let response = match client
        .get(&full_url)
        .header("Authorization", auth_header)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::warn!("Twitter: Failed to fetch thread context: {}", e);
            return None;
        }
    };

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        log::warn!(
            "Twitter: Thread context API error ({}): {}",
            status,
            if body.len() > 200 { &body[..200] } else { &body }
        );
        return None;
    }

    let data: ThreadSearchResponse = match serde_json::from_str(&body) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Twitter: Failed to parse thread context response: {}", e);
            return None;
        }
    };

    let tweets = data.data?;
    if tweets.is_empty() {
        return None;
    }

    // Build author_id → username map from includes
    let user_map: std::collections::HashMap<String, String> = data
        .includes
        .and_then(|inc| inc.users)
        .unwrap_or_default()
        .into_iter()
        .map(|u| (u.id, u.username))
        .collect();

    // API returns newest-first; reverse for chronological order
    let mut thread_tweets: Vec<_> = tweets
        .into_iter()
        .filter(|t| t.id != current_tweet_id) // exclude the current tweet
        .collect();
    thread_tweets.reverse();

    if thread_tweets.is_empty() {
        return None;
    }

    // Build thread context string, respecting character limit
    let mut context_lines = Vec::new();
    let mut total_chars = 0;
    let mut truncated = false;

    for tweet in &thread_tweets {
        let username = tweet
            .author_id
            .as_ref()
            .and_then(|aid| user_map.get(aid))
            .map(|u| u.as_str())
            .unwrap_or("unknown");
        let line = format!("@{}: {}", username, tweet.text);

        if total_chars + line.len() > MAX_THREAD_CONTEXT_CHARS {
            truncated = true;
            break;
        }
        total_chars += line.len();
        context_lines.push(line);
    }

    if context_lines.is_empty() {
        return None;
    }

    let mut result = context_lines.join("\n");
    if truncated {
        result.push_str("\n[thread truncated]");
    }

    Some(result)
}

/// Extract command text from a tweet, removing @mentions
fn extract_command_text(text: &str, bot_mention_regex: &Regex) -> String {
    // Remove @bot_handle (case-insensitive) and any other @mentions at the start
    let mut result = text.to_string();

    // Remove our bot's mention using pre-compiled regex
    result = bot_mention_regex.replace_all(&result, "").to_string();

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
    force_safe_mode: bool,
    dispatcher: &Arc<MessageDispatcher>,
    broadcaster: &Arc<EventBroadcaster>,
    bot_mention_regex: &Regex,
    client: &reqwest::Client,
) -> Option<String> {
    // Extract the actual command/message text
    let command_text = extract_command_text(&tweet.text, bot_mention_regex);

    if command_text.is_empty() {
        log::debug!("Twitter: Empty command after extracting text, ignoring");
        return None;
    }

    // Fetch thread context if this tweet is part of a conversation
    let thread_context = match &tweet.conversation_id {
        Some(conv_id) if conv_id != &tweet.id => {
            log::info!(
                "Twitter: Tweet {} is part of conversation {}, fetching thread context",
                tweet.id, conv_id
            );
            fetch_thread_context(client, config, conv_id, &tweet.id).await
        }
        _ => None,
    };

    // Add source hint to help the agent understand the context.
    // Only warn about 280-char limit when we positively know it's a free/basic account.
    let char_hint = match config.subscription_tier {
        XSubscriptionTier::None | XSubscriptionTier::Basic => {
            "Keep response under 280 chars or it will be threaded. Do NOT mention character limits in your response."
        }
        _ => {
            "Keep response concise, around 600-800 characters. Do NOT mention character limits in your response."
        }
    };

    let text_with_hint = if let Some(ref ctx) = thread_context {
        format!(
            "[TWITTER MENTION from @{} - {}. Reply using say_to_user — your LAST say_to_user message becomes the tweet. It MUST contain the actual answer/content, NOT a meta-summary like 'I summarized X'. Put real data in it.]\n\n[THREAD CONTEXT - conversation history:]\n{}\n\n[POST DIRECTED TO YOU - YOU ARE REPLYING TO THIS POST:]\n@{}: {}",
            author_username, char_hint, ctx, author_username, command_text
        )
    } else {
        format!(
            "[TWITTER MENTION from @{} - {}. Reply using say_to_user — your LAST say_to_user message becomes the tweet. It MUST contain the actual answer/content, NOT a meta-summary like 'I summarized X'. Put real data in it.]\n\n{}",
            author_username, char_hint, command_text
        )
    };

    // Create normalized message for dispatcher
    let normalized = NormalizedMessage {
        channel_id,
        channel_type: ChannelType::Twitter.to_string(),
        // Each mention gets its own session (tweet ID, not conversation ID).
        // This prevents non-admin mentions from poisoning an admin session's safe mode flag.
        chat_id: tweet.id.clone(),
        chat_name: None,
        user_id: tweet.author_id.clone(),
        user_name: author_username.to_string(),
        text: text_with_hint,
        message_id: Some(tweet.id.clone()),
        session_mode: None,
        selected_network: None,
        force_safe_mode,
    };

    // Subscribe to events to capture say_to_user messages.
    // Unlike Discord/Telegram which forward events in real-time via WebSocket,
    // Twitter is polling-based and needs to capture the message for post_reply().
    let (client_id, mut event_rx) = broadcaster.subscribe();

    // Collect say_to_user messages from broadcast events in a background task
    let say_to_user_messages: Arc<tokio::sync::Mutex<Vec<String>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let messages_clone = say_to_user_messages.clone();
    let event_channel_id = channel_id;
    let event_task = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            // Only capture events for this channel
            let ev_channel = event.data.get("channel_id").and_then(|v| v.as_i64());
            if ev_channel != Some(event_channel_id) {
                continue;
            }
            // Capture say_to_user tool results
            if event.event == "tool.result" {
                let tool_name = event.data.get("tool_name").and_then(|v| v.as_str());
                let success = event.data.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                let content = event.data.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if tool_name == Some("say_to_user") && success && !content.is_empty() {
                    messages_clone.lock().await.push(content.to_string());
                }
            }
        }
    });

    // Dispatch to AI
    log::info!("Twitter: Dispatching message to AI for @{}", author_username);
    let result = dispatcher.dispatch_safe(normalized).await;

    // Unsubscribe prevents new events; give the capture task a moment to drain
    // any events already in the mpsc buffer (avoids race where say_to_user event
    // is buffered but not yet processed when we check).
    broadcaster.unsubscribe(&client_id);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    event_task.abort();

    log::info!(
        "Twitter: Dispatch complete for @{}, error={:?}",
        author_username,
        result.error
    );

    // Priority: say_to_user messages are the actual tweet content the AI composed.
    // result.response is often just a task_fully_completed summary, not tweet-worthy.
    let captured = say_to_user_messages.lock().await;
    if !captured.is_empty() {
        let last = captured.last().unwrap().clone();
        log::info!("Twitter: Using say_to_user message ({} chars, {} total captured)", last.len(), captured.len());
        Some(last)
    } else if let Some(error) = result.error {
        // Never tweet internal errors (loop detection, AI failures, etc.)
        log::warn!(
            "Twitter: Suppressing error response for @{}: {}",
            author_username,
            error
        );
        None
    } else {
        // Never fall back to result.response — it contains internal tool logs,
        // task summaries, and raw AI text not intended for public tweets.
        if !result.response.is_empty() {
            log::warn!(
                "Twitter: No say_to_user captured for @{}, suppressing result.response ({} chars)",
                author_username,
                result.response.len()
            );
        } else {
            log::warn!("Twitter: No response from dispatch and no say_to_user events for @{}", author_username);
        }
        None
    }
}

/// Split a response into tweet-sized chunks for threading
fn split_for_twitter(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
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
            let max_chunk_chars = max_chars - 5;

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
        if !current_chunk.is_empty() && current_chunk.chars().count() < max_chars - 5 {
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
    let chunks = split_for_twitter(text, config.max_chars());
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
        let re = Regex::new(r"(?i)@starkbot").unwrap();
        // Basic case
        assert_eq!(
            extract_command_text("@starkbot hello world", &re),
            "hello world"
        );
        // Mixed case - should be case-insensitive
        assert_eq!(
            extract_command_text("@StarkBot what's the price?", &re),
            "what's the price?"
        );
        // Fully uppercase
        assert_eq!(
            extract_command_text("@STARKBOT test message", &re),
            "test message"
        );
        // Weird mixed case
        assert_eq!(
            extract_command_text("@StArKbOt random case", &re),
            "random case"
        );
        // Multiple mentions with our bot
        assert_eq!(
            extract_command_text("@user1 @starkbot help me", &re),
            "help me"
        );
        // Bot mention in middle of text (should be removed, may leave extra space)
        assert_eq!(
            extract_command_text("Hey @StarkBot can you help?", &re),
            "Hey  can you help?"
        );
        // Just the mention, no text
        assert_eq!(
            extract_command_text("@starkbot", &re),
            ""
        );
    }

    #[test]
    fn test_split_for_twitter() {
        // Short message - no split
        let short = "Hello world!";
        assert_eq!(split_for_twitter(short, TWITTER_MAX_CHARS), vec!["Hello world!"]);

        // Long message - should split at 280
        let long = "a ".repeat(200);
        let chunks = split_for_twitter(&long, TWITTER_MAX_CHARS);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= TWITTER_MAX_CHARS);
        }

        // Premium mode - same long message should NOT split at 25k limit
        let chunks_pro = split_for_twitter(&long, TWITTER_PREMIUM_MAX_CHARS);
        assert_eq!(chunks_pro.len(), 1);
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

    #[test]
    fn test_thread_context_detection() {
        // Standalone tweet: conversation_id == tweet.id → no thread
        let standalone = Tweet {
            id: "100".to_string(),
            text: "@bot hello".to_string(),
            author_id: "200".to_string(),
            conversation_id: Some("100".to_string()),
            in_reply_to_user_id: None,
            referenced_tweets: None,
        };
        let is_thread = match &standalone.conversation_id {
            Some(conv_id) if conv_id != &standalone.id => true,
            _ => false,
        };
        assert!(!is_thread, "Standalone tweet should not be detected as thread");

        // Thread reply: conversation_id != tweet.id → is thread
        let thread_reply = Tweet {
            id: "105".to_string(),
            text: "@bot what do you think?".to_string(),
            author_id: "200".to_string(),
            conversation_id: Some("100".to_string()),
            in_reply_to_user_id: Some("300".to_string()),
            referenced_tweets: Some(vec![ReferencedTweet {
                ref_type: "replied_to".to_string(),
                id: "104".to_string(),
            }]),
        };
        let is_thread = match &thread_reply.conversation_id {
            Some(conv_id) if conv_id != &thread_reply.id => true,
            _ => false,
        };
        assert!(is_thread, "Thread reply should be detected as thread");

        // No conversation_id at all → not a thread
        let no_conv = Tweet {
            id: "110".to_string(),
            text: "@bot hi".to_string(),
            author_id: "200".to_string(),
            conversation_id: None,
            in_reply_to_user_id: None,
            referenced_tweets: None,
        };
        let is_thread = match &no_conv.conversation_id {
            Some(conv_id) if conv_id != &no_conv.id => true,
            _ => false,
        };
        assert!(!is_thread, "Tweet without conversation_id should not be thread");
    }

    #[test]
    fn test_is_implicit_reply_to_bot() {
        let re = Regex::new(r"(?i)@starkbot").unwrap();
        let config = TwitterConfig {
            bot_handle: "starkbot".to_string(),
            bot_user_id: "999".to_string(),
            poll_interval_secs: 120,
            subscription_tier: XSubscriptionTier::None,
            reply_chance: 100,
            max_mentions_per_hour: 0,
            admin_user_id: None,
            credentials: TwitterCredentials::new(
                "k".to_string(), "s".to_string(), "t".to_string(), "ts".to_string(),
            ),
        };

        // Reply to bot with only auto-prepended mention (no real text) → implicit, should skip
        let implicit = Tweet {
            id: "1".to_string(),
            text: "@starkbot".to_string(),
            author_id: "200".to_string(),
            conversation_id: Some("50".to_string()),
            in_reply_to_user_id: Some("999".to_string()),
            referenced_tweets: None,
        };
        assert!(is_implicit_reply_to_bot(&implicit, &config, &re));

        // Reply to bot with auto-prepended mention + real text → NOT implicit, should process
        let reply_with_text = Tweet {
            id: "1b".to_string(),
            text: "@starkbot thanks for the info!".to_string(),
            author_id: "200".to_string(),
            conversation_id: Some("50".to_string()),
            in_reply_to_user_id: Some("999".to_string()),
            referenced_tweets: None,
        };
        assert!(!is_implicit_reply_to_bot(&reply_with_text, &config, &re));

        // Reply to bot with explicit @starkbot in body → NOT implicit, should process
        let explicit = Tweet {
            id: "2".to_string(),
            text: "@starkbot hey @starkbot what about tomorrow?".to_string(),
            author_id: "200".to_string(),
            conversation_id: Some("50".to_string()),
            in_reply_to_user_id: Some("999".to_string()),
            referenced_tweets: None,
        };
        assert!(!is_implicit_reply_to_bot(&explicit, &config, &re));

        // Reply to someone else (not the bot) → not implicit, should process
        let reply_to_other = Tweet {
            id: "3".to_string(),
            text: "@otheruser @starkbot what do you think?".to_string(),
            author_id: "200".to_string(),
            conversation_id: Some("50".to_string()),
            in_reply_to_user_id: Some("300".to_string()),
            referenced_tweets: None,
        };
        assert!(!is_implicit_reply_to_bot(&reply_to_other, &config, &re));

        // Direct mention (not a reply at all) → not implicit, should process
        let direct = Tweet {
            id: "4".to_string(),
            text: "@starkbot what's the price of ETH?".to_string(),
            author_id: "200".to_string(),
            conversation_id: Some("4".to_string()),
            in_reply_to_user_id: None,
            referenced_tweets: None,
        };
        assert!(!is_implicit_reply_to_bot(&direct, &config, &re));

        // Reply to bot in thread with multiple auto-prepended mentions but real text → NOT implicit
        let thread_reply_with_text = Tweet {
            id: "5".to_string(),
            text: "@starkbot @alice I agree with alice".to_string(),
            author_id: "200".to_string(),
            conversation_id: Some("50".to_string()),
            in_reply_to_user_id: Some("999".to_string()),
            referenced_tweets: None,
        };
        assert!(!is_implicit_reply_to_bot(&thread_reply_with_text, &config, &re));

        // Reply to bot in thread with only auto-prepended mentions, no real text → implicit
        let thread_reply_mentions_only = Tweet {
            id: "5b".to_string(),
            text: "@starkbot @alice".to_string(),
            author_id: "200".to_string(),
            conversation_id: Some("50".to_string()),
            in_reply_to_user_id: Some("999".to_string()),
            referenced_tweets: None,
        };
        assert!(is_implicit_reply_to_bot(&thread_reply_mentions_only, &config, &re));
    }

    #[test]
    fn test_thread_context_hint_format() {
        let re = Regex::new(r"(?i)@starkbot").unwrap();
        let command_text = extract_command_text("@starkbot what do you think?", &re);

        let author_username = "charlie";
        let char_hint = "Keep response under 280 chars or it will be threaded";

        // With thread context
        let thread_ctx = "@alice: Has anyone tried the new DeFi protocol?\n@bob: Yeah it looks promising but the TVL is low";

        let hint_with_thread = format!(
            "[TWITTER MENTION from @{} - {}. Reply using say_to_user — your LAST say_to_user message becomes the tweet. It MUST contain the actual answer/content, NOT a meta-summary like 'I summarized X'. Put real data in it.]\n\n[THREAD CONTEXT - conversation history:]\n{}\n\n[POST DIRECTED TO YOU - YOU ARE REPLYING TO THIS POST:]\n@{}: {}",
            author_username, char_hint, thread_ctx, author_username, command_text
        );

        assert!(hint_with_thread.contains("[THREAD CONTEXT - conversation history:]"));
        assert!(hint_with_thread.contains("@alice: Has anyone tried"));
        assert!(hint_with_thread.contains("@bob: Yeah it looks promising"));
        assert!(hint_with_thread.contains("[POST DIRECTED TO YOU - YOU ARE REPLYING TO THIS POST:]"));
        assert!(hint_with_thread.contains("@charlie: what do you think?"));

        // Without thread context (standalone)
        let hint_standalone = format!(
            "[TWITTER MENTION from @{} - {}. Reply using say_to_user — your LAST say_to_user message becomes the tweet. It MUST contain the actual answer/content, NOT a meta-summary like 'I summarized X'. Put real data in it.]\n\n{}",
            author_username, char_hint, command_text
        );

        assert!(!hint_standalone.contains("[THREAD CONTEXT"));
        assert!(hint_standalone.contains("what do you think?"));
    }
}
