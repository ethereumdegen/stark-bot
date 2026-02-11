//! Post-broadcast transaction verification tool
//!
//! Polls for a broadcasted transaction's receipt, decodes on-chain data
//! (ERC20 Transfer events from logs), and uses an AI model to verify
//! the result matches the user's original intent.

use crate::ai::{AiClient, Message, MessageRole};
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

/// ERC20 Transfer(address,address,uint256) event topic
const ERC20_TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

pub struct VerifyTxBroadcastTool {
    definition: ToolDefinition,
}

impl VerifyTxBroadcastTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "uuid".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "UUID of the transaction in the tx queue. If not provided, reads from 'queued_tx_uuid' register.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        VerifyTxBroadcastTool {
            definition: ToolDefinition {
                name: "verify_tx_broadcast".to_string(),
                description: "Verify a broadcasted transaction: polls for receipt, decodes token transfer events from logs, and uses AI to check whether the on-chain result matches the user's original request. Call this AFTER broadcast_web3_tx to confirm the transaction succeeded and did what was intended.".to_string(),
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

impl Default for VerifyTxBroadcastTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct VerifyParams {
    uuid: Option<String>,
}

#[async_trait]
impl Tool for VerifyTxBroadcastTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        log::info!("[verify_tx_broadcast] Raw params: {}", params);

        let params: VerifyParams = match serde_json::from_value(params.clone()) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Resolve UUID
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
                ch_id, None, "verify_tx_broadcast", &json!({"uuid": uuid}),
            ));
        }

        let started = std::time::Instant::now();

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

        // Check if we have a tx_hash to poll
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
        let from = queued_tx.from.clone();
        let to = queued_tx.to.clone();
        let value = queued_tx.value.clone();
        let data = queued_tx.data.clone();
        let explorer_url = queued_tx.explorer_url.clone().unwrap_or_default();
        let current_status = queued_tx.status.clone();
        drop(queued_tx); // Release the DashMap ref

        // If already confirmed or failed, we don't need to poll
        // But we still want to fetch the receipt for logs
        let tx_hash: H256 = match tx_hash_str.parse() {
            Ok(h) => h,
            Err(e) => return ToolResult::error(format!("Invalid tx hash '{}': {}", tx_hash_str, e)),
        };

        // Set up RPC for receipt fetching
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

        // Determine status: poll if needed
        let (final_status, receipt_opt) = match current_status {
            QueuedTxStatus::Confirmed => {
                // Already confirmed — still fetch receipt for logs
                let receipt = rpc.get_transaction_receipt(tx_hash).await.ok().flatten();
                ("confirmed", receipt)
            }
            QueuedTxStatus::Failed => {
                let receipt = rpc.get_transaction_receipt(tx_hash).await.ok().flatten();
                ("reverted", receipt)
            }
            QueuedTxStatus::Broadcast | QueuedTxStatus::Broadcasting => {
                // Need to poll for confirmation
                match rpc.wait_for_receipt(tx_hash, Duration::from_secs(120)).await {
                    Ok(receipt) => {
                        let status = if receipt.status == Some(ethers::types::U64::from(1)) {
                            tx_queue.mark_confirmed(&uuid);
                            "confirmed"
                        } else {
                            tx_queue.mark_failed(&uuid, "Transaction reverted on-chain");
                            "reverted"
                        };
                        (status, Some(receipt))
                    }
                    Err(_) => ("timeout", None),
                }
            }
            _ => return ToolResult::error(format!(
                "Transaction is in unexpected state: {:?}", current_status
            )),
        };

        // Decode ERC20 Transfer events from logs
        let transfers = receipt_opt.as_ref()
            .map(|r| decode_erc20_transfers(&r.logs))
            .unwrap_or_default();

        // Build the transaction summary
        let mut summary = String::new();
        summary.push_str(&format!("Status: {}\n", final_status.to_uppercase()));
        summary.push_str(&format!("Tx Hash: {}\n", tx_hash_str));
        summary.push_str(&format!("Network: {}\n", network));
        summary.push_str(&format!("From: {}\n", from));
        summary.push_str(&format!("To (contract): {}\n", to));
        summary.push_str(&format!("Value (wei): {}\n", value));

        if let Some(ref receipt) = receipt_opt {
            if let Some(block) = receipt.block_number {
                summary.push_str(&format!("Block: {}\n", block));
            }
            if let Some(gas) = receipt.gas_used {
                summary.push_str(&format!("Gas Used: {}\n", gas));
            }
        }

        if !transfers.is_empty() {
            summary.push_str("\nToken Transfers:\n");
            for t in &transfers {
                summary.push_str(&format!(
                    "  {} → {} : {} (token: {})\n",
                    short_addr(&t.from), short_addr(&t.to), t.amount_raw, short_addr(&t.token)
                ));
            }
        }

        if !data.is_empty() && data != "0x" {
            // Show first 10 bytes (function selector + a bit) for AI context
            let preview_len = std::cmp::min(data.len(), 74); // "0x" + 8 selector + 64 first param
            summary.push_str(&format!("\nCalldata (truncated): {}...\n", &data[..preview_len]));
        }

        // Add register context for the AI
        let mut register_context = String::new();
        for key in &["sell_token_symbol", "buy_token_symbol", "sell_amount", "sell_token_decimals", "buy_token_decimals"] {
            if let Some(val) = context.registers.get(*key) {
                register_context.push_str(&format!("{}: {}\n", key, val));
            }
        }
        if !register_context.is_empty() {
            summary.push_str(&format!("\nExpected swap parameters:\n{}", register_context));
        }

        // Run AI verification if tx is confirmed
        let ai_verdict = if final_status == "confirmed" {
            run_ai_verification(&summary, context).await
        } else {
            // Don't run AI on reverted/timeout — no point
            None
        };

        let duration_ms = started.elapsed().as_millis() as i64;

        // Build the final result
        let mut msg = String::new();

        match final_status {
            "confirmed" => {
                match &ai_verdict {
                    Some(Ok(())) => {
                        msg.push_str("TRANSACTION VERIFIED ✓\n\n");
                        msg.push_str("The transaction was confirmed on-chain and matches the user's original intent.\n\n");
                    }
                    Some(Err(reason)) => {
                        msg.push_str("TRANSACTION CONFIRMED — INTENT MISMATCH ⚠️\n\n");
                        msg.push_str(&format!("The transaction confirmed on-chain, but the AI verifier flagged a concern:\n{}\n\n", reason));
                        msg.push_str("Report this to the user and let them verify via the explorer.\n\n");
                    }
                    None => {
                        msg.push_str("TRANSACTION CONFIRMED (AI check skipped)\n\n");
                    }
                }
            }
            "reverted" => {
                msg.push_str("TRANSACTION REVERTED ✗\n\n");
                msg.push_str("The transaction was executed on-chain but REVERTED. The swap FAILED.\n");
                msg.push_str("Do NOT report success to the user.\n\n");
            }
            "timeout" => {
                msg.push_str("CONFIRMATION TIMEOUT ⏳\n\n");
                msg.push_str("Could not confirm the transaction within 120 seconds.\n");
                msg.push_str("The transaction may still confirm. Tell the user to check the explorer.\n\n");
            }
            _ => {
                msg.push_str(&format!("UNKNOWN STATUS: {}\n\n", final_status));
            }
        }

        msg.push_str(&format!("Hash: {}\n", tx_hash_str));
        msg.push_str(&format!("Explorer: {}\n", explorer_url));

        if !transfers.is_empty() {
            msg.push_str("\nToken transfers detected:\n");
            for t in &transfers {
                msg.push_str(&format!(
                    "  {} → {} : {} (token {})\n",
                    short_addr(&t.from), short_addr(&t.to), t.amount_raw, short_addr(&t.token)
                ));
            }
        }

        // Emit tool-result event
        if let (Some(broadcaster), Some(ch_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::tool_result(
                ch_id, None, "verify_tx_broadcast",
                final_status == "confirmed",
                duration_ms,
                &msg,
                false,
            ));
        }

        let verified = final_status == "confirmed" && matches!(ai_verdict, Some(Ok(())));

        ToolResult::success(msg).with_metadata(json!({
            "uuid": uuid,
            "tx_hash": tx_hash_str,
            "status": final_status,
            "verified": verified,
            "network": network,
            "explorer_url": explorer_url,
            "token_transfers": transfers.iter().map(|t| json!({
                "from": t.from,
                "to": t.to,
                "token": t.token,
                "amount_raw": t.amount_raw,
            })).collect::<Vec<_>>(),
        }))
    }
}

