//! Bridge USDC cross-chain via Across Protocol.

use crate::tx_queue::{QueuedTransaction, TxQueueManager};
use crate::wallet::WalletProvider;
use crate::x402::X402EvmRpc;
use crate::rpc_config::resolve_rpc;
use ethers::prelude::*;
use ethers::types::transaction::eip1559::Eip1559TransactionRequest;
use ethers::types::transaction::eip2718::TypedTransaction;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

const ACROSS_API_URL: &str = "https://app.across.to/api";

const CHAIN_CONFIG: &[(&str, u64, &str)] = &[
    ("ethereum", 1, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
    ("mainnet", 1, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
    ("base", 8453, "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
    ("polygon", 137, "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359"),
    ("arbitrum", 42161, "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"),
    ("optimism", 10, "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85"),
];

fn get_chain_id(chain: &str) -> Result<u64, String> {
    CHAIN_CONFIG.iter()
        .find(|(name, _, _)| *name == chain.to_lowercase())
        .map(|(_, id, _)| *id)
        .ok_or_else(|| format!("Unsupported chain: {}", chain))
}

fn get_usdc_address(chain: &str) -> Result<&'static str, String> {
    CHAIN_CONFIG.iter()
        .find(|(name, _, _)| *name == chain.to_lowercase())
        .map(|(_, _, addr)| *addr)
        .ok_or_else(|| format!("No USDC address for chain: {}", chain))
}

fn chain_to_network(chain: &str) -> &str {
    match chain.to_lowercase().as_str() {
        "ethereum" | "mainnet" => "mainnet",
        "base" => "base",
        "polygon" => "polygon",
        "arbitrum" => "arbitrum",
        "optimism" => "optimism",
        _ => chain,
    }
}

#[derive(Debug, Deserialize)]
struct AcrossSwapResponse {
    #[serde(rename = "approvalTxns", default)]
    approval_txns: Vec<AcrossTxn>,
    #[serde(rename = "swapTx")]
    swap_tx: Option<AcrossSwapTx>,
    #[serde(rename = "expectedOutputAmount")]
    expected_output_amount: Option<String>,
    #[serde(rename = "expectedFillTime")]
    expected_fill_time: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AcrossSwapTx {
    to: String,
    data: String,
}

#[derive(Debug, Deserialize)]
struct AcrossTxn {
    to: String,
    data: String,
}

pub async fn bridge_usdc(
    amount: &str,
    from_chain: &str,
    to_chain: &str,
    recipient: Option<&str>,
    wallet_provider: &Arc<dyn WalletProvider>,
    tx_queue: &Arc<TxQueueManager>,
) -> Result<Value, String> {
    let from_chain_id = get_chain_id(from_chain)?;
    let to_chain_id = get_chain_id(to_chain)?;
    if from_chain_id == to_chain_id {
        return Err("Source and destination chains must be different".to_string());
    }

    let usdc_from = get_usdc_address(from_chain)?;
    let usdc_to = get_usdc_address(to_chain)?;
    let wallet_address = wallet_provider.get_address();
    let recipient = recipient.unwrap_or(&wallet_address);

    let parsed: f64 = amount.parse().map_err(|_| format!("Invalid amount: {}", amount))?;
    if parsed <= 0.0 { return Err("Amount must be positive".to_string()); }
    let amount_raw = (parsed * 1_000_000.0).round() as u64;

    let client = crate::http::shared_client();
    let url = format!(
        "{}/swap/approval?tradeType=exactInput&amount={}&inputToken={}&originChainId={}&outputToken={}&destinationChainId={}&depositor={}&recipient={}&slippage=0.005",
        ACROSS_API_URL, amount_raw, usdc_from, from_chain_id, usdc_to, to_chain_id, wallet_address, recipient
    );

    let response = client.get(&url).send().await.map_err(|e| format!("Across API error: {}", e))?;
    let status = response.status();
    let response_text = response.text().await.map_err(|e| format!("Failed to read response: {}", e))?;
    if !status.is_success() {
        return Err(format!("Across API error ({}): {}", status, response_text));
    }

    let across: AcrossSwapResponse = serde_json::from_str(&response_text)
        .map_err(|e| format!("Failed to parse Across response: {}", e))?;
    let swap_tx = across.swap_tx.ok_or("Across API did not return a swap transaction")?;

    let network = chain_to_network(from_chain);
    let rpc_config = resolve_rpc(network);
    let mut queued_uuids = Vec::new();

    // Queue approval txns
    for approval in &across.approval_txns {
        let uuid = sign_and_queue(
            from_chain_id, network, &approval.to, &approval.data, "0",
            wallet_provider, tx_queue, &rpc_config,
        ).await?;
        queued_uuids.push(("approval".to_string(), uuid));
    }

    // Queue bridge txn
    let bridge_uuid = sign_and_queue(
        from_chain_id, network, &swap_tx.to, &swap_tx.data, "0",
        wallet_provider, tx_queue, &rpc_config,
    ).await?;
    queued_uuids.push(("bridge".to_string(), bridge_uuid.clone()));

    Ok(json!({
        "status": "queued",
        "from_chain": from_chain,
        "to_chain": to_chain,
        "amount": amount,
        "amount_raw": amount_raw.to_string(),
        "expected_output": across.expected_output_amount,
        "estimated_fill_time": across.expected_fill_time,
        "recipient": recipient,
        "queued_transactions": queued_uuids,
    }))
}

async fn sign_and_queue(
    chain_id: u64,
    network: &str,
    to: &str,
    data_hex: &str,
    value: &str,
    wallet_provider: &Arc<dyn WalletProvider>,
    tx_queue: &Arc<TxQueueManager>,
    rpc_config: &crate::rpc_config::ResolvedRpcConfig,
) -> Result<String, String> {
    let rpc = X402EvmRpc::new_with_wallet_provider(
        wallet_provider.clone(), network, Some(rpc_config.url.clone()), rpc_config.use_x402,
    )?;

    let from_str = wallet_provider.get_address();
    let from_address: Address = from_str.parse().map_err(|_| format!("Invalid wallet: {}", from_str))?;
    let to_address: Address = to.parse().map_err(|_| format!("Invalid to: {}", to))?;
    let tx_value = U256::from_dec_str(value).unwrap_or_default();

    let data = hex::decode(data_hex.strip_prefix("0x").unwrap_or(data_hex))
        .map_err(|e| format!("Invalid data hex: {}", e))?;

    let nonce = rpc.get_transaction_count(from_address).await?;
    let gas = rpc.estimate_gas(from_address, to_address, &data, tx_value).await
        .map_err(|e| format!("Gas estimation failed: {}", e))?;
    let gas = gas * U256::from(130) / U256::from(100);
    let (max_fee, priority_fee) = rpc.estimate_eip1559_fees().await?;

    let tx = Eip1559TransactionRequest::new()
        .from(from_address).to(to_address).value(tx_value)
        .data(data.clone()).nonce(nonce).gas(gas)
        .max_fee_per_gas(max_fee).max_priority_fee_per_gas(priority_fee)
        .chain_id(chain_id);

    let typed_tx: TypedTransaction = tx.into();
    let signature = wallet_provider.sign_transaction(&typed_tx).await
        .map_err(|e| format!("Sign failed: {}", e))?;
    let signed_tx_hex = format!("0x{}", hex::encode(typed_tx.rlp_signed(&signature)));

    let uuid = uuid::Uuid::new_v4().to_string();
    tx_queue.queue(QueuedTransaction::new(
        uuid.clone(), network.to_string(), from_str, format!("{:?}", to_address),
        tx_value.to_string(), format!("0x{}", hex::encode(&data)),
        gas.to_string(), max_fee.to_string(), priority_fee.to_string(),
        nonce.as_u64(), signed_tx_hex, None,
    ));
    Ok(uuid)
}
