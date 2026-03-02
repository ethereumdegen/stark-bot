//! X402-backed EVM RPC client
//!
//! Provides high-level EVM RPC methods using defirelay.com with x402 payments.
//! RPC calls can go through x402 payment protocol or regular HTTP depending on config.

use ethers::types::{Address, Bytes, H256, U256, U64};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

use super::client::CreditsAuthClient;
use crate::wallet::WalletProvider;

/// Default RPC endpoints for defirelay (used when no custom config)
const DEFAULT_RPC_BASE: &str = "https://rpc.defirelay.com/rpc/light/base";
const DEFAULT_RPC_MAINNET: &str = "https://rpc.defirelay.com/rpc/light/mainnet";

/// X402-backed EVM RPC client
pub struct X402EvmRpc {
    client: CreditsAuthClient,
    network: String,
    /// Custom RPC URL (overrides default based on network)
    rpc_url: Option<String>,
    /// Whether to use x402 payment for RPC calls
    use_x402: bool,
}

/// JSON-RPC request structure
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    method: String,
    params: Value,
    id: u64,
}

/// JSON-RPC response structure
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    result: Option<Value>,
    error: Option<JsonRpcError>,
    #[allow(dead_code)]
    id: u64,
}

/// JSON-RPC error
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

/// Transaction receipt from eth_getTransactionReceipt
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionReceipt {
    pub transaction_hash: H256,
    pub block_hash: Option<H256>,
    pub block_number: Option<U64>,
    pub status: Option<U64>,
    pub gas_used: Option<U256>,
    pub effective_gas_price: Option<U256>,
    #[serde(default)]
    pub logs: Vec<TxLog>,
}

/// A single log entry from a transaction receipt
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxLog {
    pub address: Address,
    pub topics: Vec<H256>,
    pub data: Bytes,
}

impl X402EvmRpc {
    /// Create a new X402 EVM RPC client with default settings (x402 enabled)
    pub fn new(private_key: &str, network: &str) -> Result<Self, String> {
        let client = CreditsAuthClient::from_private_key(private_key)?;
        Ok(Self {
            client,
            network: network.to_string(),
            rpc_url: None,
            use_x402: true,
        })
    }

    /// Create a new X402 EVM RPC client with custom configuration
    pub fn new_with_config(
        private_key: &str,
        network: &str,
        rpc_url: Option<String>,
        use_x402: bool,
    ) -> Result<Self, String> {
        let client = CreditsAuthClient::from_private_key(private_key)?;
        Ok(Self {
            client,
            network: network.to_string(),
            rpc_url,
            use_x402,
        })
    }

    /// Create a new X402 EVM RPC client with a WalletProvider (for Flash mode)
    /// This uses the WalletProvider for x402 payment signing instead of a raw private key
    pub fn new_with_wallet_provider(
        wallet_provider: Arc<dyn WalletProvider>,
        network: &str,
        rpc_url: Option<String>,
        use_x402: bool,
    ) -> Result<Self, String> {
        let client = CreditsAuthClient::new(wallet_provider)?;
        Ok(Self {
            client,
            network: network.to_string(),
            rpc_url,
            use_x402,
        })
    }

    /// Get the RPC endpoint URL for the current network
    fn rpc_url(&self) -> String {
        if let Some(ref url) = self.rpc_url {
            url.clone()
        } else {
            match self.network.as_str() {
                "mainnet" => DEFAULT_RPC_MAINNET.to_string(),
                _ => DEFAULT_RPC_BASE.to_string(),
            }
        }
    }

    /// Check if x402 payment is enabled
    pub fn uses_x402(&self) -> bool {
        self.use_x402
    }

    /// Get the chain ID for the current network
    pub fn chain_id(&self) -> u64 {
        match self.network.as_str() {
            "mainnet" => 1,
            _ => 8453, // Base
        }
    }

