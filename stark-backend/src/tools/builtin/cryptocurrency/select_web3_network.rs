//! Select Web3 Network tool
//!
//! Allows the Finance agent to select the active blockchain network for subsequent
//! web3 operations. This is especially important for tools like Polymarket (Polygon),
//! token operations on specific chains, etc.
//!
//! The selected network is stored in the `network_name` register and will be used
//! by default for subsequent web3 calls unless explicitly overridden.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Select Web3 Network tool - sets the active network for Finance operations
pub struct SelectWeb3NetworkTool {
    definition: ToolDefinition,
}

impl SelectWeb3NetworkTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "network".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The blockchain network to select. Common options:\n\
                    • 'mainnet' or 'ethereum' - Ethereum mainnet (chain ID 1)\n\
                    • 'base' - Base L2 (chain ID 8453)\n\
                    • 'polygon' - Polygon PoS (chain ID 137)\n\
                    • 'arbitrum' - Arbitrum One (chain ID 42161)\n\
                    • 'optimism' - Optimism L2 (chain ID 10)\n\
                    • 'bsc' - BNB Smart Chain (chain ID 56)"
                    .to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "mainnet".to_string(),
                    "ethereum".to_string(),
                    "base".to_string(),
                    "polygon".to_string(),
                    "arbitrum".to_string(),
                    "optimism".to_string(),
                    "bsc".to_string(),
                ]),
            },
        );

        SelectWeb3NetworkTool {
            definition: ToolDefinition {
                name: "select_web3_network".to_string(),
                description: "Select the active blockchain network for web3 operations.\n\n\
                    Call this tool BEFORE performing network-specific operations:\n\
                    • Polymarket trading requires 'polygon'\n\
                    • Starkbot token operations require 'base'\n\
                    • Most DeFi on Ethereum requires 'mainnet'\n\n\
                    The selected network is stored in the 'network_name' register and used \
                    by subsequent web3 calls. Call this whenever:\n\
                    • A skill instructs you to select a specific network\n\
                    • The user mentions a specific chain (Base, Polygon, mainnet)\n\
                    • Working with tokens that exist on a specific network"
                    .to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["network".to_string()],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }

    /// Get network info (name, chain_id) for a given network identifier
    fn get_network_info(network: &str) -> Option<(&'static str, u64)> {
        match network.to_lowercase().as_str() {
            "mainnet" | "ethereum" | "eth" => Some(("Ethereum Mainnet", 1)),
            "base" | "base mainnet" => Some(("Base", 8453)),
            "polygon" | "matic" | "polygon pos" => Some(("Polygon", 137)),
            "arbitrum" | "arbitrum one" | "arb" => Some(("Arbitrum One", 42161)),
            "optimism" | "op" | "op mainnet" => Some(("Optimism", 10)),
            "bsc" | "bnb" | "binance" | "binance smart chain" => Some(("BNB Smart Chain", 56)),
            "avalanche" | "avax" => Some(("Avalanche C-Chain", 43114)),
            "fantom" | "ftm" => Some(("Fantom", 250)),
            "gnosis" | "xdai" => Some(("Gnosis", 100)),
            "zksync" | "zksync era" => Some(("zkSync Era", 324)),
            "linea" => Some(("Linea", 59144)),
            "scroll" => Some(("Scroll", 534352)),
            _ => None,
        }
    }

    /// Canonicalize network name to a standard identifier
    fn canonicalize_network(network: &str) -> &'static str {
        match network.to_lowercase().as_str() {
            "mainnet" | "ethereum" | "eth" => "mainnet",
            "base" | "base mainnet" => "base",
            "polygon" | "matic" | "polygon pos" => "polygon",
            "arbitrum" | "arbitrum one" | "arb" => "arbitrum",
            "optimism" | "op" | "op mainnet" => "optimism",
            "bsc" | "bnb" | "binance" | "binance smart chain" => "bsc",
            "avalanche" | "avax" => "avalanche",
            "fantom" | "ftm" => "fantom",
            "gnosis" | "xdai" => "gnosis",
            "zksync" | "zksync era" => "zksync",
            "linea" => "linea",
            "scroll" => "scroll",
            _ => "unknown",
        }
    }
}

