//! Web3 utility types and functions for EVM contract interaction.
//!
//! Shared by `web3_function_call` (manual mode) and `web3_preset_function_call` (preset mode).
//! Provides ABI loading, encoding/decoding, transaction signing, and call execution.

use crate::tools::builtin::cryptocurrency::verify_intent::{self, TransactionIntent};
use crate::tools::builtin::cryptocurrency::web3_tx::parse_u256;
use crate::tools::rpc_config::{resolve_rpc_from_context, Network, ResolvedRpcConfig};
use crate::tools::types::{ToolContext, ToolResult};
use crate::tx_queue::QueuedTransaction;
use crate::wallet::WalletProvider;
use crate::x402::X402EvmRpc;
use ethers::abi::{Abi, Function, ParamType, Token};
use ethers::prelude::*;
use ethers::types::transaction::eip1559::Eip1559TransactionRequest;
use ethers::types::transaction::eip2718::TypedTransaction;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

// ---- Shared types and helpers (used by both manual and preset tools) ----

/// Signed transaction result for queuing (not broadcast)
#[derive(Debug)]
pub struct SignedTxForQueue {
    pub from: String,
    pub to: String,
    pub value: String,
    pub data: String,
    pub gas_limit: String,
    pub max_fee_per_gas: String,
    pub max_priority_fee_per_gas: String,
    pub nonce: u64,
    pub signed_tx_hex: String,
    pub network: String,
}

/// ABI file structure
#[derive(Debug, Deserialize)]
pub struct AbiFile {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub abi: Vec<Value>,
    #[serde(default)]
    pub address: HashMap<String, String>,
}

/// Resolve the network from params, context, or default
pub fn resolve_network(param_network: Option<&str>, context_network: Option<&str>) -> Result<Network, String> {
    let network_str = param_network
        .or(context_network)
        .unwrap_or("base");

    Network::from_str(network_str)
        .map_err(|_| format!("Invalid network '{}'. Must be one of: base, mainnet, polygon", network_str))
}

/// Determine abis directory -- always relative to the repo root
pub fn default_abis_dir() -> PathBuf {
    crate::config::repo_root().join("abis")
}

/// Load ABI from file
pub fn load_abi(abis_dir: &PathBuf, name: &str) -> Result<AbiFile, String> {
    let path = abis_dir.join(format!("{}.json", name));

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to load ABI '{}': {}. Available ABIs are in the /abis folder.", name, e))?;

    let abi_file: AbiFile = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse ABI '{}': {}", name, e))?;

    Ok(abi_file)
}

/// Parse ethers Abi from our ABI file format
pub fn parse_abi(abi_file: &AbiFile) -> Result<Abi, String> {
    let abi_json = serde_json::to_string(&abi_file.abi)
        .map_err(|e| format!("Failed to serialize ABI: {}", e))?;

    serde_json::from_str(&abi_json)
        .map_err(|e| format!("Failed to parse ABI: {}", e))
}

/// Find function in ABI, selecting the correct overload by parameter count when ambiguous
pub fn find_function<'a>(abi: &'a Abi, name: &str) -> Result<&'a Function, String> {
    abi.function(name)
        .map_err(|_| format!("Function '{}' not found in ABI", name))
}

/// Find function in ABI, matching by name AND parameter count for overloaded functions
pub fn find_function_with_params<'a>(
    abi: &'a Abi,
    name: &str,
    param_count: usize,
) -> Result<&'a Function, String> {
    // Look through all overloads and find one matching the param count
    if let Some(functions) = abi.functions.get(name) {
        for func in functions {
            if func.inputs.len() == param_count {
                return Ok(func);
            }
        }
        // No exact match -- list available overloads in the error
        let overloads: Vec<String> = functions
            .iter()
            .map(|f| {
                let params: Vec<String> = f.inputs.iter().map(|i| format!("{}: {}", i.name, i.kind)).collect();
                format!("{}({})", name, params.join(", "))
            })
            .collect();
        Err(format!(
            "No '{}' overload with {} parameters. Available: {}",
            name, param_count, overloads.join(", ")
        ))
    } else {
        Err(format!("Function '{}' not found in ABI", name))
    }
}

