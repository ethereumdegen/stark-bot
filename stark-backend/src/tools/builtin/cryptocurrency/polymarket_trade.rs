//! Polymarket CLOB Trading Tool
//!
//! Enables trading on Polymarket prediction markets using the polymarket-client-sdk.
//! Uses the burner wallet private key for signing EIP-712 orders.
//!
//! ## Discovery Actions (no auth required)
//! - `search_markets`: Search markets by keyword
//! - `trending_markets`: Get popular/high-volume markets
//! - `get_market`: Get market details by slug
//! - `get_price`: Get current price/orderbook for a token_id
//!
//! ## Trading Actions (requires wallet)
//! - `place_order`: Place a limit order on a market
//! - `cancel_order`: Cancel a specific order by ID
//! - `cancel_all`: Cancel all open orders
//! - `get_orders`: List open orders
//! - `get_positions`: Get current positions and balances
//! - `get_balance`: Get USDC balance and allowances on Polygon

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

// Polymarket SDK imports - use SDK's re-exports
use polymarket_client_sdk::auth::state::Authenticated;
use polymarket_client_sdk::auth::{LocalSigner, Normal, Signer};
use polymarket_client_sdk::clob::types::request::OrdersRequest;
use polymarket_client_sdk::clob::types::{OrderType, Side};
use polymarket_client_sdk::clob::{Client, Config as ClobConfig};
use polymarket_client_sdk::types::{Decimal, U256};
use polymarket_client_sdk::POLYGON;

/// Type alias for authenticated CLOB client
type AuthenticatedClient = Client<Authenticated<Normal>>;

/// Polymarket CLOB API base URL
const CLOB_API_URL: &str = "https://clob.polymarket.com";

/// Polymarket trading tool
pub struct PolymarketTradeTool {
    definition: ToolDefinition,
    /// Cached authenticated client (lazily initialized)
    client_cache: Arc<Mutex<Option<CachedClient>>>,
}

/// Cached authenticated client
/// Note: We don't cache the signer since creating it is cheap
struct CachedClient {
    client: AuthenticatedClient,
}

