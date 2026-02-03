//! To Raw Amount tool for converting human-readable token amounts to raw blockchain units
//!
//! Converts amounts like "1.5" to raw units based on token decimals.
//! For example: 1.5 with 18 decimals = "1500000000000000000"

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// To Raw Amount tool
pub struct ToRawAmountTool {
    definition: ToolDefinition,
}

impl ToRawAmountTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "amount".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Human-readable amount (e.g., '1', '100', '0.5', '1.25'). This is what users typically mean when they say 'send 1 token'.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "decimals".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Token decimals (e.g., 18 for ETH/most tokens, 6 for USDC, 8 for cbBTC). If not provided, reads from 'token_address_decimals' register.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "decimals_register".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Register name containing the decimals value. Defaults to 'token_address_decimals'. Use 'sell_token_decimals' or 'buy_token_decimals' for swaps.".to_string(),
                default: Some(json!("token_address_decimals")),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "cache_as".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Register name to cache the raw amount. Defaults to 'raw_amount'.".to_string(),
                default: Some(json!("raw_amount")),
                items: None,
                enum_values: None,
            },
        );

        ToRawAmountTool {
            definition: ToolDefinition {
                name: "to_raw_amount".to_string(),
                description: "Convert human-readable token amount to raw blockchain units. Use AFTER token_lookup and BEFORE web3_function_call. Multiplies amount by 10^decimals. Example: amount='1', decimals=18 → '1000000000000000000'.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["amount".to_string()],
                },
                group: ToolGroup::Finance,
            },
        }
    }

    /// Convert human-readable amount to raw units
    /// Handles decimal amounts like "1.5" properly
    fn convert_to_raw(amount: &str, decimals: u8) -> Result<String, String> {
        let amount = amount.trim();

        // Handle the conversion based on whether there's a decimal point
        let (integer_part, decimal_part) = if let Some(dot_pos) = amount.find('.') {
            let int_str = &amount[..dot_pos];
            let dec_str = &amount[dot_pos + 1..];

            // Validate parts are numeric
            if !int_str.is_empty() && !int_str.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("Invalid integer part: '{}'", int_str));
            }
            if !dec_str.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("Invalid decimal part: '{}'", dec_str));
            }

            (
                if int_str.is_empty() { "0" } else { int_str },
                dec_str,
            )
        } else {
            // No decimal point - validate it's a valid integer
            if !amount.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("Invalid amount: '{}'. Must be a number.", amount));
            }
            (amount, "")
        };

        let decimals = decimals as usize;
        let decimal_len = decimal_part.len();

        if decimal_len > decimals {
            return Err(format!(
                "Amount '{}' has {} decimal places but token only has {} decimals. Maximum precision exceeded.",
                amount, decimal_len, decimals
            ));
        }

        // Build the raw amount string
        // integer_part + decimal_part + remaining zeros
        let zeros_to_add = decimals - decimal_len;
        let raw = format!(
            "{}{}{}",
            integer_part,
            decimal_part,
            "0".repeat(zeros_to_add)
        );

        // Remove leading zeros (but keep at least one digit)
        let raw = raw.trim_start_matches('0');
        if raw.is_empty() {
            Ok("0".to_string())
        } else {
            Ok(raw.to_string())
        }
    }
}

impl Default for ToRawAmountTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ToRawAmountParams {
    amount: String,
    decimals: Option<u8>,
    #[serde(default = "default_decimals_register")]
    decimals_register: String,
    #[serde(default = "default_cache_as")]
    cache_as: String,
}

fn default_decimals_register() -> String {
    "token_address_decimals".to_string()
}

fn default_cache_as() -> String {
    "raw_amount".to_string()
}

