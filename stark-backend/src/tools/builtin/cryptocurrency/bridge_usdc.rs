//! Bridge USDC Tool - Cross-chain USDC bridging via Across Protocol
//!
//! Bridges USDC between supported chains using Across Protocol's fast bridge.
//! Supports: Ethereum, Base, Polygon, Arbitrum, Optimism
//!
//! Features:
//! - ~2 second fill times via Across relayers
//! - Native CCTP integration for USDC
//! - Automatic approval handling
//! - Transaction queuing (not auto-broadcast)
//!
//! ## Usage
//! ```json
//! {
//!     "from_chain": "base",
//!     "to_chain": "polygon",
//!     "amount": "100"  // 100 USDC
//! }
//! ```

use super::verify_intent::{self, TransactionIntent};
use crate::tools::registry::Tool;
use crate::tools::rpc_config::{resolve_rpc_from_context, ResolvedRpcConfig};
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tx_queue::QueuedTransaction;
use crate::wallet::WalletProvider;
use crate::x402::X402EvmRpc;
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::types::transaction::eip1559::Eip1559TransactionRequest;
use ethers::types::transaction::eip2718::TypedTransaction;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Across Protocol API base URL
const ACROSS_API_URL: &str = "https://app.across.to/api";

/// Supported chains with their chain IDs and USDC addresses
const CHAIN_CONFIG: &[(&str, u64, &str)] = &[
    (
        "ethereum",
        1,
        "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
    ),
    (
        "mainnet",
        1,
        "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
    ), // alias
    ("base", 8453, "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
    (
        "polygon",
        137,
        "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359",
    ),
    (
        "arbitrum",
        42161,
        "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
    ),
    (
        "optimism",
        10,
        "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85",
    ),
];

/// Bridge USDC tool
pub struct BridgeUsdcTool {
    definition: ToolDefinition,
}

impl BridgeUsdcTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "from_chain".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Source chain: ethereum, base, polygon, arbitrum, optimism"
                    .to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "ethereum".to_string(),
                    "base".to_string(),
                    "polygon".to_string(),
                    "arbitrum".to_string(),
                    "optimism".to_string(),
                ]),
            },
        );

        properties.insert(
            "to_chain".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Destination chain: ethereum, base, polygon, arbitrum, optimism"
                    .to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "ethereum".to_string(),
                    "base".to_string(),
                    "polygon".to_string(),
                    "arbitrum".to_string(),
                    "optimism".to_string(),
                ]),
            },
        );

        properties.insert(
            "amount".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Amount of USDC to bridge (human-readable, e.g., '100' for 100 USDC)"
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "recipient".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Optional: recipient address on destination chain. Defaults to sender wallet."
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "slippage".to_string(),
            PropertySchema {
                schema_type: "number".to_string(),
                description: "Slippage tolerance (0.005 = 0.5%). Default: 0.005".to_string(),
                default: Some(json!(0.005)),
                items: None,
                enum_values: None,
            },
        );

        BridgeUsdcTool {
            definition: ToolDefinition {
                name: "bridge_usdc".to_string(),
                description: "Bridge USDC between chains (Ethereum, Base, Polygon, Arbitrum, Optimism) using Across Protocol. Fast ~2s fills. Transactions are QUEUED - use broadcast_web3_tx to send.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![
                        "from_chain".to_string(),
                        "to_chain".to_string(),
                        "amount".to_string(),
                    ],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }

    /// Get chain ID for a chain name
    fn get_chain_id(chain: &str) -> Result<u64, String> {
        let chain_lower = chain.to_lowercase();
        CHAIN_CONFIG
            .iter()
            .find(|(name, _, _)| *name == chain_lower)
            .map(|(_, id, _)| *id)
            .ok_or_else(|| {
                format!(
                    "Unsupported chain: {}. Supported: ethereum, base, polygon, arbitrum, optimism",
                    chain
                )
            })
    }

    /// Get USDC address for a chain
    fn get_usdc_address(chain: &str) -> Result<&'static str, String> {
        let chain_lower = chain.to_lowercase();
        CHAIN_CONFIG
            .iter()
            .find(|(name, _, _)| *name == chain_lower)
            .map(|(_, _, addr)| *addr)
            .ok_or_else(|| format!("No USDC address configured for chain: {}", chain))
    }

    /// Convert human-readable USDC amount to raw (6 decimals)
    fn parse_usdc_amount(amount: &str) -> Result<u64, String> {
        let parsed: f64 = amount
            .parse()
            .map_err(|_| format!("Invalid amount: {}", amount))?;

        if parsed <= 0.0 {
            return Err("Amount must be positive".to_string());
        }

        // USDC has 6 decimals
        let raw = (parsed * 1_000_000.0).round() as u64;
        Ok(raw)
    }

    /// Map chain name to network name for RPC config
    fn chain_to_network(chain: &str) -> &str {
        match chain.to_lowercase().as_str() {
            "ethereum" | "mainnet" => "mainnet",
            "base" => "base",
            "polygon" => "polygon",
            "arbitrum" => "arbitrum",
            "optimism" => "optimism",
            _ => chain,
        }
    }

    /// Sign a transaction for queueing using WalletProvider (works in both Standard and Flash mode)
    async fn sign_transaction_for_queue(
        chain_id: u64,
        network: &str,
        to: Address,
        value: U256,
        data: Vec<u8>,
        rpc_config: &ResolvedRpcConfig,
        wallet_provider: &Arc<dyn WalletProvider>,
    ) -> Result<SignedTxForQueue, String> {
        let rpc = X402EvmRpc::new_with_wallet_provider(
            wallet_provider.clone(),
            network,
            Some(rpc_config.url.clone()),
            rpc_config.use_x402,
        )?;

        // Get wallet address from WalletProvider
        let from_str = wallet_provider.get_address();
        let from_address: Address = from_str.parse()
            .map_err(|_| format!("Invalid wallet address: {}", from_str))?;

        // Get nonce
        let nonce = rpc.get_transaction_count(from_address).await?;

        // Estimate gas
        let gas: U256 = rpc
            .estimate_gas(from_address, to, &data, value)
            .await
            .map_err(|e| format!("Gas estimation failed: {}", e))?;
        let gas = gas * U256::from(130) / U256::from(100); // 30% buffer for bridge txs

        // Get gas prices
        let (max_fee, priority_fee) = rpc.estimate_eip1559_fees().await?;

        log::info!(
            "[bridge_usdc] Signing tx: to={:?}, value={}, data_len={}, gas={}, nonce={} on {}",
            to,
            value,
            data.len(),
            gas,
            nonce,
            network
        );

        // Build EIP-1559 transaction
        let tx = Eip1559TransactionRequest::new()
            .from(from_address)
            .to(to)
            .value(value)
            .data(data.clone())
            .nonce(nonce)
            .gas(gas)
            .max_fee_per_gas(max_fee)
            .max_priority_fee_per_gas(priority_fee)
            .chain_id(chain_id);

        // Sign using WalletProvider (works in both Standard and Flash mode)
        let typed_tx: TypedTransaction = tx.into();
        let signature = wallet_provider
            .sign_transaction(&typed_tx)
            .await
            .map_err(|e| format!("Failed to sign transaction: {}", e))?;

        let signed_tx = typed_tx.rlp_signed(&signature);
        let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));

        Ok(SignedTxForQueue {
            from: from_str,
            to: format!("{:?}", to),
            value: value.to_string(),
            data: format!("0x{}", hex::encode(&data)),
            gas_limit: gas.to_string(),
            max_fee_per_gas: max_fee.to_string(),
            max_priority_fee_per_gas: priority_fee.to_string(),
            nonce: nonce.as_u64(),
            signed_tx_hex,
            network: network.to_string(),
        })
    }
}

