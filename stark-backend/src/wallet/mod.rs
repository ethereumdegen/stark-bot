//! Wallet Provider Abstraction
//!
//! This module provides a unified interface for wallet management that supports
//! two operational modes:
//!
//! - **Standard Mode**: Private key loaded from ENV (BURNER_WALLET_BOT_PRIVATE_KEY)
//!   - Signs transactions locally using LocalWallet
//! - **Flash Mode**: Wallet managed by Privy via Flash control plane
//!   - Signs transactions remotely via Flash's signing proxy
//!
//! The mode is determined by the `STARKBOT_MODE` environment variable:
//! - `standard` (default): Use EnvWalletProvider
//! - `flash`: Use FlashWalletProvider

mod env_provider;
mod flash_provider;

pub use env_provider::EnvWalletProvider;
pub use flash_provider::FlashWalletProvider;

use async_trait::async_trait;
use ethers::types::{Signature, H256, transaction::eip2718::TypedTransaction};
use std::sync::Arc;

/// Environment variable for mode selection
pub const STARKBOT_MODE_ENV: &str = "STARKBOT_MODE";

/// Trait for wallet providers - abstracts wallet access for different modes
#[async_trait]
pub trait WalletProvider: Send + Sync {
    /// Sign a message (EIP-191 personal_sign)
    async fn sign_message(&self, message: &[u8]) -> Result<Signature, String>;

    /// Sign a typed transaction
    async fn sign_transaction(&self, tx: &TypedTransaction) -> Result<Signature, String>;

    /// Sign a raw 32-byte hash (for EIP-712 when hash is pre-computed)
    /// Standard mode: uses LocalWallet.sign_hash()
    /// Flash mode: NOT SUPPORTED - use sign_typed_data instead
    async fn sign_hash(&self, hash: H256) -> Result<Signature, String>;

    /// Sign EIP-712 typed data
    /// Standard mode: computes hash and signs with LocalWallet
    /// Flash mode: calls Privy's eth_signTypedData_v4
    async fn sign_typed_data(&self, typed_data: &serde_json::Value) -> Result<Signature, String>;

    /// Get the wallet address (always available, cached)
    fn get_address(&self) -> String;

    /// Refresh wallet/connection if needed
    async fn refresh(&self) -> Result<(), String> {
        Ok(())
    }

    /// Get the mode name for logging
    fn mode_name(&self) -> &'static str;
}

/// Create the appropriate wallet provider based on STARKBOT_MODE env var
///
/// - `STARKBOT_MODE=standard` (or unset): EnvWalletProvider
/// - `STARKBOT_MODE=flash`: FlashWalletProvider
pub async fn create_wallet_provider() -> Result<Arc<dyn WalletProvider>, String> {
    let mode = std::env::var(STARKBOT_MODE_ENV)
        .unwrap_or_else(|_| "standard".to_string())
        .to_lowercase();

    log::info!("Initializing wallet provider in {} mode", mode);

    match mode.as_str() {
        "standard" | "env" => {
            let provider = EnvWalletProvider::from_env()?;
            log::info!(
                "Wallet provider initialized (standard mode): {}",
                provider.get_address()
            );
            Ok(Arc::new(provider))
        }
        "flash" | "lite" => {
            let provider = FlashWalletProvider::new().await?;
            log::info!(
                "Wallet provider initialized (flash mode): {}",
                provider.get_address()
            );
            Ok(Arc::new(provider))
        }
        _ => Err(format!(
            "Unknown STARKBOT_MODE '{}'. Use 'standard' or 'flash'.",
            mode
        )),
    }
}

/// Check if we're running in Flash mode
pub fn is_flash_mode() -> bool {
    std::env::var(STARKBOT_MODE_ENV)
        .map(|m| m.to_lowercase() == "flash" || m.to_lowercase() == "lite")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_flash_mode_default() {
        // When STARKBOT_MODE is not set, should be false
        // SAFETY: This test runs single-threaded so env mutation is safe
        unsafe { std::env::remove_var(STARKBOT_MODE_ENV); }
        assert!(!is_flash_mode());
    }
}
