//! Backup module for starkbot
//!
//! Provides structures and utilities for backing up and restoring user data
//! to/from the keystore server.
//!
//! ## Schema resilience
//!
//! All structs use `#[serde(default)]` at the struct level so that:
//! - **Missing fields** in old backups get sensible defaults (deserialization never fails)
//! - **Unknown fields** from newer backups are silently ignored (serde default behavior)
//! This means you can freely add/remove fields without breaking existing backups.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Current backup format version
pub const BACKUP_VERSION: u32 = 1;

/// Complete backup data structure
///
/// This is the encrypted payload stored on the keystore server.
/// All data is serialized to JSON before encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    pub cron_jobs: Vec<CronJobEntry>,
    /// Heartbeat config (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_config: Option<HeartbeatConfigEntry>,
    /// Memories (optional - can be large)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memories: Option<Vec<MemoryEntry>>,
    /// Bot settings (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_settings: Option<BotSettingsEntry>,
    /// Channel settings (key-value configs per channel)
    pub channel_settings: Vec<ChannelSettingEntry>,
    /// Channels (with bot tokens)
    pub channels: Vec<ChannelEntry>,
    /// Soul document content (SOUL.md - agent's personality and truths)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soul_document: Option<String>,
    /// Identity document content (IDENTITY.json - EIP-8004 agent identity registration)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_document: Option<String>,
    /// Discord user registrations (discord_user_id → public_address mappings)
    pub discord_registrations: Vec<DiscordRegistrationEntry>,
    /// Skills (custom agent skills)
    pub skills: Vec<SkillEntry>,
    /// AI model / agent settings (endpoint, archetype, tokens, etc.)
    pub agent_settings: Vec<AgentSettingsEntry>,
    /// On-chain agent identity registration (NFT token ID, tx hash, registry, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_identity: Option<AgentIdentityEntry>,
}

/// Manual Default because DateTime<Utc> doesn't derive Default
impl Default for BackupData {
    fn default() -> Self {
        Self {
            version: 0,
            created_at: Utc::now(),
            wallet_address: String::new(),
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
            identity_document: None,
            discord_registrations: Vec::new(),
            skills: Vec::new(),
            agent_settings: Vec::new(),
            agent_identity: None,
        }
    }
}

impl BackupData {
    /// Create a new backup with the current timestamp
    pub fn new(wallet_address: String) -> Self {
        Self {
            version: BACKUP_VERSION,
            created_at: Utc::now(),
            wallet_address,
            ..Default::default()
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
            + if self.identity_document.is_some() { 1 } else { 0 }
            + self.discord_registrations.len()
            + self.skills.len()
            + self.agent_settings.len()
            + if self.agent_identity.is_some() { 1 } else { 0 }
    }
}

/// API key entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiKeyEntry {
    pub key_name: String,
    pub key_value: String,
}

/// Mind map node entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MindConnectionEntry {
    pub parent_id: i64,
    pub child_id: i64,
}

/// Cron job entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelSettingEntry {
    pub channel_id: i64,
    pub setting_key: String,
    pub setting_value: String,
}

/// Channel entry in backup (the actual channel with tokens)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelEntry {
    pub id: i64,
    pub channel_type: String,
    pub name: String,
    pub enabled: bool,
    pub bot_token: String,
    pub app_token: Option<String>,
}

/// Discord user registration entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordRegistrationEntry {
    pub discord_user_id: String,
    pub discord_username: Option<String>,
    pub public_address: String,
    pub registered_at: Option<String>,
}

/// Skill entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub body: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    pub enabled: bool,
    pub requires_tools: Vec<String>,
    pub requires_binaries: Vec<String>,
    /// Arguments serialized as JSON string
    pub arguments: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    pub scripts: Vec<SkillScriptEntry>,
}

/// Skill script entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillScriptEntry {
    pub name: String,
    pub code: String,
    pub language: String,
}

/// AI model / agent settings entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettingsEntry {
    pub endpoint: String,
    pub model_archetype: String,
    pub max_response_tokens: i32,
    pub max_context_tokens: i32,
    pub enabled: bool,
    /// Secret key is included so the user doesn't have to re-enter API keys after restore.
    /// The entire backup payload is already encrypted with ECIES — this is not stored in plaintext.
    pub secret_key: Option<String>,
}

/// On-chain agent identity registration entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentIdentityEntry {
    pub agent_id: i64,
    pub agent_registry: String,
    pub chain_id: i64,
    pub registration_uri: Option<String>,
    pub registration_hash: Option<String>,
    pub wallet_address: String,
    pub owner_address: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: bool,
    pub tx_hash: Option<String>,
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
