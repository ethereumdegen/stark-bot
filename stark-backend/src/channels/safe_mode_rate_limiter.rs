//! Rate limiter for safe mode channel creation and queries
//!
//! Ensures max 1 safe mode channel is created per second globally,
//! AND limits each user to X queries per 10 minutes (configurable in Bot Settings).

use chrono::{DateTime, Duration, Utc};

/// Result of a successful safe mode query rate limit check
#[derive(Debug, Clone)]
pub struct SafeModeQueryResult {
    /// Number of queries used in the current 10-minute window
    pub queries_used: usize,
    /// Number of queries remaining in the current window
    pub queries_remaining: usize,
    /// Total limit per 10 minutes
    pub limit: usize,
}
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;
use tokio::sync::oneshot;

use crate::db::Database;
use crate::models::Channel;

/// Minimum interval between safe mode channel creations (1 second)
const MIN_CREATION_INTERVAL_MS: u64 = 1000;

/// Maximum queue size for pending channel creations
const MAX_QUEUE_SIZE: usize = 50;

/// Time window for per-user rate limiting (10 minutes)
const USER_RATE_LIMIT_WINDOW_MINS: i64 = 10;

/// Request for creating a safe mode channel
#[derive(Debug)]
struct ChannelCreationRequest {
    channel_type: String,
    name: String,
    bot_token: String,
    app_token: Option<String>,
    user_id: String,
    platform: String,
    /// Sender to notify when channel is created (or error)
    response_tx: oneshot::Sender<Result<Channel, String>>,
}

/// Tracks query timestamps for a single user
#[derive(Debug, Clone)]
struct UserQueryHistory {
    /// Timestamps of queries within the rate limit window
    query_times: Vec<DateTime<Utc>>,
    /// Platform (discord, telegram, twitter, etc.)
    platform: String,
}

impl UserQueryHistory {
    fn new(platform: &str) -> Self {
        Self {
            query_times: Vec::new(),
            platform: platform.to_string(),
        }
    }

    /// Clean up old entries and return count of queries within window
    fn count_recent_queries(&mut self) -> usize {
        let cutoff = Utc::now() - Duration::minutes(USER_RATE_LIMIT_WINDOW_MINS);
        self.query_times.retain(|t| *t > cutoff);
        self.query_times.len()
    }

    /// Record a new query
    fn record_query(&mut self) {
        self.query_times.push(Utc::now());
    }
}

/// Internal state for the rate limiter
#[derive(Debug)]
struct RateLimiterState {
    /// Queue of pending channel creation requests
    queue: VecDeque<ChannelCreationRequest>,
    /// When the last safe mode channel was created (global rate limit)
    last_creation_time: Option<DateTime<Utc>>,
    /// Whether the processor task is running
    processor_running: bool,
    /// Per-user query history: key is "platform:user_id"
    user_histories: HashMap<String, UserQueryHistory>,
}

impl Default for RateLimiterState {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            last_creation_time: None,
            processor_running: false,
            user_histories: HashMap::new(),
        }
    }
}

/// Rate limiter for safe mode channel creation
///
/// Enforces two rate limits:
/// 1. Global: Max 1 channel creation per second
/// 2. Per-user: Max X queries per 10 minutes (configurable in Bot Settings)
#[derive(Clone)]
pub struct SafeModeChannelRateLimiter {
    db: Arc<Database>,
    state: Arc<Mutex<RateLimiterState>>,
}

