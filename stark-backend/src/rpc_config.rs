//! RPC Provider Configuration
//!
//! Loads RPC provider configurations from config/rpc_providers.ron
//! Supports x402-enabled (paid) and regular (free) RPC endpoints.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{OnceLock, RwLock};
use strum::{Display, EnumString, AsRefStr};

/// Supported blockchain networks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Base,
    Mainnet,
    Polygon,
}

impl Network {
    /// Get the chain ID for this network
    pub fn chain_id(&self) -> u64 {
        match self {
            Network::Base => 8453,
            Network::Mainnet => 1,
            Network::Polygon => 137,
        }
    }

    /// Get the native currency symbol
    pub fn native_currency(&self) -> &'static str {
        match self {
            Network::Base => "ETH",
            Network::Mainnet => "ETH",
            Network::Polygon => "MATIC",
        }
    }

    /// Get the block explorer URL
    pub fn explorer_url(&self) -> &'static str {
        match self {
            Network::Base => "https://basescan.org",
            Network::Mainnet => "https://etherscan.io",
            Network::Polygon => "https://polygonscan.com",
        }
    }

    /// Get the USDC contract address for this network
    pub fn usdc_address(&self) -> &'static str {
        match self {
            Network::Base => "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            Network::Mainnet => "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            Network::Polygon => "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359",
        }
    }

    /// All supported networks
    pub fn all() -> &'static [Network] {
        &[Network::Base, Network::Mainnet, Network::Polygon]
    }

    /// Try to detect network from a known contract address
    pub fn from_contract_address(address: &str) -> Option<Network> {
        let addr_lower = address.to_lowercase();
        match addr_lower.as_str() {
            // USDC addresses
            "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913" => Some(Network::Base),
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48" => Some(Network::Mainnet),
            "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359" => Some(Network::Polygon),
            _ => None,
        }
    }
}

impl Default for Network {
    fn default() -> Self {
        Network::Base
    }
}

/// Global storage for RPC providers
static RPC_PROVIDERS: OnceLock<HashMap<String, RpcProvider>> = OnceLock::new();

/// Global storage for Alchemy API key (loaded from DB at startup)
static ALCHEMY_API_KEY: OnceLock<String> = OnceLock::new();

/// Store the Alchemy API key for use by the unified RPC resolver.
/// Called once at startup after loading the key from DB / env.
pub fn set_alchemy_api_key(key: String) {
    if ALCHEMY_API_KEY.set(key).is_err() {
        log::warn!("[rpc_config] Alchemy API key already set");
    }
}

/// Get the stored Alchemy API key, if any.
pub fn get_alchemy_api_key() -> Option<&'static String> {
    ALCHEMY_API_KEY.get()
}

/// Global storage for user-configured custom RPC endpoints (from bot_settings).
/// Uses RwLock because these can be updated at runtime when the user changes settings.
static CUSTOM_RPC_ENDPOINTS: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

/// Store custom RPC endpoints globally so all codepaths (including eip8004) can use them.
/// Called at startup and whenever bot_settings are updated.
pub fn set_custom_rpc_endpoints(endpoints: HashMap<String, String>) {
    let non_empty: HashMap<String, String> = endpoints.into_iter()
        .filter(|(_, v)| !v.is_empty())
        .collect();
    if non_empty.is_empty() {
        *CUSTOM_RPC_ENDPOINTS.write().unwrap_or_else(|e| e.into_inner()) = None;
    } else {
        log::info!("[rpc_config] Custom RPC endpoints set for: {:?}", non_empty.keys().collect::<Vec<_>>());
        *CUSTOM_RPC_ENDPOINTS.write().unwrap_or_else(|e| e.into_inner()) = Some(non_empty);
    }
}

/// Get a custom RPC endpoint for a network, if configured.
fn custom_rpc_url(network: &str) -> Option<String> {
    CUSTOM_RPC_ENDPOINTS.read().unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|endpoints| endpoints.get(network).cloned())
}

/// Build an Alchemy RPC URL for the given network and API key.
/// Returns `None` if the network has no known Alchemy subdomain.
fn alchemy_url(network: &str, key: &str) -> Option<String> {
    let subdomain = match network {
        "base" => "base-mainnet",
        "mainnet" => "eth-mainnet",
        "polygon" => "polygon-mainnet",
        "arbitrum" => "arb-mainnet",
        "optimism" => "opt-mainnet",
        "base-sepolia" => "base-sepolia",
        _ => return None,
    };
    Some(format!("https://{}.g.alchemy.com/v2/{}", subdomain, key))
}

