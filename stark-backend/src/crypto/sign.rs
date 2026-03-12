//! Transaction signing and message signing.

use crate::wallet::WalletProvider;
use crate::x402::X402EvmRpc;
use crate::rpc_config::resolve_rpc;
use ethers::prelude::*;
use ethers::types::transaction::eip1559::Eip1559TransactionRequest;
use ethers::types::transaction::eip2718::TypedTransaction;
use serde_json::{json, Value};
use std::sync::Arc;

/// Sign a message with EIP-191 personal_sign.
pub async fn sign_message(
    message: &str,
    wallet_provider: &Arc<dyn WalletProvider>,
) -> Result<Value, String> {
    let signature = wallet_provider.sign_message(message.as_bytes()).await
        .map_err(|e| format!("Failed to sign message: {}", e))?;
    let sig_hex = format!("0x{}", hex::encode(signature.to_vec()));
    let address = wallet_provider.get_address();

    Ok(json!({
        "signature": sig_hex,
        "address": address,
        "message": message,
    }))
}

/// Sign an EIP-1559 transaction without broadcasting.
pub async fn sign_raw_tx(
    to: &str,
    data: &str,
    value: &str,
    chain_id: u64,
    gas: Option<&str>,
    nonce: Option<u64>,
    wallet_provider: &Arc<dyn WalletProvider>,
) -> Result<Value, String> {
    let network = match chain_id {
        1 => "mainnet",
        137 => "polygon",
        42161 => "arbitrum",
        10 => "optimism",
        _ => "base",
    };

    let rpc_config = resolve_rpc(network);
    let rpc = X402EvmRpc::new_with_wallet_provider(
        wallet_provider.clone(), network, Some(rpc_config.url), rpc_config.use_x402,
    )?;

    let from_str = wallet_provider.get_address();
    let from_address: Address = from_str.parse().map_err(|_| format!("Invalid wallet: {}", from_str))?;
    let to_address: Address = to.parse().map_err(|_| format!("Invalid to: {}", to))?;
    let tx_value: U256 = value.parse().unwrap_or_default();

    let calldata: ethers::types::Bytes = hex::decode(data.trim_start_matches("0x"))
        .map_err(|e| format!("Invalid calldata hex: {}", e))?.into();

    let nonce = match nonce {
        Some(n) => U256::from(n),
        None => rpc.get_transaction_count(from_address).await?,
    };

    let (max_fee, priority_fee) = rpc.estimate_eip1559_fees().await?;
    let gas = match gas {
        Some(g) => g.parse().unwrap_or(U256::from(350_000u64)),
        None => U256::from(350_000u64),
    };

    let tx_req = Eip1559TransactionRequest::new()
        .from(from_address).to(to_address).value(tx_value)
        .nonce(nonce).gas(gas).max_fee_per_gas(max_fee)
        .max_priority_fee_per_gas(priority_fee).chain_id(chain_id)
        .data(calldata);

    let typed_tx: TypedTransaction = tx_req.into();
    let signature = wallet_provider.sign_transaction(&typed_tx).await
        .map_err(|e| format!("Failed to sign: {}", e))?;

    let signed_tx = typed_tx.rlp_signed(&signature);
    let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));
    let tx_hash = format!("0x{}", hex::encode(ethers::utils::keccak256(&signed_tx)));

    Ok(json!({
        "signed_tx": signed_tx_hex,
        "tx_hash": tx_hash,
        "from": from_str,
        "to": to,
        "nonce": nonce.as_u64(),
        "chain_id": chain_id,
    }))
}
