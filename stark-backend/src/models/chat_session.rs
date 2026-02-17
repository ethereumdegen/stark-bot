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

/// Completion status of an agent session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompletionStatus {
    /// Session is active and can continue processing
    Active,
    /// Session completed successfully (task_fully_completed was called)
    Complete,
    /// Session was cancelled by user
    Cancelled,
    /// Session failed with an error
    Failed,
}

impl CompletionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CompletionStatus::Active => "active",
            CompletionStatus::Complete => "complete",
            CompletionStatus::Cancelled => "cancelled",
            CompletionStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "active" => Some(CompletionStatus::Active),
            "complete" => Some(CompletionStatus::Complete),
            "cancelled" | "canceled" => Some(CompletionStatus::Cancelled),
            "failed" => Some(CompletionStatus::Failed),
            _ => None,
        }
    }

    /// Check if the session should stop processing
    pub fn should_stop(&self) -> bool {
        matches!(self, CompletionStatus::Complete | CompletionStatus::Cancelled | CompletionStatus::Failed)
    }
}

impl Default for CompletionStatus {
    fn default() -> Self {
        CompletionStatus::Active
    }
}

impl std::fmt::Display for CompletionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
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
    // Context management fields
    /// Estimated token count of current context
    pub context_tokens: i32,
    /// Maximum context window size (default: 100000 for Claude)
    pub max_context_tokens: i32,
    /// DEPRECATED: Legacy compaction memory ID. No longer used - compaction summaries
    /// are now stored directly in `compaction_summary` column. Kept for migration compatibility.
    pub compaction_id: Option<i64>,
    /// Completion status of the session
    #[serde(default)]
    pub completion_status: CompletionStatus,
    /// Whether this session was used in safe mode context
    #[serde(default)]
    pub safe_mode: bool,
    /// Special role name if this safe-mode session has enriched permissions
    #[serde(default)]
    pub special_role_name: Option<String>,
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
    // Context management
    pub context_tokens: i32,
    pub max_context_tokens: i32,
    pub compaction_id: Option<i64>,
    // Completion status
    pub completion_status: CompletionStatus,
    // Initial query (first user message) - for web sessions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_query: Option<String>,
    // Safe mode - from the channel settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_mode: Option<bool>,
    // Special role name if this safe-mode session has enriched permissions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub special_role_name: Option<String>,
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
            context_tokens: session.context_tokens,
            max_context_tokens: session.max_context_tokens,
            compaction_id: session.compaction_id,
            completion_status: session.completion_status,
            initial_query: None,
            safe_mode: if session.safe_mode { Some(true) } else { None },
            special_role_name: session.special_role_name,
        }
    }
}
