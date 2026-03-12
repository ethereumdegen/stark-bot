//! Database model modules - extends Database with domain-specific methods
//!
//! Each module adds `impl Database` blocks with methods for a specific table group.

mod auth;           // auth_sessions, auth_challenges
mod api_keys;       // external_api_keys
mod bot_settings;   // bot_settings
pub mod broadcasted_transactions; // broadcasted_transactions (crypto tx history)
pub mod x402_payment_limits; // x402_payment_limits (per-call max amounts per token)
pub mod starflask_agents;    // starflask_agents + starflask_command_log
