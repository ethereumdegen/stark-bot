//! Register Set tool for storing values in the register store
//!
//! This tool allows skills to set values into registers for use by other tools.
//! Used in conjunction with tools that read from registers (like web3_tx with from_register).

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Register Set tool
pub struct RegisterSetTool {
    definition: ToolDefinition,
}

impl RegisterSetTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "key".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Register key name (e.g., 'sell_token', 'buy_token', 'sell_amount')".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "value".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "String value to store in the register. For simple values like addresses or amounts.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "json_value".to_string(),
            PropertySchema {
                schema_type: "object".to_string(),
                description: "JSON object to store in the register. Use this for complex data like transaction params: {to, value, data, gas}. Mutually exclusive with 'value'.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        RegisterSetTool {
            definition: ToolDefinition {
                name: "register_set".to_string(),
                description: "Store a value in a named register for use by other tools. Use this to set token addresses, amounts, and other parameters that will be read by preset operations.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["key".to_string()],  // value OR json_value required, enforced in execute
                },
                group: ToolGroup::Finance,
            },
        }
    }

    /// Validate a register key name
    fn validate_key(key: &str) -> Result<(), String> {
        if key.is_empty() {
            return Err("Register key cannot be empty".to_string());
        }
        if key.len() > 64 {
            return Err("Register key too long (max 64 characters)".to_string());
        }
        // Allow alphanumeric and underscore only
        if !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err("Register key must contain only alphanumeric characters and underscores".to_string());
        }
        Ok(())
    }

    /// Check if a string is a valid Ethereum address
    fn is_valid_eth_address(s: &str) -> bool {
        // Must start with 0x and be 42 characters total (0x + 40 hex chars)
        if !s.starts_with("0x") && !s.starts_with("0X") {
            return false;
        }
        if s.len() != 42 {
            return false;
        }
        // All characters after 0x must be hex
        s[2..].chars().all(|c| c.is_ascii_hexdigit())
    }

    /// Register keys that CANNOT be set via register_set - must use specific tools
    const BLOCKED_REGISTERS: &'static [(&'static str, &'static str)] = &[
        ("sell_token", "Use 'token_lookup' tool with cache_as: 'sell_token'"),
        ("buy_token", "Use 'token_lookup' tool with cache_as: 'buy_token'"),
        ("wallet_address", "This is an intrinsic register - automatically available from wallet config"),
        ("network", "Use 'network_name' instead - 'network' is reserved"),
        // Swap execution registers - must be set by decode_calldata tool
        ("swap_param_0", "Use 'decode_calldata' tool with abi: '0x_settler' and calldata_register: 'swap_quote' to set swap parameters"),
        ("swap_param_1", "Use 'decode_calldata' tool with abi: '0x_settler' and calldata_register: 'swap_quote' to set swap parameters"),
        ("swap_param_2", "Use 'decode_calldata' tool with abi: '0x_settler' and calldata_register: 'swap_quote' to set swap parameters"),
        ("swap_param_3", "Use 'decode_calldata' tool with abi: '0x_settler' and calldata_register: 'swap_quote' to set swap parameters"),
        ("swap_param_4", "Use 'decode_calldata' tool with abi: '0x_settler' and calldata_register: 'swap_quote' to set swap parameters"),
        ("swap_value", "Use 'decode_calldata' tool with abi: '0x_settler' and calldata_register: 'swap_quote' to set swap parameters"),
        ("swap_function", "Use 'decode_calldata' tool with abi: '0x_settler' and calldata_register: 'swap_quote' to set swap parameters"),
        ("swap_contract", "Use 'decode_calldata' tool with abi: '0x_settler' and calldata_register: 'swap_quote' to set swap parameters"),
        // ETH transfer registers - amount_raw must be set by to_raw_amount tool
        ("amount_raw", "Use 'to_raw_amount' tool with decimals: 18 for ETH. This ensures safe conversion from human amounts."),
        ("transfer_data", "Do not use transfer_data. Set 'send_to' and 'amount_raw' registers separately instead."),
        ("transfer_tx", "Do not use transfer_tx. Set 'send_to' and 'amount_raw' registers separately instead."),
    ];

    /// Register keys that MUST contain valid Ethereum addresses (if not blocked)
    const ADDRESS_REGISTERS: &'static [&'static str] = &[
        "from_address",
        "to_address",
        "token_address",
        "contract_address",
        "spender",
    ];

    /// Check if a register key is blocked from being set via register_set
    fn check_blocked(key: &str) -> Result<(), String> {
        for (blocked_key, hint) in Self::BLOCKED_REGISTERS {
            if key == *blocked_key {
                return Err(format!(
                    "Cannot set '{}' via register_set. {}",
                    key, hint
                ));
            }
        }
        Ok(())
    }

    /// Check if a register key requires an Ethereum address value
    fn requires_address(key: &str) -> bool {
        Self::ADDRESS_REGISTERS.contains(&key)
    }

    /// Validate that address registers contain valid addresses
    fn validate_address_register(key: &str, value: &Value) -> Result<(), String> {
        if !Self::requires_address(key) {
            return Ok(());
        }

        let addr = match value.as_str() {
            Some(s) => s,
            None => {
                return Err(format!(
                    "Register '{}' must contain a valid Ethereum address (string starting with 0x), not {}.",
                    key,
                    if value.is_object() { "a JSON object" } else { "this value type" }
                ));
            }
        };

        if !Self::is_valid_eth_address(addr) {
            return Err(format!(
                "Register '{}' must contain a valid Ethereum address (0x + 40 hex chars), got '{}'.",
                key, addr
            ));
        }

        Ok(())
    }
}

