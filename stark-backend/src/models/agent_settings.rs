use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Agent settings stored in database (x402 endpoint configuration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    pub id: i64,
    pub endpoint: String,
    pub model_archetype: String,
    /// Model name sent in request body for unified router dispatch
    pub model: Option<String>,
    pub max_response_tokens: i32,
    pub max_context_tokens: i32,
    pub enabled: bool,
    pub secret_key: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Minimum allowed context tokens (ensures compaction has room to work)
pub const MIN_CONTEXT_TOKENS: i32 = 80_000;
/// Default context tokens (Claude/most models)
pub const DEFAULT_CONTEXT_TOKENS: i32 = 100_000;

impl Default for AgentSettings {
    /// Returns default kimi-turbo agent settings (used when no agent is configured)
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: 0,
            endpoint: "https://inference.defirelay.com/api/v1/chat/completions".to_string(),
            model_archetype: "kimi".to_string(),
            model: Some("kimi-turbo".to_string()),
            max_response_tokens: 40000,
            max_context_tokens: DEFAULT_CONTEXT_TOKENS,
            enabled: true,
            secret_key: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Response type for agent settings API
#[derive(Debug, Clone, Serialize)]
pub struct AgentSettingsResponse {
    pub id: i64,
    pub endpoint: String,
    pub model_archetype: String,
    pub model: Option<String>,
    pub max_response_tokens: i32,
    pub max_context_tokens: i32,
    pub enabled: bool,
    pub has_secret_key: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<AgentSettings> for AgentSettingsResponse {
    fn from(settings: AgentSettings) -> Self {
        Self {
            id: settings.id,
            endpoint: settings.endpoint,
            model_archetype: settings.model_archetype,
            model: settings.model,
            max_response_tokens: settings.max_response_tokens,
            max_context_tokens: settings.max_context_tokens,
            enabled: settings.enabled,
            has_secret_key: settings.secret_key.is_some(),
            created_at: settings.created_at,
            updated_at: settings.updated_at,
        }
    }
}

/// Request type for updating agent settings
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateAgentSettingsRequest {
    pub endpoint: String,
    #[serde(default = "default_archetype")]
    pub model_archetype: String,
    /// Model name for unified router dispatch (e.g. "kimi-turbo", "gpt-5-mini")
    pub model: Option<String>,
    #[serde(default = "default_max_response_tokens")]
    pub max_response_tokens: i32,
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: i32,
    pub secret_key: Option<String>,
}

fn default_archetype() -> String {
    "kimi".to_string()
}

fn default_max_response_tokens() -> i32 {
    40000
}

fn default_max_context_tokens() -> i32 {
    DEFAULT_CONTEXT_TOKENS
}