/// Best free public RPC URL per network (last resort).
fn public_rpc_url(network: &str) -> Option<&'static str> {
    match network {
        "base" => Some("https://mainnet.base.org"),
        "mainnet" => Some("https://eth.llamarpc.com"),
        "polygon" => Some("https://polygon-rpc.com"),
        "arbitrum" => Some("https://arb1.arbitrum.io/rpc"),
        "optimism" => Some("https://mainnet.optimism.io"),
        "base-sepolia" => Some("https://sepolia.base.org"),
        _ => None,
    }
}

/// DeFi Relay x402 URL for a network.
fn defirelay_url(network: &str) -> String {
    if let Some((url, _)) = get_rpc_endpoint("defirelay", network) {
        url
    } else {
        format!("https://rpc.defirelay.com/rpc/light/{}", network)
    }
}

/// Canonical RPC resolution: Custom -> Alchemy -> DeFi Relay (x402).
///
/// Use this for codepaths that go through X402EvmRpc (which handles 402 responses).
pub fn resolve_rpc(network: &str) -> ResolvedRpcConfig {
    // Tier 0: User-configured custom endpoint (from bot_settings)
    if let Some(url) = custom_rpc_url(network) {
        log::info!("[rpc_config] Custom endpoint for {}: {}", network, url);
        return ResolvedRpcConfig { url, use_x402: false };
    }

    // Tier 1: Alchemy (free, no x402)
    if let Some(key) = get_alchemy_api_key() {
        if let Some(url) = alchemy_url(network, key) {
            log::info!("[rpc_config] Tier 1 (Alchemy) for {}: {}", network, &url[..url.len().min(60)]);
            return ResolvedRpcConfig { url, use_x402: false };
        }
    }

    // Tier 2: DeFi Relay (x402 paid RPC)
    let url = defirelay_url(network);
    log::info!("[rpc_config] Tier 2 (DeFi Relay) for {}: {}", network, url);
    ResolvedRpcConfig { url, use_x402: true }
}

/// Read-only RPC resolution for raw HTTP callers that can't handle x402 402-responses.
/// Priority: Custom -> Alchemy -> Public -> DeFi Relay.
pub fn resolve_rpc_readonly(network: &str) -> ResolvedRpcConfig {
    // Tier 0: User-configured custom endpoint (from bot_settings)
    if let Some(url) = custom_rpc_url(network) {
        log::info!("[rpc_config] Custom endpoint readonly for {}: {}", network, url);
        return ResolvedRpcConfig { url, use_x402: false };
    }

    // Tier 1: Alchemy (free, no x402)
    if let Some(key) = get_alchemy_api_key() {
        if let Some(url) = alchemy_url(network, key) {
            log::info!("[rpc_config] Tier 1 (Alchemy) readonly for {}: {}", network, &url[..url.len().min(60)]);
            return ResolvedRpcConfig { url, use_x402: false };
        }
    }

    // Tier 2: Public RPC (free, no x402)
    if let Some(url) = public_rpc_url(network) {
        log::info!("[rpc_config] Tier 2 (Public) readonly for {}: {}", network, url);
        return ResolvedRpcConfig { url: url.to_string(), use_x402: false };
    }

    // Tier 3: DeFi Relay (x402 — caller may not handle this well)
    let url = defirelay_url(network);
    log::warn!("[rpc_config] Tier 3 (DeFi Relay) readonly fallback for {}: {} — caller may not handle x402", network, url);
    ResolvedRpcConfig { url, use_x402: true }
}

/// RPC Provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcProvider {
    /// Display name for UI (e.g., "DeFi Relay (x402)")
    pub display_name: String,
    /// Description of the provider
    pub description: String,
    /// Whether this provider uses x402 payment protocol
    pub x402: bool,
    /// Network -> URL mapping (e.g., "base" -> "https://...")
    pub endpoints: HashMap<String, String>,
}

impl RpcProvider {
    /// Get the endpoint URL for a specific network
    pub fn get_endpoint(&self, network: &str) -> Option<&String> {
        self.endpoints.get(network)
    }

    /// Get list of supported networks
    pub fn supported_networks(&self) -> Vec<&String> {
        self.endpoints.keys().collect()
    }
}

