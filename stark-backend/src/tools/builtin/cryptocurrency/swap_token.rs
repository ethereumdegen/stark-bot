//! Composite swap_token tool — full token swap in one tool call
//!
//! Combines: token lookup, allowance check, approval, amount conversion,
//! quote fetching, calldata decoding, and swap execution.
//!
//! Reduces a typical 8-iteration swap flow to 1 tool call + 1 broadcast.

use super::broadcast_web3_tx::BroadcastWeb3TxTool;
use super::token_lookup::TokenLookupTool;
use super::to_raw_amount::ToRawAmountTool;
use super::x402_preset_fetch::fetch_x402_preset;
use crate::tools::presets::{get_chain_id, get_network_name, get_web3_preset};
use crate::tools::registry::Tool;
use crate::tools::rpc_config::resolve_rpc_from_context;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::web3::{
    self, call_function, default_abis_dir, encode_call, execute_resolved_call,
    find_function_with_params, load_abi, parse_abi, resolve_network, token_to_value,
};
use async_trait::async_trait;
use ethers::abi::ParamType;
use ethers::types::{Address, U256};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Native ETH sentinel address (not a real contract)
const ETH_SENTINEL: &str = "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE";

/// 0x AllowanceHolder contract address (same on all supported chains)
const ALLOWANCE_HOLDER: &str = "0x0000000000001fF3684f28c67538d4D072C22734";

/// Max uint256 for ERC-20 approvals
const MAX_UINT256: &str =
    "115792089237316195423570985008687907853269984665640564039457584007913129639935";

/// Composite swap tool
pub struct SwapTokenTool {
    definition: ToolDefinition,
}

impl SwapTokenTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "sell_token".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Token symbol to sell (e.g., 'USDC', 'ETH', 'WETH'). Case-insensitive."
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "buy_token".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Token symbol to buy (e.g., 'ETH', 'USDC', 'WETH'). Case-insensitive."
                    .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "amount".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description:
                    "Human-readable amount of sell token (e.g., '100', '0.5', '1.25')."
                        .to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "network".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Blockchain network. Defaults to 'base'.".to_string(),
                default: Some(json!("base")),
                items: None,
                enum_values: Some(vec![
                    "base".to_string(),
                    "mainnet".to_string(),
                    "polygon".to_string(),
                ]),
            },
        );

        SwapTokenTool {
            definition: ToolDefinition {
                name: "swap_token".to_string(),
                description: "Execute a complete token swap in one call. Handles token lookup, \
                    allowance check/approval, quote fetching, and swap execution. \
                    Returns a queued transaction UUID — broadcast via broadcast_web3_tx."
                    .to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![
                        "sell_token".to_string(),
                        "buy_token".to_string(),
                        "amount".to_string(),
                    ],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }
}

impl Default for SwapTokenTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SwapTokenParams {
    sell_token: String,
    buy_token: String,
    amount: String,
    #[serde(default = "default_network")]
    network: String,
}

fn default_network() -> String {
    "base".to_string()
}

