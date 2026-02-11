//! Send ETH tool - simple native ETH transfers only
//!
//! Signs and queues native ETH transfers using the burner wallet.
//! This tool is RESTRICTED to ETH transfers only (data must be "0x" or empty).
//! For contract calls, use web3_function_call instead.
//!
//! ## Flow
//! 1. send_eth signs transaction and queues it (returns UUID)
//! 2. list_queued_web3_tx shows queued transactions
//! 3. broadcast_web3_tx broadcasts by UUID
//!
//! All RPC calls go through defirelay.com with x402 payments.

use super::verify_intent::{self, TransactionIntent};
use crate::tools::registry::Tool;
use crate::tools::rpc_config::{resolve_rpc_from_context, Network, ResolvedRpcConfig};
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
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

/// Signed transaction result with all details needed for queuing
#[derive(Debug)]
struct SignedTxResult {
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

/// Send ETH tool - native ETH transfers only
pub struct SendEthTool {
    definition: ToolDefinition,
}

impl SendEthTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "network".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Network: 'base', 'mainnet', or 'polygon'. If not specified, uses the user's selected network from the UI.".to_string(),
                default: None,  // No default - will use context's selected_network
                items: None,
                enum_values: Some(vec!["base".to_string(), "mainnet".to_string(), "polygon".to_string()]),
            },
        );

        SendEthTool {
            definition: ToolDefinition {
                name: "send_eth".to_string(),
                description: "Send native ETH to an address. Reads 'send_to' (recipient) and 'amount_raw' (wei value) from registers. Use 'set_address' to set 'send_to', and 'to_raw_amount' with decimals=18 to set 'amount_raw'. Transaction is QUEUED - use broadcast_web3_tx to broadcast.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }

    /// Get chain ID for a network
    fn get_chain_id(network: &str) -> u64 {
        match network {
            "mainnet" => 1,
            "polygon" => 137,
            "arbitrum" => 42161,
            "optimism" => 10,
            _ => 8453, // Base
        }
    }

    /// Sign an ETH transfer using WalletProvider (works in both Standard and Flash mode)
    async fn sign_eth_transfer(
        network: &str,
        to: &str,
        value: &str,
        rpc_config: &ResolvedRpcConfig,
        wallet_provider: &Arc<dyn WalletProvider>,
    ) -> Result<SignedTxResult, String> {
        // Create RPC client using WalletProvider for x402 payments
        let rpc = X402EvmRpc::new_with_wallet_provider(
            wallet_provider.clone(),
            network,
            Some(rpc_config.url.clone()),
            rpc_config.use_x402,
        )?;
        let chain_id = Self::get_chain_id(network);

        // Get wallet address from WalletProvider
        let from_str = wallet_provider.get_address();
        let from_address: Address = from_str.parse()
            .map_err(|_| format!("Invalid wallet address: {}", from_str))?;

        // Parse recipient address
        let to_address: Address = to.parse()
            .map_err(|_| format!("Invalid 'to' address: {}", to))?;

        // Parse value
        let tx_value: U256 = parse_u256(value)?;

        // Get nonce
        let nonce = rpc.get_transaction_count(from_address).await?;

        // Simple ETH transfer is always 21000 gas
        let gas = U256::from(21000u64);

        // Auto-estimate gas prices
        let (max_fee, priority_fee) = rpc.estimate_eip1559_fees().await?;

        log::info!(
            "[send_eth] Signing ETH transfer: to={}, value={}, gas={}, nonce={} on {}",
            to, value, gas, nonce, network
        );

        // Build EIP-1559 transaction (empty data for ETH transfer)
        let tx = Eip1559TransactionRequest::new()
            .from(from_address)
            .to(to_address)
            .value(tx_value)
            .nonce(nonce)
            .gas(gas)
            .max_fee_per_gas(max_fee)
            .max_priority_fee_per_gas(priority_fee)
            .chain_id(chain_id);

        // Sign the transaction using WalletProvider (works in both Standard and Flash mode)
        let typed_tx: TypedTransaction = tx.into();
        let signature = wallet_provider
            .sign_transaction(&typed_tx)
            .await
            .map_err(|e| format!("Failed to sign transaction: {}", e))?;

        let signed_tx = typed_tx.rlp_signed(&signature);
        let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));

        log::info!("[send_eth] Transaction signed, nonce={}", nonce);

        Ok(SignedTxResult {
            from: from_str,
            to: to.to_string(),
            value: tx_value.to_string(),
            data: "0x".to_string(),
            gas_limit: gas.to_string(),
            max_fee_per_gas: max_fee.to_string(),
            max_priority_fee_per_gas: priority_fee.to_string(),
            nonce: nonce.as_u64(),
            signed_tx_hex,
            network: network.to_string(),
        })
    }

    /// Format wei as human-readable ETH
    pub fn format_eth(wei: &str) -> String {
        if let Ok(w) = wei.parse::<u128>() {
            let eth = w as f64 / 1e18;
            if eth >= 0.0001 {
                format!("{:.6} ETH", eth)
            } else {
                format!("{} wei", wei)
            }
        } else {
            format!("{} wei", wei)
        }
    }

    /// Format wei as gwei for gas prices
    pub fn format_gwei(wei: &str) -> String {
        if let Ok(w) = wei.parse::<u128>() {
            let gwei = w as f64 / 1e9;
            format!("{:.4} gwei", gwei)
        } else {
            format!("{} wei", wei)
        }
    }

    /// Parse RPC errors and provide actionable feedback
    fn parse_rpc_error(error: &str, tx_data: &ResolvedTxData, network: &str) -> String {
        let mut result = String::new();

        if error.contains("insufficient funds") {
            result.push_str("INSUFFICIENT FUNDS\n\n");
            result.push_str("The wallet doesn't have enough ETH to cover gas + value.\n");
            if let (Some(have_start), Some(want_start)) = (error.find("have "), error.find("want ")) {
                let have = error[have_start + 5..].split_whitespace().next().unwrap_or("?");
                let want = error[want_start + 5..].split_whitespace().next().unwrap_or("?");
                result.push_str(&format!("* Have: {} ({})\n", have, Self::format_eth(have)));
                result.push_str(&format!("* Need: {} ({})\n", want, Self::format_eth(want)));
            }
            result.push_str("\nAction: Fund the wallet or reduce the amount.");
        } else if error.contains("nonce too low") {
            result.push_str("NONCE TOO LOW\n\n");
            result.push_str("A transaction with this nonce was already mined.\n");
            result.push_str("Action: Retry - the nonce will be re-fetched automatically.");
        } else {
            result.push_str(&format!("TRANSFER FAILED\n\n{}\n", error));
        }

        result.push_str("\n--- Transfer Details ---\n");
        result.push_str(&format!("Source: {}\n", tx_data.source));
        result.push_str(&format!("Network: {}\n", network));
        result.push_str(&format!("To: {}\n", tx_data.to));
        result.push_str(&format!("Value: {} ({})\n", tx_data.value, Self::format_eth(&tx_data.value)));

        result
    }
}

