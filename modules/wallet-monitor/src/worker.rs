//! Background worker for wallet monitoring.
//!
//! Polls Alchemy Enhanced APIs every N seconds for new activity on watched wallets.
//! Detects swaps, estimates USD values, and sends HTTP callbacks for large trades.

use crate::alchemy;
use crate::db::Db;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use wallet_monitor_types::{LargeTradeAlert, WatchlistEntry};

type PriceCache = HashMap<String, (f64, std::time::Instant)>;

const PRICE_CACHE_TTL_SECS: u64 = 60;

pub async fn run_worker(
    db: Arc<Db>,
    api_key: String,
    poll_interval_secs: u64,
    alert_callback_url: Option<String>,
    last_tick_at: Arc<Mutex<Option<String>>>,
) {
    log::info!("[WALLET_MONITOR] Worker started (poll interval: {}s)", poll_interval_secs);
    let client = reqwest::Client::new();
    let price_cache: Arc<Mutex<PriceCache>> = Arc::new(Mutex::new(HashMap::new()));

    // First poll quickly after startup, then use the configured interval
    let mut first_run = true;

    loop {
        let delay = if first_run { 5 } else { poll_interval_secs };
        tokio::time::sleep(Duration::from_secs(delay)).await;
        first_run = false;

        match wallet_monitor_tick(&db, &client, &api_key, &price_cache, &alert_callback_url).await
        {
            Ok(_) => {
                let now = chrono::Utc::now().to_rfc3339();
                *last_tick_at.lock().await = Some(now);
            }
            Err(e) => {
                log::error!("[WALLET_MONITOR] Tick error: {}", e);
            }
        }
    }
}

