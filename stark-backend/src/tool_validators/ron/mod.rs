//! RON-based tool validators
//!
//! This module provides runtime-loadable validators defined in RON (Rusty Object Notation) files.
//! Validators can be defined in `.ron` files and loaded at startup without recompilation.
//!
//! # Directory Structure
//!
//! Validators can be placed in:
//! - `config/validators/` - Global validators
//! - `skills/{skill_name}/validators/` - Skill-specific validators
//!
//! # Example RON Validator
//!
//! ```ron
//! ValidatorDef(
//!     id: "my_validator",
//!     name: "My Validator",
//!     description: "Description of what it does",
//!     applies_to: ["tool_name"],
//!     priority: Normal,
//!     rules: [
//!         (
//!             when: UrlContains("example.com"),
//!             then: Block("Not allowed"),
//!         ),
//!     ],
//!     default: Allow,
//! )
//! ```

mod schema;
mod interpreter;
mod validator;

pub use schema::*;
pub use interpreter::*;
pub use validator::*;
