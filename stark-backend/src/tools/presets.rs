//! Preset configurations loaded from RON files
//!
//! Presets define how tools should build requests from register values,
//! preventing hallucination of URLs, params, and other critical data.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// Global preset storage (loaded once at startup)
static FETCH_PRESETS: OnceLock<HashMap<String, FetchPreset>> = OnceLock::new();
static RPC_PRESETS: OnceLock<HashMap<String, RpcPreset>> = OnceLock::new();
static WEB3_PRESETS: OnceLock<HashMap<String, Web3Preset>> = OnceLock::new();
static NETWORKS: OnceLock<HashMap<String, NetworkConfig>> = OnceLock::new();

/// x402_preset_fetch preset configuration
#[derive(Debug, Clone, Deserialize)]
pub struct FetchPreset {
    pub base_url: String,
    pub jq_filter: String,
    /// (register_key, url_param_name) pairs
    pub params: Vec<(String, String)>,
    /// Static params like chainId
    #[serde(default)]
    pub static_params: Vec<(String, String)>,
    pub description: String,
}

/// x402_rpc preset configuration
#[derive(Debug, Clone, Deserialize)]
pub struct RpcPreset {
    pub method: String,
    /// Register keys to read for params
    pub params: Vec<String>,
    /// Whether to append "latest" as final param
    #[serde(default)]
    pub append_latest: bool,
    pub description: String,
}

/// web3_function_call preset configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Web3Preset {
    /// ABI file name (without .json)
    pub abi: String,
    /// Contract address per network (use this OR contract_register, not both)
    #[serde(default)]
    pub contracts: HashMap<String, String>,
    /// Register key to read contract address from (for dynamic contracts like any ERC20)
    pub contract_register: Option<String>,
    /// Function name to call
    pub function: String,
    /// Register keys to read for function params (in order)
    #[serde(default)]
    pub params_registers: Vec<String>,
    /// Register key for ETH value (for payable functions)
    pub value_register: Option<String>,
    /// Static params (not from registers)
    #[serde(default)]
    pub static_params: Vec<String>,
    pub description: String,
}

/// Network configuration
#[derive(Debug, Clone, Deserialize)]
pub struct NetworkConfig {
    pub chain_id: u64,
    pub name: String,
    pub native_token: String,
    pub explorer: String,
}