async fn wallet_monitor_tick(
    db: &Db,
    client: &reqwest::Client,
    api_key: &str,
    price_cache: &Arc<Mutex<PriceCache>>,
    alert_callback_url: &Option<String>,
) -> Result<(), String> {
    let watchlist = db
        .list_active_watchlist()
        .map_err(|e| format!("Failed to list watchlist: {}", e))?;

    if watchlist.is_empty() {
        return Ok(());
    }

    log::debug!(
        "[WALLET_MONITOR] Tick: checking {} wallets",
        watchlist.len()
    );

    let mut total_new = 0usize;
    let mut alerts: Vec<LargeTradeAlert> = Vec::new();

    for entry in &watchlist {
        match process_wallet(db, client, api_key, entry, price_cache).await {
            Ok((new_count, entry_alerts)) => {
                total_new += new_count;
                alerts.extend(entry_alerts);
            }
            Err(e) => {
                log::warn!(
                    "[WALLET_MONITOR] Error processing wallet {} ({}): {}",
                    entry.address,
                    entry.chain,
                    e
                );
            }
        }
    }

    // Send alerts via HTTP callback if configured
    if !alerts.is_empty() {
        if let Some(url) = alert_callback_url {
            for alert in &alerts {
                if let Err(e) = client.post(url).json(alert).send().await {
                    log::warn!("[WALLET_MONITOR] Failed to send alert callback: {}", e);
                }
            }
        }
        log::warn!(
            "[WALLET_MONITOR] LARGE TRADE ALERTS: {}",
            alerts
                .iter()
                .map(|a| a.message.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }

    if total_new > 0 {
        log::info!(
            "[WALLET_MONITOR] Tick complete: {} new transactions, {} large trades",
            total_new,
            alerts.len()
        );
    }

    Ok(())
}

/// How many blocks to look back on first run (~2 hours on mainnet, ~10 min on Base).
const FIRST_RUN_LOOKBACK_BLOCKS: i64 = 500;

async fn process_wallet(
    db: &Db,
    client: &reqwest::Client,
    api_key: &str,
    entry: &WatchlistEntry,
    price_cache: &Arc<Mutex<PriceCache>>,
) -> Result<(usize, Vec<LargeTradeAlert>), String> {
    let from_block = match entry.last_checked_block {
        Some(b) => Some(b + 1),
        None => {
            // First run: start from a recent block instead of scanning full history
            let latest = alchemy::get_block_number(client, &entry.chain, api_key).await?;
            let start = (latest - FIRST_RUN_LOOKBACK_BLOCKS).max(0);
            log::info!(
                "[WALLET_MONITOR] First run for {} on {}: starting from block {} (latest: {})",
                entry.address, entry.chain, start, latest
            );
            Some(start)
        }
    };

    let (outgoing, incoming) = tokio::join!(
        alchemy::get_asset_transfers(client, &entry.chain, api_key, &entry.address, from_block, "from"),
        alchemy::get_asset_transfers(client, &entry.chain, api_key, &entry.address, from_block, "to"),
    );

    let outgoing = outgoing?;
    let incoming = incoming?;

    if outgoing.is_empty() && incoming.is_empty() {
        if let Ok(latest) = alchemy::get_block_number(client, &entry.chain, api_key).await {
            let _ = db.update_watchlist_cursor(entry.id, latest);
        }
        return Ok((0, Vec::new()));
    }

    // Group all transfers by tx_hash for swap detection
    let mut tx_groups: HashMap<String, Vec<(alchemy::AssetTransfer, &str)>> = HashMap::new();
    for t in &outgoing {
        tx_groups
            .entry(t.hash.clone())
            .or_default()
            .push((t.clone(), "outgoing"));
    }
    for t in &incoming {
        tx_groups
            .entry(t.hash.clone())
            .or_default()
            .push((t.clone(), "incoming"));
    }

    let mut new_count = 0usize;
    let mut max_block: i64 = entry.last_checked_block.unwrap_or(0);
    let mut alerts = Vec::new();

    for (tx_hash, transfers) in &tx_groups {
        let block_number = transfers
            .first()
            .map(|(t, _)| alchemy::parse_block_number(&t.block_num))
            .unwrap_or(0);
        if block_number > max_block {
            max_block = block_number;
        }

        let block_timestamp = transfers
            .first()
            .and_then(|(t, _)| t.metadata.as_ref())
            .and_then(|m| m.block_timestamp.as_deref());

        let has_outgoing_erc20 = transfers
            .iter()
            .any(|(t, dir)| *dir == "outgoing" && t.category == "erc20");
        let has_incoming_erc20 = transfers
            .iter()
            .any(|(t, dir)| *dir == "incoming" && t.category == "erc20");
        let is_swap = has_outgoing_erc20 && has_incoming_erc20;

        let (swap_from_token, swap_from_amount, swap_to_token, swap_to_amount) = if is_swap {
            let out_erc20 = transfers
                .iter()
                .find(|(t, dir)| *dir == "outgoing" && t.category == "erc20");
            let in_erc20 = transfers
                .iter()
                .find(|(t, dir)| *dir == "incoming" && t.category == "erc20");
            (
                out_erc20.and_then(|(t, _)| t.asset.clone()),
                out_erc20.and_then(|(t, _)| t.value.map(|v| format!("{}", v))),
                in_erc20.and_then(|(t, _)| t.asset.clone()),
                in_erc20.and_then(|(t, _)| t.value.map(|v| format!("{}", v))),
            )
        } else {
            (None, None, None, None)
        };

        for (transfer, direction) in transfers {
            let activity_type = if is_swap {
                "swap".to_string()
            } else {
                match transfer.category.as_str() {
                    "external" => "eth_transfer".to_string(),
                    "internal" => "internal".to_string(),
                    "erc20" => "erc20_transfer".to_string(),
                    other => other.to_string(),
                }
            };

            let amount_formatted = transfer.value.map(|v| format!("{}", v));

            let usd_value = estimate_usd_value(
                client,
                transfer.asset.as_deref(),
                transfer.value,
                &entry.chain,
                price_cache,
            )
            .await;

            let is_large_trade =
                usd_value.map_or(false, |v| v >= entry.large_trade_threshold_usd);

            let to_address = transfer.to.as_deref().unwrap_or("0x0");
            let raw_data_str;
            let raw_data = if is_swap || is_large_trade {
                raw_data_str = serde_json::to_string(transfer).unwrap_or_default();
                Some(raw_data_str.as_str())
            } else {
                None
            };

            let result = db.insert_activity(
                entry.id,
                &entry.chain,
                tx_hash,
                block_number,
                block_timestamp,
                &transfer.from,
                to_address,
                &activity_type,
                transfer.asset.as_deref(),
                transfer
                    .raw_contract
                    .as_ref()
                    .and_then(|c| c.address.as_deref()),
                transfer
                    .raw_contract
                    .as_ref()
                    .and_then(|c| c.value.as_deref()),
                amount_formatted.as_deref(),
                usd_value,
                is_large_trade,
                swap_from_token.as_deref(),
                swap_from_amount.as_deref(),
                swap_to_token.as_deref(),
                swap_to_amount.as_deref(),
                raw_data,
            );

            match result {
                Ok(id) if id > 0 => {
                    new_count += 1;
                    if is_large_trade {
                        let label = entry.label.as_deref().unwrap_or(&entry.address);
                        let usd_str = usd_value
                            .map(|v| format!("${:.0}", v))
                            .unwrap_or_else(|| "unknown".to_string());

                        let message = if is_swap {
                            format!(
                                "**{}** ({}) swapped {} {} -> {} {} ({}) on {} [tx: {}]",
                                label,
                                &entry.address[..10],
                                swap_from_amount.as_deref().unwrap_or("?"),
                                swap_from_token.as_deref().unwrap_or("?"),
                                swap_to_amount.as_deref().unwrap_or("?"),
                                swap_to_token.as_deref().unwrap_or("?"),
                                usd_str,
                                entry.chain,
                                tx_hash
                            )
                        } else {
                            let asset = transfer.asset.as_deref().unwrap_or("ETH");
                            let amt = amount_formatted.as_deref().unwrap_or("?");
                            let dir_str = if *direction == "outgoing" {
                                "sent"
                            } else {
                                "received"
                            };
                            format!(
                                "**{}** ({}) {} {} {} ({}) on {} [tx: {}]",
                                label,
                                &entry.address[..10],
                                dir_str,
                                amt,
                                asset,
                                usd_str,
                                entry.chain,
                                tx_hash
                            )
                        };

                        alerts.push(LargeTradeAlert {
                            watchlist_id: entry.id,
                            address: entry.address.clone(),
                            label: entry.label.clone(),
                            chain: entry.chain.clone(),
                            tx_hash: tx_hash.clone(),
                            activity_type: activity_type.clone(),
                            usd_value,
                            asset_symbol: transfer.asset.clone(),
                            amount_formatted: amount_formatted.clone(),
                            swap_from_token: swap_from_token.clone(),
                            swap_from_amount: swap_from_amount.clone(),
                            swap_to_token: swap_to_token.clone(),
                            swap_to_amount: swap_to_amount.clone(),
                            message,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    if max_block > entry.last_checked_block.unwrap_or(0) {
        let _ = db.update_watchlist_cursor(entry.id, max_block);
    }

    Ok((new_count, alerts))
}

async fn estimate_usd_value(
    client: &reqwest::Client,
    asset: Option<&str>,
    value: Option<f64>,
    chain: &str,
    price_cache: &Arc<Mutex<PriceCache>>,
) -> Option<f64> {
    let value = value?;
    if value == 0.0 {
        return Some(0.0);
    }

    let symbol = asset.unwrap_or("ETH").to_uppercase();

    match symbol.as_str() {
        "USDC" | "USDT" | "DAI" | "BUSD" | "TUSD" | "FRAX" => return Some(value),
        _ => {}
    }

    {
        let cache = price_cache.lock().await;
        if let Some((price, ts)) = cache.get(&symbol) {
            if ts.elapsed() < Duration::from_secs(PRICE_CACHE_TTL_SECS) {
                return Some(value * price);
            }
        }
    }

    let dex_chain = match chain {
        "base" => "base",
        _ => "ethereum",
    };
    let url = format!(
        "https://api.dexscreener.com/latest/dex/search?q={}",
        symbol
    );
    let price = match client.get(&url).send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                json["pairs"]
                    .as_array()
                    .and_then(|pairs| {
                        pairs.iter().find(|p| {
                            p["chainId"].as_str() == Some(dex_chain)
                                && p["baseToken"]["symbol"]
                                    .as_str()
                                    .map(|s| s.to_uppercase() == symbol)
                                    .unwrap_or(false)
                        })
                    })
                    .and_then(|p| p["priceUsd"].as_str())
                    .and_then(|p| p.parse::<f64>().ok())
            } else {
                None
            }
        }
        Err(_) => None,
    };

    if let Some(price) = price {
        let mut cache = price_cache.lock().await;
        cache.insert(symbol, (price, std::time::Instant::now()));
        Some(value * price)
    } else {
        let fallback_price = match symbol.as_str() {
            "ETH" | "WETH" => Some(2500.0),
            _ => None,
        };
        fallback_price.map(|p| value * p)
    }
}
