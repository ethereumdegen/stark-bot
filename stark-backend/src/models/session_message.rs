use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Message role in the conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "user" => Some(MessageRole::User),
            "assistant" => Some(MessageRole::Assistant),
            "system" => Some(MessageRole::System),
            _ => None,
        }
    }
}

/// Session message - individual message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub id: i64,
    pub session_id: i64,
    pub role: MessageRole,
    pub content: String,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub platform_message_id: Option<String>,
    pub tokens_used: Option<i32>,
    pub created_at: DateTime<Utc>,
}

/// Request to add a message to a session
#[derive(Debug, Clone, Deserialize)]
pub struct AddMessageRequest {
    pub role: MessageRole,
    pub content: String,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub platform_message_id: Option<String>,
    pub tokens_used: Option<i32>,
}

/// Response containing session transcript
#[derive(Debug, Clone, Serialize)]
pub struct SessionTranscriptResponse {
    pub session_id: i64,
    pub messages: Vec<SessionMessage>,
    pub total_count: i64,
}