/// Load presets from config directory
pub fn load_presets(config_dir: &Path) {
    // Load fetch presets
    let fetch_path = config_dir.join("x402_fetch_presets.ron");
    if fetch_path.exists() {
        match std::fs::read_to_string(&fetch_path) {
            Ok(content) => {
                match ron::from_str::<HashMap<String, FetchPreset>>(&content) {
                    Ok(presets) => {
                        log::info!("[presets] Loaded {} fetch presets from {:?}", presets.len(), fetch_path);
                        let _ = FETCH_PRESETS.set(presets);
                    }
                    Err(e) => log::error!("[presets] Failed to parse fetch presets: {}", e),
                }
            }
            Err(e) => log::error!("[presets] Failed to read fetch presets file: {}", e),
        }
    } else {
        log::warn!("[presets] Fetch presets file not found: {:?}", fetch_path);
        let _ = FETCH_PRESETS.set(default_fetch_presets());
    }

    // Load RPC presets
    let rpc_path = config_dir.join("x402_rpc_presets.ron");
    if rpc_path.exists() {
        match std::fs::read_to_string(&rpc_path) {
            Ok(content) => {
                match ron::from_str::<HashMap<String, RpcPreset>>(&content) {
                    Ok(presets) => {
                        log::info!("[presets] Loaded {} RPC presets from {:?}", presets.len(), rpc_path);
                        let _ = RPC_PRESETS.set(presets);
                    }
                    Err(e) => log::error!("[presets] Failed to parse RPC presets: {}", e),
                }
            }
            Err(e) => log::error!("[presets] Failed to read RPC presets file: {}", e),
        }
    } else {
        log::warn!("[presets] RPC presets file not found: {:?}", rpc_path);
        let _ = RPC_PRESETS.set(default_rpc_presets());
    }

    // Load Web3 presets
    let web3_path = config_dir.join("web3_presets.ron");
    if web3_path.exists() {
        match std::fs::read_to_string(&web3_path) {
            Ok(content) => {
                match ron::from_str::<HashMap<String, Web3Preset>>(&content) {
                    Ok(presets) => {
                        log::info!("[presets] Loaded {} Web3 presets from {:?}", presets.len(), web3_path);
                        let _ = WEB3_PRESETS.set(presets);
                    }
                    Err(e) => log::error!("[presets] Failed to parse Web3 presets: {}", e),
                }
            }
            Err(e) => log::error!("[presets] Failed to read Web3 presets file: {}", e),
        }
    } else {
        log::warn!("[presets] Web3 presets file not found: {:?}", web3_path);
        let _ = WEB3_PRESETS.set(default_web3_presets());
    }

    // Load networks
    let networks_path = config_dir.join("networks.ron");
    if networks_path.exists() {
        match std::fs::read_to_string(&networks_path) {
            Ok(content) => {
                match ron::from_str::<HashMap<String, NetworkConfig>>(&content) {
                    Ok(networks) => {
                        log::info!("[presets] Loaded {} networks from {:?}", networks.len(), networks_path);
                        let _ = NETWORKS.set(networks);
                    }
                    Err(e) => {
                        log::error!("[presets] Failed to parse networks config: {}", e);
                        let _ = NETWORKS.set(default_networks());
                    }
                }
            }
            Err(e) => {
                log::error!("[presets] Failed to read networks file: {}", e);
                let _ = NETWORKS.set(default_networks());
            }
        }
    } else {
        log::warn!("[presets] Networks file not found: {:?}, using defaults", networks_path);
        let _ = NETWORKS.set(default_networks());
    }
}

/// Get networks, loading defaults if not already loaded
fn get_networks() -> &'static HashMap<String, NetworkConfig> {
    NETWORKS.get_or_init(default_networks)
}

/// Get a fetch preset by name
pub fn get_fetch_preset(name: &str) -> Option<FetchPreset> {
    FETCH_PRESETS.get()
        .or_else(|| {
            // Fallback to defaults if not loaded
            let _ = FETCH_PRESETS.set(default_fetch_presets());
            FETCH_PRESETS.get()
        })
        .and_then(|p| p.get(name).cloned())
}

/// Get an RPC preset by name
pub fn get_rpc_preset(name: &str) -> Option<RpcPreset> {
    RPC_PRESETS.get()
        .or_else(|| {
            let _ = RPC_PRESETS.set(default_rpc_presets());
            RPC_PRESETS.get()
        })
        .and_then(|p| p.get(name).cloned())
}

/// Get a Web3 preset by name
pub fn get_web3_preset(name: &str) -> Option<Web3Preset> {
    WEB3_PRESETS.get()
        .or_else(|| {
            let _ = WEB3_PRESETS.set(default_web3_presets());
            WEB3_PRESETS.get()
        })
        .and_then(|p| p.get(name).cloned())
}

/// Get network config by name
pub fn get_network(name: &str) -> Option<NetworkConfig> {
    get_networks().get(name).cloned()
}

/// List available fetch preset names
pub fn list_fetch_presets() -> Vec<String> {
    FETCH_PRESETS.get()
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_else(|| vec!["swap_quote".to_string()])
}

/// List available RPC preset names
pub fn list_rpc_presets() -> Vec<String> {
    RPC_PRESETS.get()
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_else(|| vec!["gas_price".to_string(), "get_balance".to_string(), "get_nonce".to_string(), "block_number".to_string()])
}

