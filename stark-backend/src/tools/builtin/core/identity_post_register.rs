//! Post-registration tool for EIP-8004 identity
//!
//! After the agent broadcasts a `register()` transaction via the `identity_register`
//! preset, this tool fetches the receipt, decodes the `Registered(agentId, agentURI, owner)`
//! event, and persists the registration to the local `agent_identity` SQLite table.
//! This mirrors how `verify_tx_broadcast` works for swaps — it's the final step
//! that closes the loop between on-chain state and local state.

use crate::eip8004::config::Eip8004Config;
use crate::gateway::protocol::GatewayEvent;
use crate::tools::registry::Tool;
use crate::tools::rpc_config::resolve_rpc_from_context;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tx_queue::QueuedTxStatus;
use crate::x402::{TxLog, X402EvmRpc};
use async_trait::async_trait;
use ethers::types::{H256, U256};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

/// Registered(uint256 indexed agentId, string agentURI, address indexed owner)
/// keccak256("Registered(uint256,string,address)")
const REGISTERED_EVENT_TOPIC: &str =
    "0xca52e62c367d81bb2e328eb795f7c7ba24afb478408a26c0e201d155c449bc4a";

pub struct IdentityPostRegisterTool {
    definition: ToolDefinition,
}

impl IdentityPostRegisterTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "uuid".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "UUID of the register transaction in the tx queue. If not provided, reads from 'queued_tx_uuid' register.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        IdentityPostRegisterTool {
            definition: ToolDefinition {
                name: "identity_post_register".to_string(),
                description: "Finalize EIP-8004 registration: fetch the register() transaction receipt, decode the Registered event to extract your agentId, and save the registration to the local database. Call this AFTER broadcast_web3_tx confirms the identity_register transaction.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec![],
                },
                group: ToolGroup::Finance,
            },
        }
    }
}

impl Default for IdentityPostRegisterTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct PostRegisterParams {
    uuid: Option<String>,
}

/// Decoded Registered event data
struct RegisteredEvent {
    agent_id: u64,
    agent_uri: String,
    owner: String,
}

