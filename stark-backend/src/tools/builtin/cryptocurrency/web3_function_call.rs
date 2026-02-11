//! DEPRECATED: Web3 function call utilities have moved to `crate::web3`.
//! This module re-exports them for backward compatibility.

// Re-export everything from the new location
pub use crate::web3::*;

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

// ---- Web3FunctionCallTool (manual mode only -- no preset param) ----

/// Web3 function call tool (manual mode)
pub struct Web3FunctionCallTool {
    definition: ToolDefinition,
}

impl Web3FunctionCallTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "abi".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Name of the ABI file (without .json). Available: 'erc20', 'weth', '0x_settler'.".to_string(),
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

        Web3FunctionCallTool {
            definition: ToolDefinition {
                name: "web3_function_call".to_string(),
                description: "Call a smart contract function by specifying abi/contract/function directly. For preset operations (weth_deposit, erc20_balance, swap_execute, etc.) use web3_preset_function_call instead. Write transactions are QUEUED (not broadcast) - use broadcast_web3_tx to broadcast.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["abi".to_string(), "contract".to_string(), "function".to_string()],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }
}

impl Default for Web3FunctionCallTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct Web3FunctionCallParams {
    abi: String,
    contract: String,
    function: String,
    #[serde(default)]
    params: Vec<Value>,
    #[serde(default = "default_value")]
    value: String,
    network: Option<String>,
    #[serde(default)]
    call_only: bool,
}

fn default_value() -> String {
    "0".to_string()
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

        let network = match resolve_network(
            params.network.as_deref(),
            context.selected_network.as_deref()
        ) {
            Ok(n) => n,
            Err(e) => return ToolResult::error(e),
        };

        log::info!("[WEB3_FUNCTION_CALL] Using network: {} (from param: {:?}, context: {:?})",
            network, params.network, context.selected_network);

        let abis_dir = default_abis_dir();

        execute_resolved_call(
            &abis_dir,
            &params.abi,
            &params.contract,
            &params.function,
            &params.params,
            &params.value,
            params.call_only,
            &network,
            context,
            None,
        ).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::RegisterStore;
    use crate::tools::registry::Tool;
    use crate::web3::*;
    use ethers::abi::ParamType;
    use ethers::types::U256;
    use serde_json::json;
    use std::path::PathBuf;

    /// Helper: create a Web3FunctionCallTool
    fn make_tool() -> Web3FunctionCallTool {
        Web3FunctionCallTool::new()
    }

    /// Helper: get the repo's abis/ dir for direct ABI loading in tests
    fn test_abis_dir() -> PathBuf {
        let repo_abis = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("abis");
        if repo_abis.exists() {
            repo_abis
        } else {
            default_abis_dir()
        }
    }

    // ---- Transfer safety checks ----

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

    // ---- ABI loading / encoding ----

    #[test]
    fn test_load_erc20_abi() {
        let _tool = make_tool();
        let abi_file = load_abi(&test_abis_dir(), "erc20").expect("Should load erc20.json");
        assert_eq!(abi_file.name, "ERC20");
    }

    #[test]
    fn test_find_transfer_function() {
        let _tool = make_tool();
        let abi_file = load_abi(&test_abis_dir(), "erc20").unwrap();
        let abi = parse_abi(&abi_file).unwrap();
        let func = find_function(&abi, "transfer").expect("Should find transfer");
        assert_eq!(func.inputs.len(), 2);
    }

    #[test]
    fn test_encode_transfer_call() {
        let _tool = make_tool();
        let abi_file = load_abi(&test_abis_dir(), "erc20").unwrap();
        let abi = parse_abi(&abi_file).unwrap();
        let func = find_function(&abi, "transfer").unwrap();

        let params = vec![
            json!("0x1234567890abcdef1234567890abcdef12345678"),
            json!("1000000"),
        ];
        let encoded = encode_call(func, &params);
        assert!(encoded.is_ok(), "Should encode transfer call: {:?}", encoded.err());
        // transfer(address,uint256) selector = 0xa9059cbb
        assert_eq!(&encoded.unwrap()[..4], &[0xa9, 0x05, 0x9c, 0xbb]);
    }

    #[test]
    fn test_encode_call_wrong_param_count() {
        let _tool = make_tool();
        let abi_file = load_abi(&test_abis_dir(), "erc20").unwrap();
        let abi = parse_abi(&abi_file).unwrap();
        let func = find_function(&abi, "transfer").unwrap();

        // Only 1 param instead of 2
        let params = vec![json!("0x1234567890abcdef1234567890abcdef12345678")];
        let result = encode_call(func, &params);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expects 2 parameters"));
    }

    #[test]
    fn test_value_to_token_address() {
        let token = value_to_token(
            &json!("0x1234567890abcdef1234567890abcdef12345678"),
            &ParamType::Address,
        );
        assert!(token.is_ok());
    }

    #[test]
    fn test_value_to_token_invalid_address() {
        let token = value_to_token(&json!("not-an-address"), &ParamType::Address);
        assert!(token.is_err());
    }

    #[test]
    fn test_value_to_token_uint256() {
        let token = value_to_token(&json!("1000000"), &ParamType::Uint(256));
        assert!(token.is_ok());
        if let Ok(ethers::abi::Token::Uint(v)) = token {
            assert_eq!(v, U256::from(1_000_000u64));
        }
    }
}
