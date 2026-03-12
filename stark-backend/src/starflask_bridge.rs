//! Starflask integration bridge.
//!
//! Thin layer over the `starflask` crate for connecting to Starflask AI.

pub use starflask::Starflask;

use crate::crypto_executor::{CryptoInstruction, ExecutionResult};
use serde_json::Value;
use uuid::Uuid;

const DEFAULT_STARFLASK_URL: &str = "https://starflask.com/api";

/// Create a Starflask client from environment variables.
pub fn create_starflask_client() -> Option<Starflask> {
    let api_key = std::env::var("STARFLASK_API_KEY").ok()?;
    let base_url = std::env::var("STARFLASK_BASE_URL").ok();
    let url = base_url.as_deref().unwrap_or(DEFAULT_STARFLASK_URL);
    match Starflask::new(&api_key, Some(url)) {
        Ok(client) => Some(client),
        Err(e) => {
            log::error!("Failed to create Starflask client: {}", e);
            None
        }
    }
}

/// Create a Starflask client, trying env var first then falling back to DB.
pub fn create_starflask_client_with_db(db: &crate::db::Database) -> Option<Starflask> {
    // Try env var first
    if let Some(client) = create_starflask_client() {
        return Some(client);
    }

    // Fallback: try loading from DB (saved via API keys page)
    let api_key = db.get_api_key("STARFLASK_API_KEY").ok()??.api_key;
    if api_key.is_empty() {
        return None;
    }

    let base_url = std::env::var("STARFLASK_BASE_URL").ok();
    let url = base_url.as_deref().unwrap_or(DEFAULT_STARFLASK_URL);
    match Starflask::new(&api_key, Some(url)) {
        Ok(client) => {
            log::info!("Starflask client initialized from database API key");
            Some(client)
        }
        Err(e) => {
            log::error!("Failed to create Starflask client from DB key: {}", e);
            None
        }
    }
}

/// Get the default agent ID from environment.
pub fn default_agent_id() -> Option<Uuid> {
    std::env::var("STARFLASK_AGENT_ID").ok()
        .and_then(|s| Uuid::parse_str(&s).ok())
}

/// Parse a Starflask session result into crypto instructions.
pub fn parse_session_result(result: &Option<Value>) -> Vec<CryptoInstruction> {
    let Some(value) = result else { return vec![]; };

    // Try single instruction
    if let Ok(instr) = serde_json::from_value::<CryptoInstruction>(value.clone()) {
        return vec![instr];
    }

    // Try array of instructions
    if let Some(arr) = value.as_array() {
        return arr.iter()
            .filter_map(|v| serde_json::from_value::<CryptoInstruction>(v.clone()).ok())
            .collect();
    }

    // Try nested under "instructions" key
    if let Some(instrs) = value.get("instructions").and_then(|v| v.as_array()) {
        return instrs.iter()
            .filter_map(|v| serde_json::from_value::<CryptoInstruction>(v.clone()).ok())
            .collect();
    }

    vec![]
}

/// Format execution results for reporting back to Starflask.
pub fn format_results(results: &[ExecutionResult]) -> Value {
    serde_json::json!({
        "results": results,
    })
}

/// Extract media URLs from a session result (image/video generation).
pub fn parse_media_result(result: &Option<Value>) -> Vec<String> {
    let Some(value) = result else { return vec![]; };

    // Try "urls" array
    if let Some(urls) = value.get("urls").and_then(|v| v.as_array()) {
        return urls.iter().filter_map(|v| v.as_str().map(String::from)).collect();
    }

    // Try "url" string
    if let Some(url) = value.get("url").and_then(|v| v.as_str()) {
        return vec![url.to_string()];
    }

    // Try "media" array
    if let Some(media) = value.get("media").and_then(|v| v.as_array()) {
        return media.iter()
            .filter_map(|m| m.get("url").and_then(|v| v.as_str()).map(String::from))
            .collect();
    }

    vec![]
}

/// Extract social media post result (post URL + confirmation text).
pub fn parse_social_result(result: &Option<Value>) -> (Option<String>, String) {
    let Some(value) = result else {
        return (None, "No result".to_string());
    };

    let post_url = value.get("post_url")
        .or_else(|| value.get("url"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let confirmation = value.get("confirmation")
        .or_else(|| value.get("message"))
        .or_else(|| value.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("Post submitted")
        .to_string();

    (post_url, confirmation)
}

/// A delegation instruction from the general agent to a specialist.
#[derive(Debug, Clone)]
pub struct DelegationInstruction {
    pub delegate: String,
    pub message: String,
}

/// Check if a Starflask session result contains a delegation instruction.
///
/// The general agent may return `{"delegate": "<capability>", "message": "..."}` either
/// as the structured result directly, or embedded as JSON within a text field.
pub fn parse_delegation_result(result: &Option<Value>) -> Option<DelegationInstruction> {
    let value = result.as_ref()?;

    // Try direct JSON: result is {"delegate": "...", "message": "..."}
    if let Some(instr) = try_parse_delegation(value) {
        return Some(instr);
    }

    // Try nested objects that might directly contain delegation keys
    for key in &["structured_data"] {
        if let Some(obj) = value.get(key) {
            if let Some(instr) = try_parse_delegation(obj) {
                return Some(instr);
            }
        }
    }

    // Try extracting from text fields — the LLM might wrap JSON in prose
    for key in &["text", "message", "response", "summary", "structured_data"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            if let Some(instr) = extract_delegation_from_text(text) {
                return Some(instr);
            }
        }
    }

    // Try if the result itself is a string containing JSON
    if let Some(text) = value.as_str() {
        if let Some(instr) = extract_delegation_from_text(text) {
            return Some(instr);
        }
    }

    None
}

fn try_parse_delegation(value: &Value) -> Option<DelegationInstruction> {
    let delegate = value.get("delegate")?.as_str()?;
    let message = value.get("message").and_then(|v| v.as_str()).unwrap_or("");
    Some(DelegationInstruction {
        delegate: delegate.to_string(),
        message: message.to_string(),
    })
}

fn extract_delegation_from_text(text: &str) -> Option<DelegationInstruction> {
    // Find first '{' and last '}' to extract embedded JSON
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    let json_str = &text[start..=end];
    let parsed: Value = serde_json::from_str(json_str).ok()?;
    try_parse_delegation(&parsed)
}

/// Extract plain text from a session result.
pub fn parse_text_result(result: &Option<Value>) -> String {
    let Some(value) = result else {
        return String::new();
    };

    // Try common text fields
    if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(text) = value.get("message").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(text) = value.get("response").and_then(|v| v.as_str()) {
        return text.to_string();
    }

    // If it's a plain string
    if let Some(text) = value.as_str() {
        return text.to_string();
    }

    // Fallback: JSON representation
    serde_json::to_string_pretty(value).unwrap_or_default()
}
