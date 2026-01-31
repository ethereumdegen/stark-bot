use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default max tool iterations
pub const DEFAULT_MAX_TOOL_ITERATIONS: i32 = 50;

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
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

/// Request type for updating bot settings
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateBotSettingsRequest {
    pub bot_name: Option<String>,
    pub bot_email: Option<String>,
    pub web3_tx_requires_confirmation: Option<bool>,
    pub rpc_provider: Option<String>,
    pub custom_rpc_endpoints: Option<HashMap<String, String>>,
    pub max_tool_iterations: Option<i32>,
}
