//! Shared types for the wallet monitor service and its RPC clients.

use serde::{Deserialize, Serialize};

// =====================================================
// Domain Types
// =====================================================

/// A watched wallet entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistEntry {
    pub id: i64,
    pub address: String,
    pub label: Option<String>,
    pub chain: String,
    pub monitor_enabled: bool,
    pub large_trade_threshold_usd: f64,
    pub copy_trade_enabled: bool,
    pub copy_trade_max_usd: Option<f64>,
    pub last_checked_block: Option<i64>,
    pub last_checked_at: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// An activity entry from a watched wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub id: i64,
    pub watchlist_id: i64,
    pub chain: String,
    pub tx_hash: String,
    pub block_number: i64,
    pub block_timestamp: Option<String>,
    pub from_address: String,
    pub to_address: String,
    pub activity_type: String,
    pub asset_symbol: Option<String>,
    pub asset_address: Option<String>,
    pub amount_raw: Option<String>,
    pub amount_formatted: Option<String>,
    pub usd_value: Option<f64>,
    pub is_large_trade: bool,
    pub swap_from_token: Option<String>,
    pub swap_from_amount: Option<String>,
    pub swap_to_token: Option<String>,
    pub swap_to_amount: Option<String>,
    pub raw_data: Option<String>,
    pub created_at: String,
}

/// Filters for querying activity
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ActivityFilter {
    pub watchlist_id: Option<i64>,
    pub address: Option<String>,
    pub activity_type: Option<String>,
    pub chain: Option<String>,
    pub large_only: bool,
    pub limit: Option<usize>,
}

/// Stats about wallet activity
#[derive(Debug, Serialize, Deserialize)]
pub struct ActivityStats {
    pub total_transactions: i64,
    pub large_trades: i64,
    pub watched_wallets: i64,
    pub active_wallets: i64,
}

// =====================================================
// RPC Request Types
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct AddWalletRequest {
    pub address: String,
    pub label: Option<String>,
    pub chain: Option<String>,
    pub threshold_usd: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateWalletRequest {
    pub id: i64,
    pub label: Option<String>,
    pub threshold_usd: Option<f64>,
    pub monitor_enabled: Option<bool>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveWalletRequest {
    pub id: i64,
}

// =====================================================
// RPC Response Types
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponse<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> RpcResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

// =====================================================
// Alert Types
// =====================================================

/// Alert for large trades, sent via callback to starkbot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeTradeAlert {
    pub watchlist_id: i64,
    pub address: String,
    pub label: Option<String>,
    pub chain: String,
    pub tx_hash: String,
    pub activity_type: String,
    pub usd_value: Option<f64>,
    pub asset_symbol: Option<String>,
    pub amount_formatted: Option<String>,
    pub swap_from_token: Option<String>,
    pub swap_from_amount: Option<String>,
    pub swap_to_token: Option<String>,
    pub swap_to_amount: Option<String>,
    pub message: String,
}

// =====================================================
// Backup Types
// =====================================================

/// A watchlist entry for backup (excludes transient fields like last_checked_block)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEntry {
    pub address: String,
    pub label: Option<String>,
    pub chain: String,
    pub monitor_enabled: bool,
    pub large_trade_threshold_usd: f64,
    pub copy_trade_enabled: bool,
    pub copy_trade_max_usd: Option<f64>,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupRestoreRequest {
    pub wallets: Vec<BackupEntry>,
}

// =====================================================
// Service Status
// =====================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub running: bool,
    pub uptime_secs: u64,
    pub watched_wallets: i64,
    pub active_wallets: i64,
    pub total_transactions: i64,
    pub large_trades: i64,
    pub last_tick_at: Option<String>,
    pub poll_interval_secs: u64,
    /// Whether the background polling worker is enabled (requires ALCHEMY_API_KEY)
    #[serde(default)]
    pub worker_enabled: bool,
}