/// Signed transaction ready for queue
#[derive(Debug)]
struct SignedTxForQueue {
    from: String,
    to: String,
    value: String,
    data: String,
    gas_limit: String,
    max_fee_per_gas: String,
    max_priority_fee_per_gas: String,
    nonce: u64,
    signed_tx_hex: String,
    network: String,
}

/// Parameters for bridge_usdc tool
#[derive(Debug, Deserialize)]
struct BridgeUsdcParams {
    from_chain: String,
    to_chain: String,
    amount: String,
    recipient: Option<String>,
    #[serde(default = "default_slippage")]
    slippage: f64,
}

fn default_slippage() -> f64 {
    0.005
}

/// Across API response for /swap/approval
#[derive(Debug, Deserialize)]
struct AcrossSwapResponse {
    /// Array of approval transactions (if needed)
    #[serde(rename = "approvalTxns", default)]
    approval_txns: Vec<AcrossTransaction>,
    /// The swap/bridge transaction
    #[serde(rename = "swapTx")]
    swap_tx: Option<AcrossSwapTx>,
    /// Expected output amount (raw, 6 decimals for USDC)
    #[serde(rename = "expectedOutputAmount")]
    expected_output_amount: Option<String>,
    /// Estimated fill time in seconds
    #[serde(rename = "expectedFillTime")]
    expected_fill_time: Option<u64>,
    /// Fee breakdown (complex structure, just capture as Value)
    #[serde(rename = "fees")]
    fees: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct AcrossSwapTx {
    to: String,
    data: String,
    #[serde(rename = "chainId")]
    chain_id: Option<u64>,
    #[serde(rename = "maxFeePerGas")]
    max_fee_per_gas: Option<String>,
    #[serde(rename = "maxPriorityFeePerGas")]
    max_priority_fee_per_gas: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AcrossTransaction {
    to: String,
    data: String,
    #[serde(rename = "chainId")]
    chain_id: Option<u64>,
}

#[async_trait]
impl Tool for BridgeUsdcTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: BridgeUsdcParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Validate chains
        let from_chain_id = match Self::get_chain_id(&params.from_chain) {
            Ok(id) => id,
            Err(e) => return ToolResult::error(e),
        };

        let to_chain_id = match Self::get_chain_id(&params.to_chain) {
            Ok(id) => id,
            Err(e) => return ToolResult::error(e),
        };

        if from_chain_id == to_chain_id {
            return ToolResult::error("Source and destination chains must be different");
        }

        // Get USDC addresses
        let usdc_from = match Self::get_usdc_address(&params.from_chain) {
            Ok(addr) => addr,
            Err(e) => return ToolResult::error(e),
        };

        let usdc_to = match Self::get_usdc_address(&params.to_chain) {
            Ok(addr) => addr,
            Err(e) => return ToolResult::error(e),
        };

        // Get wallet provider (required for signing)
        let wallet_provider = match &context.wallet_provider {
            Some(wp) => wp,
            None => return ToolResult::error("Wallet not configured. Cannot bridge tokens."),
        };

        // Get wallet address from WalletProvider
        let wallet_address = wallet_provider.get_address();

        // Parse amount
        let amount_raw = match Self::parse_usdc_amount(&params.amount) {
            Ok(a) => a,
            Err(e) => return ToolResult::error(e),
        };

        // Recipient defaults to sender
        let recipient = params.recipient.unwrap_or_else(|| wallet_address.clone());

        // Validate recipient address
        if !recipient.starts_with("0x") || recipient.len() != 42 {
            return ToolResult::error(format!("Invalid recipient address: {}", recipient));
        }

        log::info!(
            "[bridge_usdc] Bridging {} USDC ({} raw) from {} to {}, wallet={}",
            params.amount,
            amount_raw,
            params.from_chain,
            params.to_chain,
            wallet_address
        );

        // Call Across API
        let http_client = context.http_client();
        let url = format!(
            "{}/swap/approval?tradeType=exactInput&amount={}&inputToken={}&originChainId={}&outputToken={}&destinationChainId={}&depositor={}&recipient={}&slippage={}",
            ACROSS_API_URL,
            amount_raw,
            usdc_from,
            from_chain_id,
            usdc_to,
            to_chain_id,
            wallet_address,
            recipient,
            params.slippage
        );

        log::info!("[bridge_usdc] Calling Across API: {}", url);

        let response = match http_client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to call Across API: {}", e)),
        };

