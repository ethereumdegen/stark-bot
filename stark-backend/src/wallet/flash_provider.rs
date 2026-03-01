//! Flash Mode Wallet Provider
//!
//! Proxies signing requests to the Flash control plane, which uses Privy
//! for secure key management. The private key never leaves Privy's infrastructure.
//!
//! Required environment variables:
//! - FLASH_KEYSTORE_URL: URL of the Flash control plane (e.g., https://flash.starkbot.io)
//! - FLASH_TENANT_ID: Tenant identifier
//! - FLASH_INSTANCE_TOKEN: Authentication token for this instance

use async_trait::async_trait;
use ethers::types::{H256, Signature, U256, transaction::eip2718::TypedTransaction};
use ethers::utils::rlp;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::WalletProvider;

/// Environment variables for Flash mode
pub mod env_vars {
    pub const FLASH_KEYSTORE_URL: &str = "FLASH_KEYSTORE_URL";
    pub const FLASH_TENANT_ID: &str = "FLASH_TENANT_ID";
    pub const FLASH_INSTANCE_TOKEN: &str = "FLASH_INSTANCE_TOKEN";
}

/// Response from /api/tenantinfo (preferred) — richer instance info
#[derive(Debug, Deserialize)]
struct TenantInfoResponse {
    wallet_id: String,
    admin_address: String,
    domain: Option<String>,
    #[allow(dead_code)]
    tenant_id: Option<String>,
    #[allow(dead_code)]
    display_name: Option<String>,
    #[allow(dead_code)]
    status: Option<String>,
    pub agent_preset: Option<serde_json::Value>,
}

/// Fields extracted from /api/tenantinfo response
struct InitInfo {
    wallet_id: String,
    admin_address: String,
    domain: Option<String>,
    agent_preset: Option<serde_json::Value>,
}

/// Request body for sign-message endpoint
#[derive(Debug, Serialize)]
struct SignMessageRequest {
    message: String,
}

/// Response from sign-message endpoint
#[derive(Debug, Deserialize)]
struct SignMessageResponse {
    signature: String,
}

/// Request body for sign-transaction endpoint
#[derive(Debug, Serialize)]
struct SignTransactionRequest {
    chain_id: u64,
    to: String,
    value: String,
    data: Option<String>,
    gas_limit: Option<String>,
    max_fee_per_gas: Option<String>,
    max_priority_fee_per_gas: Option<String>,
    nonce: Option<u64>,
}

/// Response from sign-transaction endpoint
#[derive(Debug, Deserialize)]
struct SignTransactionResponse {
    signed_transaction: String,
}

/// Request body for sign-typed-data endpoint
#[derive(Debug, Serialize)]
struct SignTypedDataRequest {
    typed_data: serde_json::Value,
}

/// Response from sign-typed-data endpoint
#[derive(Debug, Deserialize)]
struct SignTypedDataResponse {
    signature: String,
}

/// Response from refresh-token endpoint
#[derive(Debug, Deserialize)]
struct RefreshTokenResponse {
    token: String,
}

/// Wallet provider that proxies signing to Flash control plane
pub struct FlashWalletProvider {
    keystore_url: String,
    tenant_id: String,
    instance_token: Arc<RwLock<String>>,
    /// Wallet address - fetched from control plane on init
    address: String,
    /// Privy wallet ID - used for signing requests
    wallet_id: String,
    http_client: reqwest::Client,
    /// Cached ECIES encryption key (derived from signing "starkbot-backup-key-v1")
    encryption_key_hex: tokio::sync::OnceCell<String>,
    /// Instance's public domain from control plane (e.g. "starkbot-abc.starkbot.cloud")
    domain: Option<String>,
    /// Agent preset configuration from control plane (JSONB)
    agent_preset: Option<serde_json::Value>,
}