    /// Make a JSON-RPC call via x402 or regular HTTP depending on config
    async fn rpc_call(&self, method: &str, params: Value) -> Result<Value, String> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
            id: 1,
        };

        let url = self.rpc_url();
        log::debug!("[X402EvmRpc] {} to {} with params: {:?} (x402={})", method, url, request.params, self.use_x402);

        let response = if self.use_x402 {
            self.client.post_with_payment(&url, &request).await?
        } else {
            self.client.post_regular(&url, &request).await?
        };

        let status = response.response.status();
        let body = response.response.text().await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        if !status.is_success() {
            return Err(format!("RPC error ({}) from {}: {}", status, url, if body.is_empty() { "empty response" } else { &body }));
        }

        let rpc_response: JsonRpcResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse RPC response: {} - body: {}", e, body))?;

        if let Some(error) = rpc_response.error {
            return Err(format!("RPC error {}: {}", error.code, error.message));
        }

        rpc_response.result.ok_or_else(|| "RPC returned null result".to_string())
    }

    /// Get ETH balance of an address
    /// Returns balance in wei
    pub async fn get_balance(&self, address: Address) -> Result<U256, String> {
        let params = json!([format!("{:?}", address), "latest"]);
        let result = self.rpc_call("eth_getBalance", params).await?;

        let hex_str = result.as_str()
            .ok_or_else(|| "Invalid balance response".to_string())?;

        U256::from_str_radix(hex_str.trim_start_matches("0x"), 16)
            .map_err(|e| format!("Failed to parse balance: {}", e))
    }

    /// Make an eth_call (read-only contract call) - returns raw bytes
    pub async fn call(&self, to: Address, data: &[u8]) -> Result<Vec<u8>, String> {
        let result = self.eth_call(to, data).await?;
        Ok(result.to_vec())
    }

    /// Make an eth_call (read-only contract call)
    pub async fn eth_call(&self, to: Address, data: &[u8]) -> Result<Bytes, String> {
        let params = json!([
            {
                "to": format!("{:?}", to),
                "data": format!("0x{}", hex::encode(data))
            },
            "latest"
        ]);

        let result = self.rpc_call("eth_call", params).await?;

        let hex_str = result.as_str()
            .ok_or_else(|| "Invalid eth_call response".to_string())?;

        let bytes = hex::decode(hex_str.trim_start_matches("0x"))
            .map_err(|e| format!("Failed to decode eth_call result: {}", e))?;

        Ok(Bytes::from(bytes))
    }

    /// Estimate gas for a transaction
    pub async fn estimate_gas(
        &self,
        from: Address,
        to: Address,
        data: &[u8],
        value: U256,
    ) -> Result<U256, String> {
        let params = json!([
            {
                "from": format!("{:?}", from),
                "to": format!("{:?}", to),
                "data": format!("0x{}", hex::encode(data)),
                "value": format!("0x{:x}", value)
            }
        ]);

        let result = self.rpc_call("eth_estimateGas", params).await?;

        let hex_str = result.as_str()
            .ok_or_else(|| "Invalid estimateGas response".to_string())?;

        U256::from_str_radix(hex_str.trim_start_matches("0x"), 16)
            .map_err(|e| format!("Failed to parse gas estimate: {}", e))
    }

    /// Estimate EIP-1559 fees (max_fee_per_gas, max_priority_fee_per_gas)
    pub async fn estimate_eip1559_fees(&self) -> Result<(U256, U256), String> {
        // Get base fee from eth_gasPrice
        let gas_price_result = self.rpc_call("eth_gasPrice", json!([])).await?;
        let gas_price_hex = gas_price_result.as_str()
            .ok_or_else(|| "Invalid gasPrice response".to_string())?;
        let gas_price = U256::from_str_radix(gas_price_hex.trim_start_matches("0x"), 16)
            .map_err(|e| format!("Failed to parse gas price: {}", e))?;

        // Get priority fee from eth_maxPriorityFeePerGas
        let priority_result = self.rpc_call("eth_maxPriorityFeePerGas", json!([])).await?;
        let priority_hex = priority_result.as_str()
            .ok_or_else(|| "Invalid maxPriorityFeePerGas response".to_string())?;
        let priority_fee = U256::from_str_radix(priority_hex.trim_start_matches("0x"), 16)
            .map_err(|e| format!("Failed to parse priority fee: {}", e))?;

        // For L2s (Base), eth_gasPrice is usually the appropriate maxFeePerGas.
        // eth_maxPriorityFeePerGas can return unexpectedly high values from some RPC providers.
        // Cap priority_fee to be at most equal to gas_price to avoid insane estimates.
        let capped_priority_fee = std::cmp::min(priority_fee, gas_price);

        // Add a small buffer (10%) to gas_price for max_fee
        let max_fee = gas_price + gas_price / 10;

        log::debug!(
            "[X402EvmRpc] Gas estimate: gas_price={}, priority_fee={} (capped from {}), max_fee={}",
            gas_price, capped_priority_fee, priority_fee, max_fee
        );

        Ok((max_fee, capped_priority_fee))
    }

    /// Send a raw signed transaction
    pub async fn send_raw_transaction(&self, signed_tx: &[u8]) -> Result<H256, String> {
        let params = json!([format!("0x{}", hex::encode(signed_tx))]);

        let result = self.rpc_call("eth_sendRawTransaction", params).await?;

        let hash_hex = result.as_str()
            .ok_or_else(|| "Invalid sendRawTransaction response".to_string())?;

        hash_hex.parse()
            .map_err(|e| format!("Failed to parse tx hash: {}", e))
    }

    /// Get transaction receipt
    pub async fn get_transaction_receipt(&self, tx_hash: H256) -> Result<Option<TransactionReceipt>, String> {
        let params = json!([format!("{:?}", tx_hash)]);

        let result = self.rpc_call("eth_getTransactionReceipt", params).await?;

        if result.is_null() {
            return Ok(None);
        }

        let receipt: TransactionReceipt = serde_json::from_value(result)
            .map_err(|e| format!("Failed to parse receipt: {}", e))?;

        Ok(Some(receipt))
    }

    /// Get transaction count (nonce) for an address
    pub async fn get_transaction_count(&self, address: Address) -> Result<U256, String> {
        let params = json!([format!("{:?}", address), "pending"]);

        let result = self.rpc_call("eth_getTransactionCount", params).await?;

        let hex_str = result.as_str()
            .ok_or_else(|| "Invalid getTransactionCount response".to_string())?;

        U256::from_str_radix(hex_str.trim_start_matches("0x"), 16)
            .map_err(|e| format!("Failed to parse nonce: {}", e))
    }

    /// Wait for a transaction receipt with polling
    pub async fn wait_for_receipt(
        &self,
        tx_hash: H256,
        timeout: Duration,
    ) -> Result<TransactionReceipt, String> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_secs(2);

        loop {
            if start.elapsed() > timeout {
                return Err(format!("Timeout waiting for tx receipt: {:?}", tx_hash));
            }

            match self.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => return Ok(receipt),
                Ok(None) => {
                    log::debug!("[X402EvmRpc] Waiting for receipt of {:?}...", tx_hash);
                    tokio::time::sleep(poll_interval).await;
                }
                Err(e) => {
                    log::warn!("[X402EvmRpc] Error fetching receipt: {}, retrying...", e);
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }
}
