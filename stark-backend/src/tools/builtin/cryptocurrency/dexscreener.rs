//! DexScreener API tool for fetching token and pair information
//!
//! Provides access to DexScreener's public API for:
//! - Searching tokens by name/symbol
//! - Getting token info by address
//! - Getting pair information
//! - Getting trending/boosted tokens

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

const BASE_URL: &str = "https://api.dexscreener.com";

/// DexScreener API tool
pub struct DexScreenerTool {
    definition: ToolDefinition,
}

impl DexScreenerTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action: 'search' (find tokens), 'token' (get by address), 'pair' (get pool info), 'boosted' (paid promotions - NOT organic trending)".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "search".to_string(),
                    "token".to_string(),
                    "pair".to_string(),
                    "boosted".to_string(),
                ]),
            },
        );

        properties.insert(
            "query".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Search query for 'search' action (token name, symbol, or address)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "chain".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Chain for 'token'/'pair'/'boosted' actions: ethereum, base, solana, bsc, polygon, arbitrum, optimism, avalanche. For 'boosted', filters results to this chain only.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "address".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Token or pair contract address".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        DexScreenerTool {
            definition: ToolDefinition {
                name: "dexscreener".to_string(),
                description: r#"Get real-time DEX token data from DexScreener across all major chains.

ACTIONS:
- search: Find tokens by name/symbol/address (e.g., "PEPE", "0x6982...")
- token: Get all trading pairs for a token address (requires chain + address)
- pair: Get specific liquidity pool info (requires chain + pair_address)
- boosted: See PAID promotional tokens (⚠️ NOT organic trending! These are paid ads, often scams)

SUPPORTED CHAINS: ethereum, base, solana, bsc, polygon, arbitrum, optimism, avalanche

RETURNS: Price (USD), 24h change %, market cap, liquidity, volume, buy/sell txn counts, DexScreener URL

EXAMPLES:
- Search: {"action": "search", "query": "PEPE"}
- Token info: {"action": "token", "chain": "base", "address": "0x..."}
- Boosted (paid): {"action": "boosted", "chain": "base"}

⚠️ IMPORTANT: 'boosted' shows tokens that PAID for promotion, not actual trending tokens. For real trending, use 'search' and look at volume/liquidity metrics.

TIP: Low liquidity (<$10K) means high slippage risk. Same token name can exist on different chains."#.to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string()],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }
}

impl Default for DexScreenerTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct Params {
    action: String,
    query: Option<String>,
    chain: Option<String>,
    address: Option<String>,
}

// Minimal response types - just what we need for formatting
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairResponse {
    pairs: Option<Vec<Pair>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pair {
    chain_id: Option<String>,
    dex_id: Option<String>,
    pair_address: Option<String>,
    base_token: Option<Token>,
    quote_token: Option<Token>,
    price_usd: Option<String>,
    price_native: Option<String>,
    #[serde(default)]
    volume: Volume,
    #[serde(default)]
    price_change: PriceChange,
    #[serde(default)]
    liquidity: Liquidity,
    fdv: Option<f64>,
    market_cap: Option<f64>,
    #[serde(default)]
    txns: Txns,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Token {
    address: Option<String>,
    name: Option<String>,
    symbol: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Volume {
    h24: Option<f64>,
    h6: Option<f64>,
    h1: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PriceChange {
    h24: Option<f64>,
    h6: Option<f64>,
    h1: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct Liquidity {
    usd: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Txns {
    h24: Option<TxCount>,
}

#[derive(Debug, Deserialize)]
struct TxCount {
    buys: Option<u64>,
    sells: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Boost {
    chain_id: Option<String>,
    token_address: Option<String>,
    name: Option<String>,
    symbol: Option<String>,
    total_amount: Option<u64>,
    url: Option<String>,
}

fn format_number(n: f64) -> String {
    if n >= 1_000_000_000.0 {
        format!("{:.2}B", n / 1_000_000_000.0)
    } else if n >= 1_000_000.0 {
        format!("{:.2}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.2}K", n / 1_000.0)
    } else {
        format!("{:.2}", n)
    }
}

fn format_pair(p: &Pair) -> String {
    let base = p.base_token.as_ref();
    let quote = p.quote_token.as_ref();

    let symbol = base.and_then(|t| t.symbol.as_ref()).map(|s| s.as_str()).unwrap_or("???");
    let quote_sym = quote.and_then(|t| t.symbol.as_ref()).map(|s| s.as_str()).unwrap_or("???");
    let name = base.and_then(|t| t.name.as_ref()).map(|s| s.as_str()).unwrap_or("");
    let chain = p.chain_id.as_deref().unwrap_or("?");
    let dex = p.dex_id.as_deref().unwrap_or("?");

    let mut lines = vec![format!("**{}/{}** {} on {} ({})", symbol, quote_sym, name, chain, dex)];

    if let Some(price) = &p.price_usd {
        let change = p.price_change.h24.map(|c| format!(" ({:+.2}% 24h)", c)).unwrap_or_default();
        lines.push(format!("  Price: ${}{}", price, change));
    }

    if let Some(mc) = p.market_cap {
        lines.push(format!("  MCap: ${}", format_number(mc)));
    }

    if let Some(liq) = p.liquidity.usd {
        lines.push(format!("  Liquidity: ${}", format_number(liq)));
    }

    if let Some(vol) = p.volume.h24 {
        lines.push(format!("  24h Vol: ${}", format_number(vol)));
    }

    if let Some(txns) = &p.txns.h24 {
        let buys = txns.buys.unwrap_or(0);
        let sells = txns.sells.unwrap_or(0);
        lines.push(format!("  24h Txns: {} buys / {} sells", buys, sells));
    }

    if let Some(addr) = base.and_then(|t| t.address.as_ref()) {
        lines.push(format!("  Token: {}", addr));
    }

    if let Some(url) = &p.url {
        lines.push(format!("  {}", url));
    }

    lines.join("\n")
}

#[async_trait]
impl Tool for DexScreenerTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: Params = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let client = context.http_client();

        match params.action.as_str() {
            "search" => {
                let query = match &params.query {
                    Some(q) if !q.is_empty() => q,
                    _ => return ToolResult::error("'query' required for search"),
                };

                let url = format!("{}/latest/dex/search?q={}", BASE_URL, urlencoding::encode(query));

                let resp = match client.get(&url).timeout(std::time::Duration::from_secs(15)).header("User-Agent", "StarkBot/1.0").send().await {
                    Ok(r) => r,
                    Err(e) => return ToolResult::error(format!("Request failed: {}", e)),
                };

                if !resp.status().is_success() {
                    return ToolResult::error(format!("API error: {}", resp.status()));
                }

                let data: PairResponse = match resp.json().await {
                    Ok(d) => d,
                    Err(e) => return ToolResult::error(format!("Parse error: {}", e)),
                };

                let pairs = data.pairs.unwrap_or_default();
                if pairs.is_empty() {
                    return ToolResult::success(format!("No results for '{}'", query));
                }

                let mut out = format!("Found {} results for '{}':\n\n", pairs.len().min(10), query);
                for p in pairs.iter().take(10) {
                    out.push_str(&format_pair(p));
                    out.push_str("\n\n");
                }

                ToolResult::success(out).with_metadata(json!({"query": query, "count": pairs.len()}))
            }

            "token" => {
                let chain = match &params.chain {
                    Some(c) if !c.is_empty() => c,
                    _ => return ToolResult::error("'chain' required (ethereum, base, solana, etc.)"),
                };
                let address = match &params.address {
                    Some(a) if !a.is_empty() => a,
                    _ => return ToolResult::error("'address' required"),
                };

                let url = format!("{}/tokens/v1/{}/{}", BASE_URL, chain, address);

                let resp = match client.get(&url).timeout(std::time::Duration::from_secs(15)).header("User-Agent", "StarkBot/1.0").send().await {
                    Ok(r) => r,
                    Err(e) => return ToolResult::error(format!("Request failed: {}", e)),
                };

                if !resp.status().is_success() {
                    return ToolResult::error(format!("API error: {}", resp.status()));
                }

                let pairs: Vec<Pair> = match resp.json().await {
                    Ok(d) => d,
                    Err(e) => return ToolResult::error(format!("Parse error: {}", e)),
                };

                if pairs.is_empty() {
                    return ToolResult::success(format!("No pairs found for {} on {}", address, chain));
                }

                let mut out = format!("Token {} on {}:\n\n", address, chain);
                for p in pairs.iter().take(5) {
                    out.push_str(&format_pair(p));
                    out.push_str("\n\n");
                }

                ToolResult::success(out).with_metadata(json!({"chain": chain, "address": address}))
            }

            "pair" => {
                let chain = match &params.chain {
                    Some(c) if !c.is_empty() => c,
                    _ => return ToolResult::error("'chain' required"),
                };
                let address = match &params.address {
                    Some(a) if !a.is_empty() => a,
                    _ => return ToolResult::error("'address' required (pair/pool address)"),
                };

                let url = format!("{}/latest/dex/pairs/{}/{}", BASE_URL, chain, address);

                let resp = match client.get(&url).timeout(std::time::Duration::from_secs(15)).header("User-Agent", "StarkBot/1.0").send().await {
                    Ok(r) => r,
                    Err(e) => return ToolResult::error(format!("Request failed: {}", e)),
                };

                if !resp.status().is_success() {
                    return ToolResult::error(format!("API error: {}", resp.status()));
                }

                let data: PairResponse = match resp.json().await {
                    Ok(d) => d,
                    Err(e) => return ToolResult::error(format!("Parse error: {}", e)),
                };

                let pairs = data.pairs.unwrap_or_default();
                if pairs.is_empty() {
                    return ToolResult::success(format!("Pair {} not found on {}", address, chain));
                }

                let mut out = String::new();
                for p in &pairs {
                    out.push_str(&format_pair(p));
                    out.push_str("\n\n");
                }

                ToolResult::success(out).with_metadata(json!({"chain": chain, "pair": address}))
            }

            // Keep "trending" as alias for backwards compatibility, but prefer "boosted"
            "boosted" | "trending" => {
                let url = format!("{}/token-boosts/top/v1", BASE_URL);

                let resp = match client.get(&url).timeout(std::time::Duration::from_secs(15)).header("User-Agent", "StarkBot/1.0").send().await {
                    Ok(r) => r,
                    Err(e) => return ToolResult::error(format!("Request failed: {}", e)),
                };

                if !resp.status().is_success() {
                    return ToolResult::error(format!("API error: {}", resp.status()));
                }

                let boosts: Vec<Boost> = match resp.json().await {
                    Ok(d) => d,
                    Err(e) => return ToolResult::error(format!("Parse error: {}", e)),
                };

                if boosts.is_empty() {
                    return ToolResult::success("No boosted tokens found");
                }

                // Filter by chain if specified
                let chain_filter = params.chain.as_deref();
                let filtered: Vec<&Boost> = boosts
                    .iter()
                    .filter(|b| {
                        chain_filter.map_or(true, |c| {
                            b.chain_id.as_deref().map_or(false, |bc| bc.eq_ignore_ascii_case(c))
                        })
                    })
                    .collect();

                if filtered.is_empty() {
                    return ToolResult::success(format!(
                        "No boosted tokens found{}",
                        chain_filter.map_or(String::new(), |c| format!(" on {}", c))
                    ));
                }

                let chain_note = chain_filter
                    .map(|c| format!(" on {}", c))
                    .unwrap_or_default();

                let mut out = format!(
                    "⚠️ PAID PROMOTIONS (not organic trending!){}:\n\n",
                    chain_note
                );
                out.push_str("These tokens paid DexScreener for visibility. Exercise extreme caution.\n\n");

                for b in filtered.iter().take(15) {
                    let name = b.name.as_deref().unwrap_or("?");
                    let symbol = b.symbol.as_deref().unwrap_or("?");
                    let chain = b.chain_id.as_deref().unwrap_or("?");
                    let boosts_n = b.total_amount.unwrap_or(0);
                    out.push_str(&format!("**{} ({})** on {} - {} boosts\n", name, symbol, chain, boosts_n));
                    if let Some(addr) = &b.token_address {
                        out.push_str(&format!("  {}\n", addr));
                    }
                    if let Some(url) = &b.url {
                        out.push_str(&format!("  {}\n", url));
                    }
                    out.push('\n');
                }

                out.push_str("\n💡 For actual trending tokens, use 'search' action and evaluate by volume/liquidity.");

                ToolResult::success(out).with_metadata(json!({
                    "count": filtered.len(),
                    "chain_filter": chain_filter,
                    "is_paid_promotion": true
                }))
            }

            _ => ToolResult::error(format!("Unknown action '{}'. Use: search, token, pair, boosted", params.action)),
        }
    }
}
