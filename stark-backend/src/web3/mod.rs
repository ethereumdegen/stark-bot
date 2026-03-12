//! Web3 utility types and functions for EVM contract interaction.
//!
//! Provides ABI loading, encoding/decoding, transaction signing, and call execution.

use crate::rpc_config::{Network, ResolvedRpcConfig};
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
use std::sync::{Arc, Mutex, OnceLock};

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

/// Resolve the network from params or default
pub fn resolve_network(param_network: Option<&str>, context_network: Option<&str>) -> Result<Network, String> {
    let network_str = param_network
        .or(context_network)
        .unwrap_or("base");
    Network::from_str(network_str)
        .map_err(|_| format!("Invalid network '{}'. Must be one of: base, mainnet, polygon", network_str))
}

/// Determine abis directory
pub fn default_abis_dir() -> PathBuf {
    crate::config::repo_root().join("abis")
}

// ---- Global ABI content index ----

static ABI_INDEX: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn abi_index() -> &'static Mutex<HashMap<String, String>> {
    ABI_INDEX.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register_abi_content(name: &str, json_content: &str) {
    let mut index = abi_index().lock().unwrap();
    index.insert(name.to_string(), json_content.to_string());
}

pub fn clear_abi_index() {
    if let Some(index) = ABI_INDEX.get() {
        index.lock().unwrap().clear();
    }
}

/// Load ABI by name from abis/ directory or content index
pub fn load_abi(abis_dir: &PathBuf, name: &str) -> Result<AbiFile, String> {
    let global_path = abis_dir.join(format!("{}.json", name));
    if global_path.exists() {
        let content = std::fs::read_to_string(&global_path)
            .map_err(|e| format!("Failed to load ABI '{}': {}", name, e))?;
        let abi_file: AbiFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse ABI '{}': {}", name, e))?;
        return Ok(abi_file);
    }

    if let Some(content) = abi_index().lock().unwrap().get(name).cloned() {
        let abi_file: AbiFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse ABI '{}': {}", name, e))?;
        return Ok(abi_file);
    }

    Err(format!("ABI '{}' not found in {} or content index", name, abis_dir.display()))
}

/// Parse ethers Abi from our ABI file format
pub fn parse_abi(abi_file: &AbiFile) -> Result<Abi, String> {
    let abi_json = serde_json::to_string(&abi_file.abi)
        .map_err(|e| format!("Failed to serialize ABI: {}", e))?;
    serde_json::from_str(&abi_json)
        .map_err(|e| format!("Failed to parse ABI: {}", e))
}

/// Find function in ABI
pub fn find_function<'a>(abi: &'a Abi, name: &str) -> Result<&'a Function, String> {
    abi.function(name)
        .map_err(|_| format!("Function '{}' not found in ABI", name))
}

/// Find function in ABI matching by name AND parameter count
pub fn find_function_with_params<'a>(
    abi: &'a Abi, name: &str, param_count: usize,
) -> Result<&'a Function, String> {
    if let Some(functions) = abi.functions.get(name) {
        for func in functions {
            if func.inputs.len() == param_count {
                return Ok(func);
            }
        }
        let overloads: Vec<String> = functions.iter()
            .map(|f| {
                let params: Vec<String> = f.inputs.iter().map(|i| format!("{}: {}", i.name, i.kind)).collect();
                format!("{}({})", name, params.join(", "))
            })
            .collect();
        Err(format!("No '{}' overload with {} params. Available: {}", name, param_count, overloads.join(", ")))
    } else {
        Err(format!("Function '{}' not found in ABI", name))
    }
}

/// Parse a U256 from decimal or hex string
pub fn parse_u256(s: &str) -> Result<U256, String> {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        U256::from_str(s).map_err(|e| format!("Invalid hex U256: {}", e))
    } else {
        U256::from_dec_str(s).map_err(|e| format!("Invalid decimal U256: {}", e))
    }
}

