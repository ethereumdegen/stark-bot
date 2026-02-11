//! Decode calldata tool - decode raw calldata using an ABI
//!
//! This tool takes raw hex calldata and an ABI name, decodes the function
//! selector and parameters, and stores the results in registers for use
//! by web3_function_call.
//!
//! Primary use case: Decoding 0x swap quotes so they can be executed via
//! web3_function_call with proper ABI encoding.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use ethers::abi::{Abi, Token, ParamType};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::web3::default_abis_dir;

/// Decode calldata tool
pub struct DecodeCalldataTool {
    definition: ToolDefinition,
}

impl DecodeCalldataTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "abi".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Name of the ABI file (without .json) to use for decoding. E.g., '0x_settler', 'erc20'.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "calldata".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Raw hex calldata to decode (with or without 0x prefix).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "calldata_register".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Register name containing calldata. Use either 'calldata' or 'calldata_register', not both.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "cache_as".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Register prefix for storing decoded results. Will set {cache_as}_function and {cache_as}_params.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        DecodeCalldataTool {
            definition: ToolDefinition {
                name: "decode_calldata".to_string(),
                description: "Decode raw calldata using an ABI. Extracts function name and parameters, storing them in registers for use with web3_function_call.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["abi".to_string(), "cache_as".to_string()],
                },
                group: ToolGroup::Finance,
            },
        }
    }

    /// Load ABI from file
    fn load_abi(&self, abis_dir: &PathBuf, name: &str) -> Result<AbiFile, String> {
        let path = abis_dir.join(format!("{}.json", name));

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

    /// Convert ethers Token to JSON value
    fn token_to_value(&self, token: &Token) -> Value {
        match token {
            Token::Address(a) => json!(format!("{:?}", a)),
            Token::Uint(n) => json!(n.to_string()),
            Token::Int(n) => json!(ethers::types::I256::from_raw(*n).to_string()),
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

    /// Decode calldata using the ABI
    fn decode_calldata(&self, abi: &Abi, calldata: &[u8]) -> Result<(String, Vec<Value>), String> {
        if calldata.len() < 4 {
            return Err("Calldata too short - must have at least 4 bytes for function selector".to_string());
        }

        // Extract function selector (first 4 bytes)
        let selector = &calldata[0..4];
        let selector_hex = format!("0x{}", hex::encode(selector));

        // Find matching function in ABI
        for function in abi.functions() {
            let func_selector = function.short_signature();
            if func_selector == selector {
                // Found the function! Now decode the params
                let params_data = &calldata[4..];

                // Get param types
                let param_types: Vec<ParamType> = function.inputs.iter()
                    .map(|p| p.kind.clone())
                    .collect();

                // Decode parameters
                let tokens = ethers::abi::decode(&param_types, params_data)
                    .map_err(|e| format!("Failed to decode parameters for function '{}': {}", function.name, e))?;

                // Convert tokens to JSON values
                let params: Vec<Value> = tokens.iter()
                    .map(|t| self.token_to_value(t))
                    .collect();

                log::info!(
                    "[decode_calldata] Decoded function '{}' with {} params",
                    function.name, params.len()
                );

                return Ok((function.name.clone(), params));
            }
        }

        Err(format!(
            "No function found with selector {} in ABI. This may be a different contract or ABI version.",
            selector_hex
        ))
    }
}

impl Default for DecodeCalldataTool {
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
struct DecodeCalldataParams {
    abi: String,
    calldata: Option<String>,
    calldata_register: Option<String>,
    cache_as: String,
}

#[async_trait]
impl Tool for DecodeCalldataTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: DecodeCalldataParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get calldata from direct param or register
        // Also extract contract address if available
        let mut contract_address: Option<String> = None;
        let mut tx_value: Option<String> = None;

        let used_raw_calldata;
        let calldata_hex = if let Some(ref cd) = params.calldata {
            used_raw_calldata = true;
            cd.clone()
        } else if let Some(ref reg_name) = params.calldata_register {
            used_raw_calldata = false;
            // Read from register
            match context.registers.get(reg_name) {
                Some(v) => {
                    log::info!("[decode_calldata] Register '{}' type={}, keys={:?}",
                        reg_name,
                        match &v { Value::Object(_) => "object", Value::String(_) => "string", Value::Array(_) => "array", Value::Number(_) => "number", Value::Bool(_) => "bool", Value::Null => "null" },
                        v.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>())
                    );
                    // Could be a string directly, or an object with a "data" field
                    if let Some(s) = v.as_str() {
                        s.to_string()
                    } else if let Some(data) = v.get("data").and_then(|d| d.as_str()) {
                        // Also extract "to" (contract address) and "value" if present
                        // Handle any JSON type — stringify numbers, bools, etc.
                        if let Some(to_val) = v.get("to") {
                            match to_val {
                                Value::String(s) => contract_address = Some(s.clone()),
                                Value::Null => log::warn!("[decode_calldata] 'to' field is null in register '{}'", reg_name),
                                other => {
                                    let s = other.to_string().trim_matches('"').to_string();
                                    log::warn!("[decode_calldata] 'to' field is not a string (type: {}), coercing to '{}'",
                                        match other { Value::Number(_) => "number", Value::Bool(_) => "bool", _ => "other" }, s);
                                    contract_address = Some(s);
                                }
                            }
                        } else {
                            log::warn!("[decode_calldata] No 'to' field in register '{}' — {}_contract will NOT be set", reg_name, params.cache_as);
                        }
                        if let Some(val_field) = v.get("value") {
                            match val_field {
                                Value::String(s) => tx_value = Some(s.clone()),
                                Value::Number(n) => tx_value = Some(n.to_string()),
                                Value::Null => log::warn!("[decode_calldata] 'value' field is null in register '{}'", reg_name),
                                other => {
                                    tx_value = Some(other.to_string().trim_matches('"').to_string());
                                }
                            }
                        } else {
                            log::warn!("[decode_calldata] No 'value' field in register '{}' — {}_value will NOT be set", reg_name, params.cache_as);
                        }
                        data.to_string()
                    } else {
                        return ToolResult::error(format!(
                            "Register '{}' does not contain valid calldata. Expected string or object with 'data' field.",
                            reg_name
                        ));
                    }
                }
                None => {
                    return ToolResult::error(format!(
                        "Register '{}' not found. Available: {:?}",
                        reg_name, context.registers.keys()
                    ));
                }
            }
        } else {
            used_raw_calldata = false;
            return ToolResult::error("Must provide either 'calldata' or 'calldata_register'");
        };

        // Load and parse ABI
        let abis_dir = default_abis_dir();
        let abi_file = match self.load_abi(&abis_dir, &params.abi) {
            Ok(a) => a,
            Err(e) => return ToolResult::error(e),
        };

        let abi = match self.parse_abi(&abi_file) {
            Ok(a) => a,
            Err(e) => return ToolResult::error(e),
        };

        // Parse calldata hex
        let hex_str = calldata_hex.strip_prefix("0x").unwrap_or(&calldata_hex);
        let calldata = match hex::decode(hex_str) {
            Ok(b) => b,
            Err(e) => return ToolResult::error(format!("Invalid hex calldata: {}", e)),
        };

        // Decode the calldata
        let (function_name, decoded_params) = match self.decode_calldata(&abi, &calldata) {
            Ok(r) => r,
            Err(e) => return ToolResult::error(e),
        };

        // Store results in registers
        let function_key = format!("{}_function", params.cache_as);
        let params_key = format!("{}_params", params.cache_as);
        let contract_key = format!("{}_contract", params.cache_as);
        let value_key = format!("{}_value", params.cache_as);

        context.registers.set(&function_key, json!(function_name.clone()), "decode_calldata");
        context.registers.set(&params_key, json!(decoded_params.clone()), "decode_calldata");

        // Store contract address and value — these are REQUIRED for preset execution
        // If the calldata came from an object register (e.g., swap_quote) and we couldn't
        // extract "to", the downstream preset will fail. Fail early here instead.
        if let Some(ref addr) = contract_address {
            context.registers.set(&contract_key, json!(addr), "decode_calldata");
        } else if params.calldata_register.is_some() {
            // We read from a register that should have had "to" — this is a hard error
            return ToolResult::error(format!(
                "Failed to extract contract address ('to' field) from register '{}'. \
                 Register '{}' will NOT be set and downstream preset execution will fail. \
                 Check that the source data contains a valid 'to' address.",
                params.calldata_register.as_deref().unwrap_or("?"),
                contract_key,
            ));
        } else if used_raw_calldata {
            // AI passed raw calldata hex instead of using calldata_register.
            // Contract address and value can ONLY be extracted from a register object.
            // This is almost always a mistake — fail loudly so the AI self-corrects.
            return ToolResult::error(format!(
                "ERROR: You passed raw 'calldata' but register '{}' was NOT set because contract address \
                 can only be extracted from a register (which contains 'to', 'data', 'value' fields). \
                 You MUST use 'calldata_register' instead of 'calldata'. \
                 For swap: use {{\"calldata_register\": \"swap_quote\", \"cache_as\": \"swap\", \"abi\": \"0x_settler\"}}",
                contract_key,
            ));
        }
        if let Some(ref val) = tx_value {
            context.registers.set(&value_key, json!(val), "decode_calldata");
        } else if params.calldata_register.is_some() {
            // value is less critical — default to "0" rather than failing
            log::warn!("[decode_calldata] No 'value' extracted from register — defaulting {} to '0'", value_key);
            context.registers.set(&value_key, json!("0"), "decode_calldata");
        }

        // Also set individual param registers for preset compatibility
        // e.g., swap_param_0, swap_param_1, etc.
        for (i, param) in decoded_params.iter().enumerate() {
            let param_key = format!("{}_param_{}", params.cache_as, i);
            context.registers.set(&param_key, param.clone(), "decode_calldata");
        }

        log::info!(
            "[decode_calldata] Decoded {}::{} -> registers {}, {}, contract={:?}, value={:?}, + {} individual params",
            params.abi, function_name, function_key, params_key, contract_address, tx_value, decoded_params.len()
        );

        // Format params for display
        let params_display: Vec<String> = decoded_params.iter()
            .map(|p| {
                let s = p.to_string();
                if s.len() > 50 {
                    format!("{}...", &s[..50])
                } else {
                    s
                }
            })
            .collect();

        // Build result message
        let mut msg = format!(
            "Decoded calldata successfully!\n\n\
            ABI: {}\n\
            Function: {}\n\
            Parameters: {:?}\n",
            params.abi,
            function_name,
            params_display,
        );

        if let Some(ref addr) = contract_address {
            msg.push_str(&format!("Contract: {}\n", addr));
        }
        if let Some(ref val) = tx_value {
            msg.push_str(&format!("Value: {}\n", val));
        }

        msg.push_str(&format!(
            "\nStored in registers:\n\
            - {} = \"{}\"\n\
            - {} (array)\n",
            function_key, function_name,
            params_key
        ));

        if contract_address.is_some() {
            msg.push_str(&format!("- {} (contract address)\n", contract_key));
        }
        if tx_value.is_some() {
            msg.push_str(&format!("- {} (tx value)\n", value_key));
        }

        for i in 0..decoded_params.len() {
            msg.push_str(&format!("- {}_param_{}\n", params.cache_as, i));
        }

        ToolResult::success(msg).with_metadata(json!({
            "abi": params.abi,
            "function": function_name,
            "params": decoded_params,
            "contract": contract_address,
            "value": tx_value,
            "function_register": function_key,
            "params_register": params_key,
            "contract_register": contract_key,
            "value_register": value_key,
        }))
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }
}