        let status = response.status();
        let response_text = match response.text().await {
            Ok(t) => t,
            Err(e) => return ToolResult::error(format!("Failed to read Across response: {}", e)),
        };

        if !status.is_success() {
            return ToolResult::error(format!(
                "Across API error ({}): {}",
                status, response_text
            ));
        }

        let across_response: AcrossSwapResponse = match serde_json::from_str(&response_text) {
            Ok(r) => r,
            Err(e) => {
                return ToolResult::error(format!(
                    "Failed to parse Across response: {} - Response: {}",
                    e, response_text
                ))
            }
        };

        // Check for swap transaction
        let swap_tx = match across_response.swap_tx {
            Some(s) => s,
            None => {
                return ToolResult::error(format!(
                    "Across API did not return a swap transaction. Response: {}",
                    response_text
                ))
            }
        };

        // Verify intent before any signing/queueing
        let intent = TransactionIntent {
            tx_type: "bridge".to_string(),
            to: swap_tx.to.clone(),
            value: "0".to_string(),
            value_display: format!("{} USDC", params.amount),
            network: Self::chain_to_network(&params.from_chain).to_string(),
            function_name: None,
            abi_name: None,
            preset_name: None,
            destination_chain: Some(params.to_chain.clone()),
            calldata: None,
            description: format!(
                "Bridge {} USDC from {} to {} via Across Protocol, recipient {}",
                params.amount, params.from_chain, params.to_chain, recipient,
            ),
        };
        if let Err(reason) = verify_intent::verify_intent(&intent, context, None).await {
            return ToolResult::error(reason);
        }

