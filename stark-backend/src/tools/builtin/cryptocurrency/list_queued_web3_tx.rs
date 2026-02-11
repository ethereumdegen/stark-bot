//! List queued Web3 transactions
//!
//! Shows transactions that have been signed but not yet broadcast.

use crate::gateway::protocol::GatewayEvent;
use super::web3_tx::SendEthTool;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tx_queue::QueuedTxStatus;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// List queued transactions tool
pub struct ListQueuedWeb3TxTool {
    definition: ToolDefinition,
}

impl ListQueuedWeb3TxTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "uuid".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Get a specific transaction by UUID (optional)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "status".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Filter by status: pending, broadcasting, broadcast, confirmed, failed, expired (optional)".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "pending".to_string(),
                    "broadcasting".to_string(),
                    "broadcast".to_string(),
                    "confirmed".to_string(),
                    "failed".to_string(),
                    "expired".to_string(),
                ]),
            },
        );

        properties.insert(
            "limit".to_string(),
            PropertySchema {
                schema_type: "integer".to_string(),
                description: "Maximum number of transactions to return (default 10)".to_string(),
                default: Some(json!(10)),
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "cache_as".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Register name to cache the first pending transaction's UUID. Defaults to 'queued_tx_uuid'. Set to empty string to disable.".to_string(),
                default: Some(json!("queued_tx_uuid")),
                items: None,
                enum_values: None,
            },
        );

        ListQueuedWeb3TxTool {
            definition: ToolDefinition {
                name: "list_queued_web3_tx".to_string(),
                description: "List queued transactions from web3_tx. Caches first pending UUID in '{cache_as}' register (default: 'queued_tx_uuid'). Use broadcast_web3_tx to broadcast.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::Finance,
                hidden: false,
            },
        }
    }
}

impl Default for ListQueuedWeb3TxTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ListParams {
    uuid: Option<String>,
    status: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default = "default_cache_as")]
    cache_as: String,
}

fn default_limit() -> usize {
    10
}

fn default_cache_as() -> String {
    "queued_tx_uuid".to_string()
}

