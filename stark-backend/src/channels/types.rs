use serde::{Deserialize, Serialize};

/// Supported channel types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    Telegram,
    Slack,
    Discord,
    Twitter,
    ExternalChannel,
}

impl ChannelType {
    /// String representation for database/API
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Slack => "slack",
            Self::Discord => "discord",
            Self::Twitter => "twitter",
            Self::ExternalChannel => "external_channel",
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "telegram" => Some(Self::Telegram),
            "slack" => Some(Self::Slack),
            "discord" => Some(Self::Discord),
            "twitter" => Some(Self::Twitter),
            "external_channel" => Some(Self::ExternalChannel),
            _ => None,
        }
    }

    /// All supported channel types
    pub fn all() -> &'static [ChannelType] {
        &[Self::Telegram, Self::Slack, Self::Discord, Self::Twitter, Self::ExternalChannel]
    }

    /// Display name for UI
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Telegram => "Telegram",
            Self::Slack => "Slack",
            Self::Discord => "Discord",
            Self::Twitter => "Twitter",
            Self::ExternalChannel => "External Channel",
        }
    }
}

impl std::fmt::Display for ChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Normalized message from any channel (Telegram, Slack, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedMessage {
    /// Channel database ID
    pub channel_id: i64,
    /// Channel type
    pub channel_type: String,  // Keep as String for now for compatibility
    /// Platform-specific chat/conversation ID
    pub chat_id: String,
    /// Human-readable name for the chat/channel (e.g., Discord channel name)
    #[serde(default)]
    pub chat_name: Option<String>,
    /// Platform-specific user ID
    pub user_id: String,
    /// Display name of the user
    pub user_name: String,
    /// Message text content
    pub text: String,
    /// Platform-specific message ID (for replies)
    pub message_id: Option<String>,
    /// Session mode for cron jobs: "main" (shared with web) or "isolated" (separate session)
    #[serde(default)]
    pub session_mode: Option<String>,
    /// Currently selected network from UI (e.g., "base", "polygon", "mainnet")
    /// Used as default for web3 operations unless user explicitly specifies otherwise
    #[serde(default)]
    pub selected_network: Option<String>,
    /// Force safe mode for this message (e.g., non-admin Discord queries)
    #[serde(default)]
    pub force_safe_mode: bool,
}

/// Handle to a running channel listener
pub struct ChannelHandle {
    pub channel_id: i64,
    pub channel_type: String,
    pub name: String,
    pub shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl ChannelHandle {
    pub fn new(
        channel_id: i64,
        channel_type: String,
        name: String,
        shutdown_tx: tokio::sync::oneshot::Sender<()>,
    ) -> Self {
        Self {
            channel_id,
            channel_type,
            name,
            shutdown_tx,
        }
    }
}

/// Result of dispatching a message to the AI
#[derive(Debug, Clone)]
pub struct DispatchResult {
    pub response: String,
    pub error: Option<String>,
    /// Stable UUID for deduplication — set when the response was already delivered
    /// via a say_to_user WebSocket event. The frontend can use this ID to avoid
    /// rendering the same message twice.
    pub message_id: Option<String>,
}

impl DispatchResult {
    pub fn success(response: String) -> Self {
        Self {
            response,
            error: None,
            message_id: None,
        }
    }

    pub fn success_with_message_id(response: String, message_id: Option<String>) -> Self {
        Self {
            response,
            error: None,
            message_id,
        }
    }

    pub fn error(error: String) -> Self {
        Self {
            response: String::new(),
            error: Some(error),
            message_id: None,
        }
    }
}
