//! Transaction queue manager
//!
//! Thread-safe storage and management of queued transactions.

use chrono::Utc;
use dashmap::DashMap;
use std::sync::Arc;

use super::types::{QueuedTransaction, QueuedTxStatus, QueuedTxSummary};

/// Manager for the transaction queue
/// Uses DashMap for thread-safe concurrent access
pub struct TxQueueManager {
    /// Map of UUID -> QueuedTransaction
    transactions: DashMap<String, QueuedTransaction>,
}

impl TxQueueManager {
    /// Create a new transaction queue manager
    pub fn new() -> Self {
        Self {
            transactions: DashMap::new(),
        }
    }

    /// Queue a new transaction
    pub fn queue(&self, tx: QueuedTransaction) -> String {
        let uuid = tx.uuid.clone();
        log::info!("[TxQueue] Queuing transaction {} to {}", uuid, tx.to);
        self.transactions.insert(uuid.clone(), tx);
        uuid
    }

    /// Get a transaction by UUID
    pub fn get(&self, uuid: &str) -> Option<QueuedTransaction> {
        self.transactions.get(uuid).map(|r| r.clone())
    }

    /// Get a transaction summary by UUID
    pub fn get_summary(&self, uuid: &str) -> Option<QueuedTxSummary> {
        self.transactions.get(uuid).map(|r| QueuedTxSummary::from(r.value()))
    }

    /// List all transactions
    pub fn list_all(&self) -> Vec<QueuedTxSummary> {
        self.transactions
            .iter()
            .map(|r| QueuedTxSummary::from(r.value()))
            .collect()
    }

    /// List transactions sorted by created_at (most recent first)
    pub fn list_recent(&self, limit: usize) -> Vec<QueuedTxSummary> {
        let mut txs: Vec<_> = self.transactions
            .iter()
            .map(|r| QueuedTxSummary::from(r.value()))
            .collect();
        txs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        txs.truncate(limit);
        txs
    }

    /// List pending transactions only
    pub fn list_pending(&self) -> Vec<QueuedTxSummary> {
        self.transactions
            .iter()
            .filter(|r| r.value().status == QueuedTxStatus::Pending)
            .map(|r| QueuedTxSummary::from(r.value()))
            .collect()
    }

    /// List transactions by status
    pub fn list_by_status(&self, status: QueuedTxStatus) -> Vec<QueuedTxSummary> {
        self.transactions
            .iter()
            .filter(|r| r.value().status == status)
            .map(|r| QueuedTxSummary::from(r.value()))
            .collect()
    }

    /// Update transaction status
    pub fn update_status(&self, uuid: &str, status: QueuedTxStatus) -> bool {
        if let Some(mut tx) = self.transactions.get_mut(uuid) {
            log::info!("[TxQueue] Updating {} status to {:?}", uuid, status);
            tx.status = status;
            true
        } else {
            false
        }
    }

    /// Mark transaction as broadcasting
    pub fn mark_broadcasting(&self, uuid: &str) -> bool {
        self.update_status(uuid, QueuedTxStatus::Broadcasting)
    }

    /// Mark transaction as broadcast with tx_hash
    pub fn mark_broadcast(&self, uuid: &str, tx_hash: &str, explorer_url: &str) -> bool {
        if let Some(mut tx) = self.transactions.get_mut(uuid) {
            log::info!("[TxQueue] Transaction {} broadcast as {}", uuid, tx_hash);
            tx.status = QueuedTxStatus::Broadcast;
            tx.tx_hash = Some(tx_hash.to_string());
            tx.explorer_url = Some(explorer_url.to_string());
            tx.broadcast_at = Some(Utc::now());
            true
        } else {
            false
        }
    }

    /// Mark transaction as confirmed
    pub fn mark_confirmed(&self, uuid: &str) -> bool {
        if let Some(mut tx) = self.transactions.get_mut(uuid) {
            log::info!("[TxQueue] Transaction {} confirmed", uuid);
            tx.status = QueuedTxStatus::Confirmed;
            true
        } else {
            false
        }
    }

    /// Mark transaction as failed with error
    pub fn mark_failed(&self, uuid: &str, error: &str) -> bool {
        if let Some(mut tx) = self.transactions.get_mut(uuid) {
            log::warn!("[TxQueue] Transaction {} failed: {}", uuid, error);
            tx.status = QueuedTxStatus::Failed;
            tx.error = Some(error.to_string());
            true
        } else {
            false
        }
    }