impl Default for SendEthTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ResolvedTxData {
    /// Resolve transaction data from individual registers (send_to, amount_raw)
    /// IMPORTANT: We ONLY read from registers to prevent hallucination of tx data
    /// - 'send_to' must be set via set_address (validated as address)
    /// - 'amount_raw' must be set via to_raw_amount tool (enforced by blocked registers)
    fn from_registers(context: &ToolContext) -> Result<Self, String> {
        // Read recipient address from 'send_to' register
        let to = context.registers.get("send_to")
            .ok_or_else(|| {
                "Register 'send_to' not found. Use set_address to set the recipient address first.\n\nExample:\n```json\n{\"tool\": \"set_address\", \"register\": \"send_to\", \"address\": \"0x1234...\"}```".to_string()
            })?
            .as_str()
            .ok_or_else(|| "Register 'send_to' must be a string (Ethereum address)".to_string())?
            .to_string();

        // Validate it looks like an Ethereum address
        if !to.starts_with("0x") || to.len() != 42 {
            return Err(format!(
                "Register 'send_to' must contain a valid Ethereum address (0x + 40 hex chars), got '{}'",
                to
            ));
        }

        // SAFETY CHECK: Prevent sending ETH to the zero address (burns ETH)
        if to.to_lowercase() == "0x0000000000000000000000000000000000000000" {
            return Err(
                "ERROR: The recipient is the zero address (0x0000...0000). \
                Sending ETH to the zero address will BURN it permanently! \
                Please verify the correct recipient wallet address.".to_string()
            );
        }

        // Read amount from 'amount_raw' register (must be set by to_raw_amount)
        let value = context.registers.get("amount_raw")
            .ok_or_else(|| {
                "Register 'amount_raw' not found. Use to_raw_amount tool with decimals=18 to set the ETH amount.\n\nExample:\n```tool:to_raw_amount\namount: \"0.01\"\ndecimals: 18\ncache_as: amount_raw```".to_string()
            })?
            .as_str()
            .ok_or_else(|| "Register 'amount_raw' must be a string (wei value)".to_string())?
            .to_string();

        // Validate it's a numeric string
        if !value.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "Register 'amount_raw' must be a numeric string (wei), got '{}'. Use to_raw_amount to convert human amounts.",
                value
            ));
        }

        log::info!(
            "[send_eth] Resolved from registers: send_to={}, amount_raw={}",
            to, value
        );

        Ok(ResolvedTxData {
            to,
            value,
            source: "registers:send_to,amount_raw".to_string(),
        })
    }
}

