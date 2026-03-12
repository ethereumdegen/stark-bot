//! Send native ETH transfer — sign and queue.

use crate::tx_queue::{QueuedTransaction, TxQueueManager};
use crate::wallet::WalletProvider;
use crate::x402::X402EvmRpc;
use crate::rpc_config::resolve_rpc;
use ethers::prelude::*;
use ethers::types::transaction::eip1559::Eip1559TransactionRequest;
use ethers::types::transaction::eip2718::TypedTransaction;
use serde_json::{json, Value};
use std::sync::Arc;

fn get_chain_id(network: &str) -> u64 {
    match network {
        "mainnet" => 1,
        "polygon" => 137,
        "arbitrum" => 42161,
        "optimism" => 10,
        _ => 8453,
    }
}

/// Parse decimal or hex strings to U256
pub fn parse_u256(s: &str) -> Result<U256, String> {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        U256::from_str_radix(&s[2..], 16)
            .map_err(|e| format!("Invalid hex: {} - {}", s, e))
    } else {
        U256::from_dec_str(s)
            .map_err(|e| format!("Invalid decimal: {} - {}", s, e))
    }
}

/// Format wei as human-readable ETH
pub fn format_eth(wei: &str) -> String {
    if let Ok(w) = wei.parse::<u128>() {
        let eth = w as f64 / 1e18;
        if eth >= 0.0001 {
            format!("{:.6} ETH", eth)
        } else {
            format!("{} wei", wei)
        }
    } else {
        format!("{} wei", wei)
    }
}

pub async fn send_eth(
    network: &str,
    to: &str,
    amount_raw: &str,
    wallet_provider: &Arc<dyn WalletProvider>,
    tx_queue: &Arc<TxQueueManager>,
) -> Result<Value, String> {
    // Validate inputs
    if !to.starts_with("0x") || to.len() != 42 {
        return Err(format!("Invalid recipient address: {}", to));
    }
    if to.to_lowercase() == "0x0000000000000000000000000000000000000000" {
        return Err("Cannot send to zero address".to_string());
    }
    if !amount_raw.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("Invalid amount_raw (must be numeric): {}", amount_raw));
    }

    let rpc_config = resolve_rpc(network);
    let rpc = X402EvmRpc::new_with_wallet_provider(
        wallet_provider.clone(), network, Some(rpc_config.url), rpc_config.use_x402,
    )?;

    let chain_id = get_chain_id(network);
    let from_str = wallet_provider.get_address();
    let from_address: Address = from_str.parse().map_err(|_| format!("Invalid wallet address: {}", from_str))?;
    let to_address: Address = to.parse().map_err(|_| format!("Invalid 'to' address: {}", to))?;
    let tx_value: U256 = parse_u256(amount_raw)?;

    let nonce = rpc.get_transaction_count(from_address).await?;
    let gas = U256::from(21000u64);
    let (max_fee, priority_fee) = rpc.estimate_eip1559_fees().await?;

    let tx = Eip1559TransactionRequest::new()
        .from(from_address)
        .to(to_address)
        .value(tx_value)
        .nonce(nonce)
        .gas(gas)
        .max_fee_per_gas(max_fee)
        .max_priority_fee_per_gas(priority_fee)
        .chain_id(chain_id);

    let typed_tx: TypedTransaction = tx.into();
    let signature = wallet_provider.sign_transaction(&typed_tx).await
        .map_err(|e| format!("Failed to sign transaction: {}", e))?;

    let signed_tx = typed_tx.rlp_signed(&signature);
    let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));

    let uuid = uuid::Uuid::new_v4().to_string();
    let queued_tx = QueuedTransaction::new(
        uuid.clone(), network.to_string(), from_str.clone(), to.to_string(),
        tx_value.to_string(), "0x".to_string(), gas.to_string(),
        max_fee.to_string(), priority_fee.to_string(), nonce.as_u64(),
        signed_tx_hex, None,
    );
    tx_queue.queue(queued_tx);

    log::info!("[send_eth] Transaction queued with UUID: {}", uuid);

    Ok(json!({
        "uuid": uuid,
        "status": "queued",
        "network": network,
        "from": from_str,
        "to": to,
        "value": tx_value.to_string(),
        "value_formatted": format_eth(&tx_value.to_string()),
        "nonce": nonce.as_u64(),
        "gas_limit": gas.to_string(),
    }))
}
