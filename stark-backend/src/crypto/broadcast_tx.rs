//! Broadcast a queued transaction to the network.

use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::tx_queue::{QueuedTxStatus, TxQueueManager};
use crate::wallet::WalletProvider;
use crate::x402::X402EvmRpc;
use crate::rpc_config::resolve_rpc;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

use super::send_eth::format_eth;

pub async fn broadcast_tx(
    uuid: &str,
    tx_queue: &Arc<TxQueueManager>,
    wallet_provider: &Arc<dyn WalletProvider>,
    broadcaster: &Arc<EventBroadcaster>,
    _db: Option<&Arc<crate::db::Database>>,
) -> Result<Value, String> {
    let queued_tx = tx_queue.get(uuid)
        .ok_or_else(|| format!("Transaction with UUID '{}' not found", uuid))?;

    // Validate status
    match queued_tx.status {
        QueuedTxStatus::Pending => {},
        QueuedTxStatus::Broadcasting => return Err(format!("Transaction {} is already being broadcast", uuid)),
        QueuedTxStatus::Broadcast | QueuedTxStatus::Confirmed => {
            return Err(format!("Transaction {} already broadcast. Hash: {}", uuid, queued_tx.tx_hash.as_deref().unwrap_or("unknown")));
        },
        QueuedTxStatus::Failed => return Err(format!("Transaction {} previously failed: {}", uuid, queued_tx.error.as_deref().unwrap_or("unknown"))),
        QueuedTxStatus::Expired => return Err(format!("Transaction {} has expired", uuid)),
    }

    tx_queue.mark_broadcasting(uuid);

    let rpc_config = resolve_rpc(&queued_tx.network);
    let rpc = match X402EvmRpc::new_with_wallet_provider(
        wallet_provider.clone(), &queued_tx.network, Some(rpc_config.url), rpc_config.use_x402,
    ) {
        Ok(r) => r,
        Err(e) => {
            tx_queue.mark_failed(uuid, &e);
            return Err(format!("Failed to initialize RPC: {}", e));
        }
    };

    let signed_tx_bytes = match hex::decode(queued_tx.signed_tx_hex.trim_start_matches("0x")) {
        Ok(b) => b,
        Err(e) => {
            let error = format!("Invalid signed transaction hex: {}", e);
            tx_queue.mark_failed(uuid, &error);
            return Err(error);
        }
    };

    let tx_hash = match rpc.send_raw_transaction(&signed_tx_bytes).await {
        Ok(h) => h,
        Err(e) => {
            tx_queue.mark_failed(uuid, &e);
            return Err(format!("Broadcast failed: {}", e));
        }
    };

    let tx_hash_str = format!("{:?}", tx_hash);
    let explorer_base = queued_tx.get_explorer_base_url();
    let explorer_url = format!("{}/{}", explorer_base, tx_hash_str);

    tx_queue.mark_broadcast(uuid, &tx_hash_str, &explorer_url, "api");

    broadcaster.broadcast(GatewayEvent::tx_pending(
        0, &tx_hash_str, &queued_tx.network, &explorer_url,
    ));

    // Wait for receipt
    let receipt = match rpc.wait_for_receipt(tx_hash, Duration::from_secs(120)).await {
        Ok(r) => r,
        Err(e) => {
            return Ok(json!({
                "uuid": uuid,
                "tx_hash": tx_hash_str,
                "network": queued_tx.network,
                "explorer_url": explorer_url,
                "status": "broadcast",
                "warning": format!("Confirmation timed out: {}", e),
            }));
        }
    };

    let status = if receipt.status == Some(ethers::types::U64::from(1)) {
        tx_queue.mark_confirmed(uuid);
        "confirmed"
    } else {
        tx_queue.mark_failed(uuid, "Transaction reverted on-chain");
        "reverted"
    };

    broadcaster.broadcast(GatewayEvent::tx_confirmed(
        0, &tx_hash_str, &queued_tx.network, status,
    ));

    Ok(json!({
        "uuid": uuid,
        "tx_hash": tx_hash_str,
        "status": status,
        "network": queued_tx.network,
        "explorer_url": explorer_url,
        "from": queued_tx.from,
        "to": queued_tx.to,
        "value": queued_tx.value,
        "value_formatted": format_eth(&queued_tx.value),
        "gas_used": receipt.gas_used.map(|g| g.to_string()),
        "block_number": receipt.block_number.map(|b| b.as_u64()),
    }))
}
