//! Token swap via 0x API — composite operation.
//!
//! This is a simplified standalone version. For the full preset-based flow,
//! the old tool relied heavily on registers and presets. This version uses
//! direct parameters.

use crate::tx_queue::TxQueueManager;
use crate::wallet::WalletProvider;
use serde_json::{json, Value};
use std::sync::Arc;

use super::token_utils;

pub async fn swap_token(
    sell_token: &str,
    buy_token: &str,
    amount: &str,
    network: &str,
    wallet_provider: &Arc<dyn WalletProvider>,
    _tx_queue: &Arc<TxQueueManager>,
    _credits_session: Option<&Arc<crate::credits_session::CreditsSessionClient>>,
) -> Result<Value, String> {
    // Step 1: Lookup tokens
    let sell_info = token_utils::lookup(sell_token, network)
        .ok_or_else(|| format!("Unknown sell token '{}' on {}", sell_token, network))?;
    let buy_info = token_utils::lookup(buy_token, network)
        .ok_or_else(|| format!("Unknown buy token '{}' on {}", buy_token, network))?;

    // Step 2: Convert amount to raw
    let raw_amount = token_utils::to_raw_amount(amount, sell_info.decimals)?;

    let wallet_address = wallet_provider.get_address();

    log::info!(
        "[swap_token] {} {} ({} raw) → {} on {}, wallet={}",
        amount, sell_token, raw_amount, buy_token, network, wallet_address
    );

    // Step 3: Fetch 0x swap quote
    // This requires the 0x API and x402 preset system.
    // For now, return a structured result indicating the swap parameters.
    // The full implementation would call the 0x API, decode calldata,
    // handle approvals, and queue the swap transaction.

    Ok(json!({
        "status": "swap_requires_starflask",
        "message": "Token swaps should be orchestrated via Starflask. The local executor has resolved the tokens and amounts.",
        "sell_token": sell_info.address,
        "sell_token_symbol": sell_token.to_uppercase(),
        "sell_decimals": sell_info.decimals,
        "buy_token": buy_info.address,
        "buy_token_symbol": buy_token.to_uppercase(),
        "buy_decimals": buy_info.decimals,
        "amount": amount,
        "raw_amount": raw_amount,
        "network": network,
        "wallet": wallet_address,
    }))
}