/// Convert JSON value to ethers Token
pub fn value_to_token(value: &Value, param_type: &ParamType) -> Result<Token, String> {
    match param_type {
        ParamType::Address => {
            let s = value.as_str().ok_or_else(|| format!("Expected string for address, got {:?}", value))?;
            let addr: Address = s.parse().map_err(|_| format!("Invalid address: {}", s))?;
            Ok(Token::Address(addr))
        }
        ParamType::Uint(bits) => {
            let s = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return Err(format!("Expected string or number for uint{}, got {:?}", bits, value)),
            };
            let n = parse_u256(&s)?;
            Ok(Token::Uint(n))
        }
        ParamType::Int(bits) => {
            let s = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return Err(format!("Expected string or number for int{}, got {:?}", bits, value)),
            };
            let n: I256 = s.parse().map_err(|_| format!("Invalid int{}: {}", bits, s))?;
            Ok(Token::Int(n.into_raw()))
        }
        ParamType::Bool => {
            let b = value.as_bool().ok_or_else(|| format!("Expected boolean, got {:?}", value))?;
            Ok(Token::Bool(b))
        }
        ParamType::String => {
            let s = value.as_str().ok_or_else(|| format!("Expected string, got {:?}", value))?;
            Ok(Token::String(s.to_string()))
        }
        ParamType::Bytes => {
            let s = value.as_str().ok_or_else(|| format!("Expected hex string for bytes, got {:?}", value))?;
            let hex_str = s.strip_prefix("0x").unwrap_or(s);
            let bytes = hex::decode(hex_str).map_err(|e| format!("Invalid hex for bytes: {}", e))?;
            Ok(Token::Bytes(bytes))
        }
        ParamType::FixedBytes(size) => {
            let s = value.as_str().ok_or_else(|| format!("Expected hex string for bytes{}, got {:?}", size, value))?;
            let hex_str = s.strip_prefix("0x").unwrap_or(s);
            let bytes = hex::decode(hex_str).map_err(|e| format!("Invalid hex for bytes{}: {}", size, e))?;
            if bytes.len() != *size { return Err(format!("Expected {} bytes, got {}", size, bytes.len())); }
            Ok(Token::FixedBytes(bytes))
        }
        ParamType::Array(inner) => {
            let arr = value.as_array().ok_or_else(|| format!("Expected array, got {:?}", value))?;
            let tokens: Result<Vec<Token>, String> = arr.iter().map(|v| value_to_token(v, inner)).collect();
            Ok(Token::Array(tokens?))
        }
        ParamType::Tuple(types) => {
            let arr = value.as_array().ok_or_else(|| format!("Expected array for tuple, got {:?}", value))?;
            if arr.len() != types.len() { return Err(format!("Tuple expects {} elements, got {}", types.len(), arr.len())); }
            let tokens: Result<Vec<Token>, String> = arr.iter().zip(types.iter()).map(|(v, t)| value_to_token(v, t)).collect();
            Ok(Token::Tuple(tokens?))
        }
        ParamType::FixedArray(inner, size) => {
            let arr = value.as_array().ok_or_else(|| format!("Expected array, got {:?}", value))?;
            if arr.len() != *size { return Err(format!("Fixed array expects {} elements, got {}", size, arr.len())); }
            let tokens: Result<Vec<Token>, String> = arr.iter().map(|v| value_to_token(v, inner)).collect();
            Ok(Token::FixedArray(tokens?))
        }
    }
}

