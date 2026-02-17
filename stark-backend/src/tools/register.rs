//! Register Store for passing data between tools safely
//!
//! This module provides a CPU-like "register" system where tool outputs can be
//! cached and later retrieved by other tools. This prevents hallucination of
//! critical data (like transaction parameters) by ensuring data flows directly
//! between tools without passing through the agent's reasoning.
//!
//! # Example
//!
//! ```ignore
//! // Tool 1 caches its output
//! context.registers.set("swap_quote", json!({
//!     "to": "0x...",
//!     "data": "0x...",
//!     "value": "1000000000000000"
//! }));
//!
//! // Tool 2 reads from the register
//! let quote = context.registers.get("swap_quote")?;
//! let to = quote.get("to").unwrap();
//! ```

use ethers::prelude::*;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A monad for tool parameters that can either use a preset (reading from registers)
/// or custom raw parameters provided by the agent.
///
/// This enforces mutual exclusivity at the type level - you cannot accidentally
/// mix preset and custom parameters.
///
/// # Example
///
/// ```ignore
/// #[derive(Deserialize)]
/// struct MyToolParams {
///     #[serde(flatten)]
///     mode: PresetOrCustom<CustomParams>,
///     network: String,
/// }
///
/// #[derive(Deserialize)]
/// struct CustomParams {
///     url: String,
///     method: String,
/// }
/// ```
#[derive(Debug, Clone)]
pub enum PresetOrCustom<T> {
    /// Use a named preset that reads values from registers
    Preset(String),
    /// Use custom parameters provided directly
    Custom(T),
}

impl<T> PresetOrCustom<T> {
    /// Returns true if this is a preset
    pub fn is_preset(&self) -> bool {
        matches!(self, PresetOrCustom::Preset(_))
    }

    /// Returns the preset name if this is a preset
    pub fn preset_name(&self) -> Option<&str> {
        match self {
            PresetOrCustom::Preset(name) => Some(name),
            PresetOrCustom::Custom(_) => None,
        }
    }

    /// Returns the custom value if this is custom
    pub fn custom(&self) -> Option<&T> {
        match self {
            PresetOrCustom::Preset(_) => None,
            PresetOrCustom::Custom(v) => Some(v),
        }
    }

    /// Consume and return the custom value if this is custom
    pub fn into_custom(self) -> Option<T> {
        match self {
            PresetOrCustom::Preset(_) => None,
            PresetOrCustom::Custom(v) => Some(v),
        }
    }
}

/// Custom deserializer for PresetOrCustom
///
/// If "preset" field exists, returns Preset variant.
/// Otherwise, attempts to deserialize T from the remaining fields.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for PresetOrCustom<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // First deserialize as a generic Value to inspect
        let value = Value::deserialize(deserializer)?;

        // Check if "preset" field exists
        if let Some(preset) = value.get("preset").and_then(|v| v.as_str()) {
            return Ok(PresetOrCustom::Preset(preset.to_string()));
        }

        // Otherwise, try to deserialize as T
        T::deserialize(value)
            .map(PresetOrCustom::Custom)
            .map_err(serde::de::Error::custom)
    }
}

/// Intrinsic registers that are lazily computed when accessed.
/// These are always available without needing explicit tool calls.
pub enum IntrinsicRegister {
    WalletAddress,
}

impl IntrinsicRegister {
    /// Match a register name to an intrinsic
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "wallet_address" => Some(Self::WalletAddress),
            _ => None,
        }
    }

    /// Resolve the intrinsic value
    pub fn resolve(&self) -> Option<Value> {
        match self {
            Self::WalletAddress => {
                let pk = crate::config::burner_wallet_private_key()?;
                let wallet: LocalWallet = pk.parse().ok()?;
                Some(json!(format!("{:?}", wallet.address())))
            }
        }
    }
}

/// Session-scoped register store for passing data between tools
/// without flowing through the agent's reasoning.
///
/// This is critical for financial transactions where data integrity
/// must be preserved (e.g., swap calldata from 0x quotes).
#[derive(Debug, Clone, Default)]
pub struct RegisterStore {
    inner: Arc<RwLock<HashMap<String, RegisterEntry>>>,
}

/// A single register entry with metadata
#[derive(Debug, Clone)]
pub struct RegisterEntry {
    /// The stored value
    pub value: Value,
    /// Source tool that created this entry
    pub source_tool: String,
    /// Timestamp when the entry was created
    pub created_at: std::time::Instant,
}

impl RegisterStore {
    /// Create a new empty register store
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set a value in the register
    ///
    /// # Arguments
    /// * `key` - The register name (e.g., "swap_quote", "gas_price")
    /// * `value` - The JSON value to store
    /// * `source_tool` - Name of the tool that created this entry
    pub fn set(&self, key: &str, value: Value, source_tool: &str) {
        if let Ok(mut store) = self.inner.write() {
            log::info!(
                "[REGISTER] Set '{}' from tool '{}' (keys: {:?})",
                key,
                source_tool,
                value.as_object().map(|o| o.keys().collect::<Vec<_>>())
            );
            store.insert(
                key.to_string(),
                RegisterEntry {
                    value,
                    source_tool: source_tool.to_string(),
                    created_at: std::time::Instant::now(),
                },
            );
        }
    }

