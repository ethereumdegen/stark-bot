//! Generic Web3 transaction signing and queuing tool
//!
//! Signs EVM transactions using the burner wallet and queues them for broadcast.
//! This creates a safety layer where transactions can be reviewed before broadcast.
//!
//! ## Flow
//! 1. web3_tx signs transaction and queues it (returns UUID)
//! 2. list_queued_web3_tx shows queued transactions
//! 3. broadcast_web3_tx broadcasts by UUID
//!
//! All RPC calls go through defirelay.com with x402 payments.

use crate::domain_types::DomainUint256;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tx_queue::QueuedTransaction;
use crate::x402::X402EvmRpc;
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::types::transaction::eip1559::Eip1559TransactionRequest;
use ethers::types::transaction::eip2718::TypedTransaction;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
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

/// Web3 transaction tool
pub struct Web3TxTool {
    definition: ToolDefinition,
}

impl Web3TxTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "from_register".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Register name containing tx data (to, data, value, gas). The register is populated by x402_fetch with cache_as parameter.".to_string(),
                default: None,
                items: None,
                enum_values: None,
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

        properties.insert(
            "max_fee_per_gas".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Max fee per gas in wei. Get this from x402_rpc eth_gasPrice.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "max_priority_fee_per_gas".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Max priority fee per gas in wei (optional)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        Web3TxTool {
            definition: ToolDefinition {
                name: "web3_tx".to_string(),
                description: "Sign and QUEUE an EVM transaction for later broadcast. Returns a UUID. Use broadcast_web3_tx to broadcast the queued transaction. Use list_queued_web3_tx to view queued transactions.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["from_register".to_string(), "max_fee_per_gas".to_string()],
                },
                group: ToolGroup::Finance,
            },
        }
    }

    /// Get the wallet from environment
    fn get_wallet(chain_id: u64) -> Result<LocalWallet, String> {
        let private_key = crate::config::burner_wallet_private_key()
            .ok_or("BURNER_WALLET_BOT_PRIVATE_KEY not set")?;

        private_key
            .parse::<LocalWallet>()
            .map(|w| w.with_chain_id(chain_id))
            .map_err(|e| format!("Invalid private key: {}", e))
    }

    /// Get the private key from environment
    fn get_private_key() -> Result<String, String> {
        crate::config::burner_wallet_private_key()
            .ok_or_else(|| "BURNER_WALLET_BOT_PRIVATE_KEY not set".to_string())
    }

    /// Sign a transaction (but don't broadcast it)
    async fn sign_transaction(
        network: &str,
        to: &str,
        data: &str,
        value: &str,
        gas_limit: Option<U256>,
        max_fee_per_gas: Option<U256>,
        max_priority_fee_per_gas: Option<U256>,
    ) -> Result<SignedTxResult, String> {
        let private_key = Self::get_private_key()?;
        let rpc = X402EvmRpc::new(&private_key, network)?;
        let chain_id = rpc.chain_id();

        let wallet = Self::get_wallet(chain_id)?;
        let from_address = wallet.address();
        let from_str = format!("{:?}", from_address);

        // Parse recipient address
        let to_address: Address = to.parse()
            .map_err(|_| format!("Invalid 'to' address: {}", to))?;

        // Parse value - MUST use parse_u256, NOT .parse() which treats decimal as hex!
        let tx_value: U256 = parse_u256(value)?;

        // Decode calldata (auto-pad odd-length hex strings)
        let calldata = {
            let hex_str = if data.starts_with("0x") {
                &data[2..]
            } else {
                data
            };
            // Pad with leading zero if odd length (LLMs often forget to zero-pad)
            let padded = if !hex_str.is_empty() && hex_str.len() % 2 != 0 {
                format!("0{}", hex_str)
            } else {
                hex_str.to_string()
            };
            hex::decode(&padded)
                .map_err(|e| format!("Invalid hex data: {}", e))?
        };

        // Get nonce
        let nonce = rpc.get_transaction_count(from_address).await?;

        // Determine gas limit
        let gas = if let Some(gl) = gas_limit {
            log::info!("[web3_tx] Using provided gas_limit: {}", gl);
            gl
        } else {
            // Estimate gas
            log::warn!("[web3_tx] No gas_limit provided, estimating from network");
            let estimate = rpc.estimate_gas(from_address, to_address, &calldata, tx_value).await?;
            // Add 20% buffer
            estimate * 120 / 100
        };
        log::info!("[web3_tx] Gas limit resolved to: {}", gas);

        // Determine gas prices
        let (max_fee, priority_fee) = if let Some(mfpg) = max_fee_per_gas {
            log::info!("[web3_tx] Using provided max_fee_per_gas: {}", mfpg);

            let priority_fee = if let Some(mpfpg) = max_priority_fee_per_gas {
                log::info!("[web3_tx] Using provided priority_fee: {}", mpfpg);
                mpfpg
            } else {
                // Default priority fee to 1 gwei, but cap to max_fee
                log::info!("[web3_tx] No priority_fee provided, defaulting to min(1 gwei, max_fee)");
                std::cmp::min(U256::from(1_000_000_000u64), mfpg)
            };
            log::info!("[web3_tx] Priority fee resolved to: {}", priority_fee);

            (mfpg, priority_fee)
        } else {
            // Estimate fees from network
            log::warn!("[web3_tx] No max_fee_per_gas provided, estimating from network");
            rpc.estimate_eip1559_fees().await?
        };

        log::info!(
            "[web3_tx] Signing tx: to={}, value={}, data_len={} bytes, gas={}, max_fee={}, priority_fee={}, nonce={} on {}",
            to, value, calldata.len(), gas, max_fee, priority_fee, nonce, network
        );

        // Build EIP-1559 transaction
        let tx = Eip1559TransactionRequest::new()
            .from(from_address)
            .to(to_address)
            .value(tx_value)
            .data(calldata)
            .nonce(nonce)
            .gas(gas)
            .max_fee_per_gas(max_fee)
            .max_priority_fee_per_gas(priority_fee)
            .chain_id(chain_id);

        // Sign the transaction locally
        let typed_tx: TypedTransaction = tx.into();
        let signature = wallet
            .sign_transaction(&typed_tx)
            .await
            .map_err(|e| format!("Failed to sign transaction: {}", e))?;

        // Serialize the signed transaction
        let signed_tx = typed_tx.rlp_signed(&signature);
        let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));

        log::info!("[web3_tx] Transaction signed successfully, nonce={}", nonce);

        Ok(SignedTxResult {
            from: from_str,
            to: to.to_string(),
            value: tx_value.to_string(),
            data: data.to_string(),
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
    fn parse_rpc_error(error: &str, tx_data: &ResolvedTxData, params: &Web3TxParams) -> String {
        let mut result = String::new();

        // Identify the error type and provide context
        if error.contains("insufficient funds") {
            result.push_str("INSUFFICIENT FUNDS\n\n");
            result.push_str("The wallet doesn't have enough ETH to cover gas + value.\n");

            // Try to parse the have/want from the error
            if let (Some(have_start), Some(want_start)) = (error.find("have "), error.find("want ")) {
                let have = error[have_start + 5..].split_whitespace().next().unwrap_or("?");
                let want = error[want_start + 5..].split_whitespace().next().unwrap_or("?");
                result.push_str(&format!("* Have: {} ({})\n", have, Self::format_eth(have)));
                result.push_str(&format!("* Need: {} ({})\n", want, Self::format_eth(want)));
            }
            result.push_str("\nAction: Fund the wallet or reduce the transaction value/gas.");
        } else if error.contains("max priority fee per gas higher than max fee") {
            result.push_str("INVALID GAS PARAMS\n\n");
            result.push_str("max_priority_fee_per_gas cannot exceed max_fee_per_gas.\n");
            result.push_str(&format!("* max_fee_per_gas: {}\n", params.max_fee_per_gas.as_ref().map(|g| g.0.to_string()).unwrap_or_else(|| "not set".to_string())));
            result.push_str(&format!("* max_priority_fee_per_gas: {}\n", params.max_priority_fee_per_gas.as_ref().map(|g| g.0.to_string()).unwrap_or_else(|| "not set".to_string())));
            result.push_str("\nAction: Set max_priority_fee_per_gas <= max_fee_per_gas.");
        } else if error.contains("nonce too low") {
            result.push_str("NONCE TOO LOW\n\n");
            result.push_str("A transaction with this nonce was already mined.\n");
            result.push_str("Action: Retry - the nonce will be re-fetched automatically.");
        } else if error.contains("replacement transaction underpriced") {
            result.push_str("REPLACEMENT UNDERPRICED\n\n");
            result.push_str("A pending transaction exists with the same nonce but higher gas price.\n");
            result.push_str("Action: Increase max_fee_per_gas by at least 10% to replace it.");
        } else if error.contains("gas required exceeds allowance") || error.contains("out of gas") {
            result.push_str("OUT OF GAS\n\n");
            result.push_str("The transaction would run out of gas during execution.\n");
            result.push_str(&format!("* gas_limit provided: {}\n", tx_data.gas_limit.map(|g| g.to_string()).unwrap_or_else(|| "auto-estimated".to_string())));
            result.push_str("Action: Increase gas_limit or check if the transaction would revert.");
        } else if error.contains("execution reverted") {
            result.push_str("EXECUTION REVERTED\n\n");
            result.push_str("The contract rejected the transaction during simulation.\n");
            result.push_str("Common causes: slippage, insufficient approval, bad params.\n");
            result.push_str("Action: Check contract requirements and transaction parameters.");
        } else {
            result.push_str(&format!("SIGNING FAILED\n\n{}\n", error));
        }

        // Always append the attempted params for debugging
        result.push_str("\n--- Transaction Details ---\n");
        result.push_str(&format!("Source: {}\n", tx_data.source));
        result.push_str(&format!("Network: {}\n", params.network));
        result.push_str(&format!("To: {}\n", tx_data.to));
        result.push_str(&format!("Value: {} ({})\n", tx_data.value, Self::format_eth(&tx_data.value)));
        result.push_str(&format!("Data: {}...({} bytes)\n",
            &tx_data.data[..std::cmp::min(20, tx_data.data.len())],
            (tx_data.data.len().saturating_sub(2)) / 2
        ));
        if let Some(gl) = tx_data.gas_limit {
            result.push_str(&format!("Gas Limit: {}\n", gl));
        }
        if let Some(ref mfpg) = params.max_fee_per_gas {
            let mfpg_str = mfpg.0.to_string();
            result.push_str(&format!("Max Fee: {} ({})\n", mfpg_str, Self::format_gwei(&mfpg_str)));
        }
        if let Some(ref mpfpg) = params.max_priority_fee_per_gas {
            let mpfpg_str = mpfpg.0.to_string();
            result.push_str(&format!("Priority Fee: {} ({})\n", mpfpg_str, Self::format_gwei(&mpfpg_str)));
        }

        result
    }
}

impl Default for Web3TxTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ResolvedTxData {
    /// Resolve transaction data from a register
    /// IMPORTANT: We ONLY read from registers to prevent hallucination of tx data
    fn from_register(register_name: &str, context: &ToolContext) -> Result<Self, String> {
        // Read tx data from the register
        let reg_data = context.registers.get(register_name)
            .ok_or_else(|| format!(
                "Register '{}' not found. Available registers: {:?}. Make sure to call x402_fetch with cache_as first.",
                register_name,
                context.registers.keys()
            ))?;

        log::info!(
            "[web3_tx] Reading tx data from register '{}': {:?}",
            register_name,
            reg_data.as_object().map(|o| o.keys().collect::<Vec<_>>())
        );

        // Extract required fields from the register
        let to = reg_data.get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("Register '{}' missing 'to' field", register_name))?
            .to_string();

        let data = reg_data.get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("0x")
            .to_string();

        let value = reg_data.get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string();

        // gas_limit can be in "gas" or "gas_limit" field
        let gas_limit = reg_data.get("gas")
            .or_else(|| reg_data.get("gas_limit"))
            .and_then(|v| v.as_str())
            .map(parse_u256)
            .transpose()
            .map_err(|e| format!("Invalid gas in register: {}", e))?;

        log::info!(
            "[web3_tx] Resolved from register: to={}, data_len={}, value={}, gas_limit={:?}",
            to, data.len(), value, gas_limit
        );

        Ok(ResolvedTxData {
            to,
            data,
            value,
            gas_limit,
            source: format!("register:{}", register_name),
        })
    }
}

