//! Broadcast a queued Web3 transaction
//!
//! Takes a UUID from web3_tx and broadcasts the signed transaction to the network.

use super::web3_tx::SendEthTool;
use crate::gateway::protocol::GatewayEvent;
use crate::tools::registry::Tool;
use crate::tools::rpc_config::resolve_rpc_from_context;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tx_queue::QueuedTxStatus;
use crate::x402::{TxLog, X402EvmRpc};
use ethers::types::{H256, U256};
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
                description: "UUID of the transaction to broadcast. If not provided, reads from uuid_register.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "uuid_register".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Register name containing UUID. Defaults to 'queued_tx_uuid'. Only used if 'uuid' not provided.".to_string(),
                default: Some(json!("queued_tx_uuid")),
                items: None,
                enum_values: None,
            },
        );

        BroadcastWeb3TxTool {
            definition: ToolDefinition {
                name: "broadcast_web3_tx".to_string(),
                description: "Broadcast a queued transaction. Reads UUID from 'uuid' param or 'uuid_register' (default: 'queued_tx_uuid'). Returns tx hash and explorer URL.".to_string(),
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

impl Default for BroadcastWeb3TxTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct BroadcastParams {
    uuid: Option<String>,
    #[serde(default = "default_uuid_register")]
    uuid_register: String,
}

fn default_uuid_register() -> String {
    "queued_tx_uuid".to_string()
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

        // Resolve UUID: explicit param takes precedence over register
        let uuid_from_param = params.uuid.is_some();
        let uuid = match params.uuid {
            Some(u) => u,
            None => {
                match context.registers.get(&params.uuid_register) {
                    Some(val) => {
                        match val.as_str() {
                            Some(s) => s.to_string(),
                            None => {
                                return ToolResult::error(format!(
                                    "Register '{}' does not contain a valid UUID string",
                                    params.uuid_register
                                ));
                            }
                        }
                    }
                    None => {
                        return ToolResult::error(format!(
                            "No UUID provided and register '{}' not found. Either:\n\
                            1. Provide uuid parameter directly, OR\n\
                            2. Call list_queued_web3_tx first to cache the UUID",
                            params.uuid_register
                        ));
                    }
                }
            }
        };

        log::info!("[broadcast_web3_tx] Resolved UUID: {} (from {})",
            uuid,
            if uuid_from_param { "param" } else { &params.uuid_register }
        );

        // Check rogue mode from bot settings in ToolContext
        let is_rogue_mode = context.extra
            .get("rogue_mode_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !is_rogue_mode {
            // Partner mode: trigger confirmation modal instead of broadcasting
            let tx_queue = match &context.tx_queue {
                Some(q) => q,
                None => return ToolResult::error("Transaction queue not available. Contact administrator."),
            };

            // Get the transaction to show in the modal
            let queued_tx = match tx_queue.get(&uuid) {
                Some(tx) => tx,
                None => return ToolResult::error(format!(
                    "Transaction with UUID '{}' not found. Use list_queued_web3_tx to see available transactions.",
                    uuid
                )),
            };

            // Emit event to open confirmation modal
            if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
                broadcaster.broadcast(GatewayEvent::tx_queue_confirmation_required(
                    ch_id,
                    &queued_tx.uuid,
                    &queued_tx.network,
                    &queued_tx.from,
                    &queued_tx.to,
                    &queued_tx.value,
                    &queued_tx.format_value_eth(),
                    &queued_tx.data,
                ));
                log::info!("[broadcast_web3_tx] Partner mode: emitted tx_queue.confirmation_required for {}", queued_tx.uuid);
            }

            return ToolResult::success(format!(
                "PARTNER MODE - Transaction queued for user confirmation.\n\n\
                UUID: {}\n\
                Network: {}\n\
                To: {}\n\
                Value: {}\n\n\
                The user will be prompted to confirm or deny this transaction.",
                queued_tx.uuid, queued_tx.network, queued_tx.to, queued_tx.format_value_eth()
            )).with_metadata(json!({
                "uuid": queued_tx.uuid,
                "status": "awaiting_confirmation",
                "network": queued_tx.network,
                "to": queued_tx.to,
                "value": queued_tx.value,
                "value_formatted": queued_tx.format_value_eth()
            }));
        }

        // Get tx_queue
        let tx_queue = match &context.tx_queue {
            Some(q) => q,
            None => return ToolResult::error("Transaction queue not available. Contact administrator."),
        };

        // Get the queued transaction
        let queued_tx = match tx_queue.get(&uuid) {
            Some(tx) => tx,
            None => return ToolResult::error(format!(
                "Transaction with UUID '{}' not found. Use list_queued_web3_tx to see available transactions.",
                uuid
            )),
        };

        // Validate status is Pending
        match queued_tx.status {
            QueuedTxStatus::Pending => {},
            QueuedTxStatus::Broadcasting => {
                return ToolResult::error(format!(
                    "Transaction {} is already being broadcast. Please wait.",
                    uuid
                ));
            },
            QueuedTxStatus::Broadcast | QueuedTxStatus::Confirmed => {
                let tx_hash = queued_tx.tx_hash.as_deref().unwrap_or("unknown");
                let explorer_url = queued_tx.explorer_url.as_deref().unwrap_or("");
                return ToolResult::error(format!(
                    "Transaction {} has already been broadcast.\n\nTx Hash: {}\nExplorer: {}",
                    uuid, tx_hash, explorer_url
                ));
            },
            QueuedTxStatus::Failed => {
                let error = queued_tx.error.as_deref().unwrap_or("Unknown error");
                return ToolResult::error(format!(
                    "Transaction {} previously failed: {}\n\nYou may need to create a new transaction.",
                    uuid, error
                ));
            },
            QueuedTxStatus::Expired => {
                return ToolResult::error(format!(
                    "Transaction {} has expired. Please create a new transaction.",
                    uuid
                ));
            },
        }

        // Mark as broadcasting
        tx_queue.mark_broadcasting(&uuid);

        // Resolve RPC configuration from context (respects custom RPC settings)
        let rpc_config = resolve_rpc_from_context(&context.extra, &queued_tx.network);

        log::info!(
            "[broadcast_web3_tx] Broadcasting transaction {} on {} (rpc={})",
            uuid, queued_tx.network, rpc_config.url
        );

        // Get wallet provider for x402 payments during RPC calls
        let wallet_provider = match &context.wallet_provider {
            Some(wp) => wp,
            None => {
                let err = "Wallet not configured. Cannot broadcast transactions.";
                tx_queue.mark_failed(&uuid, err);
                return ToolResult::error(err);
            }
        };

        // Initialize RPC client with WalletProvider (works in both Standard and Flash mode)
        let rpc = match X402EvmRpc::new_with_wallet_provider(
            wallet_provider.clone(),
            &queued_tx.network,
            Some(rpc_config.url.clone()),
            rpc_config.use_x402,
        ) {
            Ok(r) => r,
            Err(e) => {
                tx_queue.mark_failed(&uuid, &e);
                return ToolResult::error(format!("Failed to initialize RPC: {}", e));
            }
        };

        // Decode signed transaction from hex
        let signed_tx_bytes = match hex::decode(queued_tx.signed_tx_hex.trim_start_matches("0x")) {
            Ok(b) => b,
            Err(e) => {
                let error = format!("Invalid signed transaction hex: {}", e);
                tx_queue.mark_failed(&uuid, &error);
                return ToolResult::error(error);
            }
        };

        // Broadcast the transaction
        let tx_hash = match rpc.send_raw_transaction(&signed_tx_bytes).await {
            Ok(h) => h,
            Err(e) => {
                tx_queue.mark_failed(&uuid, &e);
                return ToolResult::error(format!("Broadcast failed: {}", e));
            }
        };

        let tx_hash_str = format!("{:?}", tx_hash);
        log::info!("[broadcast_web3_tx] Transaction sent: {}", tx_hash_str);

        // Get explorer URL
        let explorer_base = queued_tx.get_explorer_base_url();
        let explorer_url = format!("{}/{}", explorer_base, tx_hash_str);

        // Mark as broadcast (rogue mode - agent initiated)
        tx_queue.mark_broadcast(&uuid, &tx_hash_str, &explorer_url, "rogue");

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
                    "uuid": uuid,
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
            tx_queue.mark_confirmed(&uuid);
            "confirmed"
        } else {
            tx_queue.mark_failed(&uuid, "Transaction reverted on-chain");
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

        // Post-processing: if this was an identity_register tx and it confirmed,
        // decode the Registered event and save agent_id to the database automatically.
        let mut identity_agent_id: Option<u64> = None;
        if status == "confirmed" {
            let is_identity_register = queued_tx.preset.as_deref()
                .map(|p| p.starts_with("identity_register"))
                .unwrap_or(false);
            if is_identity_register {
                identity_agent_id = self.handle_identity_post_register(
                    &receipt.logs, &queued_tx, &tx_hash_str, context,
                );
            }
        }

        // Build response
        let status_indicator = if status == "confirmed" { "CONFIRMED" } else { "REVERTED" };

        let mut msg = String::new();
        msg.push_str(&format!("TRANSACTION {}\n\n", status_indicator));
        msg.push_str(&format!("Hash: {}\n", tx_hash_str));
        msg.push_str(&format!("Explorer: {}\n\n", explorer_url));

        // Append identity registration info if applicable
        if let Some(agent_id) = identity_agent_id {
            msg.push_str(&format!(
                "\n--- Identity Registration ---\nAgent ID (NFT): {}\nRegistration saved to database.\n\n",
                agent_id
            ));
        }

        msg.push_str("--- Details ---\n");
        msg.push_str(&format!("UUID: {}\n", uuid));
        msg.push_str(&format!("Network: {}\n", queued_tx.network));
        msg.push_str(&format!("From: {}\n", queued_tx.from));
        msg.push_str(&format!("To: {}\n", queued_tx.to));
        msg.push_str(&format!("Value: {} ({})\n", queued_tx.value, SendEthTool::format_eth(&queued_tx.value)));

        if let Some(block) = receipt.block_number {
            msg.push_str(&format!("Block: {}\n", block));
        }

        msg.push_str("\n--- Gas ---\n");
        msg.push_str(&format!("Gas Limit: {}\n", queued_tx.gas_limit));
        if let Some(gas_used) = receipt.gas_used {
            msg.push_str(&format!("Gas Used: {}\n", gas_used));
        }
        msg.push_str(&format!("Max Fee: {} ({})\n", queued_tx.max_fee_per_gas, SendEthTool::format_gwei(&queued_tx.max_fee_per_gas)));
        if let Some(effective_price) = receipt.effective_gas_price {
            let price_str = effective_price.to_string();
            msg.push_str(&format!("Effective Price: {} ({})\n", price_str, SendEthTool::format_gwei(&price_str)));
        }

        ToolResult::success(msg).with_metadata(json!({
            "uuid": uuid,
            "tx_hash": tx_hash_str,
            "status": status,
            "network": queued_tx.network,
            "explorer_url": explorer_url,
            "from": queued_tx.from,
            "to": queued_tx.to,
            "value": queued_tx.value,
            "gas_limit": queued_tx.gas_limit,
            "gas_used": receipt.gas_used.map(|g| g.to_string()),
            "identity_agent_id": identity_agent_id,
            "block_number": receipt.block_number.map(|b| b.as_u64()),
            "effective_gas_price": receipt.effective_gas_price.map(|p| p.to_string())
        }))
    }
}