/// Convert JSON value to ethers Token based on param type
pub fn value_to_token(value: &Value, param_type: &ParamType) -> Result<Token, String> {
    match param_type {
        ParamType::Address => {
            let s = value.as_str()
                .ok_or_else(|| format!("Expected string for address, got {:?}", value))?;
            let addr: Address = s.parse()
                .map_err(|_| format!("Invalid address: {}", s))?;
            Ok(Token::Address(addr))
        }
        ParamType::Uint(bits) => {
            let s = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return Err(format!("Expected string or number for uint{}, got {:?}", bits, value)),
            };
            let n: U256 = parse_u256(&s)
                .map_err(|_| format!("Invalid uint{}: {}", bits, s))?;
            Ok(Token::Uint(n))
        }
        ParamType::Int(bits) => {
            let s = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return Err(format!("Expected string or number for int{}, got {:?}", bits, value)),
            };
            let n: I256 = s.parse()
                .map_err(|_| format!("Invalid int{}: {}", bits, s))?;
            Ok(Token::Int(n.into_raw()))
        }
        ParamType::Bool => {
            let b = value.as_bool()
                .ok_or_else(|| format!("Expected boolean, got {:?}", value))?;
            Ok(Token::Bool(b))
        }
        ParamType::String => {
            let s = value.as_str()
                .ok_or_else(|| format!("Expected string, got {:?}", value))?;
            Ok(Token::String(s.to_string()))
        }
        ParamType::Bytes => {
            let s = value.as_str()
                .ok_or_else(|| format!("Expected hex string for bytes, got {:?}", value))?;
            let hex_str = s.strip_prefix("0x").unwrap_or(s);
            let bytes = hex::decode(hex_str)
                .map_err(|e| format!("Invalid hex for bytes: {}", e))?;
            Ok(Token::Bytes(bytes))
        }
        ParamType::FixedBytes(size) => {
            let s = value.as_str()
                .ok_or_else(|| format!("Expected hex string for bytes{}, got {:?}", size, value))?;
            let hex_str = s.strip_prefix("0x").unwrap_or(s);
            let bytes = hex::decode(hex_str)
                .map_err(|e| format!("Invalid hex for bytes{}: {}", size, e))?;
            if bytes.len() != *size {
                return Err(format!("Expected {} bytes, got {}", size, bytes.len()));
            }
            Ok(Token::FixedBytes(bytes))
        }
        ParamType::Array(inner) => {
            let arr = value.as_array()
                .ok_or_else(|| format!("Expected array, got {:?}", value))?;
            let tokens: Result<Vec<Token>, String> = arr.iter()
                .map(|v| value_to_token(v, inner))
                .collect();
            Ok(Token::Array(tokens?))
        }
        ParamType::Tuple(types) => {
            let arr = value.as_array()
                .ok_or_else(|| format!("Expected array for tuple, got {:?}", value))?;
            if arr.len() != types.len() {
                return Err(format!("Tuple expects {} elements, got {}", types.len(), arr.len()));
            }
            let tokens: Result<Vec<Token>, String> = arr.iter()
                .zip(types.iter())
                .map(|(v, t)| value_to_token(v, t))
                .collect();
            Ok(Token::Tuple(tokens?))
        }
        ParamType::FixedArray(inner, size) => {
            let arr = value.as_array()
                .ok_or_else(|| format!("Expected array, got {:?}", value))?;
            if arr.len() != *size {
                return Err(format!("Fixed array expects {} elements, got {}", size, arr.len()));
            }
            let tokens: Result<Vec<Token>, String> = arr.iter()
                .map(|v| value_to_token(v, inner))
                .collect();
            Ok(Token::FixedArray(tokens?))
        }
    }
}

/// Encode function call
pub fn encode_call(function: &Function, params: &[Value]) -> Result<Vec<u8>, String> {
    if params.len() != function.inputs.len() {
        return Err(format!(
            "Function '{}' expects {} parameters, got {}. Expected: {:?}",
            function.name,
            function.inputs.len(),
            params.len(),
            function.inputs.iter().map(|i| format!("{}: {}", i.name, i.kind)).collect::<Vec<_>>()
        ));
    }

    let tokens: Result<Vec<Token>, String> = params.iter()
        .zip(function.inputs.iter())
        .map(|(value, input)| value_to_token(value, &input.kind))
        .collect();

    let tokens = tokens?;

    function.encode_input(&tokens)
        .map_err(|e| format!("Failed to encode function call: {}", e))
}