#[async_trait]
impl Tool for IdentityPostRegisterTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        log::info!("[identity_post_register] Raw params: {}", params);

        let params: PostRegisterParams = match serde_json::from_value(params.clone()) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Resolve UUID from param or register
        let uuid = match params.uuid {
            Some(u) => u,
            None => match context.registers.get("queued_tx_uuid") {
                Some(val) => match val.as_str() {
                    Some(s) => s.to_string(),
                    None => return ToolResult::error("Register 'queued_tx_uuid' is not a valid string"),
                },
                None => return ToolResult::error(
                    "No UUID provided and 'queued_tx_uuid' register not found. Call broadcast_web3_tx first."
                ),
            },
        };

        // Emit tool-call event for UI
        if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::agent_tool_call(
                ch_id, None, "identity_post_register", &json!({"uuid": uuid}),
            ));
        }

        // Get tx from queue
        let tx_queue = match &context.tx_queue {
            Some(q) => q,
            None => return ToolResult::error("Transaction queue not available."),
        };

        let queued_tx = match tx_queue.get(&uuid) {
            Some(tx) => tx,
            None => return ToolResult::error(format!(
                "Transaction '{}' not found in queue.", uuid
            )),
        };

        // Extract tx info
        let tx_hash_str = match &queued_tx.tx_hash {
            Some(h) => h.clone(),
            None => {
                return match queued_tx.status {
                    QueuedTxStatus::Pending => ToolResult::error(
                        "Transaction has not been broadcast yet. Call broadcast_web3_tx first."
                    ),
                    QueuedTxStatus::Failed => {
                        let err = queued_tx.error.as_deref().unwrap_or("Unknown error");
                        ToolResult::error(format!("Transaction FAILED: {}", err))
                    }
                    _ => ToolResult::error("Transaction has no tx_hash."),
                };
            }
        };

        let network = queued_tx.network.clone();
        let explorer_url = queued_tx.explorer_url.clone().unwrap_or_default();
        let current_status = queued_tx.status.clone();
        drop(queued_tx);

        // Parse tx hash
        let tx_hash: H256 = match tx_hash_str.parse() {
            Ok(h) => h,
            Err(e) => return ToolResult::error(format!("Invalid tx hash '{}': {}", tx_hash_str, e)),
        };

        // Set up RPC
        let rpc_config = resolve_rpc_from_context(&context.extra, &network);
        let wallet_provider = match &context.wallet_provider {
            Some(wp) => wp,
            None => return ToolResult::error("Wallet not configured."),
        };

        let rpc = match X402EvmRpc::new_with_wallet_provider(
            wallet_provider.clone(),
            &network,
            Some(rpc_config.url.clone()),
            rpc_config.use_x402,
        ) {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to initialize RPC: {}", e)),
        };

        // Get receipt (poll if needed)
        let receipt = match current_status {
            QueuedTxStatus::Confirmed => {
                match rpc.get_transaction_receipt(tx_hash).await {
                    Ok(Some(r)) => r,
                    Ok(None) => return ToolResult::error("Receipt not found despite confirmed status."),
                    Err(e) => return ToolResult::error(format!("Failed to fetch receipt: {}", e)),
                }
            }
            QueuedTxStatus::Broadcast | QueuedTxStatus::Broadcasting => {
                match rpc.wait_for_receipt(tx_hash, Duration::from_secs(120)).await {
                    Ok(r) => {
                        if r.status == Some(ethers::types::U64::from(1)) {
                            tx_queue.mark_confirmed(&uuid);
                        } else {
                            tx_queue.mark_failed(&uuid, "Transaction reverted on-chain");
                            return ToolResult::error(format!(
                                "REGISTRATION REVERTED\n\nHash: {}\nExplorer: {}\n\nThe register() transaction was reverted on-chain. Check that you approved 1000 STARKBOT first.",
                                tx_hash_str, explorer_url
                            ));
                        }
                        r
                    }
                    Err(e) => return ToolResult::error(format!(
                        "Confirmation timeout: {}. Check explorer: {}", e, explorer_url
                    )),
                }
            }
            QueuedTxStatus::Failed => {
                let err = tx_queue.get(&uuid)
                    .and_then(|tx| tx.error.clone())
                    .unwrap_or_else(|| "Unknown error".to_string());
                return ToolResult::error(format!("Transaction FAILED: {}", err));
            }
            _ => return ToolResult::error(format!(
                "Transaction in unexpected state: {:?}", current_status
            )),
        };

        // Check tx succeeded
        if receipt.status != Some(ethers::types::U64::from(1)) {
            return ToolResult::error(format!(
                "REGISTRATION REVERTED\n\nHash: {}\nExplorer: {}\n\nThe register() transaction was reverted.",
                tx_hash_str, explorer_url
            ));
        }

        // Decode the Registered event from logs
        let registered = match decode_registered_event(&receipt.logs) {
            Some(evt) => evt,
            None => {
                return ToolResult::error(format!(
                    "Transaction confirmed but no Registered event found in logs.\nHash: {}\nExplorer: {}\n\nThis may not be a register() transaction.",
                    tx_hash_str, explorer_url
                ));
            }
        };

        log::info!(
            "[identity_post_register] Decoded: agentId={}, owner={}, uri={}",
            registered.agent_id, registered.owner, registered.agent_uri
        );

        // Persist to SQLite
        let config = Eip8004Config::from_env();
        let agent_registry = config.agent_registry_string();

        let db = match &context.database {
            Some(db) => db,
            None => {
                // No DB — still report success but warn
                let msg = format!(
                    "REGISTRATION CONFIRMED (DB unavailable)\n\n\
                    Agent ID: {}\n\
                    Owner: {}\n\
                    URI: {}\n\
                    Hash: {}\n\
                    Explorer: {}\n\n\
                    Warning: Could not persist to local database.",
                    registered.agent_id, registered.owner, registered.agent_uri,
                    tx_hash_str, explorer_url
                );
                return ToolResult::success(msg).with_metadata(json!({
                    "agent_id": registered.agent_id,
                    "owner": registered.owner,
                    "agent_uri": registered.agent_uri,
                    "tx_hash": tx_hash_str,
                    "persisted": false,
                }));
            }
        };

        let conn = db.conn();

        // Read name/description from the agent_uri register or IDENTITY.json
        let name = context.registers.get("agent_name")
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        let description = context.registers.get("agent_description")
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        // Upsert: delete any existing rows first (only one identity per agent)
        let _ = conn.execute("DELETE FROM agent_identity", []);

        let insert_result = conn.execute(
            "INSERT INTO agent_identity (agent_id, agent_registry, chain_id, registration_uri, wallet_address, owner_address, name, description, is_active, tx_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
            rusqlite::params![
                registered.agent_id as i64,
                agent_registry,
                config.chain_id as i64,
                registered.agent_uri,
                registered.owner,
                registered.owner,
                name,
                description,
                tx_hash_str,
            ],
        );

        match insert_result {
            Ok(_) => {
                log::info!(
                    "[identity_post_register] Persisted agent_id={} to agent_identity table",
                    registered.agent_id
                );
            }
            Err(e) => {
                log::error!("[identity_post_register] Failed to persist: {}", e);
                let msg = format!(
                    "REGISTRATION CONFIRMED (DB write failed)\n\n\
                    Agent ID: {}\nOwner: {}\nURI: {}\nHash: {}\nExplorer: {}\n\n\
                    Error persisting to database: {}",
                    registered.agent_id, registered.owner, registered.agent_uri,
                    tx_hash_str, explorer_url, e
                );
                return ToolResult::success(msg).with_metadata(json!({
                    "agent_id": registered.agent_id,
                    "owner": registered.owner,
                    "agent_uri": registered.agent_uri,
                    "tx_hash": tx_hash_str,
                    "persisted": false,
                }));
            }
        }

        // Emit tool-result event
        if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::tool_result(
                ch_id, None, "identity_post_register",
                true, 0,
                &format!("Agent #{} registered on-chain", registered.agent_id),
                false,
            ));
        }

        let msg = format!(
            "REGISTRATION CONFIRMED ✓\n\n\
            Agent ID: {}\n\
            Owner: {}\n\
            URI: {}\n\
            Registry: {}\n\
            Chain: {} ({})\n\
            Hash: {}\n\
            Explorer: {}\n\n\
            Your EIP-8004 agent identity has been registered on-chain and saved locally.\n\
            The frontend dashboard will now show your registered identity.",
            registered.agent_id,
            registered.owner,
            registered.agent_uri,
            config.identity_registry,
            config.chain_name,
            config.chain_id,
            tx_hash_str,
            explorer_url,
        );

        ToolResult::success(msg).with_metadata(json!({
            "agent_id": registered.agent_id,
            "owner": registered.owner,
            "agent_uri": registered.agent_uri,
            "agent_registry": agent_registry,
            "chain_id": config.chain_id,
            "tx_hash": tx_hash_str,
            "explorer_url": explorer_url,
            "persisted": true,
        }))
    }
}