/// List available Web3 preset names
pub fn list_web3_presets() -> Vec<String> {
    WEB3_PRESETS.get()
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_else(|| vec![
            "weth_deposit".to_string(),
            "weth_withdraw".to_string(),
            "erc20_approve_swap".to_string(),
            "erc20_allowance_swap".to_string(),
            "erc20_balance".to_string(),
            "erc20_transfer".to_string(),
            "swap_execute".to_string(),
        ])
}

/// List available network names
pub fn list_networks() -> Vec<String> {
    get_networks().keys().cloned().collect()
}

/// Default fetch presets (fallback if config not found)
fn default_fetch_presets() -> HashMap<String, FetchPreset> {
    let mut map = HashMap::new();
    map.insert("swap_quote".to_string(), FetchPreset {
        base_url: "https://quoter.defirelay.com/swap/allowance-holder/quote".to_string(),
        jq_filter: "{to: .transaction.to, data: .transaction.data, value: .transaction.value, gas: .transaction.gas, buyAmount: .buyAmount, issues: .issues}".to_string(),
        params: vec![
            ("wallet_address".to_string(), "taker".to_string()),
            ("sell_token".to_string(), "sellToken".to_string()),
            ("buy_token".to_string(), "buyToken".to_string()),
            ("sell_amount".to_string(), "sellAmount".to_string()),
        ],
        static_params: vec![],
        description: "Get swap quote from 0x via DeFi Relay".to_string(),
    });
    map
}

/// Default RPC presets (fallback if config not found)
fn default_rpc_presets() -> HashMap<String, RpcPreset> {
    let mut map = HashMap::new();
    map.insert("gas_price".to_string(), RpcPreset {
        method: "eth_gasPrice".to_string(),
        params: vec![],
        append_latest: false,
        description: "Get current gas price".to_string(),
    });
    map.insert("block_number".to_string(), RpcPreset {
        method: "eth_blockNumber".to_string(),
        params: vec![],
        append_latest: false,
        description: "Get current block number".to_string(),
    });
    map.insert("get_balance".to_string(), RpcPreset {
        method: "eth_getBalance".to_string(),
        params: vec!["wallet_address".to_string()],
        append_latest: true,
        description: "Get ETH balance of wallet".to_string(),
    });
    map.insert("get_nonce".to_string(), RpcPreset {
        method: "eth_getTransactionCount".to_string(),
        params: vec!["wallet_address".to_string()],
        append_latest: true,
        description: "Get transaction count (nonce) of wallet".to_string(),
    });
    // Note: get_token_balance requires eth_call with encoded balanceOf(address) data
    // This is handled specially in x402_rpc.rs, not as a simple JSON-RPC preset
    map
}