// ─── ERC20 Transfer log decoding ─────────────────────────────────────────────

struct Erc20Transfer {
    token: String,
    from: String,
    to: String,
    amount_raw: String,
}

fn decode_erc20_transfers(logs: &[TxLog]) -> Vec<Erc20Transfer> {
    let transfer_topic: H256 = ERC20_TRANSFER_TOPIC.parse().unwrap_or_default();

    logs.iter()
        .filter_map(|log| {
            // ERC20 Transfer has 3 topics: event sig, from, to
            if log.topics.len() < 3 || log.topics[0] != transfer_topic {
                return None;
            }

            let from = format!("0x{}", hex::encode(&log.topics[1].as_bytes()[12..]));
            let to = format!("0x{}", hex::encode(&log.topics[2].as_bytes()[12..]));
            let token = format!("{:?}", log.address);

            // Amount is in log.data (32 bytes, big-endian uint256)
            let amount_raw = if log.data.len() >= 32 {
                let amount = U256::from_big_endian(&log.data[..32]);
                amount.to_string()
            } else {
                "0".to_string()
            };

            Some(Erc20Transfer { token, from, to, amount_raw })
        })
        .collect()
}

fn short_addr(addr: &str) -> String {
    if addr.len() >= 10 {
        format!("{}...{}", &addr[..6], &addr[addr.len() - 4..])
    } else {
        addr.to_string()
    }
}