        // Check if we're in a gateway channel without rogue mode
        let is_gateway_channel = context
            .channel_type
            .as_ref()
            .map(|ct| {
                let ct_lower = ct.to_lowercase();
                ct_lower == "discord" || ct_lower == "telegram" || ct_lower == "slack"
            })
            .unwrap_or(false);

        let is_rogue_mode = context
            .extra
            .get("rogue_mode_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if is_gateway_channel && !is_rogue_mode {
            return ToolResult::error(
                "Bridge transactions cannot be executed in Discord/Telegram/Slack channels unless Rogue Mode is enabled.",
            );
        }

        // Check tx_queue availability
        let tx_queue = match &context.tx_queue {
            Some(q) => q,
            None => return ToolResult::error("Transaction queue not available"),
        };

        // Resolve RPC config for source chain
        let network = Self::chain_to_network(&params.from_chain);
        let rpc_config = resolve_rpc_from_context(&context.extra, network);

        let mut queued_uuids = Vec::new();
        let mut current_nonce_offset = 0u64;

        // Queue approval transactions if needed (usually just one for USDC)
        for approval in &across_response.approval_txns {
            let approval_to: Address = match approval.to.parse() {
                Ok(a) => a,
                Err(_) => {
                    return ToolResult::error(format!(
                        "Invalid approval 'to' address: {}",
                        approval.to
                    ))
                }
            };

            let approval_data =
                match hex::decode(approval.data.strip_prefix("0x").unwrap_or(&approval.data)) {
                    Ok(d) => d,
                    Err(e) => return ToolResult::error(format!("Invalid approval data: {}", e)),
                };

            // Approval transactions don't send ETH value
            let approval_value = U256::zero();

            let signed_approval = match Self::sign_transaction_for_queue(
                from_chain_id,
                network,
                approval_to,
                approval_value,
                approval_data,
                &rpc_config,
                wallet_provider,
            )
            .await
            {
                Ok(s) => s,
                Err(e) => return ToolResult::error(format!("Failed to sign approval tx: {}", e)),
            };

            let approval_uuid = Uuid::new_v4().to_string();
            let queued_approval = QueuedTransaction::new(
                approval_uuid.clone(),
                signed_approval.network.clone(),
                signed_approval.from.clone(),
                signed_approval.to.clone(),
                signed_approval.value.clone(),
                signed_approval.data.clone(),
                signed_approval.gas_limit.clone(),
                signed_approval.max_fee_per_gas.clone(),
                signed_approval.max_priority_fee_per_gas.clone(),
                signed_approval.nonce,
                signed_approval.signed_tx_hex.clone(),
                context.channel_id,
            );

            tx_queue.queue(queued_approval);
            queued_uuids.push(("approval".to_string(), approval_uuid));
            current_nonce_offset = 1;

            log::info!(
                "[bridge_usdc] Approval tx queued, nonce={}",
                signed_approval.nonce
            );
        }