/// Web3 transaction parameters
/// IMPORTANT: from_register is REQUIRED to prevent hallucination of tx data
#[derive(Debug, Deserialize)]
struct Web3TxParams {
    /// Register name containing tx data (to, data, value, gas)
    /// This is REQUIRED - we never accept raw tx params from the agent
    from_register: String,
    /// Network is always specified by the agent
    #[serde(default = "default_network")]
    network: String,
    /// Gas price params are always specified by the agent (not from register)
    max_fee_per_gas: Option<DomainUint256>,
    max_priority_fee_per_gas: Option<DomainUint256>,
}

/// Resolved transaction data read from register
#[derive(Debug)]
struct ResolvedTxData {
    to: String,
    data: String,
    value: String,
    gas_limit: Option<U256>,
    source: String, // "register:<name>"
}

fn default_network() -> String {
    "base".to_string()
}

#[async_trait]
impl Tool for Web3TxTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        // Debug: log raw params to see what's actually arriving
        log::info!("[web3_tx] Raw params received: {}", params);

        let params: Web3TxParams = match serde_json::from_value(params.clone()) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Resolve transaction data from register (REQUIRED - prevents hallucination)
        let tx_data = match ResolvedTxData::from_register(&params.from_register, context) {
            Ok(d) => d,
            Err(e) => return ToolResult::error(e),
        };

        // Debug: log resolved params
        log::info!(
            "[web3_tx] Resolved tx data (source={}): to={}, data_len={}, value={}, gas_limit={:?}",
            tx_data.source, tx_data.to, tx_data.data.len(), tx_data.value, tx_data.gas_limit
        );
        log::info!(
            "[web3_tx] Gas params: max_fee={:?}, priority_fee={:?}",
            params.max_fee_per_gas, params.max_priority_fee_per_gas
        );

        // Validate network
        if params.network != "base" && params.network != "mainnet" {
            return ToolResult::error("Network must be 'base' or 'mainnet'");
        }

        // Check if tx_queue is available
        let tx_queue = match &context.tx_queue {
            Some(q) => q,
            None => return ToolResult::error("Transaction queue not available. Contact administrator."),
        };

        // Sign the transaction (but don't broadcast)
        match Self::sign_transaction(
            &params.network,
            &tx_data.to,
            &tx_data.data,
            &tx_data.value,
            tx_data.gas_limit,
            params.max_fee_per_gas.as_ref().map(|g| g.0),
            params.max_priority_fee_per_gas.as_ref().map(|g| g.0),
        ).await {
            Ok(signed) => {
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

                log::info!("[web3_tx] Transaction queued with UUID: {}", uuid);

                // Build response message
                let mut msg = String::new();
                msg.push_str("TRANSACTION QUEUED (not yet broadcast)\n\n");
                msg.push_str(&format!("UUID: {}\n", uuid));
                msg.push_str(&format!("Network: {}\n", signed.network));
                msg.push_str(&format!("From: {}\n", signed.from));
                msg.push_str(&format!("To: {}\n", signed.to));
                msg.push_str(&format!("Value: {} ({})\n", signed.value, Self::format_eth(&signed.value)));
                msg.push_str(&format!("Nonce: {}\n", signed.nonce));
                msg.push_str(&format!("Gas Limit: {}\n", signed.gas_limit));
                msg.push_str(&format!("Max Fee: {} ({})\n", signed.max_fee_per_gas, Self::format_gwei(&signed.max_fee_per_gas)));
                msg.push_str(&format!("Priority Fee: {} ({})\n", signed.max_priority_fee_per_gas, Self::format_gwei(&signed.max_priority_fee_per_gas)));
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
            Err(e) => ToolResult::error(Self::parse_rpc_error(&e, &tx_data, &params)),
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
    fn test_web3_tx_params_deserialization() {
        // Test with required fields (from_register is now required)
        let json = json!({
            "from_register": "swap_quote",
            "network": "base",
            "max_fee_per_gas": "5756709",
            "max_priority_fee_per_gas": "1000000"
        });

        let params: Web3TxParams = serde_json::from_value(json).unwrap();

        assert_eq!(params.from_register, "swap_quote");
        assert_eq!(params.network, "base");
        assert_eq!(params.max_fee_per_gas.unwrap().0, U256::from(5756709u64));
        assert_eq!(params.max_priority_fee_per_gas.unwrap().0, U256::from(1000000u64));
    }

    #[test]
    fn test_web3_tx_params_required_register() {
        // Test that from_register is required
        let json = json!({
            "network": "base",
            "max_fee_per_gas": "5756709"
        });

        // This should fail because from_register is missing
        let result: Result<Web3TxParams, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_web3_tx_params_with_hex_gas() {
        // Test that hex gas values are correctly parsed by DomainUint256
        let json = json!({
            "from_register": "swap_quote",
            "max_fee_per_gas": "0x5756a5"
        });

        let params: Web3TxParams = serde_json::from_value(json).unwrap();

        // DomainUint256 correctly parses hex strings
        assert_eq!(params.max_fee_per_gas.unwrap().0, U256::from(5723813u64));
    }

    #[test]
    fn test_resolved_tx_data_from_register() {
        use crate::tools::RegisterStore;

        let registers = RegisterStore::new();
        registers.set("swap_quote", json!({
            "to": "0x0000000000001ff3684f28c67538d4d072c22734",
            "data": "0x1234abcd",
            "value": "100000000000000",
            "gas": "331157"
        }), "x402_fetch");

        let context = crate::tools::ToolContext::new()
            .with_registers(registers);

        let tx_data = ResolvedTxData::from_register("swap_quote", &context).unwrap();

        assert_eq!(tx_data.to, "0x0000000000001ff3684f28c67538d4d072c22734");
        assert_eq!(tx_data.data, "0x1234abcd");
        assert_eq!(tx_data.value, "100000000000000");
        assert_eq!(tx_data.gas_limit, Some(U256::from(331157u64)));
        assert_eq!(tx_data.source, "register:swap_quote");
    }

    #[test]
    fn test_resolved_tx_data_missing_register() {
        let context = crate::tools::ToolContext::new();

        let result = ResolvedTxData::from_register("nonexistent", &context);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_resolved_tx_data_missing_to_field() {
        use crate::tools::RegisterStore;

        let registers = RegisterStore::new();
        registers.set("bad_quote", json!({
            "data": "0x1234",
            "value": "0"
        }), "test");

        let context = crate::tools::ToolContext::new()
            .with_registers(registers);

        let result = ResolvedTxData::from_register("bad_quote", &context);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing 'to' field"));
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