/// Encode function call
pub fn encode_call(function: &Function, params: &[Value]) -> Result<Vec<u8>, String> {
    if params.len() != function.inputs.len() {
        return Err(format!(
            "Function '{}' expects {} parameters, got {}",
            function.name, function.inputs.len(), params.len()
        ));
    }
    let tokens: Result<Vec<Token>, String> = params.iter()
        .zip(function.inputs.iter())
        .map(|(value, input)| value_to_token(value, &input.kind))
        .collect();
    function.encode_input(&tokens?).map_err(|e| format!("Failed to encode: {}", e))
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
    let tokens = function.decode_output(data).map_err(|e| format!("Failed to decode: {}", e))?;
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

/// Execute a read-only call
pub async fn call_function(
    network: &str, to: Address, calldata: Vec<u8>,
    rpc_config: &ResolvedRpcConfig, wallet_provider: &Arc<dyn WalletProvider>,
) -> Result<Vec<u8>, String> {
    let rpc = X402EvmRpc::new_with_wallet_provider(
        wallet_provider.clone(), network, Some(rpc_config.url.clone()), rpc_config.use_x402,
    )?;
    rpc.call(to, &calldata).await
}

/// Sign a transaction for queuing
pub async fn sign_transaction_for_queue(
    network: &str, to: Address, calldata: Vec<u8>, value: U256,
    rpc_config: &ResolvedRpcConfig, wallet_provider: &Arc<dyn WalletProvider>,
) -> Result<SignedTxForQueue, String> {
    let rpc = X402EvmRpc::new_with_wallet_provider(
        wallet_provider.clone(), network, Some(rpc_config.url.clone()), rpc_config.use_x402,
    )?;
    let chain_id = get_chain_id(network);

    let from_str = wallet_provider.get_address();
    let from_address: Address = from_str.parse().map_err(|_| format!("Invalid wallet address: {}", from_str))?;
    let to_str = format!("{:?}", to);

    let nonce = rpc.get_transaction_count(from_address).await?;
    let gas: U256 = rpc.estimate_gas(from_address, to, &calldata, value).await?;
    let gas = gas * U256::from(120) / U256::from(100);
    let (max_fee, priority_fee) = rpc.estimate_eip1559_fees().await?;

    let tx = Eip1559TransactionRequest::new()
        .from(from_address).to(to).value(value)
        .data(calldata.clone()).nonce(nonce).gas(gas)
        .max_fee_per_gas(max_fee).max_priority_fee_per_gas(priority_fee)
        .chain_id(chain_id);

    let typed_tx: TypedTransaction = tx.into();
    let signature = wallet_provider.sign_transaction(&typed_tx).await
        .map_err(|e| format!("Failed to sign transaction: {}", e))?;

    let signed_tx = typed_tx.rlp_signed(&signature);
    let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));

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

/// Execute a standalone web3 call (for CryptoExecutor).
/// Simplified version without ToolContext/ToolResult.
pub async fn execute_standalone_call(
    abi_name: &str,
    contract_addr: &str,
    function_name: &str,
    call_params: &[Value],
    value_str: &str,
    call_only: bool,
    network: &str,
    wallet_provider: &Arc<dyn WalletProvider>,
    tx_queue: &Arc<crate::tx_queue::TxQueueManager>,
) -> Result<Value, String> {
    let abis_dir = default_abis_dir();
    let abi_file = load_abi(&abis_dir, abi_name)?;
    let abi = parse_abi(&abi_file)?;
    let function = find_function_with_params(&abi, function_name, call_params.len())?;
    let calldata = encode_call(function, call_params)?;
    let contract: Address = contract_addr.parse().map_err(|_| format!("Invalid contract address: {}", contract_addr))?;

    let rpc_config = crate::rpc_config::resolve_rpc(network);

    if call_only {
        let result = call_function(network, contract, calldata, &rpc_config, wallet_provider).await?;
        let decoded = decode_return(function, &result)
            .unwrap_or_else(|_| json!(format!("0x{}", hex::encode(&result))));
        Ok(json!({
            "abi": abi_name,
            "contract": contract_addr,
            "function": function_name,
            "result": decoded,
        }))
    } else {
        let tx_value = parse_u256(value_str)?;
        let signed = sign_transaction_for_queue(network, contract, calldata, tx_value, &rpc_config, wallet_provider).await?;

        let uuid = uuid::Uuid::new_v4().to_string();
        let queued_tx = QueuedTransaction::new(
            uuid.clone(), signed.network.clone(),
            signed.from.clone(), signed.to.clone(),
            signed.value.clone(), signed.data.clone(),
            signed.gas_limit.clone(), signed.max_fee_per_gas.clone(),
            signed.max_priority_fee_per_gas.clone(),
            signed.nonce, signed.signed_tx_hex.clone(), None,
        );
        tx_queue.queue(queued_tx);

        Ok(json!({
            "uuid": uuid,
            "status": "queued",
            "abi": abi_name,
            "contract": contract_addr,
            "function": function_name,
            "from": signed.from,
            "to": contract_addr,
            "value": signed.value,
            "nonce": signed.nonce,
            "network": network,
        }))
    }
}