        // Queue bridge transaction
        let bridge_to: Address = match swap_tx.to.parse() {
            Ok(a) => a,
            Err(_) => {
                return ToolResult::error(format!("Invalid bridge 'to' address: {}", swap_tx.to))
            }
        };

        let bridge_data =
            match hex::decode(swap_tx.data.strip_prefix("0x").unwrap_or(&swap_tx.data)) {
                Ok(d) => d,
                Err(e) => return ToolResult::error(format!("Invalid bridge data: {}", e)),
            };

        // Bridge transactions for USDC don't require ETH value (USDC is ERC20)
        let bridge_value = U256::zero();

        // For bridge tx, we need to account for approval nonce if it was queued
        let signed_bridge = if current_nonce_offset > 0 {
            // Re-sign with incremented nonce using WalletProvider
            let rpc = match X402EvmRpc::new_with_wallet_provider(
                wallet_provider.clone(),
                network,
                Some(rpc_config.url.clone()),
                rpc_config.use_x402,
            ) {
                Ok(r) => r,
                Err(e) => return ToolResult::error(format!("Failed to create RPC: {}", e)),
            };

            // Get wallet address from WalletProvider
            let from_str = wallet_provider.get_address();
            let from_address: Address = match from_str.parse() {
                Ok(a) => a,
                Err(_) => return ToolResult::error(format!("Invalid wallet address: {}", from_str)),
            };

            let base_nonce = match rpc.get_transaction_count(from_address).await {
                Ok(n) => n,
                Err(e) => return ToolResult::error(format!("Failed to get nonce: {}", e)),
            };
            let nonce = base_nonce + U256::from(current_nonce_offset);

            let gas: U256 = match rpc
                .estimate_gas(from_address, bridge_to, &bridge_data, bridge_value)
                .await
            {
                Ok(g) => g,
                Err(e) => {
                    return ToolResult::error(format!("Gas estimation failed for bridge: {}", e))
                }
            };
            let gas = gas * U256::from(130) / U256::from(100);

            let (max_fee, priority_fee) = match rpc.estimate_eip1559_fees().await {
                Ok(fees) => fees,
                Err(e) => return ToolResult::error(format!("Failed to estimate fees: {}", e)),
            };

            let tx = Eip1559TransactionRequest::new()
                .from(from_address)
                .to(bridge_to)
                .value(bridge_value)
                .data(bridge_data.clone())
                .nonce(nonce)
                .gas(gas)
                .max_fee_per_gas(max_fee)
                .max_priority_fee_per_gas(priority_fee)
                .chain_id(from_chain_id);

            // Sign using WalletProvider (works in both Standard and Flash mode)
            let typed_tx: TypedTransaction = tx.into();
            let signature = match wallet_provider.sign_transaction(&typed_tx).await {
                Ok(s) => s,
                Err(e) => {
                    return ToolResult::error(format!("Failed to sign bridge transaction: {}", e))
                }
            };

            let signed_tx = typed_tx.rlp_signed(&signature);

            SignedTxForQueue {
                from: from_str,
                to: format!("{:?}", bridge_to),
                value: bridge_value.to_string(),
                data: format!("0x{}", hex::encode(&bridge_data)),
                gas_limit: gas.to_string(),
                max_fee_per_gas: max_fee.to_string(),
                max_priority_fee_per_gas: priority_fee.to_string(),
                nonce: nonce.as_u64(),
                signed_tx_hex: format!("0x{}", hex::encode(&signed_tx)),
                network: network.to_string(),
            }
        } else {
            match Self::sign_transaction_for_queue(
                from_chain_id,
                network,
                bridge_to,
                bridge_value,
                bridge_data,
                &rpc_config,
                wallet_provider,
            )
            .await
            {
                Ok(s) => s,
                Err(e) => return ToolResult::error(format!("Failed to sign bridge tx: {}", e)),
            }
        };