// ─── Identity registration post-processing ────────────────────────────────────

/// Registered(uint256 indexed agentId, string agentURI, address indexed owner)
const REGISTERED_EVENT_TOPIC: &str =
    "0xca52e62c367d81bb2e328eb795f7c7ba24afb478408a26c0e201d155c449bc4a";

impl BroadcastWeb3TxTool {
    /// Decode Registered event from identity_register tx and save agent_id to DB.
    /// Returns the agent_id if successful.
    fn handle_identity_post_register(
        &self,
        logs: &[TxLog],
        queued_tx: &crate::tx_queue::QueuedTransaction,
        tx_hash_str: &str,
        context: &ToolContext,
    ) -> Option<u64> {
        let event_topic: H256 = REGISTERED_EVENT_TOPIC.parse().ok()?;

        // Find the Registered event in logs
        let mut agent_id: Option<u64> = None;
        let mut agent_uri = String::new();
        let mut owner = String::new();

        for log in logs {
            if log.topics.len() < 3 || log.topics[0] != event_topic {
                continue;
            }
            let id = U256::from_big_endian(log.topics[1].as_bytes()).as_u64();
            owner = format!("0x{}", hex::encode(&log.topics[2].as_bytes()[12..]));
            // Decode agentURI from data (ABI-encoded string)
            if log.data.len() >= 64 {
                let offset = U256::from_big_endian(&log.data[0..32]).as_usize();
                if offset + 32 <= log.data.len() {
                    let length = U256::from_big_endian(&log.data[offset..offset + 32]).as_usize();
                    let start = offset + 32;
                    if start + length <= log.data.len() {
                        agent_uri = String::from_utf8(log.data[start..start + length].to_vec())
                            .unwrap_or_default();
                    }
                }
            }
            agent_id = Some(id);
            break;
        }

        let agent_id = agent_id?;

        log::info!(
            "[broadcast_web3_tx] Identity registration detected: agent_id={}, owner={}, uri={}",
            agent_id, owner, agent_uri
        );

        // Persist to agent_identity table (minimal: just NFT ID + registry + chain)
        if let Some(db) = &context.database {
            let config = crate::eip8004::config::Eip8004Config::from_env();
            let agent_registry = config.agent_registry_string();
            let conn = db.conn();

            // Upsert: clear existing rows first (one identity per agent)
            let _ = conn.execute("DELETE FROM agent_identity", []);

            match conn.execute(
                "INSERT INTO agent_identity (agent_id, agent_registry, chain_id) VALUES (?1, ?2, ?3)",
                rusqlite::params![agent_id as i64, agent_registry, config.chain_id as i64],
            ) {
                Ok(_) => {
                    log::info!(
                        "[broadcast_web3_tx] Saved identity registration: agent_id={} to agent_identity table",
                        agent_id
                    );
                }
                Err(e) => {
                    log::error!("[broadcast_web3_tx] Failed to save identity registration: {}", e);
                }
            }
        }

        Some(agent_id)
    }
}
