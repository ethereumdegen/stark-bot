//! Helper functions: get_balance, get_address, decode_calldata.

use crate::wallet::WalletProvider;
use crate::x402::X402EvmRpc;
use crate::rpc_config::resolve_rpc;
use crate::web3::{default_abis_dir, load_abi, parse_abi};
use ethers::abi::ParamType;
use ethers::prelude::*;
use serde_json::{json, Value};
use std::sync::Arc;

/// Get native ETH balance for an address on a network.
pub async fn get_balance(
    address: &str,
    network: &str,
    wallet_provider: &Arc<dyn WalletProvider>,
) -> Result<String, String> {
    let rpc_config = resolve_rpc(network);
    let rpc = X402EvmRpc::new_with_wallet_provider(
        wallet_provider.clone(), network, Some(rpc_config.url), rpc_config.use_x402,
    )?;

    let addr: Address = address.parse().map_err(|_| format!("Invalid address: {}", address))?;
    let balance = rpc.get_balance(addr).await?;
    Ok(balance.to_string())
}

/// Decode calldata using an ABI.
pub fn decode_calldata(data: &str, abi_name: &str) -> Result<Value, String> {
    let abis_dir = default_abis_dir();
    let abi_file = load_abi(&abis_dir, abi_name)?;
    let abi = parse_abi(&abi_file)?;

    let hex_str = data.strip_prefix("0x").unwrap_or(data);
    let calldata = hex::decode(hex_str).map_err(|e| format!("Invalid hex: {}", e))?;

    if calldata.len() < 4 {
        return Err("Calldata too short".to_string());
    }

    let selector = &calldata[0..4];
    for func in abi.functions() {
        if func.short_signature() != selector { continue; }

        let param_types: Vec<ParamType> = func.inputs.iter().map(|p| p.kind.clone()).collect();
        let tokens = ethers::abi::decode(&param_types, &calldata[4..])
            .map_err(|e| format!("Decode failed: {}", e))?;
        let params: Vec<Value> = tokens.iter().map(|t| crate::web3::token_to_value(t)).collect();

        return Ok(json!({
            "function": func.name,
            "params": params,
            "abi": abi_name,
        }));
    }

    Err(format!("No function found with selector 0x{} in {} ABI", hex::encode(selector), abi_name))
}
