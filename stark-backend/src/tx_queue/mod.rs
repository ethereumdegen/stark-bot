//! Transaction Queue Subsystem
//!
//! Provides a queue for signed transactions that can be reviewed before broadcast.
//!
//! ## Flow
//! 1. `web3_tx` signs a transaction and queues it (returns UUID)
//! 2. `list_queued_web3_tx` allows viewing queued transactions
//! 3. `broadcast_web3_tx` broadcasts a transaction by UUID
//!
//! This creates a safety layer where transactions can be reviewed before broadcast.

mod types;
mod manager;

pub use types::{QueuedTransaction, QueuedTxStatus, QueuedTxSummary};
pub use manager::{TxQueueManager, create_tx_queue_manager};