    /// Mark transaction as expired
    pub fn mark_expired(&self, uuid: &str) -> bool {
        if let Some(mut tx) = self.transactions.get_mut(uuid) {
            log::warn!("[TxQueue] Transaction {} expired", uuid);
            tx.status = QueuedTxStatus::Expired;
            true
        } else {
            false
        }
    }

    /// Get count of transactions by status
    pub fn count_by_status(&self, status: QueuedTxStatus) -> usize {
        self.transactions
            .iter()
            .filter(|r| r.value().status == status)
            .count()
    }

    /// Get total count of transactions
    pub fn count(&self) -> usize {
        self.transactions.len()
    }

    /// Remove a transaction by UUID (for cleanup)
    pub fn remove(&self, uuid: &str) -> Option<QueuedTransaction> {
        self.transactions.remove(uuid).map(|(_, tx)| tx)
    }

    /// Clean up old transactions (older than duration)
    pub fn cleanup_old(&self, max_age_hours: i64) -> usize {
        let cutoff = Utc::now() - chrono::Duration::hours(max_age_hours);
        let old_uuids: Vec<String> = self.transactions
            .iter()
            .filter(|r| {
                let tx = r.value();
                // Only clean up terminal states (confirmed, failed, expired)
                matches!(tx.status, QueuedTxStatus::Confirmed | QueuedTxStatus::Failed | QueuedTxStatus::Expired)
                    && tx.created_at < cutoff
            })
            .map(|r| r.key().clone())
            .collect();

        let count = old_uuids.len();
        for uuid in old_uuids {
            self.transactions.remove(&uuid);
        }

        if count > 0 {
            log::info!("[TxQueue] Cleaned up {} old transactions", count);
        }
        count
    }
}

impl Default for TxQueueManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Create an Arc-wrapped TxQueueManager for sharing across threads
pub fn create_tx_queue_manager() -> Arc<TxQueueManager> {
    Arc::new(TxQueueManager::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tx(uuid: &str) -> QueuedTransaction {
        QueuedTransaction::new(
            uuid.to_string(),
            "base".to_string(),
            "0x1234".to_string(),
            "0x5678".to_string(),
            "1000000000000000".to_string(),
            "0x".to_string(),
            "21000".to_string(),
            "1000000000".to_string(),
            "100000000".to_string(),
            0,
            "0xabcd".to_string(),
            Some(1),
        )
    }

    #[test]
    fn test_queue_and_get() {
        let manager = TxQueueManager::new();
        let tx = create_test_tx("test-uuid-1");

        manager.queue(tx);

        let retrieved = manager.get("test-uuid-1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().uuid, "test-uuid-1");
    }

    #[test]
    fn test_status_updates() {
        let manager = TxQueueManager::new();
        let tx = create_test_tx("test-uuid-2");
        manager.queue(tx);

        // Initial status should be Pending
        let tx = manager.get("test-uuid-2").unwrap();
        assert_eq!(tx.status, QueuedTxStatus::Pending);

        // Mark as broadcasting
        assert!(manager.mark_broadcasting("test-uuid-2"));
        let tx = manager.get("test-uuid-2").unwrap();
        assert_eq!(tx.status, QueuedTxStatus::Broadcasting);

        // Mark as broadcast
        assert!(manager.mark_broadcast("test-uuid-2", "0xhash", "https://basescan.org/tx/0xhash"));
        let tx = manager.get("test-uuid-2").unwrap();
        assert_eq!(tx.status, QueuedTxStatus::Broadcast);
        assert_eq!(tx.tx_hash, Some("0xhash".to_string()));

        // Mark as confirmed
        assert!(manager.mark_confirmed("test-uuid-2"));
        let tx = manager.get("test-uuid-2").unwrap();
        assert_eq!(tx.status, QueuedTxStatus::Confirmed);
    }

    #[test]
    fn test_list_pending() {
        let manager = TxQueueManager::new();

        let tx1 = create_test_tx("pending-1");
        let tx2 = create_test_tx("pending-2");
        manager.queue(tx1);
        manager.queue(tx2);

        // Both should be pending
        let pending = manager.list_pending();
        assert_eq!(pending.len(), 2);

        // Mark one as confirmed
        manager.mark_confirmed("pending-1");

        let pending = manager.list_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].uuid, "pending-2");
    }
}
