//! Transaction intent verification — safety layer before tx queueing
//!
//! Every transaction-creating tool (send_eth, web3_function_call,
//! web3_preset_function_call, bridge_usdc) calls `verify_intent()` BEFORE
//! `tx_queue.queue()`. The check is embedded inside each tool so the AI
//! agent cannot skip it.
//!
//! ## Steps
//! 1. Read `original_user_message` from `context.extra`
//! 2. Run deterministic checks (fast, no network)
//! 3. Run isolated AI verification call
//! 4. Return `Ok(())` or `Err(reason)`

use crate::ai::{AiClient, Message, MessageRole};
use crate::gateway::protocol::GatewayEvent;
use crate::tools::types::ToolContext;
use serde_json::Value;

/// Describes the transaction about to be queued.
#[derive(Debug, Clone)]
pub struct TransactionIntent {
    pub tx_type: String,
    pub to: String,
    pub value: String,
    pub value_display: String,
    pub network: String,
    pub function_name: Option<String>,
    pub abi_name: Option<String>,
    pub preset_name: Option<String>,
    pub destination_chain: Option<String>,
    pub calldata: Option<String>,
    pub description: String,
}

// ─── Public entry point ──────────────────────────────────────────────────────

/// Verify that a transaction intent matches the user's original request.
///
/// `ai_override` — pass a pre-built client in tests to skip the DB lookup.
pub async fn verify_intent(
    intent: &TransactionIntent,
    context: &ToolContext,
    ai_override: Option<&AiClient>,
) -> Result<(), String> {
    let started = std::time::Instant::now();

    // Emit tool-call event so the UI shows verify_intent as its own step
    broadcast_tool_call(context, intent);

    let result = run_verification(intent, context, ai_override).await;

    // Emit tool-result event
    let duration_ms = started.elapsed().as_millis() as i64;
    broadcast_tool_result(context, &result, duration_ms);

    result
}

/// Inner verification logic (deterministic checks + AI check).
async fn run_verification(
    intent: &TransactionIntent,
    context: &ToolContext,
    ai_override: Option<&AiClient>,
) -> Result<(), String> {
    // 1. Run deterministic checks first (cheap, no network)
    run_deterministic_checks(intent, context)?;

    // 2. Read original user message
    let user_message = context
        .extra
        .get("original_user_message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if user_message.is_empty() {
        log::warn!("[verify_intent] No original_user_message in context — skipping AI check");
        // Still pass; deterministic checks already ran.
        return Ok(());
    }

    // 3. Obtain an AI client
    let owned_client: Option<AiClient>;
    let client: &AiClient = match ai_override {
        Some(c) => c,
        None => {
            owned_client = build_client_from_db(context);
            match owned_client.as_ref() {
                Some(c) => c,
                None => {
                    log::warn!("[verify_intent] Could not build AI client — skipping AI check");
                    return Ok(());
                }
            }
        }
    };

    // 4. Run AI verification
    let prompt = format_verification_prompt(intent, &user_message);
    let messages = vec![
        Message {
            role: MessageRole::System,
            content: VERIFICATION_SYSTEM_PROMPT.to_string(),
        },
        Message {
            role: MessageRole::User,
            content: prompt,
        },
    ];

    let ai_response = client.generate_text(messages).await;

    match ai_response {
        Ok(text) => parse_verification_response(&text),
        Err(e) => {
            // Fail-open on AI errors: deterministic checks already passed,
            // and a flaky AI API shouldn't block legitimate transactions.
            log::warn!(
                "[verify_intent] AI verification failed (allowing tx): {}",
                e
            );
            Ok(())
        }
    }
}

// ─── UI event helpers ─────────────────────────────────────────────────────────

fn broadcast_tool_call(context: &ToolContext, intent: &TransactionIntent) {
    if let (Some(broadcaster), Some(channel_id)) = (&context.broadcaster, context.channel_id) {
        let params = serde_json::json!({
            "tx_type": intent.tx_type,
            "to": intent.to,
            "value_display": intent.value_display,
            "network": intent.network,
            "description": intent.description,
        });
        broadcaster.broadcast(GatewayEvent::agent_tool_call(
            channel_id, None, "verify_intent", &params,
        ));
    }
}

fn broadcast_tool_result(context: &ToolContext, result: &Result<(), String>, duration_ms: i64) {
    if let (Some(broadcaster), Some(channel_id)) = (&context.broadcaster, context.channel_id) {
        let (success, content) = match result {
            Ok(()) => (true, "Transaction intent verified — checks passed.".to_string()),
            Err(reason) => (false, reason.clone()),
        };
        broadcaster.broadcast(GatewayEvent::tool_result(
            channel_id, None, "verify_intent", success, duration_ms, &content, false, None,
        ));
    }
}

// ─── Deterministic checks ────────────────────────────────────────────────────