#[async_trait]
impl Tool for SwapTokenTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        // Check if network was explicitly provided
        let network_explicitly_set = params.get("network").is_some();

        let params: SwapTokenParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        log::info!(
            "[swap_token] Starting swap: {} {} → {} on {}",
            params.amount, params.sell_token, params.buy_token, params.network
        );

        // ─── Step 1: Resolve network ───────────────────────────────────────────

        let network_str = if network_explicitly_set {
            params.network.clone()
        } else if let Some(reg) = context.registers.get("network_name") {
            // Use previously selected network if no explicit param
            reg.as_str().unwrap_or("base").to_string()
        } else {
            params.network.clone()
        };

        let network = match resolve_network(Some(&network_str), context.selected_network.as_deref())
        {
            Ok(n) => n,
            Err(e) => return ToolResult::error(format!("Invalid network: {}", e)),
        };

        let chain_id = get_chain_id(&network_str);
        let network_name = get_network_name(&network_str);
        context.set_register("network_name", json!(&network_name), "swap_token");
        context.set_register("chain_id", json!(&chain_id), "swap_token");

        log::info!(
            "[swap_token] Network: {} (chain_id={})",
            network_name, chain_id
        );

        // ─── Step 2: Lookup sell token ─────────────────────────────────────────

        let sell_info = match TokenLookupTool::lookup(&params.sell_token, &network_str) {
            Some(info) => info,
            None => {
                return ToolResult::error(format!(
                    "Unknown sell token '{}' on {}",
                    params.sell_token, network_str
                ))
            }
        };

        let is_native_eth =
            sell_info.address.to_lowercase() == ETH_SENTINEL.to_lowercase();

        // For native ETH: keep the sentinel address — 0x API handles wrapping.
        // No allowance check needed (native ETH doesn't go through ERC-20 approval).
        let sell_address = &sell_info.address;
        let sell_symbol = params.sell_token.to_uppercase();
        let sell_decimals = sell_info.decimals;

        context.set_register("sell_token", json!(sell_address), "swap_token");
        context.set_register("sell_token_symbol", json!(&sell_symbol), "swap_token");
        context.set_register("sell_token_decimals", json!(sell_decimals), "swap_token");

        log::info!(
            "[swap_token] Sell token: {} ({}) decimals={} native_eth={}",
            sell_symbol, sell_address, sell_decimals, is_native_eth
        );

        // ─── Step 3: Lookup buy token ──────────────────────────────────────────

        let buy_info = match TokenLookupTool::lookup(&params.buy_token, &network_str) {
            Some(info) => info,
            None => {
                return ToolResult::error(format!(
                    "Unknown buy token '{}' on {}",
                    params.buy_token, network_str
                ))
            }
        };

        let buy_symbol = params.buy_token.to_uppercase();
        context.set_register("buy_token", json!(&buy_info.address), "swap_token");
        context.set_register("buy_token_symbol", json!(&buy_symbol), "swap_token");
        context.set_register("buy_token_decimals", json!(buy_info.decimals), "swap_token");

        log::info!(
            "[swap_token] Buy token: {} ({})",
            buy_symbol, buy_info.address
        );

        // ─── Step 4: Get wallet address ────────────────────────────────────────

        let wallet_provider = match &context.wallet_provider {
            Some(wp) => wp,
            None => return ToolResult::error("Wallet not configured. Cannot execute swaps."),
        };

        let wallet_address = wallet_provider.get_address();
        context.set_register("wallet_address", json!(&wallet_address), "swap_token");

        log::info!("[swap_token] Wallet: {}", wallet_address);

        // ─── Step 5: Convert amount to raw ─────────────────────────────────────

        let raw_amount = match ToRawAmountTool::convert_to_raw(&params.amount, sell_decimals) {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Invalid amount: {}", e)),
        };

        context.set_register("sell_amount", json!(&raw_amount), "swap_token");

        log::info!(
            "[swap_token] Amount: {} → {} raw (decimals={})",
            params.amount, raw_amount, sell_decimals
        );

        // ─── Step 6: Check allowance (ERC-20 tokens only) ─────────────────────

        if !is_native_eth {
            let allowance = match check_erc20_allowance(
                sell_address,
                &wallet_address,
                ALLOWANCE_HOLDER,
                &network_str,
                context,
                wallet_provider,
            )
            .await
            {
                Ok(a) => a,
                Err(e) => {
                    log::warn!("[swap_token] Allowance check failed (proceeding): {}", e);
                    U256::zero()
                }
            };

            let sell_amount_u256 =
                U256::from_dec_str(&raw_amount).unwrap_or(U256::zero());

            log::info!(
                "[swap_token] Allowance: {} (need: {})",
                allowance, sell_amount_u256
            );

            // ─── Step 7: Approve if needed ─────────────────────────────────────

            if allowance < sell_amount_u256 {
                log::info!("[swap_token] Insufficient allowance, requesting approval");

                let is_rogue_mode = context
                    .extra
                    .get("rogue_mode_enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let abis_dir = default_abis_dir();

                // Build approval params: approve(AllowanceHolder, max_uint256)
                let approval_params = vec![json!(ALLOWANCE_HOLDER), json!(MAX_UINT256)];

                let approval_result = execute_resolved_call(
                    &abis_dir,
                    "erc20",
                    sell_address,
                    "approve",
                    &approval_params,
                    "0",
                    false,
                    &network,
                    context,
                    Some("erc20_approve_swap"),
                )
                .await;

                if !approval_result.success {
                    return ToolResult::error(format!(
                        "Approval failed: {}",
                        approval_result.content
                    ));
                }

                let approval_uuid = approval_result
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("uuid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if approval_uuid.is_empty() {
                    return ToolResult::error("Failed to get approval transaction UUID");
                }

                if !is_rogue_mode {
                    // Partner mode: return early — user must approve, then re-invoke swap_token
                    return ToolResult::success(format!(
                        "APPROVAL REQUIRED\n\n\
                        The sell token ({} / {}) must be approved for the AllowanceHolder before swapping.\n\n\
                        Approval transaction queued: {}\n\n\
                        Next steps:\n\
                        1. Broadcast the approval: broadcast_web3_tx with uuid \"{}\"\n\
                        2. After confirmation, call swap_token again with the same parameters.",
                        sell_symbol, sell_address, approval_uuid, approval_uuid
                    ))
                    .with_metadata(json!({
                        "status": "approval_required",
                        "approval_uuid": approval_uuid,
                        "sell_token": sell_address,
                        "sell_token_symbol": sell_symbol,
                        "buy_token": buy_info.address,
                        "buy_token_symbol": buy_symbol,
                        "amount": params.amount,
                        "network": network_str,
                    }));
                }

                // Rogue mode: broadcast approval inline, wait for confirmation
                log::info!(
                    "[swap_token] Rogue mode: broadcasting approval {}",
                    approval_uuid
                );

                let broadcast_tool = BroadcastWeb3TxTool::new();
                let broadcast_result = broadcast_tool
                    .execute(json!({"uuid": approval_uuid}), context)
                    .await;

                if !broadcast_result.success {
                    return ToolResult::error(format!(
                        "Approval broadcast failed: {}",
                        broadcast_result.content
                    ));
                }

                log::info!("[swap_token] Approval confirmed, continuing with swap");
            }
        }

        // ─── Step 8: Fetch 0x quote ────────────────────────────────────────────

        log::info!("[swap_token] Fetching swap quote...");

        let quote = match fetch_x402_preset("swap_quote", &network_str, context).await {
            Ok(q) => q,
            Err(e) => return ToolResult::error(format!("Quote fetch failed: {}", e)),
        };

        context.set_register("swap_quote", quote.clone(), "swap_token");

        log::info!(
            "[swap_token] Got swap quote (keys: {:?})",
            quote.as_object().map(|o| o.keys().collect::<Vec<_>>())
        );

        // ─── Step 9: Decode calldata ───────────────────────────────────────────

        if let Err(e) = decode_swap_calldata(&quote, context) {
            return ToolResult::error(format!("Calldata decode failed: {}", e));
        }

        log::info!("[swap_token] Calldata decoded, registers set");

        // ─── Step 10: Execute swap ─────────────────────────────────────────────

        let swap_preset = match get_web3_preset("swap_execute") {
            Some(p) => p,
            None => {
                return ToolResult::error(
                    "swap_execute preset not found. Ensure the swap skill is loaded.",
                )
            }
        };

        // Read contract from register
        let swap_contract = match context.registers.get("swap_contract") {
            Some(v) => match v.as_str() {
                Some(s) => s.to_string(),
                None => v.to_string().trim_matches('"').to_string(),
            },
            None => {
                return ToolResult::error(
                    "swap_contract register not set after calldata decode",
                )
            }
        };

        // Read params from registers
        let mut resolved_params = Vec::new();
        for reg_key in &swap_preset.params_registers {
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
                        "swap_execute requires register '{}' but it's not set",
                        reg_key
                    ))
                }
            }
        }

        // Read value from register
        let swap_value = match swap_preset.value_register.as_ref() {
            Some(val_reg) => match context.registers.get(val_reg) {
                Some(v) => match v.as_str() {
                    Some(s) => s.to_string(),
                    None => v.to_string().trim_matches('"').to_string(),
                },
                None => "0".to_string(),
            },
            None => "0".to_string(),
        };

        let abis_dir = default_abis_dir();

        log::info!(
            "[swap_token] Executing swap: {}::{}() on {} contract={}",
            swap_preset.abi, swap_preset.function, network_str, swap_contract
        );

        let swap_result = execute_resolved_call(
            &abis_dir,
            &swap_preset.abi,
            &swap_contract,
            &swap_preset.function,
            &resolved_params,
            &swap_value,
            false,
            &network,
            context,
            Some("swap_execute"),
        )
        .await;

        if !swap_result.success {
            return swap_result;
        }

        // Extract the UUID and enrich the response
        let swap_uuid = swap_result
            .metadata
            .as_ref()
            .and_then(|m| m.get("uuid"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        ToolResult::success(format!(
            "SWAP QUEUED\n\n\
            {} {} → {} on {}\n\n\
            Transaction UUID: {}\n\n\
            Next: broadcast_web3_tx with uuid \"{}\"",
            params.amount, sell_symbol, buy_symbol, network_name, swap_uuid, swap_uuid
        ))
        .with_metadata(json!({
            "status": "swap_queued",
            "uuid": swap_uuid,
            "sell_token": sell_address,
            "sell_token_symbol": sell_symbol,
            "buy_token": buy_info.address,
            "buy_token_symbol": buy_symbol,
            "amount": params.amount,
            "raw_amount": raw_amount,
            "network": network_str,
            "swap_contract": swap_contract,
        }))
    }
}