/// Convert ethers Token to JSON value
pub fn token_to_value(token: &Token) -> Value {
    match token {
        Token::Address(a) => json!(format!("{:?}", a)),
        Token::Uint(n) => json!(n.to_string()),
        Token::Int(n) => json!(I256::from_raw(*n).to_string()),
        Token::Bool(b) => json!(b),
        Token::String(s) => json!(s),
        Token::Bytes(b) => json!(format!("0x{}", hex::encode(b))),
        Token::FixedBytes(b) => json!(format!("0x{}", hex::encode(b))),
        Token::Array(arr) | Token::FixedArray(arr) => {
            json!(arr.iter().map(|t| token_to_value(t)).collect::<Vec<_>>())
        }
        Token::Tuple(tuple) => {
            json!(tuple.iter().map(|t| token_to_value(t)).collect::<Vec<_>>())
        }
    }
}

/// Decode return value from a call
pub fn decode_return(function: &Function, data: &[u8]) -> Result<Value, String> {
    let tokens = function.decode_output(data)
        .map_err(|e| format!("Failed to decode return value: {}", e))?;

    let values: Vec<Value> = tokens.iter().map(|t| token_to_value(t)).collect();

    if values.len() == 1 {
        Ok(values.into_iter().next().unwrap())
    } else {
        Ok(Value::Array(values))
    }
}

/// Get chain ID for a network
pub fn get_chain_id(network: &str) -> u64 {
    match network {
        "mainnet" => 1,
        "polygon" => 137,
        "arbitrum" => 42161,
        "optimism" => 10,
        _ => 8453, // Base
    }
}

/// Execute a read-only call using WalletProvider
pub async fn call_function(
    network: &str,
    to: Address,
    calldata: Vec<u8>,
    rpc_config: &ResolvedRpcConfig,
    wallet_provider: &Arc<dyn WalletProvider>,
) -> Result<Vec<u8>, String> {
    let rpc = X402EvmRpc::new_with_wallet_provider(
        wallet_provider.clone(),
        network,
        Some(rpc_config.url.clone()),
        rpc_config.use_x402,
    )?;

    rpc.call(to, &calldata).await
}

/// Sign a transaction for queuing using WalletProvider
pub async fn sign_transaction_for_queue(
    network: &str,
    to: Address,
    calldata: Vec<u8>,
    value: U256,
    rpc_config: &ResolvedRpcConfig,
    wallet_provider: &Arc<dyn WalletProvider>,
) -> Result<SignedTxForQueue, String> {
    let rpc = X402EvmRpc::new_with_wallet_provider(
        wallet_provider.clone(),
        network,
        Some(rpc_config.url.clone()),
        rpc_config.use_x402,
    )?;
    let chain_id = get_chain_id(network);

    let from_str = wallet_provider.get_address();
    let from_address: Address = from_str.parse()
        .map_err(|_| format!("Invalid wallet address: {}", from_str))?;
    let to_str = format!("{:?}", to);

    let nonce = rpc.get_transaction_count(from_address).await?;

    let gas: U256 = rpc.estimate_gas(from_address, to, &calldata, value).await?;
    let gas = gas * U256::from(120) / U256::from(100); // 20% buffer

    let (max_fee, priority_fee) = rpc.estimate_eip1559_fees().await?;

    log::info!(
        "[web3_function_call] Signing tx for queue: to={:?}, value={}, data_len={} bytes, gas={}, nonce={} on {}",
        to, value, calldata.len(), gas, nonce, network
    );

    let tx = Eip1559TransactionRequest::new()
        .from(from_address)
        .to(to)
        .value(value)
        .data(calldata.clone())
        .nonce(nonce)
        .gas(gas)
        .max_fee_per_gas(max_fee)
        .max_priority_fee_per_gas(priority_fee)
        .chain_id(chain_id);

    let typed_tx: TypedTransaction = tx.into();
    let signature = wallet_provider
        .sign_transaction(&typed_tx)
        .await
        .map_err(|e| format!("Failed to sign transaction: {}", e))?;

    let signed_tx = typed_tx.rlp_signed(&signature);
    let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));

    log::info!("[web3_function_call] Transaction signed for queue, nonce={}", nonce);

    Ok(SignedTxForQueue {
        from: from_str,
        to: to_str,
        value: value.to_string(),
        data: format!("0x{}", hex::encode(&calldata)),
        gas_limit: gas.to_string(),
        max_fee_per_gas: max_fee.to_string(),
        max_priority_fee_per_gas: priority_fee.to_string(),
        nonce: nonce.as_u64(),
        signed_tx_hex,
        network: network.to_string(),
    })
}

