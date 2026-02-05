//! Backup module for starkbot
//!
//! Provides structures and utilities for backing up and restoring user data
//! to/from the keystore server.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Current backup format version
pub const BACKUP_VERSION: u32 = 1;

/// Complete backup data structure
///
/// This is the encrypted payload stored on the keystore server.
/// All data is serialized to JSON before encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupData {
    /// Backup format version for future migrations
    pub version: u32,
    /// When this backup was created
    pub created_at: DateTime<Utc>,
    /// Wallet address that created this backup
    pub wallet_address: String,
    /// API keys (always included)
    pub api_keys: Vec<ApiKeyEntry>,
    /// Mind map nodes
    pub mind_map_nodes: Vec<MindNodeEntry>,
    /// Mind map connections
    pub mind_map_connections: Vec<MindConnectionEntry>,
    /// Cron jobs (scheduled tasks)
    #[serde(default)]
    pub cron_jobs: Vec<CronJobEntry>,
    /// Heartbeat config (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat_config: Option<HeartbeatConfigEntry>,
    /// Memories (optional - can be large)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memories: Option<Vec<MemoryEntry>>,
    /// Bot settings (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_settings: Option<BotSettingsEntry>,
    /// Channel settings (key-value configs per channel)
    #[serde(default)]
    pub channel_settings: Vec<ChannelSettingEntry>,
    /// Channels (with bot tokens)
    #[serde(default)]
    pub channels: Vec<ChannelEntry>,
    /// Soul document content (SOUL.md - agent's personality and truths)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soul_document: Option<String>,
}

impl BackupData {
    /// Create a new backup with the current timestamp
    pub fn new(wallet_address: String) -> Self {
        Self {
            version: BACKUP_VERSION,
            created_at: Utc::now(),
            wallet_address,
            api_keys: Vec::new(),
            mind_map_nodes: Vec::new(),
            mind_map_connections: Vec::new(),
            cron_jobs: Vec::new(),
            heartbeat_config: None,
            memories: None,
            bot_settings: None,
            channel_settings: Vec::new(),
            channels: Vec::new(),
            soul_document: None,
        }
    }

    /// Calculate total item count for progress reporting
    pub fn item_count(&self) -> usize {
        self.api_keys.len()
            + self.mind_map_nodes.len()
            + self.mind_map_connections.len()
            + self.cron_jobs.len()
            + self.memories.as_ref().map(|m| m.len()).unwrap_or(0)
            + if self.bot_settings.is_some() { 1 } else { 0 }
            + if self.heartbeat_config.is_some() { 1 } else { 0 }
            + self.channel_settings.len()
            + self.channels.len()
            + if self.soul_document.is_some() { 1 } else { 0 }
    }
}

/// API key entry in backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    pub key_name: String,
    pub key_value: String,
}

/// Mind map node entry in backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MindNodeEntry {
    pub id: i64,
    pub body: String,
    pub position_x: Option<f64>,
    pub position_y: Option<f64>,
    pub is_trunk: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Mind map connection entry in backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MindConnectionEntry {
    pub parent_id: i64,
    pub child_id: i64,
}

/// Cron job entry in backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobEntry {
    pub name: String,
    pub description: Option<String>,
    pub schedule_type: String,
    pub schedule_value: String,
    pub timezone: Option<String>,
    pub session_mode: String,
    pub message: Option<String>,
    pub system_event: Option<String>,
    pub channel_id: Option<i64>,
    pub deliver_to: Option<String>,
    pub deliver: bool,
    pub model_override: Option<String>,
    pub thinking_level: Option<String>,
    pub timeout_seconds: Option<i32>,
    pub delete_after_run: bool,
    pub status: String,
}

/// Heartbeat config entry in backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfigEntry {
    pub channel_id: Option<i64>,
    pub interval_minutes: i32,
    pub target: String,
    pub active_hours_start: Option<String>,
    pub active_hours_end: Option<String>,
    pub active_days: Option<String>,
    pub enabled: bool,
}

/// Memory entry in backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub memory_type: String,
    pub content: String,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub importance: Option<i32>,
    pub identity_id: Option<String>,
    pub created_at: String,
}

/// Bot settings entry in backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotSettingsEntry {
    pub bot_name: String,
    pub bot_email: String,
    pub web3_tx_requires_confirmation: bool,
    pub rpc_provider: Option<String>,
    pub custom_rpc_endpoints: Option<String>,
    pub max_tool_iterations: Option<i32>,
    pub rogue_mode_enabled: bool,
    pub safe_mode_max_queries_per_10min: Option<i32>,
}

/// Channel setting entry in backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSettingEntry {
    pub channel_id: i64,
    pub setting_key: String,
    pub setting_value: String,
}

/// Channel entry in backup (the actual channel with tokens)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEntry {
    pub id: i64,
    pub channel_type: String,
    pub name: String,
    pub enabled: bool,
    pub bot_token: String,
    pub app_token: Option<String>,
}

/// Options for what to include in a backup
#[derive(Debug, Clone, Default)]
pub struct BackupOptions {
    /// Include memories (can be large)
    pub include_memories: bool,
    /// Include bot settings
    pub include_bot_settings: bool,
    /// Maximum number of memories to include (0 = unlimited)
    pub max_memories: usize,
}

impl BackupOptions {
    /// Backup everything
    pub fn full() -> Self {
        Self {
            include_memories: true,
            include_bot_settings: true,
            max_memories: 0,
        }
    }

    /// Minimal backup (API keys and mind map only)
    pub fn minimal() -> Self {
        Self {
            include_memories: false,
            include_bot_settings: false,
            max_memories: 0,
        }
    }
}
