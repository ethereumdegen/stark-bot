//! Set Address tool — typed address register setter
//!
//! Replaces the generic `register_set` for address registers.
//! Constrained to a specific enum of allowed register names
//! with ETH address validation and zero-address rejection.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Allowed register names for set_address
const ALLOWED_REGISTERS: &[&str] = &["send_to", "recipient_address", "safe_address"];

/// Set Address tool — typed address setter for send_to and recipient_address
pub struct SetAddressTool {
    definition: ToolDefinition,
}

impl SetAddressTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "register".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Which address register to set.".to_string(),
                default: None,
                items: None,
                enum_values: Some(
                    ALLOWED_REGISTERS.iter().map(|s| s.to_string()).collect(),
                ),
            },
        );

        properties.insert(
            "address".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Ethereum address (0x + 40 hex characters).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        SetAddressTool {
            definition: ToolDefinition {
                name: "set_address".to_string(),
                description: "Set an Ethereum address in a named register. Use this to set 'send_to' (for ETH transfers), 'recipient_address' (for ERC20 transfers), or 'safe_address' (for Safe multi-sig operations). Validates the address format and rejects the zero address.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["register".to_string(), "address".to_string()],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }

    /// Check if a string is a valid Ethereum address
    fn is_valid_eth_address(s: &str) -> bool {
        if !s.starts_with("0x") && !s.starts_with("0X") {
            return false;
        }
        if s.len() != 42 {
            return false;
        }
        s[2..].chars().all(|c| c.is_ascii_hexdigit())
    }

    /// Check if an address is the zero address
    fn is_zero_address(s: &str) -> bool {
        s.to_lowercase() == "0x0000000000000000000000000000000000000000"
    }
}

impl Default for SetAddressTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SetAddressParams {
    register: String,
    address: String,
}

#[async_trait]
impl Tool for SetAddressTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: SetAddressParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Validate register name is in the allowed set
        if !ALLOWED_REGISTERS.contains(&params.register.as_str()) {
            return ToolResult::error(format!(
                "Invalid register '{}'. Allowed registers: {}",
                params.register,
                ALLOWED_REGISTERS.join(", ")
            ));
        }

        // Validate ETH address format
        if !Self::is_valid_eth_address(&params.address) {
            return ToolResult::error(format!(
                "Invalid Ethereum address '{}'. Must be 0x followed by 40 hex characters.",
                params.address
            ));
        }

        // Reject zero address
        if Self::is_zero_address(&params.address) {
            return ToolResult::error(
                "Cannot set the zero address (0x0000...0000). \
                 Sending to the zero address will burn funds permanently. \
                 Please provide a valid recipient address.",
            );
        }

        // In Discord channels, recipient_address must be set by discord_resolve_user
        if params.register == "recipient_address" {
            if let Some(ref ch) = context.channel_type {
                if ch == "discord" {
                    return ToolResult::error(
                        "Cannot set 'recipient_address' via set_address in Discord channels. \
                         Use 'discord_resolve_user' tool instead — it automatically sets this register \
                         after verifying the recipient's identity.",
                    );
                }
            }
        }

        // Store in register
        context.set_register(&params.register, json!(&params.address), "set_address");

        log::info!(
            "[set_address] Set register '{}' = '{}'",
            params.register,
            params.address
        );

        ToolResult::success(format!(
            "Set '{}' = {}",
            params.register, params.address
        ))
        .with_metadata(json!({
            "register": params.register,
            "address": params.address
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_eth_address() {
        // Valid addresses
        assert!(SetAddressTool::is_valid_eth_address(
            "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"
        ));
        assert!(SetAddressTool::is_valid_eth_address(
            "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
        ));
        assert!(SetAddressTool::is_valid_eth_address(
            "0x0000000000000000000000000000000000000000"
        ));

        // Invalid - missing 0x prefix
        assert!(!SetAddressTool::is_valid_eth_address(
            "833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
        ));
        // Invalid - too short
        assert!(!SetAddressTool::is_valid_eth_address(
            "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA0291"
        ));
        // Invalid - too long
        assert!(!SetAddressTool::is_valid_eth_address(
            "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA029133"
        ));
        // Invalid - not hex
        assert!(!SetAddressTool::is_valid_eth_address(
            "0xGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG"
        ));
        // Invalid - token symbol
        assert!(!SetAddressTool::is_valid_eth_address("ETH"));
        assert!(!SetAddressTool::is_valid_eth_address("USDC"));
    }

    #[test]
    fn test_is_zero_address() {
        assert!(SetAddressTool::is_zero_address(
            "0x0000000000000000000000000000000000000000"
        ));
        assert!(!SetAddressTool::is_zero_address(
            "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
        ));
    }

    #[tokio::test]
    async fn test_valid_address_send_to() {
        let tool = SetAddressTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "register": "send_to",
                    "address": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
                }),
                &context,
            )
            .await;

        assert!(result.success, "Should accept valid address for send_to");
        assert!(result.content.contains("send_to"));
    }

    #[tokio::test]
    async fn test_valid_address_recipient_address() {
        let tool = SetAddressTool::new();
        let context = ToolContext::new().with_channel(1, "web".to_string());

        let result = tool
            .execute(
                json!({
                    "register": "recipient_address",
                    "address": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
                }),
                &context,
            )
            .await;

        assert!(
            result.success,
            "Should accept valid address for recipient_address in web"
        );
    }

    #[tokio::test]
    async fn test_invalid_register_name() {
        let tool = SetAddressTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "register": "token_address",
                    "address": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
                }),
                &context,
            )
            .await;

        assert!(!result.success, "Should reject invalid register name");
        assert!(result.content.contains("Invalid register"));
    }

    #[tokio::test]
    async fn test_invalid_address_too_short() {
        let tool = SetAddressTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "register": "send_to",
                    "address": "0x1234"
                }),
                &context,
            )
            .await;

        assert!(!result.success, "Should reject too-short address");
    }

    #[tokio::test]
    async fn test_invalid_address_no_prefix() {
        let tool = SetAddressTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "register": "send_to",
                    "address": "833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
                }),
                &context,
            )
            .await;

        assert!(!result.success, "Should reject address without 0x prefix");
    }

    #[tokio::test]
    async fn test_invalid_address_non_hex() {
        let tool = SetAddressTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "register": "send_to",
                    "address": "0xGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG"
                }),
                &context,
            )
            .await;

        assert!(!result.success, "Should reject non-hex address");
    }

    #[tokio::test]
    async fn test_zero_address_rejected() {
        let tool = SetAddressTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(
                json!({
                    "register": "send_to",
                    "address": "0x0000000000000000000000000000000000000000"
                }),
                &context,
            )
            .await;

        assert!(!result.success, "Should reject zero address");
        assert!(result.content.contains("zero address"));
    }

    #[tokio::test]
    async fn test_discord_blocks_recipient_address() {
        let tool = SetAddressTool::new();
        let context = ToolContext::new().with_channel(1, "discord".to_string());

        let result = tool
            .execute(
                json!({
                    "register": "recipient_address",
                    "address": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
                }),
                &context,
            )
            .await;

        assert!(
            !result.success,
            "Should block recipient_address in Discord"
        );
        assert!(result.content.contains("discord_resolve_user"));
    }

    #[tokio::test]
    async fn test_discord_allows_send_to() {
        let tool = SetAddressTool::new();
        let context = ToolContext::new().with_channel(1, "discord".to_string());

        let result = tool
            .execute(
                json!({
                    "register": "send_to",
                    "address": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
                }),
                &context,
            )
            .await;

        assert!(result.success, "Should allow send_to in Discord");
    }
}