// ─── Registered event decoding ────────────────────────────────────────────────

/// Decode the Registered(uint256 indexed agentId, string agentURI, address indexed owner) event.
///
/// Layout:
///   topics[0] = event signature hash
///   topics[1] = agentId (uint256, indexed, big-endian in 32 bytes)
///   topics[2] = owner (address, indexed, zero-padded to 32 bytes)
///   data = ABI-encoded string (agentURI)
fn decode_registered_event(logs: &[TxLog]) -> Option<RegisteredEvent> {
    let event_topic: H256 = REGISTERED_EVENT_TOPIC.parse().ok()?;

    for log in logs {
        if log.topics.len() < 3 || log.topics[0] != event_topic {
            continue;
        }

        // agentId from topics[1] (uint256, big-endian)
        let agent_id_u256 = U256::from_big_endian(log.topics[1].as_bytes());
        let agent_id = agent_id_u256.as_u64();

        // owner from topics[2] (address, last 20 bytes)
        let owner = format!("0x{}", hex::encode(&log.topics[2].as_bytes()[12..]));

        // agentURI from data (ABI-encoded string)
        let agent_uri = decode_abi_string(&log.data).unwrap_or_default();

        return Some(RegisteredEvent {
            agent_id,
            agent_uri,
            owner,
        });
    }

    None
}