/// Default Web3 presets (fallback if config not found)
fn default_web3_presets() -> HashMap<String, Web3Preset> {
    let mut map = HashMap::new();

    let mut weth_contracts = HashMap::new();
    weth_contracts.insert("base".to_string(), "0x4200000000000000000000000000000000000006".to_string());
    weth_contracts.insert("mainnet".to_string(), "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string());

    map.insert("weth_deposit".to_string(), Web3Preset {
        abi: "weth".to_string(),
        contracts: weth_contracts.clone(),
        contract_register: None,
        function: "deposit".to_string(),
        params_registers: vec![],
        value_register: Some("wrap_amount".to_string()),
        static_params: vec![],
        description: "Wrap ETH to WETH".to_string(),
    });

    map.insert("weth_withdraw".to_string(), Web3Preset {
        abi: "weth".to_string(),
        contracts: weth_contracts,
        contract_register: None,
        function: "withdraw".to_string(),
        params_registers: vec!["unwrap_amount".to_string()],
        value_register: None,
        static_params: vec![],
        description: "Unwrap WETH to ETH".to_string(),
    });

    map.insert("erc20_approve_swap".to_string(), Web3Preset {
        abi: "erc20".to_string(),
        contracts: HashMap::new(),
        contract_register: Some("sell_token".to_string()),
        function: "approve".to_string(),
        params_registers: vec![],
        value_register: None,
        static_params: vec![
            "0x0000000000001fF3684f28c67538d4D072C22734".to_string(), // 0x AllowanceHolder
            "115792089237316195423570985008687907853269984665640564039457584007913129639935".to_string(), // max uint256
        ],
        description: "Approve 0x AllowanceHolder to spend the sell token (max approval). Reads contract from sell_token register.".to_string(),
    });

    map.insert("erc20_allowance_swap".to_string(), Web3Preset {
        abi: "erc20".to_string(),
        contracts: HashMap::new(),
        contract_register: Some("sell_token".to_string()),
        function: "allowance".to_string(),
        params_registers: vec!["wallet_address".to_string()],
        value_register: None,
        static_params: vec![
            "0x0000000000001fF3684f28c67538d4D072C22734".to_string(), // 0x AllowanceHolder
        ],
        description: "Check AllowanceHolder allowance for the sell token. Reads contract from sell_token register.".to_string(),
    });

    map.insert("swap_execute".to_string(), Web3Preset {
        abi: "0x_settler".to_string(),
        contracts: HashMap::new(),
        contract_register: Some("swap_contract".to_string()),
        function: "exec".to_string(),
        params_registers: vec![
            "swap_param_0".to_string(),
            "swap_param_1".to_string(),
            "swap_param_2".to_string(),
            "swap_param_3".to_string(),
            "swap_param_4".to_string(),
        ],
        value_register: Some("swap_value".to_string()),
        static_params: vec![],
        description: "Execute 0x swap via AllowanceHolder.exec(). Use decode_calldata first to decode swap_quote into swap_contract, swap_param_0-4, swap_value registers.".to_string(),
    });

    map.insert("erc20_balance".to_string(), Web3Preset {
        abi: "erc20".to_string(),
        contracts: HashMap::new(),
        contract_register: Some("token_address".to_string()),
        function: "balanceOf".to_string(),
        params_registers: vec!["wallet_address".to_string()],
        value_register: None,
        static_params: vec![],
        description: "Get ERC20 token balance. Set token_address register first.".to_string(),
    });

    map.insert("erc20_transfer".to_string(), Web3Preset {
        abi: "erc20".to_string(),
        contracts: HashMap::new(),
        contract_register: Some("token_address".to_string()),
        function: "transfer".to_string(),
        params_registers: vec!["recipient_address".to_string(), "transfer_amount".to_string()],
        value_register: None,
        static_params: vec![],
        description: "Transfer ERC20 tokens. Set token_address, recipient_address, transfer_amount registers first.".to_string(),
    });

    map
}

/// Default networks (fallback if config not found)
fn default_networks() -> HashMap<String, NetworkConfig> {
    let mut map = HashMap::new();
    map.insert("base".to_string(), NetworkConfig {
        chain_id: 8453,
        name: "Base".to_string(),
        native_token: "ETH".to_string(),
        explorer: "https://basescan.org".to_string(),
    });
    map.insert("mainnet".to_string(), NetworkConfig {
        chain_id: 1,
        name: "Ethereum Mainnet".to_string(),
        native_token: "ETH".to_string(),
        explorer: "https://etherscan.io".to_string(),
    });
    map
}

/// Get chain ID for network (returns string for URL params)
pub fn get_chain_id(network: &str) -> String {
    get_networks()
        .get(network)
        .map(|n| n.chain_id.to_string())
        .unwrap_or_else(|| "8453".to_string()) // default to base
}

/// Get chain ID as u64 for network
pub fn get_chain_id_u64(network: &str) -> u64 {
    get_networks()
        .get(network)
        .map(|n| n.chain_id)
        .unwrap_or(8453) // default to base
}

/// Get network name (display name) for a network key
pub fn get_network_name(network: &str) -> String {
    get_networks()
        .get(network)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| network.to_string())
}

/// Get explorer URL for a network
pub fn get_explorer_url(network: &str) -> String {
    get_networks()
        .get(network)
        .map(|n| n.explorer.clone())
        .unwrap_or_else(|| "https://basescan.org".to_string())
}