impl FlashWalletProvider {
    /// Create a new Flash wallet provider from environment variables
    ///
    /// On initialization, fetches wallet info from control plane to:
    /// 1. Validate credentials
    /// 2. Get the wallet address and ID
    pub async fn new() -> Result<Self, String> {
        let keystore_url = std::env::var(env_vars::FLASH_KEYSTORE_URL)
            .map_err(|_| format!("{} not set", env_vars::FLASH_KEYSTORE_URL))?;

        let tenant_id = std::env::var(env_vars::FLASH_TENANT_ID)
            .map_err(|_| format!("{} not set", env_vars::FLASH_TENANT_ID))?;

        let instance_token = std::env::var(env_vars::FLASH_INSTANCE_TOKEN)
            .map_err(|_| format!("{} not set", env_vars::FLASH_INSTANCE_TOKEN))?;

        let http_client = crate::http::shared_client().clone();

        let instance_token = Arc::new(RwLock::new(instance_token));

        // Fetch instance info from control plane
        let token = instance_token.read().await.clone();
        let info = Self::fetch_init_info(&http_client, &keystore_url, &tenant_id, &token).await?;

        log::info!(
            "Flash wallet initialized: {} (wallet_id: {}, domain: {:?})",
            info.admin_address,
            info.wallet_id,
            info.domain
        );

        // If control plane returned a domain, set STARK_PUBLIC_URL so self_url() picks it up
        if let Some(ref domain) = info.domain {
            let url = format!("https://{}", domain);
            log::info!("Setting STARK_PUBLIC_URL from control plane: {}", url);
            // SAFETY: called once during single-threaded init before any concurrent env reads
            unsafe { std::env::set_var(crate::config::env_vars::PUBLIC_URL, &url); }
        }

        Ok(Self {
            keystore_url,
            tenant_id,
            instance_token,
            address: info.admin_address,
            wallet_id: info.wallet_id,
            http_client,
            encryption_key_hex: tokio::sync::OnceCell::new(),
            domain: info.domain,
            agent_preset: info.agent_preset,
        })
    }

    /// Get the instance's public domain from the control plane (if available)
    pub fn domain(&self) -> Option<&str> {
        self.domain.as_deref()
    }

    /// Get the agent preset configuration from the control plane (if set)
    pub fn agent_preset(&self) -> Option<&serde_json::Value> {
        self.agent_preset.as_ref()
    }

    /// Fetch init info from control plane via /api/tenantinfo.
    async fn fetch_init_info(
        http_client: &reqwest::Client,
        keystore_url: &str,
        tenant_id: &str,
        token: &str,
    ) -> Result<InitInfo, String> {
        let url = format!("{}/api/tenantinfo", keystore_url);
        log::info!("Fetching instance info from {}", url);

        let response = http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .header("X-Tenant-ID", tenant_id)
            .header("X-Instance-Token", token)
            .send()
            .await
            .map_err(|e| format!("Flash tenantinfo request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Flash tenantinfo error ({}): {}", status, body));
        }

        let data: TenantInfoResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse tenantinfo response: {}", e))?;