/// Fast, offline checks that catch obvious problems.
fn run_deterministic_checks(
    intent: &TransactionIntent,
    context: &ToolContext,
) -> Result<(), String> {
    let to_lower = intent.to.to_lowercase();

    // 1. Zero-address recipient
    if to_lower == "0x0000000000000000000000000000000000000000" {
        return Err(
            "Transaction blocked: recipient is the zero address (0x0000...0000). \
             Sending to the zero address will burn funds permanently."
                .to_string(),
        );
    }

    // 2. Self-send detection (only for plain ETH transfers)
    if intent.tx_type == "eth_transfer" {
        if let Some(wallet_addr) = context.registers.get("wallet_address") {
            if let Some(addr_str) = wallet_addr.as_str() {
                if addr_str.to_lowercase() == to_lower {
                    return Err(
                        "Transaction blocked: you are sending ETH to your own wallet. \
                         This wastes gas with no effect. Please verify the recipient."
                            .to_string(),
                    );
                }
            }
        }
    }

    // 3. Recipient address should appear in registers or context bank
    //    (anti-hallucination check)
    if intent.tx_type == "eth_transfer" {
        let address_in_registers = address_exists_in_registers(&to_lower, context);
        let address_in_context_bank = address_exists_in_context_bank(&to_lower, context);

        if !address_in_registers && !address_in_context_bank {
            return Err(format!(
                "Transaction blocked: recipient address {} was not found in any register \
                 or in the context bank. This may indicate a hallucinated address. \
                 Use set_address to store the address first.",
                intent.to
            ));
        }
    }

    // 4. Swap sell amount verification (for swap_execute preset only)
    check_swap_sell_amount(intent, context)?;

    Ok(())
}

/// Check whether `addr` (lowercase) appears as a value in any register.
fn address_exists_in_registers(addr: &str, context: &ToolContext) -> bool {
    for key in context.registers.keys() {
        if let Some(val) = context.registers.get(&key) {
            if let Some(s) = val.as_str() {
                if s.to_lowercase() == addr {
                    return true;
                }
            }
        }
    }
    false
}

/// Check whether `addr` (lowercase) appears in the context bank's eth_address items.
fn address_exists_in_context_bank(addr: &str, context: &ToolContext) -> bool {
    for item in context.context_bank.items() {
        if item.item_type == "eth_address" && item.value.to_lowercase() == addr {
            return true;
        }
    }
    false
}

/// Check that the swap sell amount matches what the user stated in their message.
///
/// Only applies to `swap_execute` preset transactions. Reads `sell_amount`,
/// `sell_token_decimals`, and `sell_token_symbol` from registers, then parses
/// the user's original message for amounts paired with the sell token.
///
/// Handles shorthand: "1k" = 1,000, "1m" = 1,000,000, "1b" = 1,000,000,000.
/// Also handles comma-separated numbers and word multipliers ("1 million").
///
/// Fails open if any register is missing (the check simply cannot run).
/// Fails closed (blocks) only when a clear mismatch is found.
fn check_swap_sell_amount(
    intent: &TransactionIntent,
    context: &ToolContext,
) -> Result<(), String> {
    // Only applies to swap_execute preset
    if intent.preset_name.as_deref() != Some("swap_execute") {
        return Ok(());
    }

    // Read required registers — skip check if any is missing
    let sell_amount_raw = match context
        .registers
        .get("sell_amount")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
    {
        Some(s) => s,
        None => return Ok(()),
    };

    let decimals: u32 = match context.registers.get("sell_token_decimals") {
        Some(v) => {
            let d = v
                .as_u64()
                .map(|d| d as u32)
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()));
            match d {
                Some(d) => d,
                None => return Ok(()),
            }
        }
        None => return Ok(()),
    };

    let sell_symbol = match context.registers.get("sell_token_symbol") {
        Some(v) => match v.as_str() {
            Some(s) => s.to_uppercase(),
            None => return Ok(()),
        },
        None => return Ok(()),
    };

    let user_message = match context
        .extra
        .get("original_user_message")
        .and_then(|v| v.as_str())
    {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(()),
    };

    // Convert raw sell amount to human-readable
    let sell_raw: f64 = match sell_amount_raw.parse() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let human_sell = sell_raw / 10f64.powi(decimals as i32);

    // Extract amounts paired with the sell token symbol from the user's message
    let paired_amounts = extract_amount_for_token(user_message, &sell_symbol);
    if paired_amounts.is_empty() {
        // User didn't mention a specific amount with the sell token — can't verify
        return Ok(());
    }

    // Check if any extracted amount matches the actual sell amount
    for amount in &paired_amounts {
        if amounts_match(*amount, human_sell) {
            log::info!(
                "[verify_intent] Swap sell amount check PASSED: user said {} {}, register has {} {}",
                amount, sell_symbol, human_sell, sell_symbol
            );
            return Ok(());
        }
    }

    // Mismatch detected — block
    log::warn!(
        "[verify_intent] Swap sell amount MISMATCH: user said {:?} {}, but register has {} raw = {} {}",
        paired_amounts, sell_symbol, sell_amount_raw, human_sell, sell_symbol
    );
    Err(format!(
        "Transaction blocked: swap sell amount mismatch. \
         User requested {:?} {} but sell_amount register contains {} ({} {}). \
         Verify the correct amount before retrying.",
        paired_amounts, sell_symbol, sell_amount_raw, human_sell, sell_symbol,
    ))
}

/// Check if two amounts match within tolerance (0.1%).
fn amounts_match(a: f64, b: f64) -> bool {
    if a == 0.0 && b == 0.0 {
        return true;
    }
    if a == 0.0 || b == 0.0 {
        return false;
    }
    let ratio = if a > b { a / b } else { b / a };
    ratio <= 1.001 // 0.1% tolerance for floating-point arithmetic
}