impl PolymarketTradeTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action: search_markets, trending_markets, get_market, get_price (discovery) | place_order, cancel_order, cancel_all, get_orders, get_positions, get_balance (trading)".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    // Discovery actions
                    "search_markets".to_string(),
                    "trending_markets".to_string(),
                    "get_market".to_string(),
                    "get_price".to_string(),
                    // Trading actions
                    "place_order".to_string(),
                    "cancel_order".to_string(),
                    "cancel_all".to_string(),
                    "get_orders".to_string(),
                    "get_positions".to_string(),
                    "get_balance".to_string(),
                ]),
            },
        );

        // Discovery parameters
        properties.insert(
            "query".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Search query for finding markets (e.g., 'bitcoin', 'election'). Used with search_markets.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "slug".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Market slug (URL identifier) for get_market action (e.g., 'will-bitcoin-hit-100k-in-2025').".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "tag".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Filter by category tag: politics, crypto, sports, finance, science, entertainment, world.".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "politics".to_string(),
                    "crypto".to_string(),
                    "sports".to_string(),
                    "finance".to_string(),
                    "science".to_string(),
                    "entertainment".to_string(),
                    "world".to_string(),
                ]),
            },
        );

        properties.insert(
            "limit".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Max results for search/trending (default: 10, max: 20).".to_string(),
                default: Some(json!(10)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "offset".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Number of results to skip for pagination (default: 0).".to_string(),
                default: Some(json!(0)),
                items: None,
                enum_values: None,
            },
        );

        // Trading parameters
        properties.insert(
            "token_id".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Token ID (condition_id) of the market outcome. Required for place_order and get_price.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "side".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Order side: 'buy' or 'sell'. Required for place_order.".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec!["buy".to_string(), "sell".to_string()]),
            },
        );

        properties.insert(
            "price".to_string(),
            PropertySchema {
                schema_type: "number".to_string(),
                description: "Limit price 0.01-0.99 (auto-rounded to 3 decimals). 0.65 = 65% probability.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "size".to_string(),
            PropertySchema {
                schema_type: "number".to_string(),
                description: "Number of shares (e.g., 100 shares @ $0.65 = $65 cost, $100 payout if correct).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "order_type".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Order type: GTC (good till cancelled), FOK (fill or kill). Default: GTC.".to_string(),
                default: Some(json!("GTC")),
                items: None,
                enum_values: Some(vec!["GTC".to_string(), "FOK".to_string()]),
            },
        );

        properties.insert(
            "order_id".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Order ID for cancellation. Required for cancel_order.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        PolymarketTradeTool {
            definition: ToolDefinition {
                name: "polymarket_trade".to_string(),
                description: "Explore and trade on Polymarket prediction markets. Discovery: search_markets, trending_markets, get_market, get_price. Trading: place_order, cancel_order, get_orders, get_positions, get_balance. Trading requires BURNER_WALLET_BOT_PRIVATE_KEY with USDC on Polygon.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string()],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
            client_cache: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the private key from environment
    fn get_private_key() -> Result<String, String> {
        crate::config::burner_wallet_private_key()
            .ok_or_else(|| "BURNER_WALLET_BOT_PRIVATE_KEY not set. Configure this env var to trade on Polymarket.".to_string())
    }

    /// Get wallet address from private key (for trading operations that use the local key)
    fn get_wallet_address() -> Result<String, String> {
        let pk = Self::get_private_key()?;
        let pk_clean = pk.strip_prefix("0x").unwrap_or(&pk);

        let wallet: ethers::signers::LocalWallet = pk_clean
            .parse()
            .map_err(|e| format!("Invalid private key: {}", e))?;

        use ethers::signers::Signer as EthersSigner;
        Ok(format!("{:?}", wallet.address()))
    }

    /// Get wallet address — prefers wallet provider (correct in Flash/Privy mode)
    fn get_wallet_address_from_context(context: &ToolContext) -> Result<String, String> {
        if let Some(ref wp) = context.wallet_provider {
            return Ok(wp.get_address());
        }
        Self::get_wallet_address()
    }

    /// Get or create authenticated CLOB client
    async fn get_authenticated_client(&self) -> Result<AuthenticatedClient, String> {
        // Check cache first
        {
            let cache = self.client_cache.lock().await;
            if let Some(cached) = cache.as_ref() {
                return Ok(cached.client.clone());
            }
        }

        // Create new authenticated client
        let pk = Self::get_private_key()?;
        let pk_clean = pk.strip_prefix("0x").unwrap_or(&pk);

        let signer = LocalSigner::from_str(pk_clean)
            .map(|s| s.with_chain_id(Some(POLYGON)))
            .map_err(|e| format!("Invalid private key: {}", e))?;

        let config = ClobConfig::builder()
            .use_server_time(true)
            .build();

        let client = Client::new(CLOB_API_URL, config)
            .map_err(|e| format!("Failed to create CLOB client: {}", e))?
            .authentication_builder(&signer)
            .authenticate()
            .await
            .map_err(|e| format!("Failed to authenticate with CLOB: {}", e))?;

        // Cache for future use
        {
            let mut cache = self.client_cache.lock().await;
            *cache = Some(CachedClient { client: client.clone() });
        }

        Ok(client)
    }

    /// Create a fresh signer for signing operations
    fn create_signer_for_signing() -> Result<impl Signer + Clone, String> {
        let pk = Self::get_private_key()?;
        let pk_clean = pk.strip_prefix("0x").unwrap_or(&pk);

        LocalSigner::from_str(pk_clean)
            .map(|s| s.with_chain_id(Some(POLYGON)))
            .map_err(|e| format!("Invalid private key: {}", e))
    }

    /// Place a limit order on Polymarket
    async fn place_order(&self, params: &PolymarketParams) -> ToolResult {
        // Validate required parameters
        let token_id_str = match &params.token_id {
            Some(t) => t,
            None => return ToolResult::error("token_id is required for place_order"),
        };

        let side_str = match &params.side {
            Some(s) => s.to_lowercase(),
            None => return ToolResult::error("side is required for place_order (buy or sell)"),
        };

        // Price must be between 0.001 and 0.999, rounded to 3 decimal places (tick size 0.001)
        let price = match params.price {
            Some(p) if p > 0.0 && p < 1.0 => {
                // Round to 3 decimal places (Polymarket tick size)
                (p * 1000.0).round() / 1000.0
            },
            Some(p) => return ToolResult::error(format!(
                "price must be between 0.01 and 0.99, got {}. Price represents probability (0.65 = 65%)",
                p
            )),
            None => return ToolResult::error("price is required for place_order"),
        };

        // Size is in shares (whole numbers recommended, 2 decimal places max)
        let size = match params.size {
            Some(s) if s > 0.0 => {
                // Round to 2 decimal places
                (s * 100.0).round() / 100.0
            },
            Some(s) => return ToolResult::error(format!("size must be positive, got {}", s)),
            None => return ToolResult::error("size is required for place_order"),
        };

        let order_type_str = params.order_type.clone().unwrap_or_else(|| "GTC".to_string());

        // Parse token_id to U256
        let token_id = match U256::from_str(token_id_str) {
            Ok(t) => t,
            Err(e) => return ToolResult::error(format!("Invalid token_id: {}", e)),
        };

        // Parse side
        let side = match side_str.as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            _ => return ToolResult::error(format!("Invalid side: {}. Use 'buy' or 'sell'", side_str)),
        };

        // Parse order type
        let order_type = match order_type_str.to_uppercase().as_str() {
            "GTC" => OrderType::GTC,
            "FOK" => OrderType::FOK,
            "GTD" => OrderType::GTD,
            _ => return ToolResult::error(format!("Invalid order_type: {}. Use 'GTC', 'FOK', or 'GTD'", order_type_str)),
        };

        // Convert price and size to Decimal
        let price_decimal = match Decimal::try_from(price) {
            Ok(d) => d,
            Err(e) => return ToolResult::error(format!("Invalid price decimal: {}", e)),
        };

        let size_decimal = match Decimal::try_from(size) {
            Ok(d) => d,
            Err(e) => return ToolResult::error(format!("Invalid size decimal: {}", e)),
        };

        // Get authenticated client and signer
        let client = match self.get_authenticated_client().await {
            Ok(c) => c,
            Err(e) => return ToolResult::error(e),
        };

        let signer = match Self::create_signer_for_signing() {
            Ok(s) => s,
            Err(e) => return ToolResult::error(e),
        };

        // Build the limit order
        let order = match client
            .limit_order()
            .token_id(token_id)
            .price(price_decimal)
            .size(size_decimal)
            .side(side)
            .order_type(order_type)
            .build()
            .await
        {
            Ok(o) => o,
            Err(e) => return ToolResult::error(format!("Failed to build order: {}", e)),
        };

        // Sign the order
        let signed_order = match client.sign(&signer, order).await {
            Ok(s) => s,
            Err(e) => return ToolResult::error(format!("Failed to sign order: {}", e)),
        };

        // Submit the order
        let wallet_address = Self::get_wallet_address().unwrap_or_else(|_| "unknown".to_string());
        match client.post_order(signed_order).await {
            Ok(response) => {
                let usdc_cost = size * price;
                let result = json!({
                    "status": "success",
                    "order_id": response.order_id,
                    "success": response.success,
                    "details": {
                        "token_id": token_id_str,
                        "side": side_str,
                        "price": price,
                        "size": size,
                        "order_type": order_type_str,
                        "usdc_cost": format!("{:.2}", usdc_cost),
                        "potential_payout": format!("{:.2}", size),
                    },
                    "wallet": wallet_address,
                    "network": "polygon"
                });
                ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
            }
            Err(e) => ToolResult::error(format!("Failed to submit order: {}", e))
        }
    }

    /// Cancel a specific order
    async fn cancel_order(&self, params: &PolymarketParams) -> ToolResult {
        let order_id = match &params.order_id {
            Some(id) => id,
            None => return ToolResult::error("order_id is required for cancel_order"),
        };

        let client = match self.get_authenticated_client().await {
            Ok(c) => c,
            Err(e) => return ToolResult::error(e),
        };

        match client.cancel_order(order_id).await {
            Ok(response) => {
                let result = json!({
                    "status": "success",
                    "order_id": order_id,
                    "cancelled": response.canceled,
                    "not_cancelled": response.not_canceled,
                });
                ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
            }
            Err(e) => ToolResult::error(format!("Failed to cancel order: {}", e))
        }
    }

    /// Cancel all open orders
    async fn cancel_all(&self) -> ToolResult {
        let client = match self.get_authenticated_client().await {
            Ok(c) => c,
            Err(e) => return ToolResult::error(e),
        };

        match client.cancel_all_orders().await {
            Ok(response) => {
                let result = json!({
                    "status": "success",
                    "cancelled": response.canceled,
                    "not_cancelled": response.not_canceled,
                });
                ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
            }
            Err(e) => ToolResult::error(format!("Failed to cancel all orders: {}", e))
        }
    }

    /// Get open orders
    async fn get_orders(&self) -> ToolResult {
        let client = match self.get_authenticated_client().await {
            Ok(c) => c,
            Err(e) => return ToolResult::error(e),
        };

        let wallet_address = Self::get_wallet_address().unwrap_or_else(|_| "unknown".to_string());

        let request = OrdersRequest::default();
        match client.orders(&request, None).await {
            Ok(orders) => {
                let orders_json: Vec<Value> = orders.data.iter().map(|o| {
                    json!({
                        "order_id": o.id,
                        "status": format!("{:?}", o.status),
                        "token_id": o.asset_id.to_string(),
                        "side": format!("{:?}", o.side),
                        "original_size": o.original_size.to_string(),
                        "size_matched": o.size_matched.to_string(),
                        "price": o.price.to_string(),
                        "outcome": o.outcome,
                        "created_at": o.created_at,
                    })
                }).collect();

                let result = json!({
                    "status": "success",
                    "count": orders.data.len(),
                    "orders": orders_json,
                    "wallet": wallet_address,
                });
                ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
            }
            Err(e) => ToolResult::error(format!("Failed to fetch orders: {}", e))
        }
    }

    /// Get current positions from Data API
    async fn get_positions(&self) -> ToolResult {
        self.get_positions_with_context(&ToolContext::new()).await
    }

    async fn get_positions_with_context(&self, context: &ToolContext) -> ToolResult {
        let wallet_address = match Self::get_wallet_address_from_context(context) {
            Ok(addr) => addr,
            Err(e) => return ToolResult::error(e),
        };

        // Fetch positions from Data API
        let http_client = context.http_client();
        let url = format!("https://data-api.polymarket.com/positions?user={}", wallet_address);

        match http_client.get(&url).send().await {
            Ok(response) => {
                match response.json::<Value>().await {
                    Ok(positions) => {
                        let result = json!({
                            "status": "success",
                            "wallet": wallet_address,
                            "positions": positions,
                        });
                        ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
                    }
                    Err(e) => ToolResult::error(format!("Failed to parse positions: {}", e))
                }
            }
            Err(e) => ToolResult::error(format!("Failed to fetch positions: {}", e))
        }
    }

    /// Get balance and allowance info
    async fn get_balance(&self) -> ToolResult {
        self.get_balance_with_context(&ToolContext::new()).await
    }

    async fn get_balance_with_context(&self, context: &ToolContext) -> ToolResult {
        let client = match self.get_authenticated_client().await {
            Ok(c) => c,
            Err(e) => return ToolResult::error(e),
        };

        let wallet_address = Self::get_wallet_address_from_context(context).unwrap_or_else(|_| "unknown".to_string());

        use polymarket_client_sdk::clob::types::request::BalanceAllowanceRequest;

        match client.balance_allowance(BalanceAllowanceRequest::default()).await {
            Ok(balance_resp) => {
                // Convert allowances HashMap to a JSON-friendly format
                let allowances: serde_json::Map<String, Value> = balance_resp.allowances
                    .iter()
                    .map(|(addr, val)| (format!("{:?}", addr), json!(val)))
                    .collect();

                let result = json!({
                    "status": "success",
                    "wallet": wallet_address,
                    "network": "polygon",
                    "balance": {
                        "usdc": balance_resp.balance.to_string(),
                        "allowances": allowances,
                    },
                    "note": "Balance in USDC (6 decimals). Divide by 1000000 for human-readable amount."
                });
                ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
            }
            Err(e) => ToolResult::error(format!("Failed to fetch balance: {}", e))
        }
    }

    // ==================== DISCOVERY METHODS ====================

    /// Search markets by keyword (lightweight summaries only)
    async fn search_markets(&self, params: &PolymarketParams, context: &ToolContext) -> ToolResult {
        let query = params.query.as_deref().unwrap_or("");
        let limit = params.limit.unwrap_or(10).min(20);
        let offset = params.offset.unwrap_or(0);
        let tag = params.tag.as_deref();

        let http_client = context.http_client();

        // Build URL with query params
        let mut url = format!(
            "https://gamma-api.polymarket.com/events?active=true&closed=false&limit={}&offset={}",
            limit, offset
        );

        if !query.is_empty() {
            url.push_str(&format!("&_q={}", urlencoding::encode(query)));
        }

        if let Some(t) = tag {
            url.push_str(&format!("&tag={}", t));
        }

        match http_client.get(&url).send().await {
            Ok(response) => {
                match response.json::<Value>().await {
                    Ok(events) => {
                        // Use lightweight summaries (no outcomes) to keep context small
                        let markets = Self::transform_events_to_summaries(&events);

                        let result = json!({
                            "status": "success",
                            "query": query,
                            "tag": tag,
                            "count": markets.len(),
                            "offset": offset,
                            "limit": limit,
                            "markets": markets,
                            "note": "Use get_market with slug to see outcomes and token_ids for trading."
                        });
                        ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
                    }
                    Err(e) => ToolResult::error(format!("Failed to parse markets: {}", e))
                }
            }
            Err(e) => ToolResult::error(format!("Failed to search markets: {}", e))
        }
    }

    /// Get trending/popular markets (lightweight summaries only)
    async fn trending_markets(&self, params: &PolymarketParams, context: &ToolContext) -> ToolResult {
        let limit = params.limit.unwrap_or(10).min(20);
        let offset = params.offset.unwrap_or(0);
        let tag = params.tag.as_deref();

        let http_client = context.http_client();

        // Get markets sorted by volume (trending)
        let mut url = format!(
            "https://gamma-api.polymarket.com/events?active=true&closed=false&limit={}&offset={}&order=volume&ascending=false",
            limit, offset
        );

        if let Some(t) = tag {
            url.push_str(&format!("&tag={}", t));
        }

        match http_client.get(&url).send().await {
            Ok(response) => {
                match response.json::<Value>().await {
                    Ok(events) => {
                        // Use lightweight summaries (no outcomes) to keep context small
                        let markets = Self::transform_events_to_summaries(&events);

                        let result = json!({
                            "status": "success",
                            "type": "trending",
                            "tag": tag,
                            "count": markets.len(),
                            "offset": offset,
                            "limit": limit,
                            "markets": markets,
                            "note": "Use get_market with slug to see outcomes and token_ids for trading."
                        });
                        ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
                    }
                    Err(e) => ToolResult::error(format!("Failed to parse markets: {}", e))
                }
            }
            Err(e) => ToolResult::error(format!("Failed to fetch trending markets: {}", e))
        }
    }

    /// Get market details by slug
    async fn get_market(&self, params: &PolymarketParams, context: &ToolContext) -> ToolResult {
        let slug = match &params.slug {
            Some(s) => s,
            None => return ToolResult::error("slug is required for get_market (e.g., 'will-bitcoin-hit-100k')"),
        };

        let http_client = context.http_client();
        let url = format!("https://gamma-api.polymarket.com/events?slug={}", slug);

        match http_client.get(&url).send().await {
            Ok(response) => {
                match response.json::<Value>().await {
                    Ok(events) => {
                        // Events is an array, get first match
                        if let Some(event) = events.as_array().and_then(|arr| arr.first()) {
                            let market_info = Self::transform_single_event(event);

                            let result = json!({
                                "status": "success",
                                "market": market_info,
                                "note": "Use the token_id values with place_order to trade specific outcomes."
                            });
                            ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
                        } else {
                            ToolResult::error(format!("Market not found with slug: {}", slug))
                        }
                    }
                    Err(e) => ToolResult::error(format!("Failed to parse market: {}", e))
                }
            }
            Err(e) => ToolResult::error(format!("Failed to fetch market: {}", e))
        }
    }

    /// Get current price and orderbook for a token
    async fn get_price(&self, params: &PolymarketParams, context: &ToolContext) -> ToolResult {
        let token_id = match &params.token_id {
            Some(t) => t,
            None => return ToolResult::error("token_id is required for get_price"),
        };

        let http_client = context.http_client();

        // Fetch midpoint, spread, and orderbook in parallel
        let midpoint_url = format!("https://clob.polymarket.com/midpoint?token_id={}", token_id);
        let spread_url = format!("https://clob.polymarket.com/spread?token_id={}", token_id);
        let book_url = format!("https://clob.polymarket.com/book?token_id={}", token_id);

        let (midpoint_res, spread_res, book_res) = tokio::join!(
            http_client.get(&midpoint_url).send(),
            http_client.get(&spread_url).send(),
            http_client.get(&book_url).send()
        );

        // Parse responses
        let midpoint: Option<Value> = match midpoint_res {
            Ok(r) => r.json().await.ok(),
            Err(_) => None,
        };
        let spread: Option<Value> = match spread_res {
            Ok(r) => r.json().await.ok(),
            Err(_) => None,
        };
        let book: Option<Value> = match book_res {
            Ok(r) => r.json().await.ok(),
            Err(_) => None,
        };

        // Extract best bid/ask from orderbook
        let best_bid = book.as_ref()
            .and_then(|b| b.get("bids"))
            .and_then(|bids| bids.as_array())
            .and_then(|arr| arr.first())
            .and_then(|bid| bid.get("price"))
            .and_then(|p| p.as_str())
            .unwrap_or("N/A");

        let best_ask = book.as_ref()
            .and_then(|b| b.get("asks"))
            .and_then(|asks| asks.as_array())
            .and_then(|arr| arr.first())
            .and_then(|ask| ask.get("price"))
            .and_then(|p| p.as_str())
            .unwrap_or("N/A");

        let mid = midpoint.as_ref()
            .and_then(|m| m.get("mid"))
            .and_then(|p| p.as_str())
            .unwrap_or("N/A");

        let result = json!({
            "status": "success",
            "token_id": token_id,
            "price": {
                "midpoint": mid,
                "best_bid": best_bid,
                "best_ask": best_ask,
                "spread": spread,
            },
            "orderbook_summary": {
                "bids": book.as_ref().and_then(|b| b.get("bids")).and_then(|b| b.as_array()).map(|a| a.len()).unwrap_or(0),
                "asks": book.as_ref().and_then(|b| b.get("asks")).and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0),
            },
            "note": "Prices are 0-1 representing probability. Use this token_id with place_order to trade."
        });
        ToolResult::success(serde_json::to_string_pretty(&result).unwrap())
    }

    // ==================== HELPER METHODS ====================

    /// Transform Gamma API events response into a cleaner market list
    /// Transform events to lightweight market summaries (no outcomes - for listing)
    fn transform_events_to_summaries(events: &Value) -> Vec<Value> {
        let empty_vec = vec![];
        let events_arr = events.as_array().unwrap_or(&empty_vec);

        events_arr.iter().filter_map(|event| {
            Self::transform_event_summary(event)
        }).collect()
    }

    /// Transform a single event into a lightweight summary (no outcomes)
    fn transform_event_summary(event: &Value) -> Option<Value> {
        let title = event.get("title")?.as_str()?;
        let slug = event.get("slug").and_then(|s| s.as_str()).unwrap_or("");
        let description = event.get("description").and_then(|d| d.as_str()).unwrap_or("");
        let end_date = event.get("endDate").and_then(|d| d.as_str()).unwrap_or("");
        let volume = event.get("volume").and_then(|v| v.as_str()).unwrap_or("0");
        let liquidity = event.get("liquidity").and_then(|l| l.as_str()).unwrap_or("0");

        Some(json!({
            "title": title,
            "slug": slug,
            "description": description.chars().take(150).collect::<String>(),
            "end_date": end_date,
            "volume": volume,
            "liquidity": liquidity
        }))
    }

    /// Transform a single event into full market info (with outcomes)
    fn transform_single_event(event: &Value) -> Option<Value> {
        let title = event.get("title")?.as_str()?;
        let slug = event.get("slug").and_then(|s| s.as_str()).unwrap_or("");
        let description = event.get("description").and_then(|d| d.as_str()).unwrap_or("");
        let end_date = event.get("endDate").and_then(|d| d.as_str()).unwrap_or("");
        let volume = event.get("volume").and_then(|v| v.as_str()).unwrap_or("0");
        let liquidity = event.get("liquidity").and_then(|l| l.as_str()).unwrap_or("0");

        // Extract markets/outcomes
        let markets = event.get("markets").and_then(|m| m.as_array())?;

        let outcomes: Vec<Value> = markets.iter().filter_map(|market| {
            let question = market.get("question").and_then(|q| q.as_str()).unwrap_or("");
            let condition_id = market.get("conditionId").and_then(|c| c.as_str()).unwrap_or("");
            let outcome_prices = market.get("outcomePrices").and_then(|p| p.as_str()).unwrap_or("[]");
            let outcomes = market.get("outcomes").and_then(|o| o.as_str()).unwrap_or("[]");

            // Parse outcome prices
            let prices: Vec<&str> = outcome_prices.trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|s| s.trim().trim_matches('"'))
                .collect();

            let outcome_names: Vec<&str> = outcomes.trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|s| s.trim().trim_matches('"'))
                .collect();

            // Get individual token IDs for each outcome
            let tokens = market.get("clobTokenIds").and_then(|t| t.as_str()).unwrap_or("[]");
            let token_ids: Vec<&str> = tokens.trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|s| s.trim().trim_matches('"'))
                .collect();

            Some(json!({
                "question": question,
                "condition_id": condition_id,
                "outcomes": outcome_names.iter().enumerate().map(|(i, name)| {
                    json!({
                        "name": name,
                        "price": prices.get(i).unwrap_or(&"N/A"),
                        "token_id": token_ids.get(i).unwrap_or(&"")
                    })
                }).collect::<Vec<Value>>()
            }))
        }).collect();

        Some(json!({
            "title": title,
            "slug": slug,
            "description": description.chars().take(200).collect::<String>(),
            "end_date": end_date,
            "volume": volume,
            "liquidity": liquidity,
            "outcomes": outcomes
        }))
    }
}

