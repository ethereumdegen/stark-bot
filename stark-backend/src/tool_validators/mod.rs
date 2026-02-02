//! Tool Validator Subsystem
//!
//! A modular, non-agentic validation layer that intercepts tool calls before execution
//! and blocks "stupid" actions using deterministic if/then/else logic.
//!
//! # Architecture
//!
//! Validators can be defined in two ways:
//!
//! ## 1. RON Files (Recommended)
//!
//! Define validators in `.ron` files under `config/validators/` or `skills/{name}/validators/`.
//! These are loaded at runtime without recompilation.
//!
//! ```ron
//! ValidatorDef(
//!     id: "my_validator",
//!     name: "My Validator",
//!     applies_to: ["some_tool"],
//!     priority: Normal,
//!     rules: [
//!         (
//!             when: UrlContains("blocked.com"),
//!             then: Block("Not allowed"),
//!         ),
//!     ],
//!     default: Allow,
//! )
//! ```
//!
//! ## 2. Rust Structs (Legacy)
//!
//! Implement the `ToolValidator` trait directly for complex validation logic
//! that can't be expressed in the RON DSL.
//!
//! # Condition Types (RON DSL)
//!
//! - `ToolName("name")` - Exact tool name match
//! - `UrlContains("substr")` - URL contains substring (case-insensitive)
//! - `UrlMatches("regex")` - URL matches regex pattern
//! - `ArgEquals("key", "value")` - Argument has exact value
//! - `ArgContains("key", "substr")` - Argument contains substring
//! - `ArgExists("key")` - Argument is present
//! - `ArgMissing("key")` - Argument is not present
//! - `CredentialExists("KEY")` - API key exists and is non-empty
//! - `CredentialMissing("KEY")` - API key does not exist
//! - `All([...])` - AND combinator
//! - `Any([...])` - OR combinator
//! - `Not(...)` - Negation

pub mod types;
pub mod traits;
pub mod registry;
pub mod ron;

pub use types::*;
pub use traits::*;
pub use registry::*;

use std::path::Path;

/// Create the default validator registry, loading RON validators from config/validators/
pub fn create_default_registry() -> ValidatorRegistry {
    let mut registry = ValidatorRegistry::new();

    // Load RON validators from config directory
    // Check ./config/validators first, then ../config/validators
    let validators_dir = if Path::new("./config/validators").exists() {
        Path::new("./config/validators")
    } else if Path::new("../config/validators").exists() {
        Path::new("../config/validators")
    } else {
        log::debug!("[TOOL_VALIDATORS] No validators directory found");
        return registry;
    };

    let count = ron::load_validators_from_dir(validators_dir, &mut registry);
    log::info!(
        "[TOOL_VALIDATORS] Loaded {} validators from {}",
        count,
        validators_dir.display()
    );

    registry
}

/// Load validators from a specific directory (for skill-specific validators)
pub fn load_validators_from_dir(dir: &Path, registry: &mut ValidatorRegistry) -> usize {
    ron::load_validators_from_dir(dir, registry)
}