/// Load RPC providers from config directory
pub fn load_rpc_providers(config_dir: &Path) {
    let config_path = config_dir.join("rpc_providers.ron");

    let providers = if config_path.exists() {
        match std::fs::read_to_string(&config_path) {
            Ok(content) => match ron::from_str::<HashMap<String, RpcProvider>>(&content) {
                Ok(providers) => {
                    log::info!(
                        "Loaded {} RPC providers from config: {:?}",
                        providers.len(),
                        providers.keys().collect::<Vec<_>>()
                    );
                    providers
                }
                Err(e) => {
                    log::error!("Failed to parse rpc_providers.ron: {}", e);
                    default_providers()
                }
            },
            Err(e) => {
                log::error!("Failed to read rpc_providers.ron: {}", e);
                default_providers()
            }
        }
    } else {
        log::info!("No rpc_providers.ron found, using defaults");
        default_providers()
    };

    if RPC_PROVIDERS.set(providers).is_err() {
        log::warn!("RPC providers already initialized");
    }
}

/// Get default providers (DeFi Relay)
fn default_providers() -> HashMap<String, RpcProvider> {
    let mut providers = HashMap::new();
    let mut endpoints = HashMap::new();
    endpoints.insert(
        "base".to_string(),
        "https://rpc.defirelay.com/rpc/light/base".to_string(),
    );
    endpoints.insert(
        "mainnet".to_string(),
        "https://rpc.defirelay.com/rpc/light/mainnet".to_string(),
    );
    endpoints.insert(
        "polygon".to_string(),
        "https://rpc.defirelay.com/rpc/light/polygon".to_string(),
    );

    providers.insert(
        "defirelay".to_string(),
        RpcProvider {
            display_name: "DeFi Relay (x402)".to_string(),
            description: "Paid RPC via x402 payment protocol".to_string(),
            x402: true,
            endpoints,
        },
    );

    providers
}

/// Get a specific RPC provider by name
pub fn get_rpc_provider(name: &str) -> Option<RpcProvider> {
    RPC_PROVIDERS
        .get()
        .and_then(|providers| providers.get(name).cloned())
}

/// List all available RPC providers
pub fn list_rpc_providers() -> Vec<(String, RpcProvider)> {
    RPC_PROVIDERS
        .get()
        .map(|providers| {
            providers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// Get the endpoint URL for a provider and network
/// Returns (url, use_x402) tuple
pub fn get_rpc_endpoint(provider_name: &str, network: &str) -> Option<(String, bool)> {
    get_rpc_provider(provider_name).and_then(|provider| {
        provider
            .get_endpoint(network)
            .map(|url| (url.clone(), provider.x402))
    })
}

/// Resolve RPC configuration from settings
/// If custom endpoints are provided, uses those (no x402)
/// Otherwise falls back to the configured provider
pub fn resolve_rpc_config(
    provider_name: &str,
    custom_endpoints: Option<&HashMap<String, String>>,
    network: &str,
) -> Option<(String, bool)> {
    // Custom endpoints take precedence (no x402)
    if let Some(endpoints) = custom_endpoints {
        if let Some(url) = endpoints.get(network) {
            if !url.is_empty() {
                return Some((url.clone(), false));
            }
        }
    }

    // Fall back to provider
    get_rpc_endpoint(provider_name, network)
}

/// Resolved RPC configuration ready for use
#[derive(Debug, Clone)]
pub struct ResolvedRpcConfig {
    pub url: String,
    pub use_x402: bool,
}

/// Resolve RPC configuration using default provider
/// Used when tool context is not available (e.g., gateway RPC methods)
pub fn resolve_rpc_from_network(network: &str) -> ResolvedRpcConfig {
    resolve_rpc(network)
}

/// Extract and resolve RPC configuration from ToolContext.extra
/// This is the canonical way to get RPC config in any tool.
pub fn resolve_rpc_from_context(
    extra: &HashMap<String, serde_json::Value>,
    network: &str,
) -> ResolvedRpcConfig {
    let rpc_provider = extra
        .get("rpc_provider")
        .and_then(|v| v.as_str())
        .unwrap_or("defirelay");

    let custom_endpoints: Option<HashMap<String, String>> = extra
        .get("custom_rpc_endpoints")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    // Custom endpoints take highest precedence (user-configured, no x402)
    if let Some(ref endpoints) = custom_endpoints {
        if let Some(url) = endpoints.get(network) {
            if !url.is_empty() {
                log::info!("[rpc_config] Custom endpoint for {}: {}", network, url);
                return ResolvedRpcConfig { url: url.clone(), use_x402: false };
            }
        }
    }

    // Check if user configured a non-default provider in the RON config
    if rpc_provider != "defirelay" {
        if let Some((url, use_x402)) = resolve_rpc_config(rpc_provider, None, network) {
            log::info!(
                "[rpc_config] Provider '{}' for {}: {} (x402={})",
                rpc_provider, network, url, use_x402
            );
            return ResolvedRpcConfig { url, use_x402 };
        }
    }

    // Delegate to unified 3-tier resolver
    resolve_rpc(network)
}