impl SafeModeChannelRateLimiter {
    /// Create a new rate limiter
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            state: Arc::new(Mutex::new(RateLimiterState::default())),
        }
    }

    /// Get the per-user query limit from bot_config.ron
    fn get_user_query_limit(&self) -> i32 {
        crate::models::BotConfig::load().safe_mode_max_queries_per_10min
    }

    /// Queue a safe mode channel creation request
    ///
    /// Returns immediately if the channel can be created now (rate limit not hit),
    /// otherwise queues the request and returns when it's processed.
    ///
    /// # Arguments
    /// * `channel_type` - Type of channel (discord, telegram, twitter, etc.)
    /// * `name` - Channel name
    /// * `bot_token` - Bot authentication token
    /// * `app_token` - Optional app token (for Slack)
    /// * `user_id` - Platform-specific user ID (Discord snowflake, Telegram ID, etc.)
    /// * `platform` - Platform name for logging
    pub async fn create_safe_mode_channel(
        &self,
        channel_type: &str,
        name: &str,
        bot_token: &str,
        app_token: Option<&str>,
        user_id: &str,
        platform: &str,
    ) -> Result<Channel, String> {
        // Check per-user rate limit BEFORE queueing
        let user_key = format!("{}:{}", platform, user_id);
        let user_limit = self.get_user_query_limit();

        {
            let mut state = self.state.lock().unwrap();

            let history = state.user_histories
                .entry(user_key.clone())
                .or_insert_with(|| UserQueryHistory::new(platform));

            let recent_count = history.count_recent_queries();

            if recent_count >= user_limit as usize {
                let oldest_query = history.query_times.first().cloned();
                let reset_time = oldest_query
                    .map(|t| t + Duration::minutes(USER_RATE_LIMIT_WINDOW_MINS))
                    .map(|t| (t - Utc::now()).num_seconds().max(0))
                    .unwrap_or(0);

                log::warn!(
                    "[SAFE_MODE_RATE_LIMIT] User {} on {} exceeded rate limit ({}/{} queries in 10 min)",
                    user_id, platform, recent_count, user_limit
                );

                return Err(format!(
                    "Rate limit exceeded: You've made {} queries in the last 10 minutes (max {}). Try again in {} seconds.",
                    recent_count, user_limit, reset_time
                ));
            }

            // Record this query attempt
            history.record_query();

            log::info!(
                "[SAFE_MODE_RATE_LIMIT] User {} on {} query count: {}/{}",
                user_id, platform, recent_count + 1, user_limit
            );
        }

        let (response_tx, response_rx) = oneshot::channel();

        let should_process_now = {
            let mut state = self.state.lock().unwrap();

            // Check queue size limit
            if state.queue.len() >= MAX_QUEUE_SIZE {
                return Err(format!(
                    "Safe mode channel queue full ({}/{}). Please try again later.",
                    state.queue.len(),
                    MAX_QUEUE_SIZE
                ));
            }

            // Check if we can process immediately (global rate limit)
            let can_process_now = self.can_create_now(&state);

            if can_process_now && state.queue.is_empty() {
                // Update last creation time and process immediately
                state.last_creation_time = Some(Utc::now());
                true
            } else {
                // Add to queue
                state.queue.push_back(ChannelCreationRequest {
                    channel_type: channel_type.to_string(),
                    name: name.to_string(),
                    bot_token: bot_token.to_string(),
                    app_token: app_token.map(|s| s.to_string()),
                    user_id: user_id.to_string(),
                    platform: platform.to_string(),
                    response_tx,
                });

                let queue_pos = state.queue.len();
                log::info!(
                    "[SAFE_MODE_RATE_LIMIT] Queued channel creation for user {} on {}, position: {}",
                    user_id, platform, queue_pos
                );

                // Start processor if not running
                if !state.processor_running {
                    state.processor_running = true;
                    self.spawn_queue_processor();
                }

                false
            }
        };

        if should_process_now {
            // Create channel directly
            log::info!(
                "[SAFE_MODE_RATE_LIMIT] Creating channel '{}' immediately for user {} on {}",
                name, user_id, platform
            );
            return self.db.create_safe_mode_channel(
                channel_type,
                name,
                bot_token,
                app_token,
            );
        }

        // Wait for queued response
        response_rx.await.map_err(|_| "Channel creation request cancelled".to_string())?
    }

    /// Check if we can create a channel now based on global rate limit
    fn can_create_now(&self, state: &RateLimiterState) -> bool {
        match state.last_creation_time {
            None => true,
            Some(last_time) => {
                let elapsed = Utc::now() - last_time;
                elapsed.num_milliseconds() >= MIN_CREATION_INTERVAL_MS as i64
            }
        }
    }

    /// Spawn the background queue processor
    fn spawn_queue_processor(&self) {
        let limiter = self.clone();

        tokio::spawn(async move {
            limiter.process_queue().await;
        });
    }

    /// Process the queue, creating channels at most once per second
    async fn process_queue(&self) {
        log::info!("[SAFE_MODE_RATE_LIMIT] Queue processor started");

        loop {
            // Calculate wait time (if needed) without holding lock across await
            let wait_ms = {
                let state = self.state.lock().unwrap();

                if state.queue.is_empty() {
                    // Will stop after releasing lock
                    0
                } else {
                    match state.last_creation_time {
                        None => 0,
                        Some(last_time) => {
                            let elapsed = Utc::now() - last_time;
                            let elapsed_ms = elapsed.num_milliseconds().max(0) as u64;
                            if elapsed_ms >= MIN_CREATION_INTERVAL_MS {
                                0
                            } else {
                                MIN_CREATION_INTERVAL_MS - elapsed_ms
                            }
                        }
                    }
                }
            };

            // Sleep outside of lock if needed
            if wait_ms > 0 {
                tokio::time::sleep(StdDuration::from_millis(wait_ms)).await;
            }

            // Now get the request (separate lock scope)
            let request = {
                let mut state = self.state.lock().unwrap();

                if state.queue.is_empty() {
                    state.processor_running = false;
                    log::info!("[SAFE_MODE_RATE_LIMIT] Queue empty, processor stopping");
                    return;
                }

                state.last_creation_time = Some(Utc::now());
                state.queue.pop_front()
            };

            // Process the request outside of lock
            if let Some(req) = request {
                log::info!(
                    "[SAFE_MODE_RATE_LIMIT] Processing queued channel creation for user {} on {}",
                    req.user_id, req.platform
                );

                let result = self.db.create_safe_mode_channel(
                    &req.channel_type,
                    &req.name,
                    &req.bot_token,
                    req.app_token.as_deref(),
                );

                // Send result (ignore if receiver dropped)
                let _ = req.response_tx.send(result);
            }
        }
    }

    /// Get current queue length
    pub fn queue_len(&self) -> usize {
        self.state.lock().unwrap().queue.len()
    }

    /// Check if rate limiter is currently processing
    pub fn is_processing(&self) -> bool {
        self.state.lock().unwrap().processor_running
    }

    /// Get time until next slot is available (in milliseconds)
    pub fn time_until_available_ms(&self) -> u64 {
        let state = self.state.lock().unwrap();
        match state.last_creation_time {
            None => 0,
            Some(last_time) => {
                let elapsed = Utc::now() - last_time;
                let elapsed_ms = elapsed.num_milliseconds().max(0) as u64;
                if elapsed_ms >= MIN_CREATION_INTERVAL_MS {
                    0
                } else {
                    MIN_CREATION_INTERVAL_MS - elapsed_ms
                }
            }
        }
    }

    /// Get user's remaining queries in the current 10-minute window
    pub fn get_user_remaining_queries(&self, user_id: &str, platform: &str) -> (usize, i32) {
        let user_key = format!("{}:{}", platform, user_id);
        let limit = self.get_user_query_limit();

        let mut state = self.state.lock().unwrap();

        let used = state.user_histories
            .entry(user_key)
            .or_insert_with(|| UserQueryHistory::new(platform))
            .count_recent_queries();

        let remaining = (limit as usize).saturating_sub(used);
        (remaining, limit)
    }

    /// Check if a user can make a safe mode query and record it if allowed
    ///
    /// Returns Ok(remaining_queries) if allowed, Err(message) if rate limited.
    /// This is used for rate limiting queries (not channel creation).
    pub fn check_and_record_query(&self, user_id: &str, platform: &str) -> Result<SafeModeQueryResult, String> {
        let user_key = format!("{}:{}", platform, user_id);
        let user_limit = self.get_user_query_limit();

        let mut state = self.state.lock().unwrap();

        let history = state.user_histories
            .entry(user_key)
            .or_insert_with(|| UserQueryHistory::new(platform));

        let recent_count = history.count_recent_queries();

        if recent_count >= user_limit as usize {
            let oldest_query = history.query_times.first().cloned();
            let reset_seconds = oldest_query
                .map(|t| t + Duration::minutes(USER_RATE_LIMIT_WINDOW_MINS))
                .map(|t| (t - Utc::now()).num_seconds().max(0))
                .unwrap_or(0);

            log::warn!(
                "[SAFE_MODE_RATE_LIMIT] User {} on {} exceeded query rate limit ({}/{} in 10 min)",
                user_id, platform, recent_count, user_limit
            );

            return Err(format!(
                "You've reached the query limit ({} queries per 10 minutes). Please try again in {} seconds.",
                user_limit, reset_seconds
            ));
        }

        // Record this query
        history.record_query();
        let queries_used = recent_count + 1;
        let queries_remaining = (user_limit as usize).saturating_sub(queries_used);

        log::info!(
            "[SAFE_MODE_RATE_LIMIT] User {} on {} query {}/{} (remaining: {})",
            user_id, platform, queries_used, user_limit, queries_remaining
        );

        Ok(SafeModeQueryResult {
            queries_used,
            queries_remaining,
            limit: user_limit as usize,
        })
    }

    /// Clean up old user histories (call periodically)
    pub fn cleanup_old_histories(&self) {
        let mut state = self.state.lock().unwrap();
        let cutoff = Utc::now() - Duration::minutes(USER_RATE_LIMIT_WINDOW_MINS * 2);

        state.user_histories.retain(|_, history| {
            // Keep if any query is recent
            history.query_times.iter().any(|t| *t > cutoff)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_state_default() {
        let state = RateLimiterState::default();
        assert!(state.queue.is_empty());
        assert!(state.last_creation_time.is_none());
        assert!(!state.processor_running);
        assert!(state.user_histories.is_empty());
    }

    #[test]
    fn test_user_query_history_basic() {
        let mut history = UserQueryHistory::new("discord");
        assert_eq!(history.count_recent_queries(), 0);
        assert_eq!(history.platform, "discord");

        history.record_query();
        assert_eq!(history.count_recent_queries(), 1);

        history.record_query();
        history.record_query();
        assert_eq!(history.count_recent_queries(), 3);
    }

    #[test]
    fn test_user_key_format() {
        // Verify user keys are formatted consistently
        let platform = "discord";
        let user_id = "123456789";
        let expected = "discord:123456789";
        assert_eq!(format!("{}:{}", platform, user_id), expected);
    }

    #[test]
    fn test_safe_mode_query_result() {
        let result = SafeModeQueryResult {
            queries_used: 3,
            queries_remaining: 2,
            limit: 5,
        };
        assert_eq!(result.queries_used, 3);
        assert_eq!(result.queries_remaining, 2);
        assert_eq!(result.limit, 5);
    }

    #[test]
    fn test_rate_limiter_with_temp_db() {
        // Create a temporary in-memory database for testing
        let db = Arc::new(Database::new(":memory:").expect("Failed to create test db"));
        let limiter = SafeModeChannelRateLimiter::new(db);

        // Test check_and_record_query
        let result = limiter.check_and_record_query("user123", "discord");
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.queries_used, 1);
        assert_eq!(info.limit, 5); // default limit

        // Record more queries
        for _ in 0..3 {
            let _ = limiter.check_and_record_query("user123", "discord");
        }

        // Should still allow one more (5th query)
        let result = limiter.check_and_record_query("user123", "discord");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().queries_used, 5);

        // 6th query should fail
        let result = limiter.check_and_record_query("user123", "discord");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("reached the query limit"));
    }

    #[test]
    fn test_per_user_isolation() {
        let db = Arc::new(Database::new(":memory:").expect("Failed to create test db"));
        let limiter = SafeModeChannelRateLimiter::new(db);

        // User A makes queries
        for _ in 0..5 {
            let _ = limiter.check_and_record_query("userA", "discord");
        }

        // User A should be rate limited
        assert!(limiter.check_and_record_query("userA", "discord").is_err());

        // User B should NOT be rate limited
        let result = limiter.check_and_record_query("userB", "discord");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().queries_used, 1);
    }

    #[test]
    fn test_cross_platform_isolation() {
        let db = Arc::new(Database::new(":memory:").expect("Failed to create test db"));
        let limiter = SafeModeChannelRateLimiter::new(db);

        // Same user ID on different platforms should be tracked separately
        for _ in 0..5 {
            let _ = limiter.check_and_record_query("user123", "discord");
        }

        // Discord user123 is rate limited
        assert!(limiter.check_and_record_query("user123", "discord").is_err());

        // Twitter user123 is NOT rate limited (different platform)
        let result = limiter.check_and_record_query("user123", "twitter");
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_remaining_queries() {
        let db = Arc::new(Database::new(":memory:").expect("Failed to create test db"));
        let limiter = SafeModeChannelRateLimiter::new(db);

        // Initial state
        let (remaining, limit) = limiter.get_user_remaining_queries("user1", "discord");
        assert_eq!(remaining, 5);
        assert_eq!(limit, 5);

        // After 2 queries
        let _ = limiter.check_and_record_query("user1", "discord");
        let _ = limiter.check_and_record_query("user1", "discord");
        let (remaining, _) = limiter.get_user_remaining_queries("user1", "discord");
        assert_eq!(remaining, 3);
    }

    #[test]
    fn test_queue_status() {
        let db = Arc::new(Database::new(":memory:").expect("Failed to create test db"));
        let limiter = SafeModeChannelRateLimiter::new(db);

        assert_eq!(limiter.queue_len(), 0);
        assert!(!limiter.is_processing());
    }
}