/// Shared execution logic: ABI loading, encoding, safety checks, call/sign/queue.
/// Used by both `Web3FunctionCallTool` (manual) and `Web3PresetFunctionCallTool` (preset).
pub async fn execute_resolved_call(
    abis_dir: &PathBuf,
    abi_name: &str,
    contract_addr: &str,
    function_name: &str,
    call_params: &[Value],
    value: &str,
    call_only: bool,
    network: &Network,
    context: &ToolContext,
    preset_name: Option<&str>,
) -> ToolResult {
    // Load ABI
    let abi_file = match load_abi(abis_dir, abi_name) {
        Ok(a) => a,
        Err(e) => return ToolResult::error(e),
    };

    // Parse ABI
    let abi = match parse_abi(&abi_file) {
        Ok(a) => a,
        Err(e) => return ToolResult::error(e),
    };

    // Find function (use param-count-aware lookup to handle overloaded functions)
    let function = match find_function_with_params(&abi, function_name, call_params.len()) {
        Ok(f) => f,
        Err(e) => return ToolResult::error(e),
    };

    // Encode call
    let calldata = match encode_call(function, call_params) {
        Ok(d) => d,
        Err(e) => return ToolResult::error(e),
    };

    // Parse contract address
    let contract: Address = match contract_addr.parse() {
        Ok(a) => a,
        Err(_) => return ToolResult::error(format!("Invalid contract address: {}", contract_addr)),
    };

    // SAFETY CHECK: Detect common mistake of passing contract address to balanceOf
    if function_name == "balanceOf" && call_params.len() == 1 {
        let param_str = match &call_params[0] {
            Value::String(s) => s.to_lowercase(),
            _ => call_params[0].to_string().trim_matches('"').to_lowercase(),
        };
        let contract_str = contract_addr.to_lowercase();

        if param_str == contract_str {
            return ToolResult::error(format!(
                "ERROR: You're calling balanceOf on the token contract with the contract's OWN address as the parameter. \
                This checks how many tokens the contract itself holds, NOT your wallet balance!\n\n\
                To check YOUR token balance, use web3_preset_function_call with preset \"erc20_balance\" which automatically uses your wallet address:\n\
                {{\"tool\": \"web3_preset_function_call\", \"preset\": \"erc20_balance\", \"network\": \"{}\", \"call_only\": true}}\n\n\
                Make sure to first set the token_address register using token_lookup.",
                network
            ));
        }
    }

    // SAFETY CHECK: For transfer function, verify amount comes from register
    if function_name.to_lowercase() == "transfer" {
        // SAFETY CHECK: Prevent sending tokens TO the token contract itself (burns tokens)
        if !call_params.is_empty() {
            let recipient_str = match &call_params[0] {
                Value::String(s) => s.to_lowercase(),
                _ => call_params[0].to_string().trim_matches('"').to_lowercase(),
            };
            let token_contract_str = contract_addr.to_lowercase();

            if recipient_str == token_contract_str {
                return ToolResult::error(
                    "ERROR: The recipient address is the same as the token contract address. \
                    Sending tokens to their own contract address will BURN them permanently! \
                    Please verify the correct recipient wallet address."
                );
            }

            // SAFETY CHECK: Prevent sending tokens to the zero address (burns tokens)
            let zero_addr = "0x0000000000000000000000000000000000000000";
            if recipient_str == zero_addr {
                return ToolResult::error(
                    "ERROR: The recipient is the zero address (0x0000...0000). \
                    Sending tokens to the zero address will BURN them permanently! \
                    Please verify the correct recipient wallet address."
                );
            }
        }

        match context.registers.get("transfer_amount") {
            Some(transfer_amount_val) => {
                let expected_amount = match transfer_amount_val.as_str() {
                    Some(s) => s.to_string(),
                    None => transfer_amount_val.to_string().trim_matches('"').to_string(),
                };

                let amount_found = call_params.iter().any(|p| {
                    let param_str = match p.as_str() {
                        Some(s) => s.to_string(),
                        None => p.to_string().trim_matches('"').to_string(),
                    };
                    param_str == expected_amount
                });

                if !amount_found {
                    return ToolResult::error(
                        "transfer_amount not found in params. Suggest using the tool to_raw_amount with cache_as: \"transfer_amount\" first."
                    );
                }
            }
            None => {
                return ToolResult::error(
                    "transfer_amount not found in register. Suggest using the tool to_raw_amount with cache_as: \"transfer_amount\" first."
                );
            }
        }
    }

    // Get wallet provider (required for signing and x402 payments)
    let wallet_provider = match &context.wallet_provider {
        Some(wp) => wp,
        None => return ToolResult::error("Wallet not configured. Cannot execute web3 calls."),
    };

    // Resolve RPC configuration from context (respects custom RPC settings)
    let rpc_config = resolve_rpc_from_context(&context.extra, network.as_ref());

    log::info!(
        "[web3_function_call] {}::{}({:?}) on {} (call_only={}, rpc={})",
        abi_name, function_name, call_params, network, call_only, rpc_config.url
    );

    if call_only {
        // Read-only call
        match call_function(network.as_ref(), contract, calldata, &rpc_config, wallet_provider).await {
            Ok(result) => {
                let decoded = decode_return(function, &result)
                    .unwrap_or_else(|_| json!(format!("0x{}", hex::encode(&result))));

                ToolResult::success(serde_json::to_string_pretty(&decoded).unwrap_or_default())
                    .with_metadata(json!({
                        "preset": preset_name,
                        "abi": abi_name,
                        "contract": contract_addr,
                        "function": function_name,
                        "result": decoded,
                    }))
            }
            Err(e) => ToolResult::error(e),
        }
    } else {
        // Transaction - use parse_u256 for correct decimal/hex handling
        let tx_value: U256 = match parse_u256(value) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(format!("Invalid value: {} - {}", value, e)),
        };

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
                "Transactions cannot be executed in Discord/Telegram/Slack channels unless Rogue Mode is enabled. \
                Please enable Rogue Mode in the bot settings to allow autonomous transactions from gateway channels."
            );
        }

        // Check if tx_queue is available
        let tx_queue = match &context.tx_queue {
            Some(q) => q,
            None => return ToolResult::error("Transaction queue not available. Contact administrator."),
        };

        // Sign the transaction
        match sign_transaction_for_queue(
            network.as_ref(),
            contract,
            calldata,
            tx_value,
            &rpc_config,
            wallet_provider,
        ).await {
            Ok(signed) => {
                // Verify intent before queueing
                let value_display = if let Ok(w) = signed.value.parse::<u128>() {
                    let eth = w as f64 / 1e18;
                    if eth >= 0.0001 {
                        format!("{:.6} ETH", eth)
                    } else if w > 0 {
                        format!("{} wei", signed.value)
                    } else {
                        "0 ETH".to_string()
                    }
                } else {
                    format!("{} wei", signed.value)
                };

                let tx_type = if preset_name.is_some() {
                    "preset_call"
                } else {
                    "contract_call"
                };

                let intent = TransactionIntent {
                    tx_type: tx_type.to_string(),
                    to: contract_addr.to_string(),
                    value: signed.value.clone(),
                    value_display,
                    network: signed.network.clone(),
                    function_name: Some(function_name.to_string()),
                    abi_name: Some(abi_name.to_string()),
                    preset_name: preset_name.map(|s| s.to_string()),
                    destination_chain: None,
                    calldata: Some(signed.data.clone()),
                    description: format!(
                        "Call {}::{}() on {}",
                        abi_name, function_name, signed.network,
                    ),
                };
                if let Err(reason) = verify_intent::verify_intent(&intent, context, None).await {
                    return ToolResult::error(reason);
                }

                let uuid = Uuid::new_v4().to_string();

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
                )
                .with_preset(preset_name);

                tx_queue.queue(queued_tx);

                log::info!("[web3_function_call] Transaction queued with UUID: {}", uuid);

                let value_eth = if let Ok(w) = signed.value.parse::<u128>() {
                    let eth = w as f64 / 1e18;
                    if eth >= 0.0001 {
                        format!("{:.6} ETH", eth)
                    } else {
                        format!("{} wei", signed.value)
                    }
                } else {
                    format!("{} wei", signed.value)
                };

                ToolResult::success(format!(
                    "TRANSACTION QUEUED (not yet broadcast)\n\n\
                    UUID: {}\n\
                    Function: {}::{}()\n\
                    Network: {}\n\
                    From: {}\n\
                    To: {}\n\
                    Value: {} ({})\n\
                    Nonce: {}\n\n\
                    --- Next Steps ---\n\
                    To view queued: use `list_queued_web3_tx`\n\
                    To broadcast: use `broadcast_web3_tx` with uuid: {}",
                    uuid, abi_name, function_name, signed.network, signed.from,
                    contract_addr, signed.value, value_eth, signed.nonce, uuid
                )).with_metadata(json!({
                    "uuid": uuid,
                    "status": "queued",
                    "preset": preset_name,
                    "abi": abi_name,
                    "contract": contract_addr,
                    "function": function_name,
                    "from": signed.from,
                    "to": contract_addr,
                    "value": signed.value,
                    "nonce": signed.nonce,
                    "network": network
                }))
            }
            Err(e) => ToolResult::error(e),
        }
    }
}