/// Extract numeric amounts from a message that appear adjacent to a token symbol.
///
/// Handles: "1 USDC", "1.5 ETH", "1m USDC", "1,000 ETH", "USDC 100",
/// "1 million USDC", "$100 usdc".
fn extract_amount_for_token(message: &str, token_symbol: &str) -> Vec<f64> {
    let symbol_lower = token_symbol.to_lowercase();
    // Normalize: strip commas (thousands separators), lowercase
    let normalized = message.to_lowercase().replace(',', "");
    let tokens: Vec<&str> = normalized.split_whitespace().collect();

    let mut amounts = Vec::new();

    for i in 0..tokens.len() {
        // Pattern 1: "NUM SYMBOL" (e.g., "1 usdc", "1.5 eth", "1m usdc")
        if let Some(amount) = parse_amount_with_suffix(tokens[i]) {
            if i + 1 < tokens.len() && tokens[i + 1] == symbol_lower {
                amounts.push(amount);
                continue;
            }
            // Pattern 2: "NUM WORD_MULTIPLIER SYMBOL" (e.g., "1 million usdc")
            if i + 2 < tokens.len() && tokens[i + 2] == symbol_lower {
                if let Some(mult) = word_multiplier(tokens[i + 1]) {
                    amounts.push(amount * mult);
                    continue;
                }
            }
        }

        // Pattern 3: "SYMBOL NUM" (e.g., "usdc 1", "usdc 1m")
        if tokens[i] == symbol_lower && i + 1 < tokens.len() {
            if let Some(amount) = parse_amount_with_suffix(tokens[i + 1]) {
                amounts.push(amount);
            }
        }
    }

    amounts
}

/// Parse a numeric string with optional magnitude suffix.
///
/// Handles: "1", "1.5", "1k" (1,000), "1m" (1,000,000), "1b" (1,000,000,000),
/// "1mil", "1million", "$100", "1.5k".
fn parse_amount_with_suffix(s: &str) -> Option<f64> {
    let s = s.trim().strip_prefix('$').unwrap_or(s);
    if s.is_empty() {
        return None;
    }

    // Check suffixes (longest first to avoid partial matches)
    let suffixes: &[(&str, f64)] = &[
        ("billion", 1_000_000_000.0),
        ("million", 1_000_000.0),
        ("thousand", 1_000.0),
        ("bil", 1_000_000_000.0),
        ("mil", 1_000_000.0),
        ("b", 1_000_000_000.0),
        ("m", 1_000_000.0),
        ("k", 1_000.0),
    ];

    for (suffix, multiplier) in suffixes {
        if s.ends_with(suffix) {
            let num_str = &s[..s.len() - suffix.len()];
            if let Ok(n) = num_str.parse::<f64>() {
                return Some(n * multiplier);
            }
        }
    }

    // No suffix — try plain number
    s.parse::<f64>().ok()
}

/// Map English multiplier words to their numeric values.
fn word_multiplier(word: &str) -> Option<f64> {
    match word {
        "k" | "thousand" => Some(1_000.0),
        "m" | "mil" | "million" => Some(1_000_000.0),
        "b" | "bil" | "billion" => Some(1_000_000_000.0),
        _ => None,
    }
}

// ─── AI verification ─────────────────────────────────────────────────────────

const VERIFICATION_SYSTEM_PROMPT: &str = "\
You are a transaction safety verifier. Your job is to compare a user's original request \
with the transaction that was constructed, and determine whether they match.

Respond with EXACTLY one of these formats (no extra text):
  APPROVED
  REJECTED: <one-line reason>
  NEED_INFO: <what is missing>

Rules:
- APPROVED means the transaction clearly matches what the user asked for.
- REJECTED means there is a mismatch in recipient, amount, network, or operation type.
- NEED_INFO means the user's request is too vague to confirm the transaction.
- When in doubt, use REJECTED. It is always safer to block than to allow.
- Do NOT add any explanation beyond the single-line reason.";

fn format_verification_prompt(intent: &TransactionIntent, user_message: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str("## User's original message\n");
    prompt.push_str(user_message);
    prompt.push_str("\n\n## Constructed transaction\n");
    prompt.push_str(&format!("Type: {}\n", intent.tx_type));
    prompt.push_str(&format!("To: {}\n", intent.to));
    prompt.push_str(&format!("Value: {} ({})\n", intent.value, intent.value_display));
    prompt.push_str(&format!("Network: {}\n", intent.network));

    if let Some(ref name) = intent.function_name {
        prompt.push_str(&format!("Function: {}\n", name));
    }
    if let Some(ref abi) = intent.abi_name {
        prompt.push_str(&format!("ABI: {}\n", abi));
    }
    if let Some(ref preset) = intent.preset_name {
        prompt.push_str(&format!("Preset: {}\n", preset));
    }
    if let Some(ref dest) = intent.destination_chain {
        prompt.push_str(&format!("Destination chain: {}\n", dest));
    }

    prompt.push_str(&format!("\nDescription: {}\n", intent.description));
    prompt.push_str("\nDoes this transaction match the user's request?");
    prompt
}

/// Parse the AI verifier response.
///
/// Tolerant on APPROVED (accepts "APPROVED", "APPROVED.", "APPROVED - looks good",
/// anything whose first line starts with "APPROVED").
/// Strict on REJECTED/NEED_INFO (only blocks if explicitly present).
/// Unparseable responses pass with a warning — fail-open because deterministic
/// checks already ran and a flaky AI shouldn't block legitimate transactions.
fn parse_verification_response(response: &str) -> Result<(), String> {
    // Take the first non-empty line for classification
    let first_line = response
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");

    if first_line.starts_with("APPROVED") {
        log::info!("[verify_intent] APPROVED");
        return Ok(());
    }

    if first_line.starts_with("REJECTED:") {
        let reason = first_line.strip_prefix("REJECTED:").unwrap_or("").trim();
        log::warn!("[verify_intent] REJECTED: {}", reason);
        return Err(format!(
            "Transaction rejected by safety verifier: {}",
            reason
        ));
    }

    if first_line.starts_with("NEED_INFO:") {
        let info = first_line.strip_prefix("NEED_INFO:").unwrap_or("").trim();
        log::warn!("[verify_intent] NEED_INFO: {}", info);
        return Err(format!(
            "Transaction blocked — more information needed: {}",
            info
        ));
    }

    // Unparseable = fail-open (deterministic checks already ran)
    log::warn!(
        "[verify_intent] Unparseable AI response (allowing tx): {}",
        first_line
    );
    Ok(())
}

