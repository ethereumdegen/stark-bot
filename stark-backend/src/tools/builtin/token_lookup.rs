//! Token Lookup tool for resolving token symbols to addresses
//!
//! Provides a lookup table for known tokens on supported networks.
//! Token data is loaded from config/tokens.ron at startup.
//! This prevents hallucination of token addresses for common tokens.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// Global token storage (loaded once at startup)
static TOKENS: OnceLock<HashMap<String, HashMap<String, TokenInfo>>> = OnceLock::new();

/// Token info loaded from config
#[derive(Debug, Clone, Deserialize)]
pub struct TokenInfo {
    pub address: String,
    pub decimals: u8,
    pub name: String,
}

/// Load tokens from config directory. Panics if config file is missing or invalid.
pub fn load_tokens(config_dir: &Path) {
    let tokens_path = config_dir.join("tokens.ron");

    if !tokens_path.exists() {
        panic!("[tokens] Config file not found: {:?}", tokens_path);
    }

    let content = std::fs::read_to_string(&tokens_path)
        .unwrap_or_else(|e| panic!("[tokens] Failed to read {:?}: {}", tokens_path, e));

    let tokens: HashMap<String, HashMap<String, TokenInfo>> = ron::from_str(&content)
        .unwrap_or_else(|e| panic!("[tokens] Failed to parse {:?}: {}", tokens_path, e));

    let total: usize = tokens.values().map(|t| t.len()).sum();
    log::info!(
        "[tokens] Loaded {} tokens across {} networks from {:?}",
        total,
        tokens.len(),
        tokens_path
    );

    let _ = TOKENS.set(tokens);
}

/// Get tokens. Panics if load_tokens() was not called.
fn get_tokens() -> &'static HashMap<String, HashMap<String, TokenInfo>> {
    TOKENS.get().expect("[tokens] Token config not loaded - call load_tokens() first")
}

/// Get all token symbols with their names (for context bank scanning)
/// Returns a list of (symbol, name) pairs from all networks
pub fn get_all_token_symbols() -> Vec<(String, String)> {
    let tokens = match TOKENS.get() {
        Some(t) => t,
        None => return Vec::new(), // Return empty if tokens not loaded yet
    };

    let mut symbols = std::collections::HashSet::new();
    let mut result = Vec::new();

    for network_tokens in tokens.values() {
        for (symbol, info) in network_tokens {
            // Avoid duplicates (same symbol on different networks)
            if symbols.insert(symbol.to_uppercase()) {
                result.push((symbol.clone(), info.name.clone()));
            }
        }
    }

    result
}

/// Token Lookup tool
pub struct TokenLookupTool {
    definition: ToolDefinition,
}

impl TokenLookupTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "symbol".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Token symbol (e.g., 'ETH', 'USDC', 'WETH'). Case-insensitive.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "network".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Network: 'base' or 'mainnet'".to_string(),
                default: Some(json!("base")),
                items: None,
                enum_values: Some(vec!["base".to_string(), "mainnet".to_string()]),
            },
        );

        properties.insert(
            "cache_as".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Register name to cache the token address. Defaults to 'token_address'. Use 'sell_token' or 'buy_token' for swaps.".to_string(),
                default: Some(serde_json::json!("token_address")),
                items: None,
                enum_values: None,
            },
        );

        TokenLookupTool {
            definition: ToolDefinition {
                name: "token_lookup".to_string(),
                description: "Look up a token's contract address by symbol. Returns address, decimals, and name. Caches address in '{cache_as}', symbol in '{cache_as}_symbol', and decimals in '{cache_as}_decimals' registers.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["symbol".to_string()],
                },
                group: ToolGroup::Finance,
            },
        }
    }

    fn lookup(symbol: &str, network: &str) -> Option<TokenInfo> {
        let symbol_upper = symbol.to_uppercase();
        let tokens = get_tokens();

        log::debug!(
            "[token_lookup] Looking up '{}' (uppercase: '{}') on network '{}'. Available networks: {:?}",
            symbol, symbol_upper, network, tokens.keys().collect::<Vec<_>>()
        );

        let result = tokens
            .get(network)
            .or_else(|| tokens.get("base"))
            .and_then(|network_tokens| {
                log::debug!(
                    "[token_lookup] Network '{}' has tokens: {:?}",
                    network, network_tokens.keys().collect::<Vec<_>>()
                );
                network_tokens.get(&symbol_upper)
            })
            .cloned();

        log::info!("[token_lookup] Lookup '{}' on '{}': {:?}", symbol_upper, network, result.is_some());
        result
    }

    fn list_available(network: &str) -> Vec<String> {
        let tokens = get_tokens();

        tokens
            .get(network)
            .or_else(|| tokens.get("base"))
            .map(|network_tokens| {
                let mut symbols: Vec<String> = network_tokens.keys().cloned().collect();
                symbols.sort();
                symbols
            })
            .unwrap_or_default()
    }
}

