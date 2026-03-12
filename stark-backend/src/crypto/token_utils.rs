//! Token lookup and amount conversion utilities.
//!
//! Self-contained reimplementation of token_lookup, to_raw_amount, from_raw_amount.

use serde::{Deserialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

static TOKENS: OnceLock<HashMap<String, HashMap<String, TokenInfo>>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
pub struct TokenInfo {
    pub address: String,
    pub decimals: u8,
    pub name: String,
}

/// Load tokens from config/tokens.ron
pub fn load_tokens(config_dir: &Path) {
    let tokens_path = config_dir.join("tokens.ron");
    if !tokens_path.exists() {
        log::warn!("[tokens] Config file not found: {:?}", tokens_path);
        return;
    }
    let content = std::fs::read_to_string(&tokens_path)
        .unwrap_or_else(|e| panic!("[tokens] Failed to read {:?}: {}", tokens_path, e));
    let tokens: HashMap<String, HashMap<String, TokenInfo>> = ron::from_str(&content)
        .unwrap_or_else(|e| panic!("[tokens] Failed to parse {:?}: {}", tokens_path, e));
    let total: usize = tokens.values().map(|t| t.len()).sum();
    log::info!("[tokens] Loaded {} tokens across {} networks", total, tokens.len());
    let _ = TOKENS.set(tokens);
}

fn get_tokens() -> &'static HashMap<String, HashMap<String, TokenInfo>> {
    TOKENS.get().expect("[tokens] Token config not loaded — call load_tokens() first")
}

/// Look up a token by symbol and network.
pub fn lookup(symbol: &str, network: &str) -> Option<TokenInfo> {
    let symbol_upper = symbol.to_uppercase();
    let tokens = get_tokens();
    tokens.get(network)
        .or_else(|| tokens.get("base"))
        .and_then(|nt| nt.get(&symbol_upper))
        .cloned()
}

/// Look up a token and return JSON result.
pub fn lookup_token(symbol: &str, network: &str) -> Result<Value, String> {
    match lookup(symbol, network) {
        Some(info) => Ok(json!({
            "symbol": symbol.to_uppercase(),
            "address": info.address,
            "decimals": info.decimals,
            "name": info.name,
            "network": network,
        })),
        None => Err(format!("Token '{}' not found on {}", symbol, network)),
    }
}

/// Convert human-readable amount to raw blockchain units.
pub fn to_raw_amount(amount: &str, decimals: u8) -> Result<String, String> {
    let amount = amount.trim();
    let (integer_part, decimal_part) = if let Some(dot_pos) = amount.find('.') {
        let int_str = &amount[..dot_pos];
        let dec_str = &amount[dot_pos + 1..];
        if !int_str.is_empty() && !int_str.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("Invalid integer part: '{}'", int_str));
        }
        if !dec_str.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("Invalid decimal part: '{}'", dec_str));
        }
        (if int_str.is_empty() { "0" } else { int_str }, dec_str)
    } else {
        if !amount.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("Invalid amount: '{}'. Must be a number.", amount));
        }
        (amount, "")
    };

    let decimals = decimals as usize;
    let decimal_len = decimal_part.len();
    if decimal_len > decimals {
        return Err(format!("Amount '{}' has {} decimal places but token only has {} decimals", amount, decimal_len, decimals));
    }
    let zeros_to_add = decimals - decimal_len;
    let raw = format!("{}{}{}", integer_part, decimal_part, "0".repeat(zeros_to_add));
    let raw = raw.trim_start_matches('0');
    if raw.is_empty() { Ok("0".to_string()) } else { Ok(raw.to_string()) }
}

/// Convert raw blockchain units to human-readable amount.
pub fn from_raw_amount(raw: &str, decimals: u8) -> Result<String, String> {
    let raw = raw.trim().trim_matches('"');
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("Invalid raw amount: '{}'", raw));
    }
    let raw = raw.trim_start_matches('0');
    let raw = if raw.is_empty() { "0" } else { raw };
    let decimals = decimals as usize;
    if decimals == 0 { return Ok(raw.to_string()); }

    if raw.len() <= decimals {
        let leading_zeros = decimals - raw.len();
        let decimal_part = format!("{}{}", "0".repeat(leading_zeros), raw);
        let trimmed = decimal_part.trim_end_matches('0');
        if trimmed.is_empty() { Ok("0".to_string()) } else { Ok(format!("0.{}", trimmed)) }
    } else {
        let split_pos = raw.len() - decimals;
        let integer_part = &raw[..split_pos];
        let decimal_part = raw[split_pos..].trim_end_matches('0');
        if decimal_part.is_empty() { Ok(integer_part.to_string()) } else { Ok(format!("{}.{}", integer_part, decimal_part)) }
    }
}