#[async_trait]
impl Tool for ListQueuedWeb3TxTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        log::info!("[list_queued_web3_tx] Raw params: {}", params);

        let params: ListParams = match serde_json::from_value(params.clone()) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Get tx_queue
        let tx_queue = match &context.tx_queue {
            Some(q) => q,
            None => return ToolResult::error("Transaction queue not available. Contact administrator."),
        };

        // If specific UUID requested, get that transaction
        if let Some(uuid) = params.uuid {
            return match tx_queue.get(&uuid) {
                Some(tx) => {
                    let mut msg = String::new();
                    msg.push_str(&format!("Transaction: {}\n", tx.uuid));
                    msg.push_str(&format!("Status: {}\n", tx.status));
                    msg.push_str(&format!("Network: {}\n", tx.network));
                    msg.push_str(&format!("From: {}\n", tx.from));
                    msg.push_str(&format!("To: {}\n", tx.to));
                    msg.push_str(&format!("Value: {} ({})\n", tx.value, SendEthTool::format_eth(&tx.value)));
                    msg.push_str(&format!("Nonce: {}\n", tx.nonce));
                    msg.push_str(&format!("Gas Limit: {}\n", tx.gas_limit));
                    msg.push_str(&format!("Max Fee: {} ({})\n", tx.max_fee_per_gas, SendEthTool::format_gwei(&tx.max_fee_per_gas)));
                    msg.push_str(&format!("Priority Fee: {} ({})\n", tx.max_priority_fee_per_gas, SendEthTool::format_gwei(&tx.max_priority_fee_per_gas)));
                    msg.push_str(&format!("Created: {}\n", tx.created_at.format("%Y-%m-%d %H:%M:%S UTC")));

                    if let Some(ref tx_hash) = tx.tx_hash {
                        msg.push_str(&format!("Tx Hash: {}\n", tx_hash));
                    }
                    if let Some(ref url) = tx.explorer_url {
                        msg.push_str(&format!("Explorer: {}\n", url));
                    }
                    if let Some(ref error) = tx.error {
                        msg.push_str(&format!("Error: {}\n", error));
                    }
                    if let Some(ref broadcast_at) = tx.broadcast_at {
                        msg.push_str(&format!("Broadcast At: {}\n", broadcast_at.format("%Y-%m-%d %H:%M:%S UTC")));
                    }

                    if tx.status == QueuedTxStatus::Pending {
                        msg.push_str("\n--- Action ---\n");
                        msg.push_str(&format!("To broadcast: use broadcast_web3_tx with uuid: {}\n", tx.uuid));
                    }

                    ToolResult::success(msg).with_metadata(json!({
                        "uuid": tx.uuid,
                        "status": tx.status.to_string(),
                        "network": tx.network,
                        "from": tx.from,
                        "to": tx.to,
                        "value": tx.value,
                        "nonce": tx.nonce,
                        "tx_hash": tx.tx_hash,
                        "explorer_url": tx.explorer_url,
                        "error": tx.error,
                        "created_at": tx.created_at.to_rfc3339()
                    }))
                },
                None => ToolResult::error(format!("Transaction with UUID '{}' not found.", uuid)),
            };
        }

        // Parse status filter
        let status_filter: Option<QueuedTxStatus> = params.status.as_ref().map(|s| {
            match s.to_lowercase().as_str() {
                "pending" => Some(QueuedTxStatus::Pending),
                "broadcasting" => Some(QueuedTxStatus::Broadcasting),
                "broadcast" => Some(QueuedTxStatus::Broadcast),
                "confirmed" => Some(QueuedTxStatus::Confirmed),
                "failed" => Some(QueuedTxStatus::Failed),
                "expired" => Some(QueuedTxStatus::Expired),
                _ => None,
            }
        }).flatten();

        // Get transactions based on filter
        let transactions = if let Some(status) = status_filter {
            tx_queue.list_by_status(status)
        } else {
            tx_queue.list_recent(params.limit)
        };

        // Limit results
        let transactions: Vec<_> = transactions.into_iter().take(params.limit).collect();

        if transactions.is_empty() {
            let filter_desc = params.status.as_ref()
                .map(|s| format!(" with status '{}'", s))
                .unwrap_or_default();

            return ToolResult::success(format!(
                "No queued transactions found{}.\n\nUse web3_tx to create a new transaction.",
                filter_desc
            ));
        }

        // Count by status
        let pending_count = transactions.iter().filter(|t| t.status == QueuedTxStatus::Pending).count();
        let confirmed_count = transactions.iter().filter(|t| t.status == QueuedTxStatus::Confirmed).count();
        let failed_count = transactions.iter().filter(|t| t.status == QueuedTxStatus::Failed).count();

        // Build response
        let mut msg = String::new();
        msg.push_str(&format!("QUEUED TRANSACTIONS ({} shown)\n", transactions.len()));
        msg.push_str(&format!("Pending: {} | Confirmed: {} | Failed: {}\n\n", pending_count, confirmed_count, failed_count));

        for tx in &transactions {
            let status_indicator = match tx.status {
                QueuedTxStatus::Pending => "[PENDING]",
                QueuedTxStatus::Broadcasting => "[BROADCASTING]",
                QueuedTxStatus::Broadcast => "[BROADCAST]",
                QueuedTxStatus::Confirmed => "[CONFIRMED]",
                QueuedTxStatus::Failed => "[FAILED]",
                QueuedTxStatus::Expired => "[EXPIRED]",
            };

            msg.push_str(&format!("{} {}\n", status_indicator, tx.uuid));
            msg.push_str(&format!("  {} | To: {}...{}\n",
                tx.network,
                &tx.to[..10.min(tx.to.len())],
                &tx.to[tx.to.len().saturating_sub(4)..]
            ));
            msg.push_str(&format!("  Value: {}\n", tx.value_formatted));

            if let Some(ref tx_hash) = tx.tx_hash {
                msg.push_str(&format!("  Hash: {}...{}\n",
                    &tx_hash[..10.min(tx_hash.len())],
                    &tx_hash[tx_hash.len().saturating_sub(6)..]
                ));
            }

            if let Some(ref error) = tx.error {
                let short_error = if error.len() > 50 {
                    format!("{}...", &error[..50])
                } else {
                    error.clone()
                };
                msg.push_str(&format!("  Error: {}\n", short_error));
            }

            msg.push_str("\n");
        }

        if pending_count > 0 {
            msg.push_str("--- Actions ---\n");
            msg.push_str("To broadcast a pending transaction:\n");
            msg.push_str("  broadcast_web3_tx with uuid: <UUID>\n");
            msg.push_str("To view details:\n");
            msg.push_str("  list_queued_web3_tx with uuid: <UUID>\n");
        }

        // Build metadata with all transaction summaries
        let tx_data: Vec<_> = transactions.iter().map(|tx| {
            json!({
                "uuid": tx.uuid,
                "status": tx.status.to_string(),
                "network": tx.network,
                "to": tx.to,
                "value": tx.value,
                "value_formatted": tx.value_formatted,
                "data": tx.data,
                "tx_hash": tx.tx_hash,
                "explorer_url": tx.explorer_url,
                "error": tx.error,
                "created_at": tx.created_at.to_rfc3339()
            })
        }).collect();

        // In partner mode, emit confirmation event for first pending tx
        let is_rogue_mode = context.extra
            .get("rogue_mode_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !is_rogue_mode && pending_count > 0 {
            if let Some(first_pending) = transactions.iter()
                .find(|t| t.status == QueuedTxStatus::Pending)
            {
                // Emit event to open modal
                if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
                    broadcaster.broadcast(GatewayEvent::tx_queue_confirmation_required(
                        ch_id,
                        &first_pending.uuid,
                        &first_pending.network,
                        &first_pending.from,
                        &first_pending.to,
                        &first_pending.value,
                        &first_pending.value_formatted,
                        &first_pending.data,
                    ));
                    log::info!("[list_queued_web3_tx] Emitted tx_queue.confirmation_required for {}", first_pending.uuid);
                }
            }
        }

        // Cache first pending transaction UUID in register
        if !params.cache_as.is_empty() {
            if let Some(first_pending) = transactions.iter()
                .find(|t| t.status == QueuedTxStatus::Pending)
            {
                context.set_register(&params.cache_as, json!(&first_pending.uuid), "list_queued_web3_tx");
                log::info!(
                    "[list_queued_web3_tx] Cached first pending tx UUID '{}' in register '{}'",
                    first_pending.uuid,
                    params.cache_as
                );
                msg.push_str(&format!("\nUUID cached in register: '{}'\n", params.cache_as));
            }
        }

        ToolResult::success(msg).with_metadata(json!({
            "count": transactions.len(),
            "pending_count": pending_count,
            "confirmed_count": confirmed_count,
            "failed_count": failed_count,
            "transactions": tx_data
        }))
    }
}
