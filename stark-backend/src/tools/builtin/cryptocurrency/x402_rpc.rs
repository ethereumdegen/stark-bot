//! x402 RPC tool for making paid EVM RPC calls via DeFi Relay
//!
//! Uses presets to build RPC params from register values, preventing hallucination.
//! Supports configurable RPC endpoints via bot settings.

use crate::tools::http_retry::HttpRetryManager;
use crate::tools::presets::{get_rpc_preset, list_rpc_presets};
use crate::tools::registry::Tool;
use crate::tools::rpc_config::resolve_rpc_from_context;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::x402::X402Client;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// JSON-RPC request structure
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
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

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[allow(dead_code)]
    data: Option<Value>,
}

/// x402 RPC tool for paid EVM RPC calls (preset-only)
pub struct X402RpcTool {
    definition: ToolDefinition,
    client: Arc<RwLock<Option<X402Client>>>,
}

impl X402RpcTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "preset".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "RPC preset. Available: 'gas_price', 'block_number', 'get_balance' (reads wallet_address), 'get_nonce' (reads wallet_address). Presets read from registers automatically.".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "gas_price".to_string(),
                    "block_number".to_string(),
                    "get_balance".to_string(),
                    "get_nonce".to_string(),
                ]),
            },
        );

        properties.insert(
            "network".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Network: 'base' or 'mainnet'".to_string(),
                default: Some(json!("base")),
                items: None,
                enum_values: Some(vec!["base".to_string(), "mainnet".to_string()]),
            },
        );

        X402RpcTool {
            definition: ToolDefinition {
                name: "x402_rpc".to_string(),
                description: "Make paid EVM RPC calls using presets. Presets read from registers. Available: gas_price, block_number, get_balance, get_nonce.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["preset".to_string()],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
            client: Arc::new(RwLock::new(None)),
        }
    }

    /// Get or create the x402 client
    /// Uses wallet_provider from context if available (Flash mode), otherwise falls back to env var
    fn get_client(&self, context: &ToolContext) -> Result<X402Client, String> {
        // Try wallet_provider from context first (works in both Standard and Flash mode)
        if let Some(ref wallet_provider) = context.wallet_provider {
            return X402Client::new(wallet_provider.clone());
        }

        // Fall back to private key from environment (Standard mode only)
        let private_key = crate::config::burner_wallet_private_key()
            .ok_or("No wallet provider in context and BURNER_WALLET_BOT_PRIVATE_KEY not set")?;

        X402Client::from_private_key(&private_key)
    }
}

impl Default for X402RpcTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct X402RpcParams {
    preset: String,
    #[serde(default = "default_network")]
    network: String,
}

fn default_network() -> String {
    "base".to_string()
}

#[async_trait]
impl Tool for X402RpcTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: X402RpcParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Validate network
        if params.network != "base" && params.network != "mainnet" {
            return ToolResult::error("Network must be 'base' or 'mainnet'");
        }

        // Get preset configuration
        let preset = match get_rpc_preset(&params.preset) {
            Some(p) => p,
            None => {
                return ToolResult::error(format!(
                    "Unknown preset: '{}'. Available: {}",
                    params.preset,
                    list_rpc_presets().join(", ")
                ))
            }
        };

        // Build params from registers
        let mut param_values: Vec<Value> = Vec::new();
        for reg_key in &preset.params {
            let value = match context.registers.get(reg_key) {
                Some(v) => match v.as_str() {
                    Some(s) => json!(s),
                    None => v,
                },
                None => {
                    return ToolResult::error(format!(
                        "Preset '{}' requires register '{}' but it was not found. Available: {:?}",
                        params.preset,
                        reg_key,
                        context.registers.keys()
                    ));
                }
            };
            param_values.push(value);
        }

        // Append "latest" if needed
        if preset.append_latest {
            param_values.push(json!("latest"));
        }

        // Resolve RPC configuration from context (respects custom RPC settings)
        let rpc_config = resolve_rpc_from_context(&context.extra, &params.network);

        log::info!(
            "[x402_rpc] Preset '{}' -> {} with {} params on {} (rpc={})",
            params.preset,
            preset.method,
            param_values.len(),
            params.network,
            rpc_config.url
        );

        // Build JSON-RPC request
        let rpc_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: preset.method.clone(),
            params: json!(param_values),
            id: 1,
        };

        // Get the x402 client
        let client = match self.get_client(context) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(e),
        };

        // Setup retry tracking
        let retry_key = format!("x402_rpc:{}:{}", params.network, params.preset);
        let retry_manager = HttpRetryManager::global();

        // Make the request (with or without x402 payment based on config)
        let response = if rpc_config.use_x402 {
            client.post_with_payment(&rpc_config.url, &rpc_request).await
        } else {
            client.post_regular(&rpc_config.url, &rpc_request).await
        };

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                let error_msg = format!("RPC request failed: {}", e);
                if HttpRetryManager::is_retryable_error(&error_msg) {
                    let delay = retry_manager.record_error(&retry_key);
                    return ToolResult::retryable_error(error_msg, delay);
                }
                return ToolResult::error(error_msg);
            }
        };

        // Check HTTP status
        let status = response.response.status();
        if !status.is_success() {
            let body = response.response.text().await.unwrap_or_default();
            let error_msg = format!("HTTP error {}: {}", status, body);
            if HttpRetryManager::is_retryable_status(status.as_u16()) {
                let delay = retry_manager.record_error(&retry_key);
                return ToolResult::retryable_error(error_msg, delay);
            }
            return ToolResult::error(error_msg);
        }

        // Success - reset backoff
        retry_manager.record_success(&retry_key);

        // Parse response
        let body = match response.response.text().await {
            Ok(b) => b,
            Err(e) => return ToolResult::error(format!("Failed to read response: {}", e)),
        };

        let rpc_response: JsonRpcResponse = match serde_json::from_str(&body) {
            Ok(r) => r,
            Err(e) => {
                return ToolResult::error(format!(
                    "Invalid JSON-RPC response: {} - Body: {}",
                    e, body
                ))
            }
        };

        // Check for RPC error
        if let Some(error) = rpc_response.error {
            return ToolResult::error(format!("RPC error {}: {}", error.code, error.message));
        }

        // Build metadata
        let mut metadata = json!({
            "preset": params.preset,
            "method": preset.method,
            "network": params.network,
            "wallet": client.wallet_address(),
        });

        if let Some(payment) = response.payment {
            metadata["payment"] = json!({
                "amount": payment.amount_formatted,
                "asset": payment.asset,
                "pay_to": payment.pay_to,
            });
        }

        // Return the result
        match rpc_response.result {
            Some(result) => ToolResult::success(
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
            )
            .with_metadata(metadata),
            None => ToolResult::success("null").with_metadata(metadata),
        }
    }
}
