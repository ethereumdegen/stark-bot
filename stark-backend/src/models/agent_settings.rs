use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Supported AI providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    Claude,
    OpenAI,
    /// OpenAI-compatible API (DigitalOcean, Azure, local servers, etc.)
    OpenAICompatible,
    Llama,
}

impl AiProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            AiProvider::Claude => "claude",
            AiProvider::OpenAI => "openai",
            AiProvider::OpenAICompatible => "openai_compatible",
            AiProvider::Llama => "llama",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" => Some(AiProvider::Claude),
            "openai" => Some(AiProvider::OpenAI),
            "openai_compatible" | "openaicompatible" | "custom" => Some(AiProvider::OpenAICompatible),
            "llama" => Some(AiProvider::Llama),
            _ => None,
        }
    }

    /// Get placeholder endpoint text for UI hints only
    pub fn placeholder_endpoint(&self) -> &'static str {
        match self {
            AiProvider::Claude => "https://api.anthropic.com/v1/messages",
            AiProvider::OpenAI => "https://api.openai.com/v1/chat/completions",
            AiProvider::OpenAICompatible => "https://your-endpoint.com/v1/chat/completions",
            AiProvider::Llama => "http://localhost:11434/api/chat",
        }
    }

    /// Get placeholder model text for UI hints only
    pub fn placeholder_model(&self) -> &'static str {
        match self {
            AiProvider::Claude => "claude-sonnet-4-20250514",
            AiProvider::OpenAI => "gpt-4o",
            AiProvider::OpenAICompatible => "your-model-name",
            AiProvider::Llama => "llama3.3",
        }
    }
}

impl std::fmt::Display for AiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Agent settings stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    pub id: i64,
    pub provider: String,
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub enabled: bool,
    pub bot_name: String,
    pub bot_email: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentSettings {
    pub fn provider_enum(&self) -> Option<AiProvider> {
        AiProvider::from_str(&self.provider)
    }
}

/// Response type for agent settings API
#[derive(Debug, Clone, Serialize)]
pub struct AgentSettingsResponse {
    pub id: i64,
    pub provider: String,
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub enabled: bool,
    pub bot_name: String,
    pub bot_email: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<AgentSettings> for AgentSettingsResponse {
    fn from(settings: AgentSettings) -> Self {
        Self {
            id: settings.id,
            provider: settings.provider,
            endpoint: settings.endpoint,
            api_key: settings.api_key,
            model: settings.model,
            enabled: settings.enabled,
            bot_name: settings.bot_name,
            bot_email: settings.bot_email,
            created_at: settings.created_at,
            updated_at: settings.updated_at,
        }
    }
}

/// Request type for updating agent settings
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateAgentSettingsRequest {
    pub provider: String,
    pub endpoint: String,
    pub api_key: String,
    pub model: Option<String>,
    pub bot_name: Option<String>,
    pub bot_email: Option<String>,
}
