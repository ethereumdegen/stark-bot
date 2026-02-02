//! Broadcast a queued Web3 transaction
//!
//! Takes a UUID from web3_tx and broadcasts the signed transaction to the network.

use crate::gateway::protocol::GatewayEvent;
use crate::tools::builtin::web3_tx::Web3TxTool;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tx_queue::QueuedTxStatus;
use crate::x402::X402EvmRpc;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

/// Broadcast queued transaction tool
pub struct BroadcastWeb3TxTool {
    definition: ToolDefinition,
}

impl BroadcastWeb3TxTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "uuid".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "UUID of the queued transaction to broadcast (from web3_tx)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        BroadcastWeb3TxTool {
            definition: ToolDefinition {
                name: "broadcast_web3_tx".to_string(),
                description: "Broadcast a previously queued transaction by UUID. The transaction must have been queued using web3_tx. Returns the transaction hash and explorer URL.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["uuid".to_string()],
                },
                group: ToolGroup::Finance,
            },
        }
    }

    /// Get the private key from environment
    fn get_private_key() -> Result<String, String> {
        crate::config::burner_wallet_private_key()
            .ok_or_else(|| "BURNER_WALLET_BOT_PRIVATE_KEY not set".to_string())
    }
}

impl Default for BroadcastWeb3TxTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct BroadcastParams {
    uuid: String,
}