// ─── DB helper ───────────────────────────────────────────────────────────────

/// Build an AiClient from DB settings (same pattern as save_session_memory).
fn build_client_from_db(context: &ToolContext) -> Option<AiClient> {
    let db = context.database.as_ref()?;
    let settings = db.get_active_agent_settings().ok()??;
    AiClient::from_settings(&settings).ok()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::context_bank::ContextBankItem;
    use crate::tools::RegisterStore;

    // ── parse_verification_response ──────────────────────────────────

    #[test]
    fn test_parse_approved() {
        assert!(parse_verification_response("APPROVED").is_ok());
        assert!(parse_verification_response("  APPROVED  ").is_ok());
    }

    #[test]
    fn test_parse_rejected() {
        let err = parse_verification_response("REJECTED: wrong recipient").unwrap_err();
        assert!(err.contains("wrong recipient"), "got: {}", err);
    }

    #[test]
    fn test_parse_need_info() {
        let err = parse_verification_response("NEED_INFO: which address?").unwrap_err();
        assert!(err.contains("which address?"), "got: {}", err);
    }

    #[test]
    fn test_parse_garbage_fails_open() {
        // Unparseable AI response should NOT block (fail-open)
        assert!(parse_verification_response("sure thing buddy").is_ok());
    }

    #[test]
    fn test_parse_empty_fails_open() {
        assert!(parse_verification_response("").is_ok());
    }

    // ── nuisance-trip prevention ─────────────────────────────────────

    #[test]
    fn test_parse_approved_with_trailing_period() {
        assert!(parse_verification_response("APPROVED.").is_ok());
    }

    #[test]
    fn test_parse_approved_with_explanation() {
        assert!(parse_verification_response("APPROVED - transaction matches the user request").is_ok());
    }

    #[test]
    fn test_parse_approved_multiline() {
        assert!(parse_verification_response("APPROVED\n\nThe user asked to send 0.001 ETH.").is_ok());
    }

    #[test]
    fn test_parse_approved_with_leading_whitespace() {
        assert!(parse_verification_response("  APPROVED  ").is_ok());
    }

    // ── deterministic checks ─────────────────────────────────────────

    fn make_intent(tx_type: &str, to: &str) -> TransactionIntent {
        TransactionIntent {
            tx_type: tx_type.to_string(),
            to: to.to_string(),
            value: "1000000000000000".to_string(),
            value_display: "0.001 ETH".to_string(),
            network: "base".to_string(),
            function_name: None,
            abi_name: None,
            preset_name: None,
            destination_chain: None,
            calldata: None,
            description: "test tx".to_string(),
        }
    }

    #[test]
    fn test_zero_address_blocked() {
        let intent = make_intent(
            "eth_transfer",
            "0x0000000000000000000000000000000000000000",
        );
        let ctx = ToolContext::new();
        let err = run_deterministic_checks(&intent, &ctx).unwrap_err();
        assert!(err.contains("zero address"), "got: {}", err);
    }

    #[test]
    fn test_self_send_blocked() {
        let wallet = "0xAbCdEf1234567890AbCdEf1234567890AbCdEf12";
        let intent = make_intent("eth_transfer", wallet);

        let registers = RegisterStore::new();
        registers.set(
            "wallet_address",
            serde_json::json!(wallet),
            "wallet_provider",
        );
        // Also add send_to so register-exists check passes
        registers.set("send_to", serde_json::json!(wallet), "set_address");
        let ctx = ToolContext::new().with_registers(registers);

        let err = run_deterministic_checks(&intent, &ctx).unwrap_err();
        assert!(err.contains("own wallet"), "got: {}", err);
    }

    #[test]
    fn test_address_not_in_registers_or_context_blocked() {
        let intent = make_intent(
            "eth_transfer",
            "0x1111111111111111111111111111111111111111",
        );
        let ctx = ToolContext::new();
        let err = run_deterministic_checks(&intent, &ctx).unwrap_err();
        assert!(err.contains("not found in any register"), "got: {}", err);
    }

    #[test]
    fn test_address_in_register_passes() {
        let addr = "0x1111111111111111111111111111111111111111";
        let intent = make_intent("eth_transfer", addr);

        let registers = RegisterStore::new();
        registers.set("send_to", serde_json::json!(addr), "set_address");
        let ctx = ToolContext::new().with_registers(registers);

        assert!(run_deterministic_checks(&intent, &ctx).is_ok());
    }

    #[test]
    fn test_address_in_context_bank_passes() {
        let addr = "0x1111111111111111111111111111111111111111";
        let intent = make_intent("eth_transfer", addr);

        let mut ctx = ToolContext::new();
        ctx.context_bank.add(ContextBankItem {
            value: addr.to_string(),
            item_type: "eth_address".to_string(),
            label: None,
        });

        assert!(run_deterministic_checks(&intent, &ctx).is_ok());
    }

    #[test]
    fn test_contract_call_skips_register_check() {
        // Contract calls don't require the "to" address to be in registers
        // because the contract address comes from ABI files, not user input
        let intent = make_intent(
            "contract_call",
            "0x1111111111111111111111111111111111111111",
        );
        let ctx = ToolContext::new();
        assert!(run_deterministic_checks(&intent, &ctx).is_ok());
    }

    // ── format_verification_prompt ───────────────────────────────────

    #[test]
    fn test_format_verification_prompt_basic() {
        let intent = make_intent(
            "eth_transfer",
            "0x1111111111111111111111111111111111111111",
        );
        let prompt = format_verification_prompt(&intent, "send 0.001 ETH to alice");
        assert!(prompt.contains("send 0.001 ETH to alice"));
        assert!(prompt.contains("eth_transfer"));
        assert!(prompt.contains("0x1111"));
        assert!(prompt.contains("0.001 ETH"));
    }

    #[test]
    fn test_format_verification_prompt_with_function() {
        let mut intent = make_intent(
            "contract_call",
            "0x1111111111111111111111111111111111111111",
        );
        intent.function_name = Some("transfer".to_string());
        intent.abi_name = Some("erc20".to_string());
        let prompt = format_verification_prompt(&intent, "send 100 USDC");
        assert!(prompt.contains("transfer"));
        assert!(prompt.contains("erc20"));
    }

    // ── integration test with MockAiClient ───────────────────────────

    use crate::ai::{MockAiClient, AiResponse};

    fn mock_client(responses: Vec<&str>) -> AiClient {
        let ai_responses: Vec<Result<AiResponse, _>> = responses
            .into_iter()
            .map(|text| Ok(AiResponse::text(text.to_string())))
            .collect();
        AiClient::Mock(MockAiClient::new(ai_responses))
    }

    #[tokio::test]
    async fn test_verify_intent_approved_by_mock() {
        let mock = mock_client(vec!["APPROVED"]);
        let addr = "0x1111111111111111111111111111111111111111";
        let intent = make_intent("eth_transfer", addr);

        let registers = RegisterStore::new();
        registers.set("send_to", serde_json::json!(addr), "set_address");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("send 0.001 ETH to 0x1111"),
        );

        let result = verify_intent(&intent, &ctx, Some(&mock)).await;
        assert!(result.is_ok(), "Expected APPROVED, got: {:?}", result);
    }

    #[tokio::test]
    async fn test_verify_intent_rejected_by_mock() {
        let mock = mock_client(vec!["REJECTED: amount mismatch"]);
        let addr = "0x1111111111111111111111111111111111111111";
        let intent = make_intent("eth_transfer", addr);

        let registers = RegisterStore::new();
        registers.set("send_to", serde_json::json!(addr), "set_address");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("send 0.001 ETH"),
        );

        let result = verify_intent(&intent, &ctx, Some(&mock)).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("amount mismatch"),
            "Expected mismatch reason"
        );
    }

    #[tokio::test]
    async fn test_verify_intent_no_user_message_still_passes() {
        // When original_user_message is missing, AI check is skipped
        // but deterministic checks still run
        let addr = "0x1111111111111111111111111111111111111111";
        let intent = make_intent("eth_transfer", addr);

        let registers = RegisterStore::new();
        registers.set("send_to", serde_json::json!(addr), "set_address");
        let ctx = ToolContext::new().with_registers(registers);

        let result = verify_intent(&intent, &ctx, None).await;
        assert!(result.is_ok(), "Should pass without AI check: {:?}", result);
    }

    // ── nuisance-trip integration tests ──────────────────────────────

    #[tokio::test]
    async fn test_verify_intent_ai_error_does_not_block() {
        // AI network error should NOT block a transaction that passed deterministic checks
        use crate::ai::AiError;
        let ai_responses: Vec<Result<AiResponse, AiError>> =
            vec![Err(AiError::new("connection timeout"))];
        let mock = AiClient::Mock(MockAiClient::new(ai_responses));

        let addr = "0x1111111111111111111111111111111111111111";
        let intent = make_intent("eth_transfer", addr);

        let registers = RegisterStore::new();
        registers.set("send_to", serde_json::json!(addr), "set_address");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("send 0.001 ETH to 0x1111"),
        );

        let result = verify_intent(&intent, &ctx, Some(&mock)).await;
        assert!(
            result.is_ok(),
            "AI error should not block: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_verify_intent_vague_user_message_with_approval() {
        // Multi-turn: user just says "yes" but AI still approves
        let mock = mock_client(vec!["APPROVED"]);
        let addr = "0x1111111111111111111111111111111111111111";
        let intent = make_intent("eth_transfer", addr);

        let registers = RegisterStore::new();
        registers.set("send_to", serde_json::json!(addr), "set_address");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("yes do it"),
        );

        let result = verify_intent(&intent, &ctx, Some(&mock)).await;
        assert!(result.is_ok(), "Should pass: {:?}", result);
    }

    #[tokio::test]
    async fn test_verify_intent_ai_returns_chatty_approval() {
        // AI doesn't follow format perfectly but starts with APPROVED
        let mock = mock_client(vec![
            "APPROVED. The user requested 0.001 ETH to the correct address."
        ]);
        let addr = "0x1111111111111111111111111111111111111111";
        let intent = make_intent("eth_transfer", addr);

        let registers = RegisterStore::new();
        registers.set("send_to", serde_json::json!(addr), "set_address");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("send 0.001 ETH to 0x1111"),
        );

        let result = verify_intent(&intent, &ctx, Some(&mock)).await;
        assert!(result.is_ok(), "Chatty APPROVED should still pass: {:?}", result);
    }

    #[tokio::test]
    async fn test_verify_intent_ai_returns_gibberish_does_not_block() {
        // AI returns nonsense — should NOT block (fail-open)
        let mock = mock_client(vec!["I'm not sure what you mean"]);
        let addr = "0x1111111111111111111111111111111111111111";
        let intent = make_intent("eth_transfer", addr);

        let registers = RegisterStore::new();
        registers.set("send_to", serde_json::json!(addr), "set_address");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("send 0.001 ETH to 0x1111"),
        );

        let result = verify_intent(&intent, &ctx, Some(&mock)).await;
        assert!(result.is_ok(), "Gibberish should fail-open: {:?}", result);
    }

    #[tokio::test]
    async fn test_normal_send_eth_flow_passes_deterministic() {
        // Simulates the actual send_eth flow: send_to register is always set
        // before send_eth runs, so anti-hallucination check should pass
        let addr = "0xAbCdEf1234567890AbCdEf1234567890AbCdEf12";
        let wallet = "0x9999999999999999999999999999999999999999";

        let intent = TransactionIntent {
            tx_type: "eth_transfer".to_string(),
            to: addr.to_string(),
            value: "10000000000000000".to_string(),
            value_display: "0.01 ETH".to_string(),
            network: "base".to_string(),
            function_name: None,
            abi_name: None,
            preset_name: None,
            destination_chain: None,
            calldata: None,
            description: "Send 0.01 ETH".to_string(),
        };

        // Set up registers exactly as the real send_eth flow does
        let registers = RegisterStore::new();
        registers.set("send_to", serde_json::json!(addr), "set_address");
        registers.set("amount_raw", serde_json::json!("10000000000000000"), "to_raw_amount");
        registers.set("wallet_address", serde_json::json!(wallet), "wallet_provider");
        let ctx = ToolContext::new().with_registers(registers);

        // No AI override, no user message — just deterministic checks
        let result = verify_intent(&intent, &ctx, None).await;
        assert!(result.is_ok(), "Normal send_eth flow must pass: {:?}", result);
    }

    #[tokio::test]
    async fn test_contract_call_no_register_needed() {
        // Contract calls (function_call, preset, bridge) should not require
        // the contract address to be in registers — only eth_transfer does
        let mock = mock_client(vec!["APPROVED"]);

        let intent = TransactionIntent {
            tx_type: "contract_call".to_string(),
            to: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".to_string(),
            value: "0".to_string(),
            value_display: "0 ETH".to_string(),
            network: "base".to_string(),
            function_name: Some("transfer".to_string()),
            abi_name: Some("erc20".to_string()),
            preset_name: None,
            destination_chain: None,
            calldata: None,
            description: "ERC20 transfer".to_string(),
        };

        // No registers at all — should still pass deterministic checks
        let mut ctx = ToolContext::new();
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("send 100 USDC to alice"),
        );

        let result = verify_intent(&intent, &ctx, Some(&mock)).await;
        assert!(result.is_ok(), "Contract call should not need address in register: {:?}", result);
    }

    #[tokio::test]
    async fn test_bridge_no_register_needed() {
        let mock = mock_client(vec!["APPROVED"]);

        let intent = TransactionIntent {
            tx_type: "bridge".to_string(),
            to: "0xABCD1234ABCD1234ABCD1234ABCD1234ABCD1234".to_string(),
            value: "0".to_string(),
            value_display: "100 USDC".to_string(),
            network: "base".to_string(),
            function_name: None,
            abi_name: None,
            preset_name: None,
            destination_chain: Some("polygon".to_string()),
            calldata: None,
            description: "Bridge 100 USDC from base to polygon".to_string(),
        };

        let mut ctx = ToolContext::new();
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("bridge 100 USDC to polygon"),
        );

        let result = verify_intent(&intent, &ctx, Some(&mock)).await;
        assert!(result.is_ok(), "Bridge should not need address in register: {:?}", result);
    }

    // ── parse_amount_with_suffix ──────────────────────────────────────

    #[test]
    fn test_parse_amount_plain_numbers() {
        assert_eq!(parse_amount_with_suffix("1"), Some(1.0));
        assert_eq!(parse_amount_with_suffix("100"), Some(100.0));
        assert_eq!(parse_amount_with_suffix("1.5"), Some(1.5));
        assert_eq!(parse_amount_with_suffix("0.001"), Some(0.001));
    }

    #[test]
    fn test_parse_amount_suffix_k() {
        assert_eq!(parse_amount_with_suffix("1k"), Some(1_000.0));
        assert_eq!(parse_amount_with_suffix("1.5k"), Some(1_500.0));
        assert_eq!(parse_amount_with_suffix("100k"), Some(100_000.0));
    }

    #[test]
    fn test_parse_amount_suffix_m() {
        assert_eq!(parse_amount_with_suffix("1m"), Some(1_000_000.0));
        assert_eq!(parse_amount_with_suffix("1.5m"), Some(1_500_000.0));
        assert_eq!(parse_amount_with_suffix("1mil"), Some(1_000_000.0));
        assert_eq!(parse_amount_with_suffix("1million"), Some(1_000_000.0));
    }

    #[test]
    fn test_parse_amount_suffix_b() {
        assert_eq!(parse_amount_with_suffix("1b"), Some(1_000_000_000.0));
        assert_eq!(parse_amount_with_suffix("2.5bil"), Some(2_500_000_000.0));
        assert_eq!(parse_amount_with_suffix("1billion"), Some(1_000_000_000.0));
    }

    #[test]
    fn test_parse_amount_dollar_prefix() {
        assert_eq!(parse_amount_with_suffix("$100"), Some(100.0));
        assert_eq!(parse_amount_with_suffix("$1m"), Some(1_000_000.0));
        assert_eq!(parse_amount_with_suffix("$1.5k"), Some(1_500.0));
    }

    #[test]
    fn test_parse_amount_invalid() {
        assert_eq!(parse_amount_with_suffix(""), None);
        assert_eq!(parse_amount_with_suffix("abc"), None);
        assert_eq!(parse_amount_with_suffix("usdc"), None);
    }

    // ── extract_amount_for_token ──────────────────────────────────────

    #[test]
    fn test_extract_amount_basic() {
        let amounts = extract_amount_for_token("swap 1 USDC to STARKBOT", "USDC");
        assert_eq!(amounts, vec![1.0]);
    }

    #[test]
    fn test_extract_amount_decimal() {
        let amounts = extract_amount_for_token("swap 0.5 ETH for USDC", "ETH");
        assert_eq!(amounts, vec![0.5]);
    }

    #[test]
    fn test_extract_amount_suffix_m() {
        let amounts = extract_amount_for_token("swap 1m USDC to STARKBOT", "USDC");
        assert_eq!(amounts, vec![1_000_000.0]);
    }

    #[test]
    fn test_extract_amount_suffix_k() {
        let amounts = extract_amount_for_token("convert 10k USDC to ETH", "USDC");
        assert_eq!(amounts, vec![10_000.0]);
    }

    #[test]
    fn test_extract_amount_commas() {
        let amounts = extract_amount_for_token("swap 1,000 USDC for STARKBOT", "USDC");
        assert_eq!(amounts, vec![1_000.0]);
    }

    #[test]
    fn test_extract_amount_word_multiplier() {
        let amounts = extract_amount_for_token("swap 1 million USDC to ETH", "USDC");
        assert_eq!(amounts, vec![1_000_000.0]);
    }

    #[test]
    fn test_extract_amount_symbol_first() {
        // "USDC 100" pattern
        let amounts = extract_amount_for_token("sell USDC 100 for ETH", "USDC");
        assert_eq!(amounts, vec![100.0]);
    }

    #[test]
    fn test_extract_amount_case_insensitive() {
        let amounts = extract_amount_for_token("swap 1 usdc to starkbot", "USDC");
        assert_eq!(amounts, vec![1.0]);
    }

    #[test]
    fn test_extract_amount_no_match() {
        // Amount is for buy token, not sell token
        let amounts = extract_amount_for_token("buy 50 STARKBOT", "USDC");
        assert!(amounts.is_empty());
    }

    #[test]
    fn test_extract_amount_no_number() {
        let amounts = extract_amount_for_token("swap all my USDC", "USDC");
        assert!(amounts.is_empty());
    }

    #[test]
    fn test_extract_amount_vague_message() {
        let amounts = extract_amount_for_token("yes do it", "USDC");
        assert!(amounts.is_empty());
    }

    // ── amounts_match ─────────────────────────────────────────────────

    #[test]
    fn test_amounts_match_exact() {
        assert!(amounts_match(1.0, 1.0));
        assert!(amounts_match(1000000.0, 1000000.0));
    }

    #[test]
    fn test_amounts_match_within_tolerance() {
        assert!(amounts_match(1.0, 1.0009)); // 0.09% off
    }

    #[test]
    fn test_amounts_mismatch() {
        assert!(!amounts_match(1.0, 2.0));
        assert!(!amounts_match(1.0, 100.0));
        assert!(!amounts_match(1.0, 1000000.0));
    }

    #[test]
    fn test_amounts_match_zero() {
        assert!(amounts_match(0.0, 0.0));
        assert!(!amounts_match(0.0, 1.0));
        assert!(!amounts_match(1.0, 0.0));
    }

    // ── check_swap_sell_amount (deterministic) ────────────────────────

    fn make_swap_intent() -> TransactionIntent {
        TransactionIntent {
            tx_type: "preset_call".to_string(),
            to: "0x1111111111111111111111111111111111111111".to_string(),
            value: "0".to_string(),
            value_display: "0 ETH".to_string(),
            network: "base".to_string(),
            function_name: Some("exec".to_string()),
            abi_name: Some("0x_settler".to_string()),
            preset_name: Some("swap_execute".to_string()),
            destination_chain: None,
            calldata: None,
            description: "Swap via 0x".to_string(),
        }
    }

    #[test]
    fn test_swap_amount_check_passes_matching() {
        // User says "swap 1 USDC", sell_amount = 1000000 (1 USDC at 6 decimals)
        let intent = make_swap_intent();
        let registers = RegisterStore::new();
        registers.set("sell_amount", serde_json::json!("1000000"), "to_raw_amount");
        registers.set("sell_token_decimals", serde_json::json!(6), "token_lookup");
        registers.set("sell_token_symbol", serde_json::json!("USDC"), "token_lookup");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("swap 1 USDC to STARKBOT"),
        );

        assert!(check_swap_sell_amount(&intent, &ctx).is_ok());
    }

    #[test]
    fn test_swap_amount_check_passes_1m_shorthand() {
        // User says "swap 1m USDC", sell_amount = 1000000000000 (1M USDC)
        let intent = make_swap_intent();
        let registers = RegisterStore::new();
        registers.set("sell_amount", serde_json::json!("1000000000000"), "to_raw_amount");
        registers.set("sell_token_decimals", serde_json::json!(6), "token_lookup");
        registers.set("sell_token_symbol", serde_json::json!("USDC"), "token_lookup");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("swap 1m USDC to STARKBOT"),
        );

        assert!(check_swap_sell_amount(&intent, &ctx).is_ok());
    }

    #[test]
    fn test_swap_amount_check_blocks_mismatch() {
        // User says "swap 1 USDC" but sell_amount is 1000 USDC (wrong!)
        let intent = make_swap_intent();
        let registers = RegisterStore::new();
        registers.set("sell_amount", serde_json::json!("1000000000"), "to_raw_amount"); // 1000 USDC
        registers.set("sell_token_decimals", serde_json::json!(6), "token_lookup");
        registers.set("sell_token_symbol", serde_json::json!("USDC"), "token_lookup");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("swap 1 USDC to STARKBOT"),
        );

        let result = check_swap_sell_amount(&intent, &ctx);
        assert!(result.is_err(), "Should block mismatched amount");
        assert!(result.unwrap_err().contains("mismatch"), "Should mention mismatch");
    }

    #[test]
    fn test_swap_amount_check_passes_eth_18_decimals() {
        // User says "swap 0.5 ETH", sell_amount = 500000000000000000 (0.5 ETH)
        let intent = make_swap_intent();
        let registers = RegisterStore::new();
        registers.set("sell_amount", serde_json::json!("500000000000000000"), "to_raw_amount");
        registers.set("sell_token_decimals", serde_json::json!(18), "token_lookup");
        registers.set("sell_token_symbol", serde_json::json!("ETH"), "token_lookup");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("swap 0.5 ETH for USDC"),
        );

        assert!(check_swap_sell_amount(&intent, &ctx).is_ok());
    }

    #[test]
    fn test_swap_amount_check_skips_no_amount_in_message() {
        // User says "swap some USDC" — no parseable amount, should skip (pass)
        let intent = make_swap_intent();
        let registers = RegisterStore::new();
        registers.set("sell_amount", serde_json::json!("1000000"), "to_raw_amount");
        registers.set("sell_token_decimals", serde_json::json!(6), "token_lookup");
        registers.set("sell_token_symbol", serde_json::json!("USDC"), "token_lookup");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("swap some USDC for STARKBOT"),
        );

        assert!(check_swap_sell_amount(&intent, &ctx).is_ok());
    }

    #[test]
    fn test_swap_amount_check_skips_buy_amount_only() {
        // User says "buy 50 STARKBOT" — amount is for buy token, not sell token
        let intent = make_swap_intent();
        let registers = RegisterStore::new();
        registers.set("sell_amount", serde_json::json!("970000"), "to_raw_amount");
        registers.set("sell_token_decimals", serde_json::json!(6), "token_lookup");
        registers.set("sell_token_symbol", serde_json::json!("USDC"), "token_lookup");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("buy 50 STARKBOT"),
        );

        assert!(check_swap_sell_amount(&intent, &ctx).is_ok(), "Should skip when no sell amount in message");
    }

    #[test]
    fn test_swap_amount_check_skips_missing_registers() {
        // Missing sell_amount register — should skip (pass)
        let intent = make_swap_intent();
        let ctx = ToolContext::new();
        assert!(check_swap_sell_amount(&intent, &ctx).is_ok());
    }

    #[test]
    fn test_swap_amount_check_skips_non_swap() {
        // Not a swap_execute preset — should skip
        let intent = make_intent("contract_call", "0x1111111111111111111111111111111111111111");
        let ctx = ToolContext::new();
        assert!(check_swap_sell_amount(&intent, &ctx).is_ok());
    }

    #[test]
    fn test_swap_amount_check_skips_vague_message() {
        // Multi-turn: user just says "yes" — should skip
        let intent = make_swap_intent();
        let registers = RegisterStore::new();
        registers.set("sell_amount", serde_json::json!("1000000"), "to_raw_amount");
        registers.set("sell_token_decimals", serde_json::json!(6), "token_lookup");
        registers.set("sell_token_symbol", serde_json::json!("USDC"), "token_lookup");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("yes do it"),
        );

        assert!(check_swap_sell_amount(&intent, &ctx).is_ok());
    }

    #[test]
    fn test_swap_amount_check_commas_in_number() {
        // "1,000 USDC" should match 1000 USDC
        let intent = make_swap_intent();
        let registers = RegisterStore::new();
        registers.set("sell_amount", serde_json::json!("1000000000"), "to_raw_amount"); // 1000 USDC
        registers.set("sell_token_decimals", serde_json::json!(6), "token_lookup");
        registers.set("sell_token_symbol", serde_json::json!("USDC"), "token_lookup");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("swap 1,000 USDC to STARKBOT"),
        );

        assert!(check_swap_sell_amount(&intent, &ctx).is_ok());
    }

    #[test]
    fn test_swap_amount_check_word_million() {
        // "1 million USDC"
        let intent = make_swap_intent();
        let registers = RegisterStore::new();
        registers.set("sell_amount", serde_json::json!("1000000000000"), "to_raw_amount");
        registers.set("sell_token_decimals", serde_json::json!(6), "token_lookup");
        registers.set("sell_token_symbol", serde_json::json!("USDC"), "token_lookup");
        let mut ctx = ToolContext::new().with_registers(registers);
        ctx.extra.insert(
            "original_user_message".to_string(),
            serde_json::json!("swap 1 million USDC to ETH"),
        );

        assert!(check_swap_sell_amount(&intent, &ctx).is_ok());
    }
}
