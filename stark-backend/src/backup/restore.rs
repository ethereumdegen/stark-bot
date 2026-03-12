//! Restore backup data into the database.

use super::BackupData;
use crate::db::Database;
use std::sync::Arc;

/// Result of a restore operation
pub struct RestoreResult {
    pub api_keys: usize,
    pub bot_settings: bool,
    pub x402_payment_limits: usize,
    pub bot_config: bool,
}

impl RestoreResult {
    pub fn summary(&self) -> String {
        format!(
            "Restored: {} API keys, bot_settings={}, {} x402 limits, bot_config={}",
            self.api_keys, self.bot_settings, self.x402_payment_limits, self.bot_config
        )
    }
}

/// Restore all backup data into the database
pub async fn restore_all(
    db: &Arc<Database>,
    backup: &mut BackupData,
) -> Result<RestoreResult, String> {
    let mut result = RestoreResult {
        api_keys: 0,
        bot_settings: false,
        x402_payment_limits: 0,
        bot_config: false,
    };

    // Restore API keys
    for key in &backup.api_keys {
        if !key.service_name.is_empty() && !key.api_key.is_empty() {
            match db.upsert_api_key(&key.service_name, &key.api_key) {
                Ok(_) => result.api_keys += 1,
                Err(e) => log::warn!("[Restore] Failed to restore API key '{}': {}", key.service_name, e),
            }
        }
    }

    // Restore bot settings
    if let Some(ref settings) = backup.bot_settings {
        if let Err(e) = db.update_bot_settings(
            Some(&settings.bot_name),
            Some(&settings.bot_email),
            Some(settings.web3_tx_requires_confirmation),
        ) {
            log::warn!("[Restore] Failed to restore bot settings: {}", e);
        } else {
            result.bot_settings = true;
        }
    }

    // Restore x402 payment limits
    for limit in &backup.x402_payment_limits {
        if !limit.asset.is_empty() {
            match db.set_x402_payment_limit(
                &limit.asset,
                &limit.max_amount,
                limit.decimals,
                &limit.display_name,
                limit.address.as_deref(),
            ) {
                Ok(_) => result.x402_payment_limits += 1,
                Err(e) => log::warn!("[Restore] Failed to restore x402 limit '{}': {}", limit.asset, e),
            }
        }
    }

    // Restore bot config
    if let Some(ref config_content) = backup.bot_config {
        let bot_config_path = crate::config::bot_config_path();
        if let Some(parent) = bot_config_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        match std::fs::write(&bot_config_path, config_content) {
            Ok(_) => {
                result.bot_config = true;
                log::info!("[Restore] Wrote bot_config.ron");
            }
            Err(e) => log::warn!("[Restore] Failed to write bot_config.ron: {}", e),
        }
    }

    Ok(result)
}