    /// Get a value from the register
    ///
    /// Returns None if the key doesn't exist.
    /// Falls back to intrinsic resolution for special registers like `wallet_address`.
    pub fn get(&self, key: &str) -> Option<Value> {
        // First check explicit registers
        if let Some(entry) = self.get_entry(key) {
            return Some(entry.value);
        }

        // Fall back to intrinsic resolution
        IntrinsicRegister::from_name(key).and_then(|i| i.resolve())
    }

    /// Get the full entry (value + metadata) from the register
    pub fn get_entry(&self, key: &str) -> Option<RegisterEntry> {
        self.inner.read().ok()?.get(key).cloned()
    }

    /// Get entry with metadata, falling back to intrinsic if not set
    pub fn get_entry_or_intrinsic(&self, key: &str) -> Option<RegisterEntry> {
        // Check explicit first
        if let Some(entry) = self.get_entry(key) {
            return Some(entry);
        }

        // Fall back to intrinsic
        IntrinsicRegister::from_name(key).and_then(|i| {
            i.resolve().map(|value| RegisterEntry {
                value,
                source_tool: "intrinsic".to_string(),
                created_at: std::time::Instant::now(),
            })
        })
    }

    /// Get a specific field from a register value
    ///
    /// # Arguments
    /// * `key` - The register name
    /// * `field` - The field path (e.g., "to", "transaction.data")
    pub fn get_field(&self, key: &str, field: &str) -> Option<Value> {
        let value = self.get(key)?;

        // Handle nested field paths (e.g., "transaction.data")
        let mut current = &value;
        for part in field.split('.') {
            current = current.get(part)?;
        }

        Some(current.clone())
    }

    /// Check if a register exists
    pub fn exists(&self, key: &str) -> bool {
        self.inner
            .read()
            .ok()
            .map(|s| s.contains_key(key))
            .unwrap_or(false)
    }

    /// Clear all registers (at end of execution)
    pub fn clear(&self) {
        if let Ok(mut store) = self.inner.write() {
            log::info!("[REGISTER] Clearing all registers");
            store.clear();
        }
    }

    /// Remove a specific register
    pub fn remove(&self, key: &str) -> Option<Value> {
        self.inner
            .write()
            .ok()
            .and_then(|mut s| s.remove(key))
            .map(|e| e.value)
    }

    /// List all register keys (for debugging)
    pub fn keys(&self) -> Vec<String> {
        self.inner
            .read()
            .ok()
            .map(|s| s.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get age of a register entry in seconds
    pub fn age_secs(&self, key: &str) -> Option<u64> {
        self.get_entry(key)
            .map(|e| e.created_at.elapsed().as_secs())
    }

    /// Check if a register is stale (older than max_age_secs)
    pub fn is_stale(&self, key: &str, max_age_secs: u64) -> bool {
        self.age_secs(key)
            .map(|age| age > max_age_secs)
            .unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_register_set_get() {
        let store = RegisterStore::new();

        store.set(
            "test_key",
            json!({"to": "0x123", "value": "1000"}),
            "test_tool",
        );

        let value = store.get("test_key").unwrap();
        assert_eq!(value.get("to").unwrap(), "0x123");
        assert_eq!(value.get("value").unwrap(), "1000");
    }

    #[test]
    fn test_register_get_field() {
        let store = RegisterStore::new();

        store.set(
            "quote",
            json!({
                "transaction": {
                    "to": "0xabc",
                    "data": "0x1234"
                },
                "buyAmount": "5000"
            }),
            "x402_preset_fetch",
        );

        assert_eq!(
            store.get_field("quote", "transaction.to").unwrap(),
            json!("0xabc")
        );
        assert_eq!(
            store.get_field("quote", "transaction.data").unwrap(),
            json!("0x1234")
        );
        assert_eq!(
            store.get_field("quote", "buyAmount").unwrap(),
            json!("5000")
        );
    }

    #[test]
    fn test_register_clear() {
        let store = RegisterStore::new();

        store.set("key1", json!("value1"), "tool1");
        store.set("key2", json!("value2"), "tool2");

        assert!(store.exists("key1"));
        assert!(store.exists("key2"));

        store.clear();

        assert!(!store.exists("key1"));
        assert!(!store.exists("key2"));
    }

    #[test]
    fn test_register_clone_shares_state() {
        let store1 = RegisterStore::new();
        let store2 = store1.clone();

        store1.set("shared", json!("data"), "tool1");

        // store2 should see the data set by store1
        assert_eq!(store2.get("shared").unwrap(), json!("data"));
    }

    #[test]
    fn test_register_entry_metadata() {
        let store = RegisterStore::new();

        store.set("test", json!({"key": "value"}), "my_tool");

        let entry = store.get_entry("test").unwrap();
        assert_eq!(entry.source_tool, "my_tool");
        assert!(entry.created_at.elapsed().as_secs() < 1);
    }
}
