//! EIP-8004 Configuration
//!
//! Contract addresses and chain configuration for EIP-8004 registries.

use crate::tools::rpc_config;
use crate::wallet::WalletProvider;
use crate::x402::X402EvmRpc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// EIP-8004 registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip8004Config {
    /// Identity Registry contract address
    pub identity_registry: String,
    /// Reputation Registry contract address
    pub reputation_registry: String,
    /// Validation Registry contract address (optional)
    pub validation_registry: Option<String>,
    /// Chain ID
    pub chain_id: u64,
    /// Chain name for display
    pub chain_name: String,
    /// Block explorer URL
    pub explorer_url: String,
}

impl Eip8004Config {
    /// Base Mainnet configuration
    pub fn base_mainnet() -> Self {
        Self {
            identity_registry: "0xa23a42D266653846e05d8f356a52298844537472".to_string(),
            reputation_registry: "0x0000000000000000000000000000000000000000".to_string(),
            validation_registry: None,
            chain_id: 8453,
            chain_name: "Base".to_string(),
            explorer_url: "https://basescan.org".to_string(),
        }
    }

    /// Base Sepolia testnet configuration (for development)
    pub fn base_sepolia() -> Self {
        Self {
            // TODO: Replace with testnet deployed addresses
            identity_registry: "0x0000000000000000000000000000000000000000".to_string(),
            reputation_registry: "0x0000000000000000000000000000000000000000".to_string(),
            validation_registry: None,
            chain_id: 84532,
            chain_name: "Base Sepolia".to_string(),
            explorer_url: "https://sepolia.basescan.org".to_string(),
        }
    }

    /// Load from environment or use defaults
    pub fn from_env() -> Self {
        let chain_id = std::env::var("EIP8004_CHAIN_ID")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8453);

        match chain_id {
            84532 => Self::base_sepolia(),
            _ => {
                let mut config = Self::base_mainnet();

                // Override with environment variables if set
                if let Ok(addr) = std::env::var("EIP8004_IDENTITY_REGISTRY") {
                    config.identity_registry = addr;
                }
                if let Ok(addr) = std::env::var("EIP8004_REPUTATION_REGISTRY") {
                    config.reputation_registry = addr;
                }
                if let Ok(addr) = std::env::var("EIP8004_VALIDATION_REGISTRY") {
                    config.validation_registry = Some(addr);
                }
                config
            }
        }
    }

    /// Check if contracts are deployed (not zero address)
    pub fn is_identity_deployed(&self) -> bool {
        !self.identity_registry.contains("0x0000000000000000000000000000000000000000")
    }

    pub fn is_reputation_deployed(&self) -> bool {
        !self.reputation_registry.contains("0x0000000000000000000000000000000000000000")
    }

    pub fn is_validation_deployed(&self) -> bool {
        self.validation_registry
            .as_ref()
            .map(|addr| !addr.contains("0x0000000000000000000000000000000000000000"))
            .unwrap_or(false)
    }

    /// Get block explorer URL for a transaction
    pub fn tx_url(&self, tx_hash: &str) -> String {
        format!("{}/tx/{}", self.explorer_url, tx_hash)
    }

    /// Get block explorer URL for an address
    pub fn address_url(&self, address: &str) -> String {
        format!("{}/address/{}", self.explorer_url, address)
    }

    /// Get block explorer URL for an NFT token
    pub fn token_url(&self, token_id: u64) -> String {
        format!("{}/token/{}?a={}", self.explorer_url, self.identity_registry, token_id)
    }

    /// Get the network name for this chain config.
    pub fn network(&self) -> &str {
        match self.chain_id {
            1 => "mainnet",
            84532 => "base-sepolia",
            _ => "base",
        }
    }

    /// Build an X402EvmRpc for this chain using the unified RPC resolver.
    /// Uses `resolve_rpc` which respects bot settings custom RPC endpoints.
    pub fn build_rpc(&self, wallet_provider: Option<Arc<dyn WalletProvider>>) -> Result<X402EvmRpc, String> {
        let network = self.network();
        let resolved = rpc_config::resolve_rpc(network);

        if let Some(wp) = wallet_provider {
            return X402EvmRpc::new_with_wallet_provider(wp, network, Some(resolved.url), resolved.use_x402);
        }

        let private_key = crate::config::burner_wallet_private_key()
            .ok_or("BURNER_WALLET_BOT_PRIVATE_KEY not set")?;
        let wp: Arc<dyn WalletProvider> = Arc::new(
            crate::wallet::EnvWalletProvider::from_private_key(&private_key)?
        );
        X402EvmRpc::new_with_wallet_provider(wp, network, Some(resolved.url), resolved.use_x402)
    }

    /// Format agent registry string
    pub fn agent_registry_string(&self) -> String {
        format!("eip155:{}:{}", self.chain_id, self.identity_registry.to_lowercase())
    }
}

impl Default for Eip8004Config {
    fn default() -> Self {
        Self::from_env()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_mainnet_config() {
        let config = Eip8004Config::base_mainnet();
        assert_eq!(config.chain_id, 8453);
        assert_eq!(config.chain_name, "Base");
    }

    #[test]
    fn test_explorer_urls() {
        let config = Eip8004Config::base_mainnet();
        assert!(config.tx_url("0x123").contains("basescan.org/tx/0x123"));
        assert!(config.address_url("0xabc").contains("basescan.org/address/0xabc"));
    }
}