impl Default for RegisterSetTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct RegisterSetParams {
    key: String,
    /// String value (for simple values like addresses)
    value: Option<String>,
    /// JSON object value (for complex data like tx params)
    json_value: Option<Value>,
}

#[async_trait]
impl Tool for RegisterSetTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: RegisterSetParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Validate key
        if let Err(e) = Self::validate_key(&params.key) {
            return ToolResult::error(e);
        }

        // Check if this register is blocked (must use specific tools)
        if let Err(e) = Self::check_blocked(&params.key) {
            return ToolResult::error(e);
        }

        // Determine value to store (json_value takes precedence, then value)
        let (store_value, display_value) = match (&params.json_value, &params.value) {
            (Some(jv), _) => {
                // Store JSON object directly
                let display = serde_json::to_string(jv).unwrap_or_else(|_| "{}".to_string());
                let truncated = if display.len() > 50 {
                    format!("{}...", &display[..50])
                } else {
                    display.clone()
                };
                (jv.clone(), truncated)
            }
            (None, Some(v)) => {
                // Store string as JSON string
                let truncated = if v.len() > 50 {
                    format!("{}...", &v[..50])
                } else {
                    v.clone()
                };
                (json!(v), truncated)
            }
            (None, None) => {
                return ToolResult::error("Either 'value' or 'json_value' must be provided");
            }
        };

        // Validate address registers contain valid Ethereum addresses
        if let Err(e) = Self::validate_address_register(&params.key, &store_value) {
            return ToolResult::error(e);
        }

        // Store in register (with broadcast to UI)
        context.set_register(&params.key, store_value.clone(), "register_set");

        log::info!(
            "[register_set] Set register '{}' = '{}'",
            params.key,
            display_value
        );

        ToolResult::success(format!("Set register '{}' = {}", params.key, display_value))
            .with_metadata(json!({
                "key": params.key,
                "value": store_value
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_key() {
        assert!(RegisterSetTool::validate_key("sell_token").is_ok());
        assert!(RegisterSetTool::validate_key("buyToken").is_ok());
        assert!(RegisterSetTool::validate_key("amount123").is_ok());

        assert!(RegisterSetTool::validate_key("").is_err());
        assert!(RegisterSetTool::validate_key("key-with-dash").is_err());
        assert!(RegisterSetTool::validate_key("key.with.dots").is_err());
        assert!(RegisterSetTool::validate_key("key with spaces").is_err());
    }

    #[test]
    fn test_is_valid_eth_address() {
        // Valid addresses
        assert!(RegisterSetTool::is_valid_eth_address("0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"));
        assert!(RegisterSetTool::is_valid_eth_address("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"));
        assert!(RegisterSetTool::is_valid_eth_address("0x0000000000000000000000000000000000000000"));

        // Invalid - missing 0x prefix
        assert!(!RegisterSetTool::is_valid_eth_address("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"));
        // Invalid - too short
        assert!(!RegisterSetTool::is_valid_eth_address("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA0291"));
        // Invalid - too long
        assert!(!RegisterSetTool::is_valid_eth_address("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA029133"));
        // Invalid - not hex
        assert!(!RegisterSetTool::is_valid_eth_address("0xGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG"));
        // Invalid - token symbol
        assert!(!RegisterSetTool::is_valid_eth_address("ETH"));
        assert!(!RegisterSetTool::is_valid_eth_address("USDC"));
    }

    #[test]
    fn test_blocked_registers() {
        // sell_token, buy_token, wallet_address are blocked
        assert!(RegisterSetTool::check_blocked("sell_token").is_err());
        assert!(RegisterSetTool::check_blocked("buy_token").is_err());
        assert!(RegisterSetTool::check_blocked("wallet_address").is_err());

        // Swap execution registers are blocked (must use decode_calldata)
        assert!(RegisterSetTool::check_blocked("swap_param_0").is_err());
        assert!(RegisterSetTool::check_blocked("swap_param_1").is_err());
        assert!(RegisterSetTool::check_blocked("swap_param_2").is_err());
        assert!(RegisterSetTool::check_blocked("swap_param_3").is_err());
        assert!(RegisterSetTool::check_blocked("swap_param_4").is_err());
        assert!(RegisterSetTool::check_blocked("swap_value").is_err());
        assert!(RegisterSetTool::check_blocked("swap_function").is_err());
        assert!(RegisterSetTool::check_blocked("swap_contract").is_err());

        // ETH transfer registers are blocked (must use to_raw_amount)
        assert!(RegisterSetTool::check_blocked("amount_raw").is_err());
        assert!(RegisterSetTool::check_blocked("transfer_data").is_err());
        assert!(RegisterSetTool::check_blocked("transfer_tx").is_err());

        // Other registers are allowed
        assert!(RegisterSetTool::check_blocked("sell_amount").is_ok());
        assert!(RegisterSetTool::check_blocked("swap_quote").is_ok());
        assert!(RegisterSetTool::check_blocked("gas_price").is_ok());
        assert!(RegisterSetTool::check_blocked("send_to").is_ok());
    }

    #[test]
    fn test_validate_address_register() {
        let valid_addr = json!("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
        let invalid_symbol = json!("USDC");
        let invalid_object = json!({"to": "0x123"});

        // to_address requires valid address
        assert!(RegisterSetTool::validate_address_register("to_address", &valid_addr).is_ok());
        assert!(RegisterSetTool::validate_address_register("to_address", &invalid_symbol).is_err());
        assert!(RegisterSetTool::validate_address_register("to_address", &invalid_object).is_err());

        // contract_address requires valid address
        assert!(RegisterSetTool::validate_address_register("contract_address", &valid_addr).is_ok());
        assert!(RegisterSetTool::validate_address_register("contract_address", &invalid_symbol).is_err());

        // sell_amount does NOT require address (any value ok)
        assert!(RegisterSetTool::validate_address_register("sell_amount", &json!("1000000")).is_ok());
        assert!(RegisterSetTool::validate_address_register("sell_amount", &invalid_symbol).is_ok());
    }
}