/// Decode an ABI-encoded string from bytes.
///
/// ABI layout for a single string parameter:
///   bytes[0..32]  = offset (always 0x20 for a single string)
///   bytes[32..64] = length of the string
///   bytes[64..]   = UTF-8 string data (padded to 32-byte boundary)
fn decode_abi_string(data: &[u8]) -> Option<String> {
    if data.len() < 64 {
        return None;
    }

    // Read offset (first 32 bytes) — should be 0x20
    let offset = U256::from_big_endian(&data[0..32]).as_usize();
    if offset + 32 > data.len() {
        return None;
    }

    // Read string length
    let length = U256::from_big_endian(&data[offset..offset + 32]).as_usize();
    let start = offset + 32;
    if start + length > data.len() {
        return None;
    }

    String::from_utf8(data[start..start + length].to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::Address;

    #[test]
    fn test_tool_creation() {
        let tool = IdentityPostRegisterTool::new();
        assert_eq!(tool.definition().name, "identity_post_register");
        assert_eq!(tool.definition().group, ToolGroup::Finance);
    }

    #[test]
    fn test_decode_abi_string() {
        // Encode "https://identity.defirelay.com/api/identity/abc123/raw"
        let test_str = "https://identity.defirelay.com/api/identity/abc123/raw";
        let mut data = Vec::new();

        // offset = 0x20
        let mut offset_bytes = [0u8; 32];
        U256::from(32).to_big_endian(&mut offset_bytes);
        data.extend_from_slice(&offset_bytes);

        // length
        let mut len_bytes = [0u8; 32];
        U256::from(test_str.len()).to_big_endian(&mut len_bytes);
        data.extend_from_slice(&len_bytes);

        // string data (padded to 32 bytes)
        data.extend_from_slice(test_str.as_bytes());
        let padding = (32 - (test_str.len() % 32)) % 32;
        data.extend_from_slice(&vec![0u8; padding]);

        let decoded = decode_abi_string(&data).unwrap();
        assert_eq!(decoded, test_str);
    }

    #[test]
    fn test_decode_registered_event() {
        let event_topic: H256 = REGISTERED_EVENT_TOPIC.parse().unwrap();

        // agentId = 42
        let mut agent_id_bytes = [0u8; 32];
        U256::from(42u64).to_big_endian(&mut agent_id_bytes);
        let agent_id_topic = H256::from(agent_id_bytes);

        // owner = 0x1111...1111
        let mut owner_bytes = [0u8; 32];
        owner_bytes[12..].copy_from_slice(&[0x11; 20]);
        let owner_topic = H256::from(owner_bytes);

        // data = ABI-encoded string "https://example.com/identity.json"
        let uri = "https://example.com/identity.json";
        let mut data = Vec::new();
        let mut offset_bytes = [0u8; 32];
        U256::from(32).to_big_endian(&mut offset_bytes);
        data.extend_from_slice(&offset_bytes);
        let mut len_bytes = [0u8; 32];
        U256::from(uri.len()).to_big_endian(&mut len_bytes);
        data.extend_from_slice(&len_bytes);
        data.extend_from_slice(uri.as_bytes());
        let padding = (32 - (uri.len() % 32)) % 32;
        data.extend_from_slice(&vec![0u8; padding]);

        let log = TxLog {
            address: Address::zero(),
            topics: vec![event_topic, agent_id_topic, owner_topic],
            data: ethers::types::Bytes::from(data),
        };

        let evt = decode_registered_event(&[log]).unwrap();
        assert_eq!(evt.agent_id, 42);
        assert_eq!(evt.agent_uri, uri);
        assert!(evt.owner.contains("1111111111"));
    }

    #[test]
    fn test_decode_ignores_non_registered_event() {
        let log = TxLog {
            address: Address::zero(),
            topics: vec![H256::zero()],
            data: ethers::types::Bytes::from(vec![0u8; 32]),
        };

        assert!(decode_registered_event(&[log]).is_none());
    }
}
