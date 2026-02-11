//! Web3 Preset Function Call tool — execute preset smart contract calls
//!
//! This tool has a minimal schema (preset + network + call_only) that prevents
//! the LLM from hallucinating contract addresses, ABIs, or calldata.
//! All parameters are resolved from registers set by earlier tool calls.

use crate::web3::{default_abis_dir, execute_resolved_call, resolve_network};
use crate::tools::presets::{get_web3_preset, list_web3_presets};
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Web3 preset function call tool
pub struct Web3PresetFunctionCallTool {
    definition: ToolDefinition,
}

impl Web3PresetFunctionCallTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "preset".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Preset name. Available: weth_deposit, weth_withdraw, weth_balance, erc20_balance, erc20_approve_swap, erc20_allowance_swap, erc20_transfer, swap_execute.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "network".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Network: 'base', 'mainnet', or 'polygon'. If not specified, uses the user's selected network.".to_string(),
                default: None,
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

        Web3PresetFunctionCallTool {
            definition: ToolDefinition {
                name: "web3_preset_function_call".to_string(),
                description: "Execute a preset smart contract call. All parameters are read from registers — just specify the preset name and network. Available presets: weth_deposit, weth_withdraw, weth_balance, erc20_balance, erc20_approve_swap, erc20_allowance_swap, erc20_transfer, swap_execute.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["preset".to_string()],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }
}

impl Default for Web3PresetFunctionCallTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct PresetParams {
    preset: String,
    network: Option<String>,
    #[serde(default)]
    call_only: bool,
}

#[async_trait]
impl Tool for Web3PresetFunctionCallTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: PresetParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let network = match resolve_network(
            params.network.as_deref(),
            context.selected_network.as_deref(),
        ) {
            Ok(n) => n,
            Err(e) => return ToolResult::error(e),
        };

        log::info!(
            "[WEB3_PRESET_FUNCTION_CALL] preset={}, network={} (from param: {:?}, context: {:?})",
            params.preset, network, params.network, context.selected_network
        );

        // Resolve preset
        let preset = match get_web3_preset(&params.preset) {
            Some(p) => p,
            None => {
                let available = list_web3_presets().join(", ");
                return ToolResult::error(format!(
                    "Unknown preset '{}'. Available: {}",
                    params.preset, available
                ));
            }
        };

        // Defense-in-depth: In Discord channels, erc20_transfer requires
        // recipient_address to have been set by discord_resolve_user (not set_address
        // or any other tool). This prevents sending tokens to unverified addresses
        // when discord_resolve_user fails but the AI continues the flow.
        if params.preset == "erc20_transfer" {
            if let Some(ref ch) = context.channel_type {
                if ch == "discord" {
                    match context.registers.get_entry("recipient_address") {
                        Some(entry) if entry.source_tool == "discord_resolve_user" => {
                            // Valid — address was verified via Discord user resolution
                        }
                        Some(entry) => {
                            return ToolResult::error(format!(
                                "SAFETY BLOCK: In Discord channels, 'recipient_address' must be set by \
                                 'discord_resolve_user' tool (source was '{}'). This prevents sending \
                                 tokens to unverified addresses. Resolve the Discord user first.",
                                entry.source_tool
                            ));
                        }
                        None => {
                            return ToolResult::error(
                                "SAFETY BLOCK: 'recipient_address' register is not set. In Discord \
                                 channels, you must use 'discord_resolve_user' first to resolve the \
                                 recipient's mention to a verified wallet address.",
                            );
                        }
                    }
                }
            }
        }

        // Get contract address — either from register or hardcoded per network
        let contract = if let Some(ref contract_reg) = preset.contract_register {
            match context.registers.get(contract_reg) {
                Some(v) => match v.as_str() {
                    Some(s) => s.to_string(),
                    None => v.to_string().trim_matches('"').to_string(),
                },
                None => {
                    return ToolResult::error(format!(
                        "Preset '{}' requires register '{}' for contract address but it's not set",
                        params.preset, contract_reg
                    ));
                }
            }
        } else {
            match preset.contracts.get(network.as_ref()) {
                Some(c) => c.clone(),
                None => {
                    return ToolResult::error(format!(
                        "Preset '{}' has no contract for network '{}'",
                        params.preset, network
                    ));
                }
            }
        };

        // Read params from registers
        let mut resolved_params = Vec::new();
        for reg_key in &preset.params_registers {
            match context.registers.get(reg_key) {
                Some(v) => {
                    let param_str = match v.as_str() {
                        Some(s) => s.to_string(),
                        None => v.to_string().trim_matches('"').to_string(),
                    };
                    resolved_params.push(json!(param_str));
                }
                None => {
                    return ToolResult::error(format!(
                        "Preset '{}' requires register '{}' but it's not set",
                        params.preset, reg_key
                    ));
                }
            }
        }

        // Append static params (not from registers)
        for static_val in &preset.static_params {
            resolved_params.push(json!(static_val));
        }

        // Read value from register if specified
        let value = if let Some(ref val_reg) = preset.value_register {
            match context.registers.get(val_reg) {
                Some(v) => match v.as_str() {
                    Some(s) => s.to_string(),
                    None => v.to_string().trim_matches('"').to_string(),
                },
                None => {
                    return ToolResult::error(format!(
                        "Preset '{}' requires register '{}' but it's not set",
                        params.preset, val_reg
                    ));
                }
            }
        } else {
            "0".to_string()
        };

        log::info!(
            "[web3_preset_function_call] Using preset '{}': {}::{}",
            params.preset, preset.abi, preset.function
        );

        let abis_dir = default_abis_dir();

        execute_resolved_call(
            &abis_dir,
            &preset.abi,
            &contract,
            &preset.function,
            &resolved_params,
            &value,
            params.call_only,
            &network,
            context,
            Some(&params.preset),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::types::ToolContext;

    #[tokio::test]
    async fn test_erc20_transfer_discord_blocks_wrong_source() {
        let tool = Web3PresetFunctionCallTool::new();
        let context = ToolContext::new()
            .with_channel(1, "discord".to_string())
            .with_selected_network(Some("base".to_string()));

        // Set recipient_address via set_address (wrong source for Discord)
        context.set_register(
            "recipient_address",
            json!("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
            "set_address",
        );

        let result = tool
            .execute(json!({"preset": "erc20_transfer"}), &context)
            .await;

        assert!(!result.success);
        assert!(result.content.contains("SAFETY BLOCK"));
        assert!(result.content.contains("set_address"));
    }

    #[tokio::test]
    async fn test_erc20_transfer_discord_blocks_missing_register() {
        let tool = Web3PresetFunctionCallTool::new();
        let context = ToolContext::new()
            .with_channel(1, "discord".to_string())
            .with_selected_network(Some("base".to_string()));

        // Don't set recipient_address at all
        let result = tool
            .execute(json!({"preset": "erc20_transfer"}), &context)
            .await;

        assert!(!result.success);
        assert!(result.content.contains("SAFETY BLOCK"));
        assert!(result.content.contains("not set"));
    }

    #[tokio::test]
    async fn test_erc20_transfer_discord_allows_correct_source() {
        let tool = Web3PresetFunctionCallTool::new();
        let context = ToolContext::new()
            .with_channel(1, "discord".to_string())
            .with_selected_network(Some("base".to_string()));

        // Set recipient_address via discord_resolve_user (correct source)
        context.set_register(
            "recipient_address",
            json!("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
            "discord_resolve_user",
        );
        // Set other required registers so it passes the safety check
        // (it will fail later due to missing token_address, but NOT with SAFETY BLOCK)
        let result = tool
            .execute(json!({"preset": "erc20_transfer"}), &context)
            .await;

        // Should NOT contain SAFETY BLOCK — it should fail for a different reason
        // (missing token_address register or ABI file)
        assert!(
            !result.content.contains("SAFETY BLOCK"),
            "Should pass safety check; got: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn test_erc20_transfer_web_skips_source_check() {
        let tool = Web3PresetFunctionCallTool::new();
        let context = ToolContext::new()
            .with_channel(1, "web".to_string())
            .with_selected_network(Some("base".to_string()));

        // Set recipient_address via set_address — should be fine for web
        context.set_register(
            "recipient_address",
            json!("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
            "set_address",
        );

        let result = tool
            .execute(json!({"preset": "erc20_transfer"}), &context)
            .await;

        // Should NOT contain SAFETY BLOCK — web channels are not restricted
        assert!(
            !result.content.contains("SAFETY BLOCK"),
            "Web channels should skip source check; got: {}",
            result.content
        );
    }
}
