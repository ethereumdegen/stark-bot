//! Transaction queue data types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a queued transaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueuedTxStatus {
    /// Signed but not yet broadcast
    Pending,
    /// Currently being broadcast to the network
    Broadcasting,
    /// Sent to network, has tx_hash but not yet confirmed
    Broadcast,
    /// Confirmed on-chain
    Confirmed,
    /// Broadcast or confirmation failed
    Failed,
    /// Transaction expired (timed out)
    Expired,
}

impl std::fmt::Display for QueuedTxStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueuedTxStatus::Pending => write!(f, "pending"),
            QueuedTxStatus::Broadcasting => write!(f, "broadcasting"),
            QueuedTxStatus::Broadcast => write!(f, "broadcast"),
            QueuedTxStatus::Confirmed => write!(f, "confirmed"),
            QueuedTxStatus::Failed => write!(f, "failed"),
            QueuedTxStatus::Expired => write!(f, "expired"),
        }
    }
}

/// A queued transaction waiting to be broadcast
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedTransaction {
    /// Unique identifier for this queued transaction
    pub uuid: String,
    /// Network: "base" or "mainnet"
    pub network: String,
    /// Sender address
    pub from: String,
    /// Recipient address
    pub to: String,
    /// Value in wei (as string to handle large numbers)
    pub value: String,
    /// Hex-encoded calldata
    pub data: String,
    /// Gas limit
    pub gas_limit: String,
    /// Max fee per gas in wei
    pub max_fee_per_gas: String,
    /// Max priority fee per gas in wei
    pub max_priority_fee_per_gas: String,
    /// Transaction nonce
    pub nonce: u64,
    /// RLP-encoded signed transaction bytes (hex-encoded for serialization)
    pub signed_tx_hex: String,
    /// Current status
    pub status: QueuedTxStatus,
    /// Transaction hash (set after broadcast)
    pub tx_hash: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
    /// When the transaction was queued
    pub created_at: DateTime<Utc>,
    /// When the transaction was broadcast
    pub broadcast_at: Option<DateTime<Utc>>,
    /// Channel ID that queued this transaction
    pub channel_id: Option<i64>,
    /// Explorer URL (set after tx_hash is known)
    pub explorer_url: Option<String>,
    /// Preset name that created this tx (e.g. "identity_register"), for post-processing hooks
    pub preset: Option<String>,
}

impl QueuedTransaction {
    /// Create a new queued transaction
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        uuid: String,
        network: String,
        from: String,
        to: String,
        value: String,
        data: String,
        gas_limit: String,
        max_fee_per_gas: String,
        max_priority_fee_per_gas: String,
        nonce: u64,
        signed_tx_hex: String,
        channel_id: Option<i64>,
    ) -> Self {
        Self {
            uuid,
            network,
            from,
            to,
            value,
            data,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            nonce,
            signed_tx_hex,
            status: QueuedTxStatus::Pending,
            tx_hash: None,
            error: None,
            created_at: Utc::now(),
            broadcast_at: None,
            channel_id,
            explorer_url: None,
            preset: None,
        }
    }

    /// Set the preset name that created this transaction
    pub fn with_preset(mut self, preset: Option<&str>) -> Self {
        self.preset = preset.map(|s| s.to_string());
        self
    }

    /// Get the explorer URL for this transaction's network
    pub fn get_explorer_base_url(&self) -> &'static str {
        if self.network == "mainnet" {
            "https://etherscan.io/tx"
        } else {
            "https://basescan.org/tx"
        }
    }

    /// Format value as human-readable ETH
    pub fn format_value_eth(&self) -> String {
        if let Ok(w) = self.value.parse::<u128>() {
            let eth = w as f64 / 1e18;
            if eth >= 0.0001 {
                format!("{:.6} ETH", eth)
            } else {
                format!("{} wei", self.value)
            }
        } else {
            format!("{} wei", self.value)
        }
    }
}

/// Summary info for listing transactions (lighter than full QueuedTransaction)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedTxSummary {
    pub uuid: String,
    pub network: String,
    pub from: String,
    pub to: String,
    pub value: String,
    pub value_formatted: String,
    /// Hex-encoded calldata (for function selector lookup)
    pub data: String,
    pub status: QueuedTxStatus,
    pub tx_hash: Option<String>,
    pub explorer_url: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub broadcast_at: Option<DateTime<Utc>>,
}

impl From<&QueuedTransaction> for QueuedTxSummary {
    fn from(tx: &QueuedTransaction) -> Self {
        Self {
            uuid: tx.uuid.clone(),
            network: tx.network.clone(),
            from: tx.from.clone(),
            to: tx.to.clone(),
            value: tx.value.clone(),
            value_formatted: tx.format_value_eth(),
            data: tx.data.clone(),
            status: tx.status,
            tx_hash: tx.tx_hash.clone(),
            explorer_url: tx.explorer_url.clone(),
            error: tx.error.clone(),
            created_at: tx.created_at,
            broadcast_at: tx.broadcast_at,
        }
    }
}