#[async_trait]
impl Tool for ToRawAmountTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ToRawAmountParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get decimals: explicit param > register > error
        let decimals: u8 = if let Some(d) = params.decimals {
            d
        } else {
            // Try to read from register
            match context.registers.get(&params.decimals_register) {
                Some(val) => {
                    // Handle both number and string representations
                    if let Some(d) = val.as_u64() {
                        d as u8
                    } else if let Some(s) = val.as_str() {
                        match s.parse::<u8>() {
                            Ok(d) => d,
                            Err(_) => {
                                return ToolResult::error(format!(
                                    "Register '{}' contains invalid decimals value: '{}'",
                                    params.decimals_register, s
                                ));
                            }
                        }
                    } else {
                        return ToolResult::error(format!(
                            "Register '{}' contains invalid decimals value: {:?}. Expected a number.",
                            params.decimals_register, val
                        ));
                    }
                }
                None => {
                    return ToolResult::error(format!(
                        "No decimals provided and register '{}' not found. Use token_lookup first to set decimals, or provide decimals parameter explicitly.",
                        params.decimals_register
                    ));
                }
            }
        };

        // Convert amount
        let raw_amount = match Self::convert_to_raw(&params.amount, decimals) {
            Ok(raw) => raw,
            Err(e) => return ToolResult::error(e),
        };

        // Store in register
        context.set_register(&params.cache_as, json!(&raw_amount), "to_raw_amount");

        log::info!(
            "[to_raw_amount] Converted {} (decimals={}) → {} (cached in '{}')",
            params.amount,
            decimals,
            raw_amount,
            params.cache_as
        );

        ToolResult::success(format!(
            "{} × 10^{} = {}\nCached in register: '{}'",
            params.amount,
            decimals,
            raw_amount,
            params.cache_as
        )).with_metadata(json!({
            "human_amount": params.amount,
            "decimals": decimals,
            "raw_amount": raw_amount,
            "cached_in_register": params.cache_as
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whole_numbers() {
        assert_eq!(ToRawAmountTool::convert_to_raw("1", 18).unwrap(), "1000000000000000000");
        assert_eq!(ToRawAmountTool::convert_to_raw("100", 18).unwrap(), "100000000000000000000");
        assert_eq!(ToRawAmountTool::convert_to_raw("1", 6).unwrap(), "1000000");
        assert_eq!(ToRawAmountTool::convert_to_raw("100", 6).unwrap(), "100000000");
        assert_eq!(ToRawAmountTool::convert_to_raw("1", 8).unwrap(), "100000000");
    }

    #[test]
    fn test_decimal_amounts() {
        assert_eq!(ToRawAmountTool::convert_to_raw("0.5", 18).unwrap(), "500000000000000000");
        assert_eq!(ToRawAmountTool::convert_to_raw("1.5", 18).unwrap(), "1500000000000000000");
        assert_eq!(ToRawAmountTool::convert_to_raw("0.001", 18).unwrap(), "1000000000000000");
        assert_eq!(ToRawAmountTool::convert_to_raw("10.5", 6).unwrap(), "10500000");
        assert_eq!(ToRawAmountTool::convert_to_raw("0.000001", 6).unwrap(), "1");
    }

    #[test]
    fn test_zero() {
        assert_eq!(ToRawAmountTool::convert_to_raw("0", 18).unwrap(), "0");
        assert_eq!(ToRawAmountTool::convert_to_raw("0.0", 18).unwrap(), "0");
    }

    #[test]
    fn test_leading_decimal() {
        assert_eq!(ToRawAmountTool::convert_to_raw(".5", 18).unwrap(), "500000000000000000");
        assert_eq!(ToRawAmountTool::convert_to_raw(".001", 6).unwrap(), "1000");
    }

    #[test]
    fn test_precision_exceeded() {
        // USDC has 6 decimals, so 0.0000001 is too precise
        assert!(ToRawAmountTool::convert_to_raw("0.0000001", 6).is_err());
        // But 0.000001 is fine (exactly 1 raw unit)
        assert_eq!(ToRawAmountTool::convert_to_raw("0.000001", 6).unwrap(), "1");
    }

    #[test]
    fn test_invalid_input() {
        assert!(ToRawAmountTool::convert_to_raw("abc", 18).is_err());
        assert!(ToRawAmountTool::convert_to_raw("1.2.3", 18).is_err());
        assert!(ToRawAmountTool::convert_to_raw("-1", 18).is_err());
    }

    #[test]
    fn test_whitespace_handling() {
        assert_eq!(ToRawAmountTool::convert_to_raw(" 1 ", 18).unwrap(), "1000000000000000000");
        assert_eq!(ToRawAmountTool::convert_to_raw("  0.5  ", 18).unwrap(), "500000000000000000");
    }
}