impl Default for TokenLookupTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct TokenLookupParams {
    symbol: String,
    #[serde(default = "default_network")]
    network: String,
    #[serde(default = "default_cache_as")]
    cache_as: String,
}

fn default_network() -> String {
    "base".to_string()
}

fn default_cache_as() -> String {
    "token_address".to_string()
}

#[async_trait]
impl Tool for TokenLookupTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: TokenLookupParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        match Self::lookup(&params.symbol, &params.network) {
            Some(token) => {
                // Store address in the main register (e.g., "sell_token")
                context.set_register(&params.cache_as, json!(&token.address), "token_lookup");

                // Also store symbol in a separate register (e.g., "sell_token_symbol")
                let symbol_register = format!("{}_symbol", params.cache_as);
                context.set_register(&symbol_register, json!(params.symbol.to_uppercase()), "token_lookup");

                // Store decimals in a separate register (e.g., "sell_token_decimals")
                let decimals_register = format!("{}_decimals", params.cache_as);
                context.set_register(&decimals_register, json!(token.decimals), "token_lookup");

                log::info!(
                    "[token_lookup] Cached {} in registers: '{}'={}, '{}'={}, '{}'={}",
                    params.symbol,
                    params.cache_as,
                    token.address,
                    symbol_register,
                    params.symbol.to_uppercase(),
                    decimals_register,
                    token.decimals
                );

                ToolResult::success(format!(
                    "{} ({}) on {}\nAddress: {}\nDecimals: {}\nCached in register: '{}'",
                    token.name,
                    params.symbol.to_uppercase(),
                    params.network,
                    token.address,
                    token.decimals,
                    params.cache_as
                )).with_metadata(json!({
                    "symbol": params.symbol.to_uppercase(),
                    "address": token.address,
                    "decimals": token.decimals,
                    "name": token.name,
                    "network": params.network,
                    "cached_in_register": params.cache_as
                }))
            }
            None => {
                let available = Self::list_available(&params.network);
                ToolResult::error(format!(
                    "Token '{}' not found on {}. Available tokens: {}",
                    params.symbol,
                    params.network,
                    available.join(", ")
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    static INIT: Once = Once::new();

    fn setup() {
        INIT.call_once(|| {
            let config_dir = std::path::Path::new("../config");
            load_tokens(config_dir);
        });
    }

    #[test]
    fn test_base_token_lookup() {
        setup();
        let token = TokenLookupTool::lookup("USDC", "base").unwrap();
        assert_eq!(token.address, "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
        assert_eq!(token.decimals, 6);
    }

    #[test]
    fn test_case_insensitive() {
        setup();
        let token1 = TokenLookupTool::lookup("usdc", "base").unwrap();
        let token2 = TokenLookupTool::lookup("USDC", "base").unwrap();
        let token3 = TokenLookupTool::lookup("Usdc", "base").unwrap();

        assert_eq!(token1.address, token2.address);
        assert_eq!(token2.address, token3.address);
    }

    #[test]
    fn test_eth_special_address() {
        setup();
        let token = TokenLookupTool::lookup("ETH", "base").unwrap();
        assert_eq!(token.address, "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE");
    }

    #[test]
    fn test_unknown_token() {
        setup();
        assert!(TokenLookupTool::lookup("UNKNOWN_TOKEN_XYZ", "base").is_none());
    }
}