// ─── Helper: ERC-20 allowance check ────────────────────────────────────────────

async fn check_erc20_allowance(
    token_address: &str,
    owner: &str,
    spender: &str,
    network: &str,
    context: &ToolContext,
    wallet_provider: &std::sync::Arc<dyn crate::wallet::WalletProvider>,
) -> Result<U256, String> {
    let abis_dir = default_abis_dir();
    let abi_file = load_abi(&abis_dir, "erc20")?;
    let abi = parse_abi(&abi_file)?;
    let function = find_function_with_params(&abi, "allowance", 2)?;

    let calldata = encode_call(function, &[json!(owner), json!(spender)])?;

    let contract: Address = token_address
        .parse()
        .map_err(|_| format!("Invalid token address: {}", token_address))?;

    let rpc_config = resolve_rpc_from_context(&context.extra, network);

    let result_bytes =
        call_function(network, contract, calldata, &rpc_config, wallet_provider).await?;

    // Decode as uint256
    let decoded = web3::decode_return(function, &result_bytes)?;

    // decode_return for uint256 returns json!(n.to_string())
    let allowance_str = match decoded.as_str() {
        Some(s) => s.to_string(),
        None => decoded.to_string().trim_matches('"').to_string(),
    };

    U256::from_dec_str(&allowance_str)
        .map_err(|e| format!("Failed to parse allowance '{}': {}", allowance_str, e))
}

