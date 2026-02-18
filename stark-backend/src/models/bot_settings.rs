use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default max tool iterations
pub const DEFAULT_MAX_TOOL_ITERATIONS: i32 = 100;

/// Default max safe mode queries per user per 10 minutes
pub const DEFAULT_SAFE_MODE_MAX_QUERIES_PER_10MIN: i32 = 5;

/// Bot settings stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotSettings {
    pub id: i64,
    pub bot_name: String,
    pub bot_email: String,
    pub web3_tx_requires_confirmation: bool,
    /// RPC provider name: "defirelay" or "custom"
    pub rpc_provider: String,
    /// Custom RPC endpoints per network (only used when rpc_provider == "custom")
    pub custom_rpc_endpoints: Option<HashMap<String, String>>,
    /// Maximum number of tool execution iterations per request
    pub max_tool_iterations: i32,
    /// Rogue mode: when true, bot operates in "rogue" mode instead of "partner" mode
    pub rogue_mode_enabled: bool,
    /// Maximum safe mode queries per user per 10 minutes
    pub safe_mode_max_queries_per_10min: i32,
    /// Custom keystore server URL (None = default: https://keystore.defirelay.com)
    pub keystore_url: Option<String>,
    /// Whether to save a memory entry when a chat session completes
    pub chat_session_memory_generation: bool,
    /// Whether unauthenticated users can view the guest dashboard
    pub guest_dashboard_enabled: bool,
    /// Dashboard theme accent color (e.g. "blue"). None = default orange.
    pub theme_accent: Option<String>,
    /// Optional HTTP proxy URL for tool requests (does not affect AI model API calls)
    pub proxy_url: Option<String>,
    /// Whether kanban "ready" tasks are auto-executed by the scheduler
    pub kanban_auto_execute: bool,
    /// Whether message coalescing is enabled
    #[serde(default)]
    pub coalescing_enabled: bool,
    /// Coalescing debounce time in milliseconds
    #[serde(default = "default_coalescing_debounce")]
    pub coalescing_debounce_ms: u64,
    /// Coalescing max wait time in milliseconds
    #[serde(default = "default_coalescing_max_wait")]
    pub coalescing_max_wait_ms: u64,
    /// Background compaction threshold (ratio 0.0-1.0)
    #[serde(default = "default_background_threshold")]
    pub compaction_background_threshold: f64,
    /// Aggressive compaction threshold
    #[serde(default = "default_aggressive_threshold")]
    pub compaction_aggressive_threshold: f64,
    /// Emergency compaction threshold
    #[serde(default = "default_emergency_threshold")]
    pub compaction_emergency_threshold: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Default for BotSettings {
    fn default() -> Self {
        Self {
            id: 0,
            bot_name: "StarkBot".to_string(),
            bot_email: "starkbot@users.noreply.github.com".to_string(),
            web3_tx_requires_confirmation: false,
            rpc_provider: "defirelay".to_string(),
            custom_rpc_endpoints: None,
            max_tool_iterations: DEFAULT_MAX_TOOL_ITERATIONS,
            rogue_mode_enabled: false,
            safe_mode_max_queries_per_10min: DEFAULT_SAFE_MODE_MAX_QUERIES_PER_10MIN,
            keystore_url: None, // Uses default: https://keystore.defirelay.com
            chat_session_memory_generation: true,
            guest_dashboard_enabled: false,
            theme_accent: None,
            proxy_url: None,
            kanban_auto_execute: true,
            coalescing_enabled: false,
            coalescing_debounce_ms: 1500,
            coalescing_max_wait_ms: 5000,
            compaction_background_threshold: 0.80,
            compaction_aggressive_threshold: 0.85,
            compaction_emergency_threshold: 0.95,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

fn default_coalescing_debounce() -> u64 { 1500 }
fn default_coalescing_max_wait() -> u64 { 5000 }
fn default_background_threshold() -> f64 { 0.80 }
fn default_aggressive_threshold() -> f64 { 0.85 }
fn default_emergency_threshold() -> f64 { 0.95 }

/// Request type for updating bot settings
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateBotSettingsRequest {
    pub bot_name: Option<String>,
    pub bot_email: Option<String>,
    pub web3_tx_requires_confirmation: Option<bool>,
    pub rpc_provider: Option<String>,
    pub custom_rpc_endpoints: Option<HashMap<String, String>>,
    pub max_tool_iterations: Option<i32>,
    pub rogue_mode_enabled: Option<bool>,
    pub safe_mode_max_queries_per_10min: Option<i32>,
    /// Custom keystore URL (empty string or null = use default)
    pub keystore_url: Option<String>,
    pub chat_session_memory_generation: Option<bool>,
    pub guest_dashboard_enabled: Option<bool>,
    pub theme_accent: Option<String>,
    /// Optional HTTP proxy URL for tool requests (empty string or null = direct connection)
    pub proxy_url: Option<String>,
    /// Whether kanban "ready" tasks are auto-executed by the scheduler
    pub kanban_auto_execute: Option<bool>,
    pub coalescing_enabled: Option<bool>,
    pub coalescing_debounce_ms: Option<u64>,
    pub coalescing_max_wait_ms: Option<u64>,
    pub compaction_background_threshold: Option<f64>,
    pub compaction_aggressive_threshold: Option<f64>,
    pub compaction_emergency_threshold: Option<f64>,
}
