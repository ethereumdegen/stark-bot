//! GeckoTerminal tool for interactive price chart embeds
//!
//! Searches GeckoTerminal pools and returns price data with an embeddable
//! chart marker (`[chart:URL]`) for web channels, or a plain link for
//! text-only channels (Discord, Telegram, etc.).

use crate::tools::registry::Tool;
use crate::tools::types::{
    ChannelOutputType, PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema,
    ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

const API_BASE: &str = "https://api.geckoterminal.com/api/v2";

pub struct GeckoTerminalTool {
    definition: ToolDefinition,
}

impl GeckoTerminalTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "query".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Token symbol, name, or contract address to chart (e.g. 'PEPE', '0x6982...')".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "network".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Chain hint: ethereum, base, solana, bsc, polygon, arbitrum, optimism, avalanche. Optional — omit to search all chains.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        GeckoTerminalTool {
            definition: ToolDefinition {
                name: "geckoterminal".to_string(),
                description: r#"Show an interactive GeckoTerminal price chart for a token or pool.

Searches GeckoTerminal for the best pool matching your query and displays:
- Price, 24h change, liquidity, volume, FDV
- An interactive chart embed (web UI) or link (Discord/Telegram)

EXAMPLES:
- By symbol: {"query": "PEPE"}
- By symbol on chain: {"query": "PEPE", "network": "base"}
- By address: {"query": "0x6982...", "network": "ethereum"}

SUPPORTED CHAINS: ethereum, base, solana, bsc, polygon, arbitrum, optimism, avalanche

Use this tool when users ask to "show a chart", "price chart", or want to visualize token price action."#.to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["query".to_string()],
                },
                group: ToolGroup::Finance,
            },
        }
    }
}

impl Default for GeckoTerminalTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct Params {
    query: String,
    network: Option<String>,
}

// GeckoTerminal search API response types
#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Option<Vec<PoolData>>,
}

#[derive(Debug, Deserialize)]
struct PoolData {
    attributes: Option<PoolAttributes>,
    relationships: Option<PoolRelationships>,
}

#[derive(Debug, Deserialize)]
struct PoolAttributes {
    name: Option<String>,
    address: Option<String>,
    base_token_price_usd: Option<String>,
    fdv_usd: Option<String>,
    reserve_in_usd: Option<String>,
    volume_usd: Option<VolumeUsd>,
    price_change_percentage: Option<PriceChangePercentage>,
}