/// Send ETH parameters
#[derive(Debug, Deserialize)]
struct SendEthParams {
    /// Network - if not specified, uses context's selected_network or defaults to Base
    network: Option<String>,
}

/// Resolved transfer data read from register
#[derive(Debug)]
struct ResolvedTxData {
    to: String,
    value: String,
    source: String,
}

/// Resolve the network from params, context, or default
fn resolve_network(param_network: Option<&str>, context_network: Option<&str>) -> Result<Network, String> {
    // Priority: explicit param > context selected > default (Base)
    let network_str = param_network
        .or(context_network)
        .unwrap_or("base");

    Network::from_str(network_str)
        .map_err(|_| format!("Invalid network '{}'. Must be one of: base, mainnet, polygon", network_str))
}

#[async_trait]
impl Tool for SendEthTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        log::info!("[send_eth] Raw params received: {}", params);

        let params: SendEthParams = match serde_json::from_value(params.clone()) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Resolve network: use provided network, or context's selected_network, or default to Base
        let network = match resolve_network(
            params.network.as_deref(),
            context.selected_network.as_deref()
        ) {
            Ok(n) => n,
            Err(e) => return ToolResult::error(e),
        };

        log::info!("[send_eth] Using network: {} (from param: {:?}, context: {:?})",
            network, params.network, context.selected_network);

        // Resolve transfer data from individual registers (send_to, amount_raw)
        let tx_data = match ResolvedTxData::from_registers(context) {
            Ok(d) => d,
            Err(e) => return ToolResult::error(e),
        };

        log::info!(
            "[send_eth] Resolved: to={}, value={}",
            tx_data.to, tx_data.value
        );

        // Check if we're in a gateway channel without rogue mode
        let is_gateway_channel = context.channel_type
            .as_ref()
            .map(|ct| {
                let ct_lower = ct.to_lowercase();
                ct_lower == "discord" || ct_lower == "telegram" || ct_lower == "slack"
            })
            .unwrap_or(false);

        let is_rogue_mode = context.extra
            .get("rogue_mode_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if is_gateway_channel && !is_rogue_mode {
            return ToolResult::error(
                "Transactions cannot be executed in Discord/Telegram/Slack channels unless Rogue Mode is enabled."
            );
        }

        // Check if tx_queue is available
        let tx_queue = match &context.tx_queue {
            Some(q) => q,
            None => return ToolResult::error("Transaction queue not available."),
        };

        // Get wallet provider (required for signing)
        let wallet_provider = match &context.wallet_provider {
            Some(wp) => wp,
            None => return ToolResult::error("Wallet not configured. Cannot sign transactions."),
        };

        // Resolve RPC configuration
        let rpc_config = resolve_rpc_from_context(&context.extra, network.as_ref());

        // Sign the ETH transfer using WalletProvider (works in both Standard and Flash mode)
        match Self::sign_eth_transfer(
            network.as_ref(),
            &tx_data.to,
            &tx_data.value,
            &rpc_config,
            wallet_provider,
        ).await {
            Ok(signed) => {
                // Verify intent before queueing
                let intent = TransactionIntent {
                    tx_type: "eth_transfer".to_string(),
                    to: signed.to.clone(),
                    value: signed.value.clone(),
                    value_display: Self::format_eth(&signed.value),
                    network: signed.network.clone(),
                    function_name: None,
                    abi_name: None,
                    preset_name: None,
                    destination_chain: None,
                    calldata: None,
                    description: format!(
                        "Send {} to {} on {}",
                        Self::format_eth(&signed.value),
                        signed.to,
                        signed.network,
                    ),
                };
                if let Err(reason) = verify_intent::verify_intent(&intent, context, None).await {
                    return ToolResult::error(reason);
                }

                // Generate UUID for this queued transaction
                let uuid = Uuid::new_v4().to_string();

                // Create queued transaction
                let queued_tx = QueuedTransaction::new(
                    uuid.clone(),
                    signed.network.clone(),
                    signed.from.clone(),
                    signed.to.clone(),
                    signed.value.clone(),
                    signed.data.clone(),
                    signed.gas_limit.clone(),
                    signed.max_fee_per_gas.clone(),
                    signed.max_priority_fee_per_gas.clone(),
                    signed.nonce,
                    signed.signed_tx_hex.clone(),
                    context.channel_id,
                );

                // Queue the transaction
                tx_queue.queue(queued_tx);

                log::info!("[send_eth] Transaction queued with UUID: {}", uuid);

                // Build response message
                let mut msg = String::new();
                msg.push_str("ETH TRANSFER QUEUED (not yet broadcast)\n\n");
                msg.push_str(&format!("UUID: {}\n", uuid));
                msg.push_str(&format!("Network: {}\n", signed.network));
                msg.push_str(&format!("From: {}\n", signed.from));
                msg.push_str(&format!("To: {}\n", signed.to));
                msg.push_str(&format!("Value: {} ({})\n", signed.value, Self::format_eth(&signed.value)));
                msg.push_str(&format!("Nonce: {}\n", signed.nonce));
                msg.push_str("\n--- Next Steps ---\n");
                msg.push_str("To view queued: use `list_queued_web3_tx`\n");
                msg.push_str(&format!("To broadcast: use `broadcast_web3_tx` with uuid: {}\n", uuid));

                ToolResult::success(msg).with_metadata(json!({
                    "uuid": uuid,
                    "status": "queued",
                    "network": signed.network,
                    "from": signed.from,
                    "to": signed.to,
                    "value": signed.value,
                    "nonce": signed.nonce,
                    "gas_limit": signed.gas_limit,
                    "max_fee_per_gas": signed.max_fee_per_gas,
                    "max_priority_fee_per_gas": signed.max_priority_fee_per_gas
                }))
            }
            Err(e) => ToolResult::error(Self::parse_rpc_error(&e, &tx_data, network.as_ref())),
        }
    }
}