        log::info!("Got tenant info from /api/tenantinfo");
        Ok(InitInfo {
            wallet_id: data.wallet_id,
            admin_address: data.admin_address,
            domain: data.domain,
            agent_preset: data.agent_preset,
        })
    }

    /// Parse an Ethereum signature from hex string
    fn parse_signature(sig_hex: &str) -> Result<Signature, String> {
        let sig_hex = sig_hex.strip_prefix("0x").unwrap_or(sig_hex);

        if sig_hex.len() != 130 {
            return Err(format!(
                "Invalid signature length: expected 130 hex chars, got {}",
                sig_hex.len()
            ));
        }

        let sig_bytes = hex::decode(sig_hex)
            .map_err(|e| format!("Invalid signature hex: {}", e))?;

        if sig_bytes.len() != 65 {
            return Err(format!(
                "Invalid signature bytes: expected 65, got {}",
                sig_bytes.len()
            ));
        }

        let r = U256::from_big_endian(&sig_bytes[0..32]);
        let s = U256::from_big_endian(&sig_bytes[32..64]);
        let v_byte = sig_bytes[64];

        // Normalize v value: Privy may return 0/1, but ecrecover needs 27/28
        // EIP-712 typed data signatures use v = 27 or 28
        let v = if v_byte < 27 {
            v_byte as u64 + 27
        } else {
            v_byte as u64
        };

        log::debug!("Parsed signature: v={} (raw byte was {})", v, v_byte);

        Ok(Signature { r, s, v })
    }

    /// Extract (v, r, s) from a Privy-returned RLP-encoded signed transaction.
    ///
    /// For EIP-1559 (type 2), the format is:
    ///   0x02 ++ rlp([chain_id, nonce, max_priority_fee, max_fee, gas, to, value, data, access_list, y_parity, r, s])
    /// The last 3 items in the RLP list are the signature: y_parity, r, s.
    fn extract_signature_from_signed_tx(signed_tx_hex: &str) -> Result<Signature, String> {
        let hex_str = signed_tx_hex.strip_prefix("0x").unwrap_or(signed_tx_hex);
        let tx_bytes = hex::decode(hex_str)
            .map_err(|e| format!("Invalid signed tx hex: {}", e))?;

        if tx_bytes.is_empty() {
            return Err("Empty signed transaction".to_string());
        }

        // EIP-2718 typed transactions start with a type byte (0x01 or 0x02)
        // followed by the RLP payload
        let tx_type = tx_bytes[0];
        if tx_type != 0x02 && tx_type != 0x01 {
            return Err(format!(
                "Unsupported transaction type: 0x{:02x} (expected 0x01 or 0x02)",
                tx_type
            ));
        }

        // Decode the RLP list after the type byte
        let rlp_data = rlp::Rlp::new(&tx_bytes[1..]);
        let item_count = rlp_data.item_count()
            .map_err(|e| format!("Failed to decode RLP: {}", e))?;

        // EIP-1559 (type 2): exactly 12 items [chain_id, nonce, max_priority, max_fee, gas, to, value, data, access_list, y_parity, r, s]
        // EIP-2930 (type 1): exactly 11 items [chain_id, nonce, gas_price, gas, to, value, data, access_list, y_parity, r, s]
        let expected_count = if tx_type == 0x02 { 12 } else { 11 };
        if item_count != expected_count {
            return Err(format!(
                "Unexpected RLP item count for type 0x{:02x}: {} (expected {})",
                tx_type, item_count, expected_count
            ));
        }

        // Signature is always the last 3 items: y_parity, r, s
        let y_parity: u64 = rlp_data.val_at(item_count - 3)
            .map_err(|e| format!("Failed to decode y_parity: {}", e))?;
        let r: U256 = rlp_data.val_at(item_count - 2)
            .map_err(|e| format!("Failed to decode r: {}", e))?;
        let s: U256 = rlp_data.val_at(item_count - 1)
            .map_err(|e| format!("Failed to decode s: {}", e))?;

        if y_parity > 1 {
            return Err(format!("Invalid y_parity: {} (expected 0 or 1)", y_parity));
        }

        // For EIP-1559/2930 typed transactions, v = y_parity (0 or 1)
        // ethers expects v as 0 or 1 for typed transactions (NOT 27/28)
        let v = y_parity;

        log::debug!(
            "Extracted signature from signed tx (type 0x{:02x}): v={}, r={:#x}, s={:#x}",
            tx_type, v, r, s
        );

        Ok(Signature { r, s, v })
    }

    /// Refresh the instance token by calling the control plane's refresh endpoint.
    /// The control plane accepts expired tokens (up to 30 days old) and returns a fresh one.
    async fn refresh_instance_token(&self) -> Result<(), String> {
        let url = format!("{}/api/keystore/refresh-token", self.keystore_url);
        let old_token = self.instance_token.read().await.clone();

        log::info!("Refreshing expired instance token...");

        let response = self.http_client
            .post(&url)
            .timeout(std::time::Duration::from_secs(30))
            .header("X-Tenant-ID", &self.tenant_id)
            .header("X-Instance-Token", &old_token)
            .send()
            .await
            .map_err(|e| format!("Token refresh request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Token refresh failed ({}): {}", status, body));
        }

        let data: RefreshTokenResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse refresh response: {}", e))?;

        let mut token = self.instance_token.write().await;
        *token = data.token;

        log::info!("Instance token refreshed successfully");
        Ok(())
    }

    /// Send a POST request with instance token auth headers.
    /// On 401, attempts to refresh the token and retry once.
    async fn post_with_retry<T: Serialize + ?Sized>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<reqwest::Response, String> {
        let token = self.instance_token.read().await.clone();

        let response = self.http_client
            .post(url)
            .timeout(std::time::Duration::from_secs(30))
            .header("X-Tenant-ID", &self.tenant_id)
            .header("X-Instance-Token", &token)
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            log::warn!("Got 401 from control plane, attempting token refresh...");

            self.refresh_instance_token().await?;

            let new_token = self.instance_token.read().await.clone();
            let retry_response = self.http_client
                .post(url)
                .timeout(std::time::Duration::from_secs(30))
                .header("X-Tenant-ID", &self.tenant_id)
                .header("X-Instance-Token", &new_token)
                .json(body)
                .send()
                .await
                .map_err(|e| format!("Retry request failed: {}", e))?;

            return Ok(retry_response);
        }

        Ok(response)
    }
}

