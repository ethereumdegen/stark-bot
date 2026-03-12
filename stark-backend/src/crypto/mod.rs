//! Standalone crypto operation functions.
//!
//! These are extracted from the old `tools/builtin/cryptocurrency/` module,
//! stripped of Tool trait boilerplate. Each function takes direct params.

pub mod send_eth;
pub mod broadcast_tx;
pub mod swap_token;
pub mod bridge_usdc;
pub mod web3_call;
pub mod token_utils;
pub mod x402_ops;
pub mod sign;
pub mod auth;
pub mod helpers;
