//! Starflask integration bridge.
//!
//! Thin layer over the `starflask` crate for connecting to Starflask AI.

pub use starflask::Starflask;

use crate::crypto_executor::CryptoInstruction;
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

/// Extract URLs from free-form text by whitespace-splitting and checking for URL prefixes.
///
/// Handles both bare URLs and URLs wrapped in quotes (common in JSON strings).
pub fn extract_urls_from_text(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| word.trim_start_matches(|c: char| c == '"' || c == '\'' || c == '<' || c == '[' || c == '('))
        .filter(|word| word.starts_with("http://") || word.starts_with("https://"))
        .map(|url| url.trim_end_matches(|c: char| c == ',' || c == ')' || c == ']' || c == '>' || c == '"' || c == '\'' || c == '}' || c == '{').to_string())
        .collect()
}

/// Extract `structured_data` from a session result blob.
///
/// The session result is shaped like `{ summary, structured_data, iterations, actions_taken, ... }`.
/// Returns the `structured_data` value if present and not null.
pub fn parse_structured_data(result: &Option<Value>) -> Option<Value> {
    let value = result.as_ref()?;
    let sd = value.get("structured_data")?;
    if sd.is_null() {
        return None;
    }
    Some(sd.clone())
}

/// Extract media URLs from a session result (image/video generation).
///
/// Checks structured fields first (`urls`, `url`, `media`), then falls back to
/// scanning text fields in `result` and the optional `result_summary` for URLs.
pub fn parse_media_result(result: &Option<Value>, result_summary: Option<&str>) -> Vec<String> {
    let Some(value) = result else {
        // No structured result — try result_summary text
        if let Some(summary) = result_summary {
            let urls = extract_urls_from_text(summary);
            if !urls.is_empty() { return urls; }
        }
        return vec![];
    };

    // Try "urls" array
    if let Some(urls) = value.get("urls").and_then(|v| v.as_array()) {
        let extracted: Vec<String> = urls.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        if !extracted.is_empty() { return extracted; }
    }

    // Try "url" string
    if let Some(url) = value.get("url").and_then(|v| v.as_str()) {
        return vec![url.to_string()];
    }

    // Try "media" array
    if let Some(media) = value.get("media").and_then(|v| v.as_array()) {
        let extracted: Vec<String> = media.iter()
            .filter_map(|m| m.get("url").and_then(|v| v.as_str()).map(String::from))
            .collect();
        if !extracted.is_empty() { return extracted; }
    }

    // Fallback: scan text fields in the result object for URLs
    for key in &["text", "message", "response", "summary"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            let urls = extract_urls_from_text(text);
            if !urls.is_empty() { return urls; }
        }
    }

    // Fallback: if the result itself is a string, scan it
    if let Some(text) = value.as_str() {
        let urls = extract_urls_from_text(text);
        if !urls.is_empty() { return urls; }
    }

    // Final fallback: scan result_summary
    if let Some(summary) = result_summary {
        let urls = extract_urls_from_text(summary);
        if !urls.is_empty() { return urls; }
    }

    vec![]
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
    if let Some(text) = value.get("summary").and_then(|v| v.as_str()) {
        return text.to_string();
    }

    // If it's a plain string
    if let Some(text) = value.as_str() {
        return text.to_string();
    }

    // Fallback: JSON representation
    serde_json::to_string_pretty(value).unwrap_or_default()
}