#[derive(Debug, Deserialize)]
struct PolymarketParams {
    action: String,
    // Discovery params
    query: Option<String>,
    slug: Option<String>,
    tag: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
    // Trading params
    token_id: Option<String>,
    side: Option<String>,
    price: Option<f64>,
    size: Option<f64>,
    order_type: Option<String>,
    order_id: Option<String>,
}

#[async_trait]
impl Tool for PolymarketTradeTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: PolymarketParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        match params.action.as_str() {
            // Discovery actions (no auth required)
            "search_markets" => self.search_markets(&params, context).await,
            "trending_markets" => self.trending_markets(&params, context).await,
            "get_market" => self.get_market(&params, context).await,
            "get_price" => self.get_price(&params, context).await,
            // Trading actions (require wallet)
            "place_order" => self.place_order(&params).await,
            "cancel_order" => self.cancel_order(&params).await,
            "cancel_all" => self.cancel_all().await,
            "get_orders" => self.get_orders().await,
            "get_positions" => self.get_positions_with_context(context).await,
            "get_balance" => self.get_balance_with_context(context).await,
            _ => ToolResult::error(format!(
                "Unknown action: '{}'. Discovery: search_markets, trending_markets, get_market, get_price. Trading: place_order, cancel_order, cancel_all, get_orders, get_positions, get_balance",
                params.action
            )),
        }
    }
}