#[async_trait]
impl Tool for BroadcastWeb3TxTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        log::info!("[broadcast_web3_tx] Raw params: {}", params);

        let params: BroadcastParams = match serde_json::from_value(params.clone()) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get tx_queue
        let tx_queue = match &context.tx_queue {
            Some(q) => q,
            None => return ToolResult::error("Transaction queue not available. Contact administrator."),
        };

        // Get the queued transaction
        let queued_tx = match tx_queue.get(&params.uuid) {
            Some(tx) => tx,
            None => return ToolResult::error(format!(
                "Transaction with UUID '{}' not found. Use list_queued_web3_tx to see available transactions.",
                params.uuid
            )),
        };

        // Validate status is Pending
        match queued_tx.status {
            QueuedTxStatus::Pending => {},
            QueuedTxStatus::Broadcasting => {
                return ToolResult::error(format!(
                    "Transaction {} is already being broadcast. Please wait.",
                    params.uuid
                ));
            },
            QueuedTxStatus::Broadcast | QueuedTxStatus::Confirmed => {
                let tx_hash = queued_tx.tx_hash.as_deref().unwrap_or("unknown");
                let explorer_url = queued_tx.explorer_url.as_deref().unwrap_or("");
                return ToolResult::error(format!(
                    "Transaction {} has already been broadcast.\n\nTx Hash: {}\nExplorer: {}",
                    params.uuid, tx_hash, explorer_url
                ));
            },
            QueuedTxStatus::Failed => {
                let error = queued_tx.error.as_deref().unwrap_or("Unknown error");
                return ToolResult::error(format!(
                    "Transaction {} previously failed: {}\n\nYou may need to create a new transaction.",
                    params.uuid, error
                ));
            },
            QueuedTxStatus::Expired => {
                return ToolResult::error(format!(
                    "Transaction {} has expired. Please create a new transaction.",
                    params.uuid
                ));
            },
        }

        // Mark as broadcasting
        tx_queue.mark_broadcasting(&params.uuid);

        log::info!("[broadcast_web3_tx] Broadcasting transaction {} on {}", params.uuid, queued_tx.network);

        // Initialize RPC client
        let private_key = match Self::get_private_key() {
            Ok(pk) => pk,
            Err(e) => {
                tx_queue.mark_failed(&params.uuid, &e);
                return ToolResult::error(e);
            }
        };

        let rpc = match X402EvmRpc::new(&private_key, &queued_tx.network) {
            Ok(r) => r,
            Err(e) => {
                tx_queue.mark_failed(&params.uuid, &e);
                return ToolResult::error(format!("Failed to initialize RPC: {}", e));
            }
        };

        // Decode signed transaction from hex
        let signed_tx_bytes = match hex::decode(queued_tx.signed_tx_hex.trim_start_matches("0x")) {
            Ok(b) => b,
            Err(e) => {
                let error = format!("Invalid signed transaction hex: {}", e);
                tx_queue.mark_failed(&params.uuid, &error);
                return ToolResult::error(error);
            }
        };

        // Broadcast the transaction
        let tx_hash = match rpc.send_raw_transaction(&signed_tx_bytes).await {
            Ok(h) => h,
            Err(e) => {
                tx_queue.mark_failed(&params.uuid, &e);
                return ToolResult::error(format!("Broadcast failed: {}", e));
            }
        };

        let tx_hash_str = format!("{:?}", tx_hash);
        log::info!("[broadcast_web3_tx] Transaction sent: {}", tx_hash_str);

        // Get explorer URL
        let explorer_base = queued_tx.get_explorer_base_url();
        let explorer_url = format!("{}/{}", explorer_base, tx_hash_str);

        // Mark as broadcast
        tx_queue.mark_broadcast(&params.uuid, &tx_hash_str, &explorer_url);

        // Emit tx.pending event
        if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::tx_pending(
                ch_id,
                &tx_hash_str,
                &queued_tx.network,
                &explorer_url,
            ));
            log::info!("[broadcast_web3_tx] Emitted tx.pending event for {}", tx_hash_str);
        }

        // Wait for receipt
        let receipt = match rpc.wait_for_receipt(tx_hash, Duration::from_secs(120)).await {
            Ok(r) => r,
            Err(e) => {
                // Transaction was sent but confirmation failed - still report success with warning
                let mut msg = String::new();
                msg.push_str("TRANSACTION BROADCAST (confirmation timeout)\n\n");
                msg.push_str(&format!("Hash: {}\n", tx_hash_str));
                msg.push_str(&format!("Explorer: {}\n\n", explorer_url));
                msg.push_str(&format!("Warning: Confirmation timed out: {}\n", e));
                msg.push_str("The transaction may still confirm. Check the explorer for status.");

                return ToolResult::success(msg).with_metadata(json!({
                    "uuid": params.uuid,
                    "tx_hash": tx_hash_str,
                    "network": queued_tx.network,
                    "explorer_url": explorer_url,
                    "status": "broadcast",
                    "warning": e
                }));
            }
        };

        // Determine status from receipt
        let status = if receipt.status == Some(ethers::types::U64::from(1)) {
            tx_queue.mark_confirmed(&params.uuid);
            "confirmed"
        } else {
            tx_queue.mark_failed(&params.uuid, "Transaction reverted on-chain");
            "reverted"
        };

        // Emit tx.confirmed event
        if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::tx_confirmed(
                ch_id,
                &tx_hash_str,
                &queued_tx.network,
                status,
            ));
            log::info!("[broadcast_web3_tx] Emitted tx.confirmed event for {} (status={})", tx_hash_str, status);
        }

        // Build response
        let status_indicator = if status == "confirmed" { "CONFIRMED" } else { "REVERTED" };

        let mut msg = String::new();
        msg.push_str(&format!("TRANSACTION {}\n\n", status_indicator));
        msg.push_str(&format!("Hash: {}\n", tx_hash_str));
        msg.push_str(&format!("Explorer: {}\n\n", explorer_url));

        msg.push_str("--- Details ---\n");
        msg.push_str(&format!("UUID: {}\n", params.uuid));
        msg.push_str(&format!("Network: {}\n", queued_tx.network));
        msg.push_str(&format!("From: {}\n", queued_tx.from));
        msg.push_str(&format!("To: {}\n", queued_tx.to));
        msg.push_str(&format!("Value: {} ({})\n", queued_tx.value, Web3TxTool::format_eth(&queued_tx.value)));

        if let Some(block) = receipt.block_number {
            msg.push_str(&format!("Block: {}\n", block));
        }

        msg.push_str("\n--- Gas ---\n");
        msg.push_str(&format!("Gas Limit: {}\n", queued_tx.gas_limit));
        if let Some(gas_used) = receipt.gas_used {
            msg.push_str(&format!("Gas Used: {}\n", gas_used));
        }
        msg.push_str(&format!("Max Fee: {} ({})\n", queued_tx.max_fee_per_gas, Web3TxTool::format_gwei(&queued_tx.max_fee_per_gas)));
        if let Some(effective_price) = receipt.effective_gas_price {
            let price_str = effective_price.to_string();
            msg.push_str(&format!("Effective Price: {} ({})\n", price_str, Web3TxTool::format_gwei(&price_str)));
        }

        ToolResult::success(msg).with_metadata(json!({
            "uuid": params.uuid,
            "tx_hash": tx_hash_str,
            "status": status,
            "network": queued_tx.network,
            "explorer_url": explorer_url,
            "from": queued_tx.from,
            "to": queued_tx.to,
            "value": queued_tx.value,
            "gas_limit": queued_tx.gas_limit,
            "gas_used": receipt.gas_used.map(|g| g.to_string()),
            "block_number": receipt.block_number.map(|b| b.as_u64()),
            "effective_gas_price": receipt.effective_gas_price.map(|p| p.to_string())
        }))
    }
}
