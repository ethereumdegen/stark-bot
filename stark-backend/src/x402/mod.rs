//! x402 Protocol implementation for pay-per-use AI endpoints
//!
//! This module handles the x402 payment protocol flow:
//! 1. Make initial request
//! 2. If 402 returned, parse payment requirements (including token metadata from `extra` field)
//! 3. Sign payment based on scheme:
//!    - "permit" (EIP-2612): Permit signature for facilitator to transfer tokens
//!    - "exact" (EIP-3009): TransferWithAuthorization for direct transfers
//! 4. Retry with X-PAYMENT header
//!
//! The token metadata (name, version, address, chain_id) is dynamically extracted
//! from the 402 response, allowing compatibility with any x402-enabled endpoint.

mod types;
mod client;
mod signer;
mod evm_rpc;
pub mod erc20;
pub mod payment_limits;

pub use types::*;
pub use client::{X402Client, X402Response, X402RetryResult, is_x402_endpoint, sign_402_payment, retry_with_x402_payment, check_usdc_balance};
pub use signer::X402Signer;
pub use evm_rpc::{TxLog, X402EvmRpc};