impl Default for PolymarketTradeTool {
    fn default() -> Self {
        Self::new()
    }
}

// Make the tool Clone-able for the registry
impl Clone for PolymarketTradeTool {
    fn clone(&self) -> Self {
        Self {
            definition: self.definition.clone(),
            client_cache: Arc::new(Mutex::new(None)), // Fresh cache for clone
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that searching for politics markets returns sensible results
    /// This simulates a user query like "find me politics markets on polymarket"
    #[tokio::test]
    async fn test_search_politics_markets() {
        let tool = PolymarketTradeTool::new();
        let context = ToolContext::new();

        // Search for politics markets using the tag filter
        let result = tool.execute(
            json!({
                "action": "search_markets",
                "tag": "politics",
                "limit": 10
            }),
            &context,
        ).await;

        // Should succeed
        assert!(result.success, "Search should succeed. Error: {:?}", result.error);

        // Parse the response
        let response: Value = serde_json::from_str(&result.content)
            .expect("Response should be valid JSON");

        // Verify response structure
        assert_eq!(response["status"], "success", "Status should be success");

        // Should have markets array
        let markets = response["markets"].as_array()
            .expect("Response should contain markets array");

        // Should return at least some markets (polymarket always has politics markets)
        assert!(!markets.is_empty(), "Should return at least one politics market");

        // Verify each market has expected structure (search returns summaries without outcomes)
        for market in markets {
            // Each market should have a title
            assert!(market.get("title").is_some(), "Market should have title: {:?}", market);
            let title = market["title"].as_str().unwrap();
            assert!(!title.is_empty(), "Title should not be empty");

            // Summaries should have a slug for further lookup
            assert!(market.get("slug").is_some(), "Market should have slug: {:?}", market);
        }

        println!("Found {} politics markets", markets.len());
        println!("First market: {}", markets[0]["title"]);
    }

    /// Test searching with a query string (like "find me politics markets")
    #[tokio::test]
    async fn test_search_markets_with_query() {
        let tool = PolymarketTradeTool::new();
        let context = ToolContext::new();

        // Search using a query string similar to "find me politics markets"
        let result = tool.execute(
            json!({
                "action": "search_markets",
                "query": "politics",
                "limit": 5
            }),
            &context,
        ).await;

        assert!(result.success, "Search with query should succeed. Error: {:?}", result.error);

        let response: Value = serde_json::from_str(&result.content)
            .expect("Response should be valid JSON");

        assert_eq!(response["status"], "success");
        assert_eq!(response["query"], "politics", "Query should be echoed back");

        let markets = response["markets"].as_array()
            .expect("Should have markets array");

        // Politics is a major category, should have results
        println!("Query 'politics' returned {} markets", markets.len());

        // Print market titles for manual verification
        for market in markets.iter().take(3) {
            println!("  - {}", market["title"].as_str().unwrap_or("N/A"));
        }
    }

    /// Test trending politics markets
    #[tokio::test]
    async fn test_trending_politics_markets() {
        let tool = PolymarketTradeTool::new();
        let context = ToolContext::new();

        let result = tool.execute(
            json!({
                "action": "trending_markets",
                "tag": "politics",
                "limit": 5
            }),
            &context,
        ).await;

        assert!(result.success, "Trending markets should succeed. Error: {:?}", result.error);

        let response: Value = serde_json::from_str(&result.content)
            .expect("Response should be valid JSON");

        assert_eq!(response["status"], "success");
        assert_eq!(response["type"], "trending");

        let markets = response["markets"].as_array()
            .expect("Should have markets array");

        // Trending markets should be sorted by volume
        println!("Found {} trending politics markets", markets.len());

        // Verify markets have volume info (used for sorting)
        for market in markets {
            let volume = market.get("volume");
            assert!(volume.is_some(), "Trending market should have volume");
        }
    }
}
