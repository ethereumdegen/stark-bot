use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Session scope determines the context type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionScope {
    Dm,
    Group,
    Cron,
    Webhook,
    Api,
}

impl SessionScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionScope::Dm => "dm",
            SessionScope::Group => "group",
            SessionScope::Cron => "cron",
            SessionScope::Webhook => "webhook",
            SessionScope::Api => "api",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "dm" => Some(SessionScope::Dm),
            "group" => Some(SessionScope::Group),
            "cron" => Some(SessionScope::Cron),
            "webhook" => Some(SessionScope::Webhook),
            "api" => Some(SessionScope::Api),
            _ => None,
        }
    }
}

impl Default for SessionScope {
    fn default() -> Self {
        SessionScope::Dm
    }
}

/// Reset policy determines when a session should be reset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResetPolicy {
    Daily,
    Idle,
    Manual,
    Never,
}

impl ResetPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResetPolicy::Daily => "daily",
            ResetPolicy::Idle => "idle",
            ResetPolicy::Manual => "manual",
            ResetPolicy::Never => "never",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "daily" => Some(ResetPolicy::Daily),
            "idle" => Some(ResetPolicy::Idle),
            "manual" => Some(ResetPolicy::Manual),
            "never" => Some(ResetPolicy::Never),
            _ => None,
        }
    }
}

impl Default for ResetPolicy {
    fn default() -> Self {
        ResetPolicy::Daily
    }
}

/// Chat session - conversation context container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: i64,
    pub session_key: String,
    pub agent_id: Option<String>,
    pub scope: SessionScope,
    pub channel_type: String,
    pub channel_id: i64,
    pub platform_chat_id: String,
    pub is_active: bool,
    pub reset_policy: ResetPolicy,
    pub idle_timeout_minutes: Option<i32>,
    pub daily_reset_hour: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Request to get or create a chat session
#[derive(Debug, Clone, Deserialize)]
pub struct GetOrCreateSessionRequest {
    pub channel_type: String,
    pub channel_id: i64,
    pub platform_chat_id: String,
    #[serde(default)]
    pub scope: Option<SessionScope>,
    pub agent_id: Option<String>,
}

/// Request to update session reset policy
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateResetPolicyRequest {
    pub reset_policy: ResetPolicy,
    pub idle_timeout_minutes: Option<i32>,
    pub daily_reset_hour: Option<i32>,
}

/// Chat session response for API
#[derive(Debug, Clone, Serialize)]
pub struct ChatSessionResponse {
    pub id: i64,
    pub session_key: String,
    pub agent_id: Option<String>,
    pub scope: SessionScope,
    pub channel_type: String,
    pub channel_id: i64,
    pub platform_chat_id: String,
    pub is_active: bool,
    pub reset_policy: ResetPolicy,
    pub idle_timeout_minutes: Option<i32>,
    pub daily_reset_hour: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub message_count: Option<i64>,
}

impl From<ChatSession> for ChatSessionResponse {
    fn from(session: ChatSession) -> Self {
        ChatSessionResponse {
            id: session.id,
            session_key: session.session_key,
            agent_id: session.agent_id,
            scope: session.scope,
            channel_type: session.channel_type,
            channel_id: session.channel_id,
            platform_chat_id: session.platform_chat_id,
            is_active: session.is_active,
            reset_policy: session.reset_policy,
            idle_timeout_minutes: session.idle_timeout_minutes,
            daily_reset_hour: session.daily_reset_hour,
            created_at: session.created_at,
            updated_at: session.updated_at,
            last_activity_at: session.last_activity_at,
            message_count: None,
        }
    }
}