impl Default for SelectWeb3NetworkTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SelectWeb3NetworkParams {
    network: String,
}

#[async_trait]
impl Tool for SelectWeb3NetworkTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: SelectWeb3NetworkParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Validate and get network info
        let (display_name, chain_id) = match Self::get_network_info(&params.network) {
            Some(info) => info,
            None => {
                return ToolResult::error(format!(
                    "Unknown network '{}'. Valid options: mainnet, base, polygon, arbitrum, optimism, bsc, avalanche, fantom",
                    params.network
                ))
            }
        };

        // Get canonical network name
        let canonical_name = Self::canonicalize_network(&params.network);

        // Store in register for use by other tools
        context.set_register("network_name", json!(canonical_name), "select_web3_network");

        log::info!(
            "[select_web3_network] Selected network: {} ({}, chain_id: {})",
            canonical_name,
            display_name,
            chain_id
        );

        ToolResult::success(format!(
            "Selected network: {} (chain ID: {})\n\n\
             The 'network_name' register is now set to '{}'. \
             Subsequent web3 calls will use this network by default.",
            display_name, chain_id, canonical_name
        ))
        .with_metadata(json!({
            "network": canonical_name,
            "display_name": display_name,
            "chain_id": chain_id,
            "register_source": "select_web3_network"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_network_info() {
        // Mainnet aliases
        assert_eq!(
            SelectWeb3NetworkTool::get_network_info("mainnet"),
            Some(("Ethereum Mainnet", 1))
        );
        assert_eq!(
            SelectWeb3NetworkTool::get_network_info("ethereum"),
            Some(("Ethereum Mainnet", 1))
        );
        assert_eq!(
            SelectWeb3NetworkTool::get_network_info("eth"),
            Some(("Ethereum Mainnet", 1))
        );

        // Base
        assert_eq!(
            SelectWeb3NetworkTool::get_network_info("base"),
            Some(("Base", 8453))
        );

        // Polygon aliases
        assert_eq!(
            SelectWeb3NetworkTool::get_network_info("polygon"),
            Some(("Polygon", 137))
        );
        assert_eq!(
            SelectWeb3NetworkTool::get_network_info("matic"),
            Some(("Polygon", 137))
        );

        // Case insensitive
        assert_eq!(
            SelectWeb3NetworkTool::get_network_info("POLYGON"),
            Some(("Polygon", 137))
        );
        assert_eq!(
            SelectWeb3NetworkTool::get_network_info("Base"),
            Some(("Base", 8453))
        );

        // Unknown
        assert_eq!(SelectWeb3NetworkTool::get_network_info("unknown_network"), None);
    }

    #[test]
    fn test_canonicalize_network() {
        assert_eq!(SelectWeb3NetworkTool::canonicalize_network("mainnet"), "mainnet");
        assert_eq!(SelectWeb3NetworkTool::canonicalize_network("ethereum"), "mainnet");
        assert_eq!(SelectWeb3NetworkTool::canonicalize_network("eth"), "mainnet");
        assert_eq!(SelectWeb3NetworkTool::canonicalize_network("base"), "base");
        assert_eq!(SelectWeb3NetworkTool::canonicalize_network("polygon"), "polygon");
        assert_eq!(SelectWeb3NetworkTool::canonicalize_network("matic"), "polygon");
        assert_eq!(SelectWeb3NetworkTool::canonicalize_network("POLYGON"), "polygon");
    }

    #[tokio::test]
    async fn test_select_valid_network() {
        let tool = SelectWeb3NetworkTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "network": "polygon" }), &context)
            .await;

        assert!(result.success);
        assert!(result.content.contains("Polygon"));
        assert!(result.content.contains("137"));
    }

    #[tokio::test]
    async fn test_select_invalid_network() {
        let tool = SelectWeb3NetworkTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "network": "invalid_chain" }), &context)
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Unknown network"));
    }
}
