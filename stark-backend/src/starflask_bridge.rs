//! Starflask integration bridge.
//!
//! Thin layer over the `starflask` crate for connecting to Starflask AI.

pub use starflask::Starflask;

use crate::crypto_executor::{CryptoInstruction, ExecutionResult};
use serde_json::Value;
use uuid::Uuid;

/// Create a Starflask client from environment variables.
pub fn create_starflask_client() -> Option<Starflask> {
    let api_key = std::env::var("STARFLASK_API_KEY").ok()?;
    let base_url = std::env::var("STARFLASK_BASE_URL").ok();
    match Starflask::new(&api_key, base_url.as_deref()) {
        Ok(client) => Some(client),
        Err(e) => {
            log::error!("Failed to create Starflask client: {}", e);
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