#[derive(Debug, Deserialize)]
struct VolumeUsd {
    h24: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PriceChangePercentage {
    h24: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PoolRelationships {
    network: Option<NetworkRel>,
}

#[derive(Debug, Deserialize)]
struct NetworkRel {
    data: Option<NetworkData>,
}

#[derive(Debug, Deserialize)]
struct NetworkData {
    id: Option<String>,
}

fn normalize_network(chain: &str) -> &str {
    match chain.to_lowercase().as_str() {
        "ethereum" | "eth" | "mainnet" => "eth",
        "base" => "base",
        "solana" | "sol" => "solana",
        "bsc" | "binance" => "bsc",
        "polygon" | "matic" => "polygon_pos",
        "arbitrum" | "arb" => "arbitrum",
        "optimism" | "op" => "optimism",
        "avalanche" | "avax" => "avax",
        // Pass through as-is for other networks (GeckoTerminal may support them)
        _ => chain,
    }
}

fn format_number(n: f64) -> String {
    if n >= 1_000_000_000.0 {
        format!("${:.2}B", n / 1_000_000_000.0)
    } else if n >= 1_000_000.0 {
        format!("${:.2}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("${:.2}K", n / 1_000.0)
    } else {
        format!("${:.2}", n)
    }
}

fn parse_f64(s: &str) -> Option<f64> {
    s.parse::<f64>().ok()
}

fn format_pool_output(pool: &PoolData, output_type: ChannelOutputType, network_fallback: Option<&str>) -> Option<String> {
    let attrs = pool.attributes.as_ref()?;
    let name = attrs.name.as_deref().unwrap_or("Unknown Pool");
    let address = attrs.address.as_deref()?;
    let network_id = pool
        .relationships
        .as_ref()
        .and_then(|r| r.network.as_ref())
        .and_then(|n| n.data.as_ref())
        .and_then(|d| d.id.as_deref())
        .or(network_fallback)
        .unwrap_or("unknown");

    let mut lines = Vec::new();

    // Header
    lines.push(format!("**{}** on {}", name, network_id));

    // Price + 24h change
    if let Some(price_str) = &attrs.base_token_price_usd {
        let change = attrs
            .price_change_percentage
            .as_ref()
            .and_then(|p| p.h24.as_deref())
            .and_then(|s| parse_f64(s))
            .map(|c| format!(" ({:+.1}% 24h)", c))
            .unwrap_or_default();
        lines.push(format!("  Price: ${}{}", price_str, change));
    }

    // Liquidity
    if let Some(liq_str) = &attrs.reserve_in_usd {
        if let Some(liq) = parse_f64(liq_str) {
            lines.push(format!("  Liquidity: {}", format_number(liq)));
        }
    }

    // 24h Volume
    if let Some(vol) = attrs.volume_usd.as_ref().and_then(|v| v.h24.as_deref()) {
        if let Some(v) = parse_f64(vol) {
            lines.push(format!("  24h Vol: {}", format_number(v)));
        }
    }

    // FDV
    if let Some(fdv_str) = &attrs.fdv_usd {
        if let Some(fdv) = parse_f64(fdv_str) {
            lines.push(format!("  FDV: {}", format_number(fdv)));
        }
    }

    // Page link (always shown)
    let page_url = format!(
        "https://www.geckoterminal.com/{}/pools/{}",
        network_id, address
    );
    lines.push(format!("  {}", page_url));


    Some(lines.join("\n"))
}

#[async_trait]
impl Tool for GeckoTerminalTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: Params = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        if params.query.trim().is_empty() {
            return ToolResult::error("'query' is required (token symbol, name, or address)");
        }

        let client = crate::http::shared_client();

        // Build search URL
        let mut url = format!(
            "{}/search/pools?query={}",
            API_BASE,
            urlencoding::encode(params.query.trim())
        );

        // Use explicit network param, or fall back to the globally selected network
        let effective_network = params.network.as_deref()
            .or(context.selected_network.as_deref());

        if let Some(net) = effective_network {
            let normalized = normalize_network(net.trim());
            url.push_str(&format!("&network={}", urlencoding::encode(normalized)));
        }

        let resp = match client.get(&url).timeout(std::time::Duration::from_secs(15)).header("User-Agent", "StarkBot/1.0").send().await {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("GeckoTerminal request failed: {}", e)),
        };

        if !resp.status().is_success() {
            return ToolResult::error(format!("GeckoTerminal API error: {}", resp.status()));
        }

        let data: SearchResponse = match resp.json().await {
            Ok(d) => d,
            Err(e) => return ToolResult::error(format!("Failed to parse response: {}", e)),
        };

        let pools = data.data.unwrap_or_default();
        if pools.is_empty() {
            return ToolResult::error(format!(
                "No pools found for '{}'. Try a different query or specify a network.",
                params.query
            ));
        }

        // Pick the top pool (GeckoTerminal sorts by relevance/liquidity)
        let top = &pools[0];
        let network_fallback = effective_network.map(|n| normalize_network(n));

        match format_pool_output(top, context.output_type, network_fallback) {
            Some(output) => ToolResult::success(output),
            None => ToolResult::error("Pool data incomplete — try a different query"),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_network() {
        assert_eq!(normalize_network("ethereum"), "eth");
        assert_eq!(normalize_network("ETH"), "eth");
        assert_eq!(normalize_network("mainnet"), "eth");
        assert_eq!(normalize_network("base"), "base");
        assert_eq!(normalize_network("solana"), "solana");
        assert_eq!(normalize_network("sol"), "solana");
        assert_eq!(normalize_network("bsc"), "bsc");
        assert_eq!(normalize_network("binance"), "bsc");
        assert_eq!(normalize_network("polygon"), "polygon_pos");
        assert_eq!(normalize_network("matic"), "polygon_pos");
        assert_eq!(normalize_network("arbitrum"), "arbitrum");
        assert_eq!(normalize_network("arb"), "arbitrum");
        assert_eq!(normalize_network("optimism"), "optimism");
        assert_eq!(normalize_network("op"), "optimism");
        assert_eq!(normalize_network("avalanche"), "avax");
        assert_eq!(normalize_network("avax"), "avax");
        assert_eq!(normalize_network("fantom"), "fantom"); // passthrough
    }

    #[test]
    fn test_format_pool_output_rich_html() {
        let pool = make_test_pool();
        let output = format_pool_output(&pool, ChannelOutputType::RichHtml, None).unwrap();

        assert!(output.contains("**PEPE / WETH** on base"));
        assert!(output.contains("Price: $0.00001234"));
        assert!(output.contains("+15.3% 24h"));
        assert!(output.contains("Liquidity:"));
        assert!(output.contains("24h Vol:"));
        assert!(output.contains("FDV:"));
        assert!(output.contains("https://www.geckoterminal.com/base/pools/0xabc123"));
    }

    #[test]
    fn test_format_pool_output_text_only() {
        let pool = make_test_pool();
        let output = format_pool_output(&pool, ChannelOutputType::TextOnly, None).unwrap();

        assert!(output.contains("**PEPE / WETH** on base"));
        assert!(output.contains("https://www.geckoterminal.com/base/pools/0xabc123"));
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(1_500_000_000.0), "$1.50B");
        assert_eq!(format_number(5_234_567.89), "$5.23M");
        assert_eq!(format_number(1_234.56), "$1.23K");
        assert_eq!(format_number(42.5), "$42.50");
    }

    fn make_test_pool() -> PoolData {
        PoolData {
            attributes: Some(PoolAttributes {
                name: Some("PEPE / WETH".to_string()),
                address: Some("0xabc123".to_string()),
                base_token_price_usd: Some("0.00001234".to_string()),
                fdv_usd: Some("5234567.89".to_string()),
                reserve_in_usd: Some("1234567.89".to_string()),
                volume_usd: Some(VolumeUsd {
                    h24: Some("5678901.23".to_string()),
                }),
                price_change_percentage: Some(PriceChangePercentage {
                    h24: Some("15.3".to_string()),
                }),
            }),
            relationships: Some(PoolRelationships {
                network: Some(NetworkRel {
                    data: Some(NetworkData {
                        id: Some("base".to_string()),
                    }),
                }),
            }),
        }
    }
}