/// Parse decimal or hex strings to U256 (exposed for testing)
/// IMPORTANT: Do NOT use str.parse::<U256>() - it treats strings as hex!
/// Use U256::from_dec_str() for decimal strings.
pub fn parse_u256(s: &str) -> Result<U256, String> {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        U256::from_str_radix(&s[2..], 16)
            .map_err(|e| format!("Invalid hex: {} - {}", s, e))
    } else {
        // MUST use from_dec_str, NOT parse() - parse() treats input as hex!
        U256::from_dec_str(s)
            .map_err(|e| format!("Invalid decimal: {} - {}", s, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_u256_decimal() {
        // Debug: see what's happening
        let input = "331157";
        let result = parse_u256(input);
        println!("Input: '{}'", input);
        println!("Result: {:?}", result);
        println!("Expected: {:?}", U256::from(331157u64));

        // Try direct methods
        println!("Direct parse: {:?}", input.parse::<U256>());
        println!("from_dec_str: {:?}", U256::from_dec_str(input));

        // Basic decimal parsing
        assert_eq!(parse_u256("331157").unwrap(), U256::from(331157u64));
        assert_eq!(parse_u256("5756709").unwrap(), U256::from(5756709u64));
        assert_eq!(parse_u256("100000000000000").unwrap(), U256::from(100000000000000u64));
        assert_eq!(parse_u256("0").unwrap(), U256::from(0u64));
        assert_eq!(parse_u256("1").unwrap(), U256::from(1u64));

        // With whitespace
        assert_eq!(parse_u256("  331157  ").unwrap(), U256::from(331157u64));
    }

    #[test]
    fn test_parse_u256_hex() {
        // Hex parsing - verify correct conversions
        // 0x50d95 = 331157 decimal
        assert_eq!(parse_u256("0x50d95").unwrap(), U256::from(331157u64));
        assert_eq!(parse_u256("0x5756709").unwrap(), U256::from(0x5756709u64));
        assert_eq!(parse_u256("0xf4240").unwrap(), U256::from(1000000u64));

        // Hex parsing (uppercase 0X)
        assert_eq!(parse_u256("0X50D95").unwrap(), U256::from(331157u64));

        // Common gas prices on Base
        assert_eq!(parse_u256("0x5756a5").unwrap(), U256::from(5723813u64));
    }

    #[test]
    fn test_parse_u256_errors() {
        // Invalid strings
        assert!(parse_u256("abc").is_err());
        assert!(parse_u256("0xGGG").is_err());
        assert!(parse_u256("-1").is_err());
        // Note: empty string may parse as 0 depending on implementation
    }

    #[test]
    fn test_send_eth_params_deserialization() {
        let json = json!({
            "network": "base"
        });

        let params: SendEthParams = serde_json::from_value(json).unwrap();

        assert_eq!(params.network, Some("base".to_string()));
    }

    #[test]
    fn test_send_eth_params_default_network() {
        let json = json!({});

        let params: SendEthParams = serde_json::from_value(json).unwrap();
        // When no network is specified, it's None - the runtime code defaults to "base"
        assert_eq!(params.network, None);
    }

    #[test]
    fn test_resolved_tx_data_from_registers() {
        use crate::tools::RegisterStore;

        let registers = RegisterStore::new();
        registers.set("send_to", json!("0x1234567890abcdef1234567890abcdef12345678"), "set_address");
        registers.set("amount_raw", json!("100000000000000"), "to_raw_amount");

        let context = crate::tools::ToolContext::new()
            .with_registers(registers);

        let tx_data = ResolvedTxData::from_registers(&context).unwrap();

        assert_eq!(tx_data.to, "0x1234567890abcdef1234567890abcdef12345678");
        assert_eq!(tx_data.value, "100000000000000");
        assert_eq!(tx_data.source, "registers:send_to,amount_raw");
    }

    #[test]
    fn test_resolved_tx_data_missing_send_to() {
        use crate::tools::RegisterStore;

        let registers = RegisterStore::new();
        registers.set("amount_raw", json!("100000000000000"), "to_raw_amount");

        let context = crate::tools::ToolContext::new()
            .with_registers(registers);

        let result = ResolvedTxData::from_registers(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("send_to"));
    }

    #[test]
    fn test_resolved_tx_data_missing_amount_raw() {
        use crate::tools::RegisterStore;

        let registers = RegisterStore::new();
        registers.set("send_to", json!("0x1234567890abcdef1234567890abcdef12345678"), "set_address");

        let context = crate::tools::ToolContext::new()
            .with_registers(registers);

        let result = ResolvedTxData::from_registers(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("amount_raw"));
    }

    #[test]
    fn test_resolved_tx_data_invalid_address() {
        use crate::tools::RegisterStore;

        let registers = RegisterStore::new();
        registers.set("send_to", json!("not-an-address"), "set_address");
        registers.set("amount_raw", json!("100000000000000"), "to_raw_amount");

        let context = crate::tools::ToolContext::new()
            .with_registers(registers);

        let result = ResolvedTxData::from_registers(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("valid Ethereum address"));
    }

    #[test]
    fn test_resolved_tx_data_invalid_amount() {
        use crate::tools::RegisterStore;

        let registers = RegisterStore::new();
        registers.set("send_to", json!("0x1234567890abcdef1234567890abcdef12345678"), "set_address");
        registers.set("amount_raw", json!("not-a-number"), "to_raw_amount");

        let context = crate::tools::ToolContext::new()
            .with_registers(registers);

        let result = ResolvedTxData::from_registers(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("numeric string"));
    }

    #[test]
    fn test_resolved_tx_data_zero_address_rejected() {
        use crate::tools::RegisterStore;

        let registers = RegisterStore::new();
        registers.set("send_to", json!("0x0000000000000000000000000000000000000000"), "set_address");
        registers.set("amount_raw", json!("100000000000000"), "to_raw_amount");

        let context = crate::tools::ToolContext::new()
            .with_registers(registers);

        let result = ResolvedTxData::from_registers(&context);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("zero address"));
    }

    #[test]
    fn test_value_parsing_decimal_not_hex() {
        // This is the critical bug that caused 0.001 ETH to become 1.15 ETH!
        // "1000000000000000" decimal (0.001 ETH) was being parsed as hex
        // which gives 0x1000000000000000 = 1.15 ETH

        let value_str = "1000000000000000"; // 0.001 ETH in wei
        let parsed = parse_u256(value_str).unwrap();

        // Should be 10^15 = 0.001 ETH
        assert_eq!(parsed, U256::from(1_000_000_000_000_000u64));

        // NOT 0x1000000000000000 = 1152921504606846976 = 1.15 ETH
        assert_ne!(parsed, U256::from(0x1000000000000000u64));

        // Verify the difference
        let wrong_value = U256::from(0x1000000000000000u64);
        println!("Correct: {} wei ({} ETH)", parsed, parsed.as_u128() as f64 / 1e18);
        println!("Wrong:   {} wei ({} ETH)", wrong_value, wrong_value.as_u128() as f64 / 1e18);
    }
}