        let bridge_uuid = Uuid::new_v4().to_string();
        let queued_bridge = QueuedTransaction::new(
            bridge_uuid.clone(),
            signed_bridge.network.clone(),
            signed_bridge.from.clone(),
            signed_bridge.to.clone(),
            signed_bridge.value.clone(),
            signed_bridge.data.clone(),
            signed_bridge.gas_limit.clone(),
            signed_bridge.max_fee_per_gas.clone(),
            signed_bridge.max_priority_fee_per_gas.clone(),
            signed_bridge.nonce,
            signed_bridge.signed_tx_hex.clone(),
            context.channel_id,
        );

        tx_queue.queue(queued_bridge);
        queued_uuids.push(("bridge".to_string(), bridge_uuid.clone()));

        log::info!(
            "[bridge_usdc] Bridge tx queued, nonce={}",
            signed_bridge.nonce
        );

        // Format expected output
        let expected_output_usdc = across_response
            .expected_output_amount
            .as_ref()
            .map(|o| {
                let raw: u64 = o.parse().unwrap_or(0);
                format!("{:.2}", raw as f64 / 1_000_000.0)
            })
            .unwrap_or_else(|| "~".to_string() + &params.amount);

        let fill_time = across_response
            .expected_fill_time
            .map(|t| format!("~{} seconds", t))
            .unwrap_or_else(|| "~2 seconds".to_string());

        // Build response
        let uuids_display: Vec<String> = queued_uuids
            .iter()
            .map(|(typ, uuid)| format!("{}: {}", typ, uuid))
            .collect();

        let result = format!(
            "BRIDGE QUEUED (not yet broadcast)\n\n\
            Route: {} → {}\n\
            Amount: {} USDC\n\
            Expected: {} USDC (after fees)\n\
            Est. fill time: {}\n\
            Recipient: {}\n\n\
            Transactions queued:\n{}\n\n\
            --- Next Steps ---\n\
            To view queued: use `list_queued_web3_tx`\n\
            To broadcast: use `broadcast_web3_tx` (broadcasts in order)\n\n\
            Note: Broadcast approval first, wait for confirmation, then broadcast bridge.",
            params.from_chain,
            params.to_chain,
            params.amount,
            expected_output_usdc,
            fill_time,
            recipient,
            uuids_display.join("\n")
        );

        ToolResult::success(result).with_metadata(json!({
            "status": "queued",
            "from_chain": params.from_chain,
            "to_chain": params.to_chain,
            "amount": params.amount,
            "amount_raw": amount_raw.to_string(),
            "expected_output": expected_output_usdc,
            "estimated_fill_time": across_response.expected_fill_time,
            "recipient": recipient,
            "queued_transactions": queued_uuids,
            "fees": across_response.fees,
        }))
    }
}

impl Default for BridgeUsdcTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for BridgeUsdcTool {
    fn clone(&self) -> Self {
        Self {
            definition: self.definition.clone(),
        }
    }
}