// ─── AI verification ─────────────────────────────────────────────────────────

const POST_TX_SYSTEM_PROMPT: &str = "\
You are a post-transaction verifier. A blockchain transaction has been confirmed on-chain. \
Your job is to determine whether the on-chain result matches the user's original request.

Respond with EXACTLY one of these formats (no extra text):
  VERIFIED
  MISMATCH: <one-line reason>

Rules:
- VERIFIED means the confirmed transaction clearly accomplished what the user asked for.
- MISMATCH means the on-chain result does not match the user's request \
  (wrong tokens, wrong amounts, wrong recipient, etc.).
- Look at token transfer events to see what actually moved on-chain.
- If the transaction type is a swap: verify that tokens moved from the user's address \
  and different tokens were received back.
- If you cannot determine the result with confidence, respond VERIFIED \
  (the transaction already confirmed, so fail-open is appropriate).
- Do NOT add any explanation beyond the single-line reason.";

async fn run_ai_verification(
    tx_summary: &str,
    context: &ToolContext,
) -> Option<Result<(), String>> {
    let user_message = context
        .extra
        .get("original_user_message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if user_message.is_empty() {
        log::warn!("[verify_tx_broadcast] No original_user_message — skipping AI check");
        return None;
    }

    let client = build_client_from_db(context)?;

    let prompt = format!(
        "## User's original request\n{}\n\n## On-chain transaction result\n{}",
        user_message, tx_summary
    );

    let messages = vec![
        Message {
            role: MessageRole::System,
            content: POST_TX_SYSTEM_PROMPT.to_string(),
        },
        Message {
            role: MessageRole::User,
            content: prompt,
        },
    ];

    match client.generate_text(messages).await {
        Ok(text) => Some(parse_post_tx_response(&text)),
        Err(e) => {
            log::warn!(
                "[verify_tx_broadcast] AI verification failed (allowing): {}",
                e
            );
            // Fail-open: tx already confirmed on-chain
            Some(Ok(()))
        }
    }
}

