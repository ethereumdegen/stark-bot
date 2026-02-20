//! Shared types for the social monitor service and its RPC clients.

use serde::{Deserialize, Serialize};

// =====================================================
// Domain Types
// =====================================================

/// A monitored Twitter/X account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredAccount {
    pub id: i64,
    pub twitter_user_id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub monitor_enabled: bool,
    pub custom_keywords: Option<String>,
    pub notes: Option<String>,
    pub last_tweet_id: Option<String>,
    pub last_checked_at: Option<String>,
    pub total_tweets_captured: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// A captured tweet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedTweet {
    pub id: i64,
    pub account_id: i64,
    pub tweet_id: String,
    pub text: String,
    pub tweet_type: String,
    pub conversation_id: Option<String>,
    pub in_reply_to_user_id: Option<String>,
    pub like_count: i64,
    pub retweet_count: i64,
    pub reply_count: i64,
    pub quote_count: i64,
    pub tweeted_at: String,
    pub captured_at: String,
    pub processed: bool,
    pub raw_json: Option<String>,
}

/// An extracted topic from a tweet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TweetTopic {
    pub id: i64,
    pub tweet_id: i64,
    pub account_id: i64,
    pub topic: String,
    pub topic_type: String,
    pub raw_form: Option<String>,
}

/// Aggregated topic score per account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicScore {
    pub id: i64,
    pub account_id: i64,
    pub topic: String,
    pub mention_count_7d: i64,
    pub mention_count_30d: i64,
    pub mention_count_total: i64,
    pub trend: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub avg_engagement_score: f64,
    pub updated_at: String,
}

/// Sentiment snapshot for an account over a time window
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentSnapshot {
    pub id: i64,
    pub account_id: i64,
    pub window_start: String,
    pub window_end: String,
    pub sentiment_score: f64,
    pub sentiment_label: String,
    pub tweet_count: i64,
    pub top_topics_json: Option<String>,
    pub signals_json: Option<String>,
    pub ai_summary: Option<String>,
    pub created_at: String,
}

/// A tracked keyword in the global watchlist
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedKeyword {
    pub id: i64,
    pub keyword: String,
    pub category: Option<String>,
    pub aliases_json: Option<String>,
    pub created_at: String,
}

/// A detected signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub signal_type: String,
    pub description: String,
    pub account_id: i64,
    pub username: String,
    pub severity: String,
}

/// Full forensics report for an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountForensicsReport {
    pub account: MonitoredAccount,
    pub top_topics: Vec<TopicScore>,
    pub recent_sentiment: Vec<SentimentSnapshot>,
    pub signals: Vec<Signal>,
    pub tweet_count: i64,
    pub date_range: Option<(String, String)>,
}

// =====================================================
// Filter / Query Types
// =====================================================

/// Filters for querying captured tweets
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TweetFilter {
    pub account_id: Option<i64>,
    pub username: Option<String>,
    pub search_text: Option<String>,
    pub tweet_type: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<usize>,
}

/// Filters for querying topic scores
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TopicFilter {
    pub account_id: Option<i64>,
    pub topic: Option<String>,
    pub trend: Option<String>,
    pub min_mentions: Option<i64>,
    pub limit: Option<usize>,
}

/// Filters for querying sentiment history
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SentimentFilter {
    pub account_id: Option<i64>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<usize>,
}

/// Tweet statistics overview
#[derive(Debug, Serialize, Deserialize)]
pub struct TweetStats {
    pub total_tweets: i64,
    pub monitored_accounts: i64,
    pub active_accounts: i64,
    pub tweets_today: i64,
    pub tweets_7d: i64,
    pub unique_topics: i64,
}

// =====================================================
// RPC Request Types
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct AddAccountRequest {
    pub username: String,
    pub notes: Option<String>,
    pub custom_keywords: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveAccountRequest {
    pub id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateAccountRequest {
    pub id: i64,
    pub monitor_enabled: Option<bool>,
    pub custom_keywords: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddKeywordRequest {
    pub keyword: String,
    pub category: Option<String>,
    pub aliases: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveKeywordRequest {
    pub id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ForensicsReportRequest {
    pub account_id: Option<i64>,
    pub username: Option<String>,
}

// =====================================================
// RPC Response Types
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponse<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> RpcResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

// =====================================================
// Backup Types
// =====================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupAccount {
    pub username: String,
    pub display_name: Option<String>,
    pub twitter_user_id: String,
    pub monitor_enabled: bool,
    pub custom_keywords: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupKeyword {
    pub keyword: String,
    pub category: Option<String>,
    pub aliases_json: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupData {
    pub accounts: Vec<BackupAccount>,
    pub keywords: Vec<BackupKeyword>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupRestoreRequest {
    pub data: BackupData,
}

// =====================================================
// Service Status
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub running: bool,
    pub uptime_secs: u64,
    pub monitored_accounts: i64,
    pub active_accounts: i64,
    pub total_tweets: i64,
    pub unique_topics: i64,
    pub last_tick_at: Option<String>,
    pub poll_interval_secs: u64,
}
