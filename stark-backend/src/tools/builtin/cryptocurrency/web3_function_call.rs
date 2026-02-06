//! Web3 Function Call tool - call any contract function using ABI
//!
//! This tool loads ABIs from the /abis folder and encodes function calls,
//! so the LLM doesn't have to deal with hex-encoded calldata.
//!
//! Supports presets for common operations (weth_deposit, weth_withdraw, etc.)
//! that read parameters from registers.
//!
//! IMPORTANT: Transactions are QUEUED, not broadcast. Use broadcast_web3_tx to broadcast.

use super::web3_tx::parse_u256;
use crate::tools::presets::{get_web3_preset, list_web3_presets};
use crate::tools::registry::Tool;
use crate::tools::rpc_config::{resolve_rpc_from_context, Network, ResolvedRpcConfig};
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tx_queue::QueuedTransaction;
use crate::wallet::WalletProvider;
use crate::x402::X402EvmRpc;
use async_trait::async_trait;
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

/// Signed transaction result for queuing (not broadcast)
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

/// Web3 function call tool
pub struct Web3FunctionCallTool {
    definition: ToolDefinition,
    abis_dir: PathBuf,
}

impl Web3FunctionCallTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "preset".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Use a preset configuration (e.g., 'weth_deposit', 'weth_withdraw'). When using a preset, only 'network' is required - other params come from registers.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "abi".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Name of the ABI file (without .json). Available: 'erc20', 'weth', '0x_settler'. Not needed if using preset.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "contract".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Contract address to call".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "function".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Function name to call (e.g., 'approve', 'transfer', 'balanceOf')".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "params".to_string(),
            PropertySchema {
                schema_type: "array".to_string(),
                description: "Function parameters as an array. Use strings for addresses and numbers, booleans for bool. Order must match the function signature.".to_string(),
                default: Some(json!([])),
                items: Some(Box::new(PropertySchema {
                    schema_type: "string".to_string(),
                    description: "Parameter value".to_string(),
                    default: None,
                    items: None,
                    enum_values: None,
                })),
                enum_values: None,
            },
        );

        properties.insert(
            "value".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "ETH value to send in wei (as decimal string). Default '0'.".to_string(),
                default: Some(json!("0")),
                items: None,
                enum_values: None,
            },
        );

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

        properties.insert(
            "call_only".to_string(),
            PropertySchema {
                schema_type: "boolean".to_string(),
                description: "If true, perform a read-only call (no transaction). Use for view/pure functions like balanceOf.".to_string(),
                default: Some(json!(false)),
                items: None,
                enum_values: None,
            },
        );

        // Determine abis directory relative to working directory
        let abis_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("abis");

        Web3FunctionCallTool {
            definition: ToolDefinition {
                name: "web3_function_call".to_string(),
                description: "Call a smart contract function. Use 'preset' for common operations (weth_deposit, weth_withdraw, weth_balance) which read params from registers. Or specify abi/contract/function directly for custom calls. Write transactions are QUEUED (not broadcast) - use broadcast_web3_tx to broadcast.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![], // No required fields - either preset or abi/contract/function
                },
                group: ToolGroup::Finance,
            },
            abis_dir,
        }
    }

    /// Load ABI from file
    fn load_abi(&self, name: &str) -> Result<AbiFile, String> {
        let path = self.abis_dir.join(format!("{}.json", name));

        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to load ABI '{}': {}. Available ABIs are in the /abis folder.", name, e))?;

        let abi_file: AbiFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse ABI '{}': {}", name, e))?;

        Ok(abi_file)
    }

    /// Parse ethers Abi from our ABI file format
    fn parse_abi(&self, abi_file: &AbiFile) -> Result<Abi, String> {
        let abi_json = serde_json::to_string(&abi_file.abi)
            .map_err(|e| format!("Failed to serialize ABI: {}", e))?;

        serde_json::from_str(&abi_json)
            .map_err(|e| format!("Failed to parse ABI: {}", e))
    }

    /// Find function in ABI
    fn find_function<'a>(&self, abi: &'a Abi, name: &str) -> Result<&'a Function, String> {
        abi.function(name)
            .map_err(|_| format!("Function '{}' not found in ABI", name))
    }

    /// Convert JSON value to ethers Token based on param type
    fn value_to_token(&self, value: &Value, param_type: &ParamType) -> Result<Token, String> {
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
                // Use parse_u256 to handle both decimal and hex strings correctly
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
                // Parse as signed - ethers handles the conversion
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
                    .map(|v| self.value_to_token(v, inner))
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
                    .map(|(v, t)| self.value_to_token(v, t))
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
                    .map(|v| self.value_to_token(v, inner))
                    .collect();
                Ok(Token::FixedArray(tokens?))
            }
        }
    }

    /// Encode function call
    fn encode_call(&self, function: &Function, params: &[Value]) -> Result<Vec<u8>, String> {
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
            .map(|(value, input)| self.value_to_token(value, &input.kind))
            .collect();

        let tokens = tokens?;

        function.encode_input(&tokens)
            .map_err(|e| format!("Failed to encode function call: {}", e))
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

    /// Execute a read-only call using WalletProvider
    async fn call_function(
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

    /// Sign a transaction for queuing using WalletProvider (works in both Standard and Flash mode)
    async fn sign_transaction_for_queue(
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
        let chain_id = Self::get_chain_id(network);

        // Get wallet address from WalletProvider
        let from_str = wallet_provider.get_address();
        let from_address: Address = from_str.parse()
            .map_err(|_| format!("Invalid wallet address: {}", from_str))?;
        let to_str = format!("{:?}", to);

        // Get nonce
        let nonce = rpc.get_transaction_count(from_address).await?;

        // Estimate gas
        let gas: U256 = rpc.estimate_gas(from_address, to, &calldata, value).await?;
        let gas = gas * U256::from(120) / U256::from(100); // 20% buffer

        // Get gas prices
        let (max_fee, priority_fee) = rpc.estimate_eip1559_fees().await?;

        log::info!(
            "[web3_function_call] Signing tx for queue: to={:?}, value={}, data_len={} bytes, gas={}, nonce={} on {}",
            to, value, calldata.len(), gas, nonce, network
        );

        // Build EIP-1559 transaction
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

        // Sign using WalletProvider (works in both Standard and Flash mode)
        let typed_tx: TypedTransaction = tx.into();
        let signature = wallet_provider
            .sign_transaction(&typed_tx)
            .await
            .map_err(|e| format!("Failed to sign transaction: {}", e))?;

        // Serialize the signed transaction
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

    /// Decode return value from a call
    fn decode_return(&self, function: &Function, data: &[u8]) -> Result<Value, String> {
        let tokens = function.decode_output(data)
            .map_err(|e| format!("Failed to decode return value: {}", e))?;

        // Convert tokens to JSON
        let values: Vec<Value> = tokens.iter().map(|t| self.token_to_value(t)).collect();

        if values.len() == 1 {
            Ok(values.into_iter().next().unwrap())
        } else {
            Ok(Value::Array(values))
        }
    }

    /// Convert ethers Token to JSON value
    fn token_to_value(&self, token: &Token) -> Value {
        match token {
            Token::Address(a) => json!(format!("{:?}", a)),
            Token::Uint(n) => json!(n.to_string()),
            Token::Int(n) => json!(I256::from_raw(*n).to_string()),
            Token::Bool(b) => json!(b),
            Token::String(s) => json!(s),
            Token::Bytes(b) => json!(format!("0x{}", hex::encode(b))),
            Token::FixedBytes(b) => json!(format!("0x{}", hex::encode(b))),
            Token::Array(arr) | Token::FixedArray(arr) => {
                json!(arr.iter().map(|t| self.token_to_value(t)).collect::<Vec<_>>())
            }
            Token::Tuple(tuple) => {
                json!(tuple.iter().map(|t| self.token_to_value(t)).collect::<Vec<_>>())
            }
        }
    }
}

impl Default for Web3FunctionCallTool {
    fn default() -> Self {
        Self::new()
    }
}

/// ABI file structure
#[derive(Debug, Deserialize)]
struct AbiFile {
    name: String,
    #[serde(default)]
    description: String,
    abi: Vec<Value>,
    #[serde(default)]
    address: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Web3FunctionCallParams {
    preset: Option<String>,
    abi: Option<String>,
    contract: Option<String>,
    function: Option<String>,
    #[serde(default)]
    params: Vec<Value>,
    #[serde(default = "default_value")]
    value: String,
    /// Network for the call. If not specified, uses context's selected_network or defaults to Base
    network: Option<String>,
    #[serde(default)]
    call_only: bool,
}

fn default_value() -> String {
    "0".to_string()
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
impl Tool for Web3FunctionCallTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: Web3FunctionCallParams = match serde_json::from_value(params) {
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

        log::info!("[WEB3_FUNCTION_CALL] Using network: {} (from param: {:?}, context: {:?})",
            network, params.network, context.selected_network);

        // Resolve preset or use direct params
        let (abi_name, contract_addr, function_name, call_params, value) = if let Some(ref preset_name) = params.preset {
            // Using preset - load config and resolve from registers
            let preset = match get_web3_preset(preset_name) {
                Some(p) => p,
                None => {
                    let available = list_web3_presets().join(", ");
                    return ToolResult::error(format!(
                        "Unknown preset '{}'. Available: {}",
                        preset_name, available
                    ));
                }
            };

            // Get contract address - either from register or hardcoded per network
            let contract = if let Some(ref contract_reg) = preset.contract_register {
                // Read contract address from register
                match context.registers.get(contract_reg) {
                    Some(v) => match v.as_str() {
                        Some(s) => s.to_string(),
                        None => v.to_string().trim_matches('"').to_string(),
                    },
                    None => {
                        return ToolResult::error(format!(
                            "Preset '{}' requires register '{}' for contract address but it's not set",
                            preset_name, contract_reg
                        ));
                    }
                }
            } else {
                // Use hardcoded contract for network
                match preset.contracts.get(network.as_ref()) {
                    Some(c) => c.clone(),
                    None => {
                        return ToolResult::error(format!(
                            "Preset '{}' has no contract for network '{}'",
                            preset_name, network
                        ));
                    }
                }
            };

            // Read params from registers
            let mut resolved_params = Vec::new();
            for reg_key in &preset.params_registers {
                match context.registers.get(reg_key) {
                    Some(v) => {
                        // Convert JSON value to string for params
                        let param_str = match v.as_str() {
                            Some(s) => s.to_string(),
                            None => v.to_string().trim_matches('"').to_string(),
                        };
                        resolved_params.push(json!(param_str));
                    }
                    None => {
                        return ToolResult::error(format!(
                            "Preset '{}' requires register '{}' but it's not set",
                            preset_name, reg_key
                        ));
                    }
                }
            }

            // Read value from register if specified
            let value = if let Some(ref val_reg) = preset.value_register {
                match context.registers.get(val_reg) {
                    Some(v) => {
                        match v.as_str() {
                            Some(s) => s.to_string(),
                            None => v.to_string().trim_matches('"').to_string(),
                        }
                    }
                    None => {
                        return ToolResult::error(format!(
                            "Preset '{}' requires register '{}' but it's not set",
                            preset_name, val_reg
                        ));
                    }
                }
            } else {
                "0".to_string()
            };

            log::info!(
                "[web3_function_call] Using preset '{}': {}::{}",
                preset_name, preset.abi, preset.function
            );

            (preset.abi, contract, preset.function, resolved_params, value)
        } else {
            // Direct params - require abi, contract, function
            let abi = match params.abi.clone() {
                Some(a) => a,
                None => return ToolResult::error("Missing 'abi' parameter (required without preset)"),
            };
            let contract = match params.contract.clone() {
                Some(c) => c,
                None => return ToolResult::error("Missing 'contract' parameter (required without preset)"),
            };
            let function = match params.function.clone() {
                Some(f) => f,
                None => return ToolResult::error("Missing 'function' parameter (required without preset)"),
            };

            (abi, contract, function, params.params.clone(), params.value.clone())
        };

        // Load ABI
        let abi_file = match self.load_abi(&abi_name) {
            Ok(a) => a,
            Err(e) => return ToolResult::error(e),
        };

        // Parse ABI
        let abi = match self.parse_abi(&abi_file) {
            Ok(a) => a,
            Err(e) => return ToolResult::error(e),
        };

        // Find function
        let function = match self.find_function(&abi, &function_name) {
            Ok(f) => f,
            Err(e) => return ToolResult::error(e),
        };

        // Encode call
        let calldata = match self.encode_call(function, &call_params) {
            Ok(d) => d,
            Err(e) => return ToolResult::error(e),
        };

        // Parse contract address
        let contract: Address = match contract_addr.parse() {
            Ok(a) => a,
            Err(_) => return ToolResult::error(format!("Invalid contract address: {}", contract_addr)),
        };

        // SAFETY CHECK: Detect common mistake of passing contract address to balanceOf
        // When checking token balance, you want balanceOf(wallet_address), NOT balanceOf(contract_address)
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
                    To check YOUR token balance, use the erc20_balance preset which automatically uses your wallet address:\n\
                    {{\n  \"preset\": \"erc20_balance\",\n  \"network\": \"{}\",\n  \"call_only\": true\n}}\n\n\
                    Make sure to first set the token_address register using token_lookup.",
                    network
                ));
            }
        }

        // SAFETY CHECK: For transfer function, verify amount comes from register
        if function_name.to_lowercase() == "transfer" {
            // SAFETY CHECK: Prevent sending tokens TO the token contract itself (burns tokens)
            if call_params.len() >= 1 {
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
                    // Get the transfer_amount as a string for comparison
                    let expected_amount = match transfer_amount_val.as_str() {
                        Some(s) => s.to_string(),
                        None => transfer_amount_val.to_string().trim_matches('"').to_string(),
                    };

                    // Check if any param matches the expected amount
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
            abi_name, function_name, call_params, network, params.call_only, rpc_config.url
        );

        if params.call_only {
            // Read-only call
            match Self::call_function(network.as_ref(), contract, calldata, &rpc_config, wallet_provider).await {
                Ok(result) => {
                    let decoded = self.decode_return(function, &result)
                        .unwrap_or_else(|_| json!(format!("0x{}", hex::encode(&result))));

                    ToolResult::success(serde_json::to_string_pretty(&decoded).unwrap_or_default())
                        .with_metadata(json!({
                            "preset": params.preset,
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
            let tx_value: U256 = match parse_u256(&value) {
                Ok(v) => v,
                Err(e) => return ToolResult::error(format!("Invalid value: {} - {}", value, e)),
            };

            // Check if we're in a gateway channel (discord, telegram, slack) without rogue mode
            // Gateway channels require rogue mode to be enabled for transactions
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

            // Sign the transaction using WalletProvider (works in both Standard and Flash mode)
            match Self::sign_transaction_for_queue(
                network.as_ref(),
                contract,
                calldata,
                tx_value,
                &rpc_config,
                wallet_provider,
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

                    log::info!("[web3_function_call] Transaction queued with UUID: {}", uuid);

                    // Format value as ETH for display
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
                        "preset": params.preset,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::RegisterStore;
    use crate::tools::registry::Tool;
    use serde_json::json;

    /// Helper: create a Web3FunctionCallTool pointing at the repo's abis/ dir
    fn make_tool() -> Web3FunctionCallTool {
        // Tests run from the workspace root, so abis/ should be found by default.
        // If not, override to the known absolute path.
        let mut tool = Web3FunctionCallTool::new();
        let repo_abis = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("abis");
        if repo_abis.exists() {
            tool.abis_dir = repo_abis;
        }
        tool
    }

    // ─── Transfer safety checks ───────────────────────────────────────

    #[tokio::test]
    async fn test_transfer_to_token_contract_blocked() {
        let tool = make_tool();
        let token_addr = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"; // USDC on Base

        let context = crate::tools::ToolContext::new();

        let result = tool.execute(json!({
            "abi": "erc20",
            "contract": token_addr,
            "function": "transfer",
            "params": [token_addr, "1000000"],  // recipient == token contract
            "network": "base"
        }), &context).await;

        assert!(!result.success, "Should have been blocked by safety check");
        assert!(result.content.contains("same as the token contract address"),
            "Expected token-contract error, got: {}", result.content);
    }

    #[tokio::test]
    async fn test_transfer_to_zero_address_blocked() {
        let tool = make_tool();
        let token_addr = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
        let zero_addr = "0x0000000000000000000000000000000000000000";

        let context = crate::tools::ToolContext::new();

        let result = tool.execute(json!({
            "abi": "erc20",
            "contract": token_addr,
            "function": "transfer",
            "params": [zero_addr, "1000000"],
            "network": "base"
        }), &context).await;

        assert!(!result.success, "Should have been blocked by safety check");
        assert!(result.content.contains("zero address"),
            "Expected zero-address error, got: {}", result.content);
    }

    #[tokio::test]
    async fn test_balance_of_own_contract_blocked() {
        let tool = make_tool();
        let token_addr = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";

        let context = crate::tools::ToolContext::new();

        let result = tool.execute(json!({
            "abi": "erc20",
            "contract": token_addr,
            "function": "balanceOf",
            "params": [token_addr],
            "call_only": true,
            "network": "base"
        }), &context).await;

        assert!(!result.success, "Should have been blocked by safety check");
        assert!(result.content.contains("contract's OWN address"),
            "Expected balanceOf-self error, got: {}", result.content);
    }

    #[tokio::test]
    async fn test_transfer_missing_amount_register_blocked() {
        let tool = make_tool();
        let token_addr = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
        let recipient = "0x1234567890abcdef1234567890abcdef12345678";

        // No transfer_amount register set
        let context = crate::tools::ToolContext::new();

        let result = tool.execute(json!({
            "abi": "erc20",
            "contract": token_addr,
            "function": "transfer",
            "params": [recipient, "1000000"],
            "network": "base"
        }), &context).await;

        assert!(!result.success, "Should require transfer_amount register");
        assert!(result.content.contains("transfer_amount"),
            "Expected transfer_amount error, got: {}", result.content);
    }

    #[tokio::test]
    async fn test_transfer_valid_passes_safety_checks() {
        let tool = make_tool();
        let token_addr = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
        let recipient = "0x1234567890abcdef1234567890abcdef12345678";
        let amount = "1000000";

        // Set up transfer_amount register so the amount check passes
        let registers = RegisterStore::new();
        registers.set("transfer_amount", json!(amount), "to_raw_amount");
        let context = crate::tools::ToolContext::new()
            .with_registers(registers);

        let result = tool.execute(json!({
            "abi": "erc20",
            "contract": token_addr,
            "function": "transfer",
            "params": [recipient, amount],
            "network": "base"
        }), &context).await;

        // Should pass all safety checks and fail at wallet (no wallet configured in test)
        assert!(!result.success);
        assert!(result.content.contains("Wallet not configured"),
            "Expected wallet error (safety checks passed), got: {}", result.content);
    }

    // ─── ABI loading / encoding ───────────────────────────────────────

    #[test]
    fn test_load_erc20_abi() {
        let tool = make_tool();
        let abi_file = tool.load_abi("erc20").expect("Should load erc20.json");
        assert_eq!(abi_file.name, "ERC20");
    }

    #[test]
    fn test_find_transfer_function() {
        let tool = make_tool();
        let abi_file = tool.load_abi("erc20").unwrap();
        let abi = tool.parse_abi(&abi_file).unwrap();
        let func = tool.find_function(&abi, "transfer").expect("Should find transfer");
        assert_eq!(func.inputs.len(), 2);
    }

    #[test]
    fn test_encode_transfer_call() {
        let tool = make_tool();
        let abi_file = tool.load_abi("erc20").unwrap();
        let abi = tool.parse_abi(&abi_file).unwrap();
        let func = tool.find_function(&abi, "transfer").unwrap();

        let params = vec![
            json!("0x1234567890abcdef1234567890abcdef12345678"),
            json!("1000000"),
        ];
        let encoded = tool.encode_call(func, &params);
        assert!(encoded.is_ok(), "Should encode transfer call: {:?}", encoded.err());
        // transfer(address,uint256) selector = 0xa9059cbb
        assert_eq!(&encoded.unwrap()[..4], &[0xa9, 0x05, 0x9c, 0xbb]);
    }

    #[test]
    fn test_encode_call_wrong_param_count() {
        let tool = make_tool();
        let abi_file = tool.load_abi("erc20").unwrap();
        let abi = tool.parse_abi(&abi_file).unwrap();
        let func = tool.find_function(&abi, "transfer").unwrap();

        // Only 1 param instead of 2
        let params = vec![json!("0x1234567890abcdef1234567890abcdef12345678")];
        let result = tool.encode_call(func, &params);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expects 2 parameters"));
    }

    #[test]
    fn test_value_to_token_address() {
        let tool = make_tool();
        let token = tool.value_to_token(
            &json!("0x1234567890abcdef1234567890abcdef12345678"),
            &ParamType::Address,
        );
        assert!(token.is_ok());
    }

    #[test]
    fn test_value_to_token_invalid_address() {
        let tool = make_tool();
        let token = tool.value_to_token(&json!("not-an-address"), &ParamType::Address);
        assert!(token.is_err());
    }

    #[test]
    fn test_value_to_token_uint256() {
        let tool = make_tool();
        let token = tool.value_to_token(&json!("1000000"), &ParamType::Uint(256));
        assert!(token.is_ok());
        if let Ok(Token::Uint(v)) = token {
            assert_eq!(v, U256::from(1_000_000u64));
        }
    }
}