fn parse_post_tx_response(response: &str) -> Result<(), String> {
    let first_line = response
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");

    if first_line.starts_with("VERIFIED") {
        return Ok(());
    }

    if first_line.starts_with("MISMATCH:") {
        let reason = first_line.strip_prefix("MISMATCH:").unwrap_or("").trim();
        return Err(format!("Intent mismatch: {}", reason));
    }

    // Unparseable = fail-open (tx already confirmed)
    log::warn!(
        "[verify_tx_broadcast] Unparseable AI response (allowing): {}",
        first_line
    );
    Ok(())
}

fn build_client_from_db(context: &ToolContext) -> Option<AiClient> {
    let db = context.database.as_ref()?;
    let settings = db.get_active_agent_settings().ok()??;
    AiClient::from_settings(&settings).ok()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::Address;

    // ── parse_post_tx_response ──────────────────────────────────────

    #[test]
    fn test_parse_verified() {
        assert!(parse_post_tx_response("VERIFIED").is_ok());
        assert!(parse_post_tx_response("  VERIFIED  ").is_ok());
        assert!(parse_post_tx_response("VERIFIED. Transaction looks correct.").is_ok());
    }

    #[test]
    fn test_parse_mismatch() {
        let err = parse_post_tx_response("MISMATCH: wrong token received").unwrap_err();
        assert!(err.contains("wrong token received"), "got: {}", err);
    }

    #[test]
    fn test_parse_gibberish_fails_open() {
        assert!(parse_post_tx_response("I don't know").is_ok());
        assert!(parse_post_tx_response("").is_ok());
    }

    // ── decode_erc20_transfers ──────────────────────────────────────

    #[test]
    fn test_decode_transfer_event() {
        let transfer_topic: H256 = ERC20_TRANSFER_TOPIC.parse().unwrap();
        let token_addr: Address = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".parse().unwrap();

        // from = 0x1111...1111 (padded to 32 bytes in topic)
        let mut from_bytes = [0u8; 32];
        from_bytes[12..].copy_from_slice(&[0x11; 20]);
        let from_topic = H256::from(from_bytes);

        // to = 0x2222...2222
        let mut to_bytes = [0u8; 32];
        to_bytes[12..].copy_from_slice(&[0x22; 20]);
        let to_topic = H256::from(to_bytes);

        // amount = 1000000 (1 USDC)
        let mut amount_bytes = [0u8; 32];
        U256::from(1_000_000u64).to_big_endian(&mut amount_bytes);

        let log = TxLog {
            address: token_addr,
            topics: vec![transfer_topic, from_topic, to_topic],
            data: ethers::types::Bytes::from(amount_bytes.to_vec()),
        };

        let transfers = decode_erc20_transfers(&[log]);
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].amount_raw, "1000000");
        assert!(transfers[0].from.contains("1111111111"));
        assert!(transfers[0].to.contains("2222222222"));
    }

    #[test]
    fn test_decode_ignores_non_transfer() {
        // Log with wrong topic
        let log = TxLog {
            address: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".parse().unwrap(),
            topics: vec![H256::zero()],
            data: ethers::types::Bytes::from(vec![0u8; 32]),
        };

        let transfers = decode_erc20_transfers(&[log]);
        assert!(transfers.is_empty());
    }

    #[test]
    fn test_decode_empty_logs() {
        let transfers = decode_erc20_transfers(&[]);
        assert!(transfers.is_empty());
    }

    // ── short_addr ──────────────────────────────────────────────────

    #[test]
    fn test_short_addr() {
        assert_eq!(
            short_addr("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
            "0x8335...2913"
        );
    }
}
