//! Backup module for starkbot
//!
//! Provides structures and utilities for backing up and restoring user data
//! to/from the keystore server.
//!
//! All structs use `#[serde(default)]` for schema resilience:
//! - Missing fields get sensible defaults
//! - Unknown fields from newer/older backups are silently ignored

pub mod restore;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Current backup format version
pub const BACKUP_VERSION: u32 = 2;

/// Complete backup data structure (simplified for Starkbot 2.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupData {
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub wallet_address: String,
    /// API keys (always included)
    pub api_keys: Vec<ApiKeyEntry>,
    /// Bot settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_settings: Option<BotSettingsEntry>,
    /// x402 payment limits
    pub x402_payment_limits: Vec<X402PaymentLimitEntry>,
    /// Bot config RON content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_config: Option<String>,
}

impl Default for BackupData {
    fn default() -> Self {
        Self {
            version: 0,
            created_at: Utc::now(),
            wallet_address: String::new(),
            api_keys: Vec::new(),
            bot_settings: None,
            x402_payment_limits: Vec::new(),
            bot_config: None,
        }
    }
}

impl BackupData {
    pub fn new(wallet_address: String) -> Self {
        Self {
            version: BACKUP_VERSION,
            created_at: Utc::now(),
            wallet_address,
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.api_keys.is_empty()
            && self.bot_settings.is_none()
            && self.x402_payment_limits.is_empty()
            && self.bot_config.is_none()
    }

    pub fn item_count(&self) -> usize {
        self.api_keys.len()
            + if self.bot_settings.is_some() { 1 } else { 0 }
            + self.x402_payment_limits.len()
            + if self.bot_config.is_some() { 1 } else { 0 }
    }
}

/// API key entry
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiKeyEntry {
    pub service_name: String,
    pub api_key: String,
}

/// Bot settings entry
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BotSettingsEntry {
    pub bot_name: String,
    pub bot_email: String,
    pub web3_tx_requires_confirmation: bool,
    pub rpc_provider: String,
    pub custom_rpc_endpoints: Option<std::collections::HashMap<String, String>>,
    pub rogue_mode_enabled: bool,
    pub keystore_url: Option<String>,
}

/// x402 payment limit entry
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct X402PaymentLimitEntry {
    pub asset: String,
    pub max_amount: String,
    pub decimals: u8,
    pub display_name: String,
    pub address: Option<String>,
}

/// Collect backup data from the database
pub async fn collect_backup_data(
    db: &Arc<crate::db::Database>,
    wallet_address: &str,
) -> BackupData {
    let mut backup = BackupData::new(wallet_address.to_string());

    // API keys
    if let Ok(keys) = db.list_api_keys() {
        backup.api_keys = keys.iter().map(|k| ApiKeyEntry {
            service_name: k.service_name.clone(),
            api_key: k.api_key.clone(),
        }).collect();
    }

    // Bot settings
    if let Ok(settings) = db.get_bot_settings() {
        backup.bot_settings = Some(BotSettingsEntry {
            bot_name: settings.bot_name,
            bot_email: settings.bot_email,
            web3_tx_requires_confirmation: settings.web3_tx_requires_confirmation,
            rpc_provider: settings.rpc_provider,
            custom_rpc_endpoints: settings.custom_rpc_endpoints,
            rogue_mode_enabled: settings.rogue_mode_enabled,
            keystore_url: settings.keystore_url,
        });
    }

    // x402 payment limits
    if let Ok(limits) = db.get_all_x402_payment_limits() {
        backup.x402_payment_limits = limits.iter().map(|l| X402PaymentLimitEntry {
            asset: l.asset.clone(),
            max_amount: l.max_amount.clone(),
            decimals: l.decimals,
            display_name: l.display_name.clone(),
            address: l.address.clone(),
        }).collect();
    }

    // Bot config file
    let bot_config_path = crate::config::bot_config_path();
    if bot_config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&bot_config_path) {
            backup.bot_config = Some(content);
        }
    }

    backup
}
