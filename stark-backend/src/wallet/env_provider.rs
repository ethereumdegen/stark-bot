//! Environment-based Wallet Provider (Standard Mode)
//!
//! Loads wallet from BURNER_WALLET_BOT_PRIVATE_KEY environment variable.
//! This is the original Starkbot behavior - wallet is configured at deploy time.
//! Signs transactions locally using ethers LocalWallet.

use async_trait::async_trait;
use ethers::core::k256::ecdsa::SigningKey;
use ethers::signers::{LocalWallet, Signer};
use ethers::types::{Address, H256, Signature, U256, transaction::eip2718::TypedTransaction};
use ethers::utils::keccak256;

use super::WalletProvider;
use crate::config::env_vars;

/// Compute EIP-712 domain separator from domain object
fn compute_domain_separator(domain: &serde_json::Value) -> Result<H256, String> {
    let type_hash = keccak256(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
    );

    let name = domain.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let version = domain.get("version").and_then(|v| v.as_str()).unwrap_or("1");
    let chain_id = domain.get("chainId")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);
    let verifying_contract = domain.get("verifyingContract")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0000000000000000000000000000000000000000");

    let name_hash = keccak256(name.as_bytes());
    let version_hash = keccak256(version.as_bytes());

    let contract_addr: Address = verifying_contract.parse()
        .map_err(|e| format!("Invalid verifying contract address: {}", e))?;

    let mut encoded = Vec::new();
    encoded.extend_from_slice(&type_hash);
    encoded.extend_from_slice(&name_hash);
    encoded.extend_from_slice(&version_hash);
    encoded.extend_from_slice(&ethers::abi::encode(&[
        ethers::abi::Token::Uint(U256::from(chain_id)),
    ]));
    encoded.extend_from_slice(&ethers::abi::encode(&[
        ethers::abi::Token::Address(contract_addr),
    ]));

    Ok(H256::from(keccak256(&encoded)))
}

/// Wallet provider that loads from environment variable
pub struct EnvWalletProvider {
    wallet: LocalWallet,
    address: String,
    /// Raw private key hex (without 0x prefix) — used as ECIES encryption key
    encryption_key_hex: String,
}

impl EnvWalletProvider {
    /// Create provider from environment variable
    ///
    /// Requires: BURNER_WALLET_BOT_PRIVATE_KEY
    pub fn from_env() -> Result<Self, String> {
        let private_key = std::env::var(env_vars::BURNER_WALLET_PRIVATE_KEY)
            .map_err(|_| format!("{} not set", env_vars::BURNER_WALLET_PRIVATE_KEY))?;

        Self::from_private_key(&private_key)
    }

    /// Create provider from a private key string
    pub fn from_private_key(private_key: &str) -> Result<Self, String> {
        let key_hex = private_key.strip_prefix("0x").unwrap_or(private_key);

        let key_bytes = hex::decode(key_hex)
            .map_err(|e| format!("Invalid private key hex: {}", e))?;

        let signing_key = SigningKey::from_bytes(key_bytes.as_slice().into())
            .map_err(|e| format!("Invalid private key: {}", e))?;

        let wallet = LocalWallet::from(signing_key);
        let address = format!("{:?}", wallet.address()).to_lowercase();

        Ok(Self {
            wallet,
            address,
            encryption_key_hex: key_hex.to_string(),
        })
    }

    /// Get the underlying LocalWallet (for internal use only)
    pub fn wallet(&self) -> &LocalWallet {
        &self.wallet
    }
}

#[async_trait]
impl WalletProvider for EnvWalletProvider {
    async fn sign_message(&self, message: &[u8]) -> Result<Signature, String> {
        self.wallet
            .sign_message(message)
            .await
            .map_err(|e| format!("Failed to sign message: {}", e))
    }

    async fn sign_transaction(&self, tx: &TypedTransaction) -> Result<Signature, String> {
        self.wallet
            .sign_transaction(tx)
            .await
            .map_err(|e| format!("Failed to sign transaction: {}", e))
    }

    async fn sign_hash(&self, hash: H256) -> Result<Signature, String> {
        self.wallet
            .sign_hash(hash)
            .map_err(|e| format!("Failed to sign hash: {}", e))
    }

    async fn sign_typed_data(&self, typed_data: &serde_json::Value) -> Result<Signature, String> {
        // For EnvWalletProvider, we compute the EIP-712 hash and sign it
        // This is a simplified implementation that expects the hash to be pre-computed
        // or the typed_data to contain a "hash" field for direct signing

        // Check if there's a pre-computed hash
        if let Some(hash_str) = typed_data.get("_hash").and_then(|v| v.as_str()) {
            let hash_hex = hash_str.strip_prefix("0x").unwrap_or(hash_str);
            let hash_bytes = hex::decode(hash_hex)
                .map_err(|e| format!("Invalid hash hex: {}", e))?;
            if hash_bytes.len() != 32 {
                return Err("Hash must be 32 bytes".to_string());
            }
            let hash = H256::from_slice(&hash_bytes);
            return self.sign_hash(hash).await;
        }

        // Otherwise, compute EIP-712 hash from the typed data
        // This requires domain, types, primaryType, and message
        let domain = typed_data.get("domain")
            .ok_or("Missing 'domain' in typed data")?;
        let _primary_type = typed_data.get("primaryType")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'primaryType' in typed data")?;
        let message = typed_data.get("message")
            .ok_or("Missing 'message' in typed data")?;

        // Compute domain separator
        let domain_separator = compute_domain_separator(domain)?;

        // Compute struct hash (simplified - just hash the JSON for now)
        // In production, this should properly encode according to EIP-712
        let struct_hash = H256::from(keccak256(
            serde_json::to_string(message)
                .map_err(|e| format!("Failed to serialize message: {}", e))?
                .as_bytes()
        ));

        // Final hash: keccak256("\x19\x01" ++ domainSeparator ++ structHash)
        let mut to_sign = Vec::with_capacity(66);
        to_sign.push(0x19);
        to_sign.push(0x01);
        to_sign.extend_from_slice(domain_separator.as_bytes());
        to_sign.extend_from_slice(struct_hash.as_bytes());
        let digest = H256::from(keccak256(&to_sign));

        self.sign_hash(digest).await
    }

    fn get_address(&self) -> String {
        self.address.clone()
    }

    async fn get_encryption_key(&self) -> Result<String, String> {
        Ok(self.encryption_key_hex.clone())
    }

    fn mode_name(&self) -> &'static str {
        "standard"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_private_key() {
        // Test with a known test private key (DO NOT USE IN PRODUCTION)
        let test_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let provider = EnvWalletProvider::from_private_key(test_key).unwrap();

        // This is the known address for this test key (Hardhat account #0)
        assert_eq!(
            provider.get_address(),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
    }

    #[test]
    fn test_from_private_key_no_prefix() {
        let test_key = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let provider = EnvWalletProvider::from_private_key(test_key).unwrap();

        assert_eq!(
            provider.get_address(),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
    }

    #[tokio::test]
    async fn test_sign_message() {
        let test_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let provider = EnvWalletProvider::from_private_key(test_key).unwrap();

        let message = b"hello world";
        let signature = provider.sign_message(message).await.unwrap();

        // Just verify we got a valid signature
        assert!(signature.r != ethers::types::U256::zero());
        assert!(signature.s != ethers::types::U256::zero());
    }
}