// ─── Helper: Decode swap calldata from quote ───────────────────────────────────

fn decode_swap_calldata(quote: &Value, context: &ToolContext) -> Result<(), String> {
    // Extract fields from the swap quote
    let data_hex = quote
        .get("data")
        .and_then(|d| d.as_str())
        .ok_or("No 'data' field in swap quote")?;

    let contract_address = quote
        .get("to")
        .and_then(|t| t.as_str())
        .ok_or("No 'to' field in swap quote")?;

    let tx_value = quote
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("0");

    // Load 0x_settler ABI
    let abis_dir = default_abis_dir();
    let abi_file = load_abi(&abis_dir, "0x_settler")?;
    let abi = parse_abi(&abi_file)?;

    // Decode calldata bytes
    let hex_str = data_hex.strip_prefix("0x").unwrap_or(data_hex);
    let calldata_bytes =
        hex::decode(hex_str).map_err(|e| format!("Invalid hex calldata: {}", e))?;

    if calldata_bytes.len() < 4 {
        return Err("Calldata too short — must have at least 4 bytes for function selector".to_string());
    }

    // Match function selector
    let selector = &calldata_bytes[0..4];
    let mut found = false;

    for func in abi.functions() {
        if func.short_signature() != selector {
            continue;
        }

        // Decode parameters
        let param_types: Vec<ParamType> =
            func.inputs.iter().map(|p| p.kind.clone()).collect();
        let tokens = ethers::abi::decode(&param_types, &calldata_bytes[4..])
            .map_err(|e| format!("Failed to decode params for '{}': {}", func.name, e))?;
        let decoded_params: Vec<Value> = tokens.iter().map(|t| token_to_value(t)).collect();

        // Set registers
        context.set_register("swap_contract", json!(contract_address), "swap_token");
        context.set_register("swap_value", json!(tx_value), "swap_token");

        for (i, param) in decoded_params.iter().enumerate() {
            context.set_register(
                &format!("swap_param_{}", i),
                param.clone(),
                "swap_token",
            );
        }

        log::info!(
            "[swap_token] Decoded {}() with {} params, contract={}, value={}",
            func.name, decoded_params.len(), contract_address, tx_value
        );

        found = true;
        break;
    }

    if !found {
        let selector_hex = format!("0x{}", hex::encode(selector));
        return Err(format!(
            "No function found with selector {} in 0x_settler ABI",
            selector_hex
        ));
    }

    Ok(())
}