#[async_trait]
impl WalletProvider for FlashWalletProvider {
    async fn sign_message(&self, message: &[u8]) -> Result<Signature, String> {
        log::debug!("Signing message via Flash control plane");

        let url = format!("{}/api/keystore/sign-message", self.keystore_url);
        let message_str = String::from_utf8_lossy(message).to_string();
        let request = SignMessageRequest { message: message_str };

        let response = self.post_with_retry(&url, &request).await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Sign message failed ({}): {}", status, body));
        }

        let data: SignMessageResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse sign response: {}", e))?;

        Self::parse_signature(&data.signature)
    }

    async fn sign_transaction(&self, tx: &TypedTransaction) -> Result<Signature, String> {
        log::debug!("Signing transaction via Flash control plane");

        let url = format!("{}/api/keystore/sign-transaction", self.keystore_url);

        // Extract transaction fields
        let chain_id = tx.chain_id()
            .ok_or("Transaction missing chain_id")?
            .as_u64();

        let to = tx.to()
            .ok_or("Transaction missing 'to' address")?
            .as_address()
            .ok_or("Invalid 'to' address")?;

        let value = tx.value().cloned().unwrap_or_default();
        let data = tx.data().map(|d| format!("0x{}", hex::encode(d)));

        // Extract EIP-1559 gas fields if available
        let (max_fee, priority_fee) = match tx {
            TypedTransaction::Eip1559(eip1559) => (
                eip1559.max_fee_per_gas.map(|g| g.to_string()),
                eip1559.max_priority_fee_per_gas.map(|g| g.to_string()),
            ),
            _ => (None, None),
        };

        let request = SignTransactionRequest {
            chain_id,
            to: format!("{:?}", to),
            value: value.to_string(),
            data,
            gas_limit: tx.gas().map(|g| g.to_string()),
            max_fee_per_gas: max_fee,
            max_priority_fee_per_gas: priority_fee,
            nonce: tx.nonce().map(|n| n.as_u64()),
        };

        let response = self.post_with_retry(&url, &request).await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Sign transaction failed ({}): {}", status, body));
        }

        let data: SignTransactionResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse sign response: {}", e))?;

        // Privy returns the full RLP-encoded signed transaction.
        // Extract (v, r, s) signature components from it.
        Self::extract_signature_from_signed_tx(&data.signed_transaction)
    }

    async fn sign_hash(&self, hash: H256) -> Result<Signature, String> {
        // In Flash mode, we can't sign raw hashes directly via Privy
        // Instead, we wrap the hash in a minimal typed data structure
        // This uses a simple "SignHash" type that just contains the hash
        let typed_data = serde_json::json!({
            "domain": {
                "name": "Starkbot",
                "version": "1",
                "chainId": 1
            },
            "types": {
                "EIP712Domain": [
                    {"name": "name", "type": "string"},
                    {"name": "version", "type": "string"},
                    {"name": "chainId", "type": "uint256"}
                ],
                "SignHash": [
                    {"name": "hash", "type": "bytes32"}
                ]
            },
            "primaryType": "SignHash",
            "message": {
                "hash": format!("0x{}", hex::encode(hash.as_bytes()))
            },
            "_hash": format!("0x{}", hex::encode(hash.as_bytes()))
        });

        self.sign_typed_data(&typed_data).await
    }

    async fn sign_typed_data(&self, typed_data: &serde_json::Value) -> Result<Signature, String> {
        log::debug!("Signing typed data via Flash control plane");

        let url = format!("{}/api/keystore/sign-typed-data", self.keystore_url);

        let request = SignTypedDataRequest {
            typed_data: typed_data.clone(),
        };

        let response = self.post_with_retry(&url, &request).await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Sign typed data failed ({}): {}", status, body));
        }

        let data: SignTypedDataResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse sign response: {}", e))?;

        Self::parse_signature(&data.signature)
    }

    fn get_address(&self) -> String {
        self.address.clone()
    }

    async fn get_encryption_key(&self) -> Result<String, String> {
        self.encryption_key_hex
            .get_or_try_init(|| async {
                let sig = self.sign_message(b"starkbot-backup-key-v1").await?;
                let sig_bytes = sig.to_vec();
                let derived_key = ethers::utils::keccak256(&sig_bytes);
                log::info!("Flash mode: derived ECIES encryption key from wallet signature");
                Ok(hex::encode(derived_key))
            })
            .await
            .cloned()
    }

    async fn refresh(&self) -> Result<(), String> {
        log::info!("Flash wallet refresh requested (no-op - wallet ID is stable)");
        Ok(())
    }

    fn mode_name(&self) -> &'static str {
        "flash"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::utils::rlp::RlpStream;

    #[test]
    fn test_env_vars_defined() {
        assert_eq!(env_vars::FLASH_KEYSTORE_URL, "FLASH_KEYSTORE_URL");
        assert_eq!(env_vars::FLASH_TENANT_ID, "FLASH_TENANT_ID");
        assert_eq!(env_vars::FLASH_INSTANCE_TOKEN, "FLASH_INSTANCE_TOKEN");
    }

    // ── helpers ──────────────────────────────────────────────────────

    /// Build a type-2 (EIP-1559) signed tx with known signature values.
    /// Returns the hex string WITH "0x" prefix.
    fn build_signed_eip1559_tx(chain_id: u64, v: u64, r: U256, s: U256) -> String {
        let mut stream = RlpStream::new_list(12);
        stream.append(&chain_id);      // chain_id
        stream.append(&0u64);          // nonce
        stream.append(&1_000_000_000u64); // max_priority_fee_per_gas
        stream.append(&2_000_000_000u64); // max_fee_per_gas
        stream.append(&21_000u64);     // gas_limit
        stream.append(&ethers::types::H160::zero()); // to
        stream.append(&U256::from(1_000_000u64));     // value
        stream.append(&Vec::<u8>::new()); // data (empty)
        // access_list (empty list)
        stream.begin_list(0);
        stream.append(&v);             // y_parity
        stream.append(&r);             // r
        stream.append(&s);             // s
        let rlp_bytes = stream.out();
        // type byte 0x02 + RLP payload
        let mut tx_bytes = vec![0x02u8];
        tx_bytes.extend_from_slice(&rlp_bytes);
        format!("0x{}", hex::encode(&tx_bytes))
    }

    /// Build a type-1 (EIP-2930) signed tx with known signature values.
    fn build_signed_eip2930_tx(chain_id: u64, v: u64, r: U256, s: U256) -> String {
        let mut stream = RlpStream::new_list(11);
        stream.append(&chain_id);      // chain_id
        stream.append(&0u64);          // nonce
        stream.append(&1_000_000_000u64); // gas_price
        stream.append(&21_000u64);     // gas_limit
        stream.append(&ethers::types::H160::zero()); // to
        stream.append(&U256::from(1_000_000u64));     // value
        stream.append(&Vec::<u8>::new()); // data
        // access_list (empty list)
        stream.begin_list(0);
        stream.append(&v);             // y_parity
        stream.append(&r);             // r
        stream.append(&s);             // s
        let rlp_bytes = stream.out();
        let mut tx_bytes = vec![0x01u8];
        tx_bytes.extend_from_slice(&rlp_bytes);
        format!("0x{}", hex::encode(&tx_bytes))
    }

    // ── parse_signature tests ───────────────────────────────────────

    #[test]
    fn test_parse_signature() {
        // A valid 65-byte signature in hex (130 chars)
        let sig_hex = "0x".to_string() + &"a".repeat(128) + "1b";
        let sig = FlashWalletProvider::parse_signature(&sig_hex).unwrap();
        assert_eq!(sig.v, 27); // 0x1b = 27
    }

    #[test]
    fn test_parse_signature_v_normalization() {
        // v=0 should be normalized to 27
        let sig_hex_v0 = "0x".to_string() + &"a".repeat(128) + "00";
        let sig = FlashWalletProvider::parse_signature(&sig_hex_v0).unwrap();
        assert_eq!(sig.v, 27);

        // v=1 should be normalized to 28
        let sig_hex_v1 = "0x".to_string() + &"a".repeat(128) + "01";
        let sig = FlashWalletProvider::parse_signature(&sig_hex_v1).unwrap();
        assert_eq!(sig.v, 28);
    }

    #[test]
    fn test_parse_signature_wrong_length() {
        let short = "0x".to_string() + &"a".repeat(64);
        let err = FlashWalletProvider::parse_signature(&short).unwrap_err();
        assert!(err.contains("Invalid signature length"), "got: {}", err);
    }

    #[test]
    fn test_parse_signature_invalid_hex() {
        // 130 chars but not valid hex
        let bad = "0x".to_string() + &"zz".repeat(65);
        let err = FlashWalletProvider::parse_signature(&bad).unwrap_err();
        // Could fail at length check (zz repeat is 130 chars) or hex decode
        assert!(
            err.contains("Invalid signature") || err.contains("hex"),
            "got: {}",
            err
        );
    }

    // ── extract_signature_from_signed_tx tests ──────────────────────

    #[test]
    fn test_extract_sig_eip1559_v0() {
        let r = U256::from(0xdeadbeef_u64);
        let s = U256::from(0xcafebabe_u64);
        let tx = build_signed_eip1559_tx(1, 0, r, s);
        let sig = FlashWalletProvider::extract_signature_from_signed_tx(&tx).unwrap();
        assert_eq!(sig.v, 0);
        assert_eq!(sig.r, r);
        assert_eq!(sig.s, s);
    }

    #[test]
    fn test_extract_sig_eip1559_v1() {
        let r = U256::from(0x1234_u64);
        let s = U256::from(0x5678_u64);
        let tx = build_signed_eip1559_tx(1, 1, r, s);
        let sig = FlashWalletProvider::extract_signature_from_signed_tx(&tx).unwrap();
        assert_eq!(sig.v, 1);
        assert_eq!(sig.r, r);
        assert_eq!(sig.s, s);
    }

    #[test]
    fn test_extract_sig_eip2930() {
        let r = U256::from(0xabcd_u64);
        let s = U256::from(0xef01_u64);
        let tx = build_signed_eip2930_tx(1, 0, r, s);
        let sig = FlashWalletProvider::extract_signature_from_signed_tx(&tx).unwrap();
        assert_eq!(sig.v, 0);
        assert_eq!(sig.r, r);
        assert_eq!(sig.s, s);
    }

    #[test]
    fn test_extract_sig_with_0x_prefix() {
        let r = U256::from(1u64);
        let s = U256::from(2u64);
        let tx = build_signed_eip1559_tx(1, 0, r, s);
        assert!(tx.starts_with("0x"));
        let sig = FlashWalletProvider::extract_signature_from_signed_tx(&tx).unwrap();
        assert_eq!(sig.r, r);
        assert_eq!(sig.s, s);
    }

    #[test]
    fn test_extract_sig_without_prefix() {
        let r = U256::from(1u64);
        let s = U256::from(2u64);
        let tx = build_signed_eip1559_tx(1, 0, r, s);
        let tx_no_prefix = tx.strip_prefix("0x").unwrap();
        let sig = FlashWalletProvider::extract_signature_from_signed_tx(tx_no_prefix).unwrap();
        assert_eq!(sig.r, r);
        assert_eq!(sig.s, s);
    }

    #[test]
    fn test_extract_sig_empty_tx() {
        let err = FlashWalletProvider::extract_signature_from_signed_tx("").unwrap_err();
        assert!(err.contains("Invalid signed tx hex") || err.contains("Empty"), "got: {}", err);
    }

    #[test]
    fn test_extract_sig_unsupported_type() {
        // Legacy transaction (type 0xc0+ is RLP list prefix, not typed tx)
        // Single byte 0x00 is "type 0" which we don't support
        let err = FlashWalletProvider::extract_signature_from_signed_tx("0x00aabbcc").unwrap_err();
        assert!(err.contains("Unsupported transaction type"), "got: {}", err);

        // Unknown type 0x05
        let err = FlashWalletProvider::extract_signature_from_signed_tx("0x05aabbcc").unwrap_err();
        assert!(err.contains("Unsupported transaction type"), "got: {}", err);
    }

    #[test]
    fn test_extract_sig_invalid_hex() {
        let err = FlashWalletProvider::extract_signature_from_signed_tx("0xZZZZZZ").unwrap_err();
        assert!(err.contains("Invalid signed tx hex"), "got: {}", err);
    }

    #[test]
    fn test_extract_sig_truncated_rlp() {
        // Type byte 0x02 followed by truncated/garbage RLP
        let err = FlashWalletProvider::extract_signature_from_signed_tx("0x02ff").unwrap_err();
        assert!(err.contains("Failed to decode RLP") || err.contains("Unexpected RLP item count"), "got: {}", err);
    }

    #[test]
    fn test_extract_sig_invalid_y_parity() {
        // Build a tx with y_parity = 5 (invalid, must be 0 or 1)
        let r = U256::from(1u64);
        let s = U256::from(2u64);
        let tx = build_signed_eip1559_tx(1, 5, r, s);
        let err = FlashWalletProvider::extract_signature_from_signed_tx(&tx).unwrap_err();
        assert!(err.contains("Invalid y_parity"), "got: {}", err);
    }

    #[test]
    fn test_extract_sig_wrong_item_count() {
        // Build a type-2 RLP with only 11 items (should be 12)
        let mut stream = RlpStream::new_list(11);
        stream.append(&1u64);          // chain_id
        stream.append(&0u64);          // nonce
        stream.append(&1_000_000_000u64); // max_priority_fee
        stream.append(&2_000_000_000u64); // max_fee
        stream.append(&21_000u64);     // gas
        stream.append(&ethers::types::H160::zero()); // to
        stream.append(&U256::from(1_000_000u64));     // value
        stream.append(&Vec::<u8>::new()); // data
        stream.append(&0u64);          // y_parity
        stream.append(&U256::from(1u64)); // r
        stream.append(&U256::from(2u64)); // s
        // Missing access_list — only 11 items for type-2 (should be 12)
        let rlp_bytes = stream.out();
        let mut tx_bytes = vec![0x02u8];
        tx_bytes.extend_from_slice(&rlp_bytes);
        let tx_hex = format!("0x{}", hex::encode(&tx_bytes));

        let err = FlashWalletProvider::extract_signature_from_signed_tx(&tx_hex).unwrap_err();
        assert!(err.contains("Unexpected RLP item count"), "got: {}", err);
    }
}
