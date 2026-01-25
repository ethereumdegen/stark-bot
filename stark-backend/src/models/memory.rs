use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Type of memory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    DailyLog,
    LongTerm,
}

impl MemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryType::DailyLog => "daily_log",
            MemoryType::LongTerm => "long_term",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "daily_log" => Some(MemoryType::DailyLog),
            "long_term" => Some(MemoryType::LongTerm),
            _ => None,
        }
    }
}

/// Memory - daily logs and long-term memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: i64,
    pub memory_type: MemoryType,
    pub content: String,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub importance: i32,
    pub identity_id: Option<String>,
    pub session_id: Option<i64>,
    pub source_channel_type: Option<String>,
    pub source_message_id: Option<String>,
    pub log_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Request to create a memory
#[derive(Debug, Clone, Deserialize)]
pub struct CreateMemoryRequest {
    pub memory_type: MemoryType,
    pub content: String,
    pub category: Option<String>,
    pub tags: Option<String>,
    #[serde(default = "default_importance")]
    pub importance: i32,
    pub identity_id: Option<String>,
    pub session_id: Option<i64>,
    pub source_channel_type: Option<String>,
    pub source_message_id: Option<String>,
    pub log_date: Option<NaiveDate>,
    pub expires_at: Option<DateTime<Utc>>,
}

fn default_importance() -> i32 {
    5
}

/// Request to search memories
#[derive(Debug, Clone, Deserialize)]
pub struct SearchMemoriesRequest {
    pub query: String,
    pub memory_type: Option<MemoryType>,
    pub identity_id: Option<String>,
    pub category: Option<String>,
    pub min_importance: Option<i32>,
    #[serde(default = "default_limit")]
    pub limit: i32,
}

fn default_limit() -> i32 {
    20
}

/// Memory response for API
#[derive(Debug, Clone, Serialize)]
pub struct MemoryResponse {
    pub id: i64,
    pub memory_type: MemoryType,
    pub content: String,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub importance: i32,
    pub identity_id: Option<String>,
    pub source_channel_type: Option<String>,
    pub log_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
}

impl From<Memory> for MemoryResponse {
    fn from(memory: Memory) -> Self {
        MemoryResponse {
            id: memory.id,
            memory_type: memory.memory_type,
            content: memory.content,
            category: memory.category,
            tags: memory.tags,
            importance: memory.importance,
            identity_id: memory.identity_id,
            source_channel_type: memory.source_channel_type,
            log_date: memory.log_date,
            created_at: memory.created_at,
        }
    }
}

/// Memory search result with relevance score
#[derive(Debug, Clone, Serialize)]
pub struct MemorySearchResult {
    pub memory: MemoryResponse,
    pub rank: f64,
}
