//! Message coalescing for rapid-fire message deduplication/batching
//!
//! Groups messages within a short window before dispatching to the AI.

use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};

/// Configuration for message coalescing
#[derive(Debug, Clone)]
pub struct CoalescerConfig {
    /// Debounce duration — wait this long after the last message before flushing (default: 1500ms)
    pub debounce_ms: u64,
    /// Maximum wait time — flush after this long regardless of new messages (default: 5000ms)
    pub max_wait_ms: u64,
    /// Whether coalescing is enabled (default: false — disabled by default)
    pub enabled: bool,
}

impl Default for CoalescerConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 1500,
            max_wait_ms: 5000,
            enabled: false,
        }
    }
}

/// A pending coalesced message batch
#[derive(Debug)]
struct PendingBatch {
    /// All messages in this batch
    messages: Vec<CoalescedMessage>,
    /// When the first message arrived
    first_message_at: Instant,
    /// When the last message arrived
    last_message_at: Instant,
}

/// A single message within a coalesced batch
#[derive(Debug, Clone)]
pub struct CoalescedMessage {
    pub channel_id: i64,
    pub user_id: String,
    pub text: String,
    pub timestamp: Instant,
}

/// Key for grouping messages (channel_id + user_id)
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct CoalesceKey {
    channel_id: i64,
    user_id: String,
}

/// Message coalescer that groups rapid messages before dispatching
pub struct MessageCoalescer {
    config: CoalescerConfig,
    /// Pending batches indexed by coalesce key
    pending: Arc<DashMap<String, PendingBatch>>,
}

impl MessageCoalescer {
    pub fn new(config: CoalescerConfig) -> Self {
        Self {
            config,
            pending: Arc::new(DashMap::new()),
        }
    }

    /// Add a message to the coalescer. Returns the coalesced text if the batch
    /// is ready to flush (max_wait exceeded), or None if still accumulating.
    pub fn add_message(&self, channel_id: i64, user_id: &str, text: &str) -> Option<String> {
        if !self.config.enabled {
            return Some(text.to_string()); // Pass through immediately
        }

        let key = format!("{}:{}", channel_id, user_id);
        let now = Instant::now();

        let mut entry = self.pending.entry(key.clone()).or_insert_with(|| PendingBatch {
            messages: Vec::new(),
            first_message_at: now,
            last_message_at: now,
        });

        entry.messages.push(CoalescedMessage {
            channel_id,
            user_id: user_id.to_string(),
            text: text.to_string(),
            timestamp: now,
        });
        entry.last_message_at = now;

        // Check if max_wait exceeded
        let elapsed = now.duration_since(entry.first_message_at);
        if elapsed >= Duration::from_millis(self.config.max_wait_ms) {
            // Flush immediately
            drop(entry);
            return self.flush_key(&key);
        }

        None
    }

    /// Check if any pending batch has exceeded its debounce timeout.
    /// Returns (key, coalesced_text) pairs that are ready.
    pub fn check_timeouts(&self) -> Vec<(String, String)> {
        let now = Instant::now();
        let debounce = Duration::from_millis(self.config.debounce_ms);
        let mut ready = Vec::new();

        for entry in self.pending.iter() {
            let elapsed_since_last = now.duration_since(entry.last_message_at);
            if elapsed_since_last >= debounce {
                ready.push(entry.key().clone());
            }
        }

        ready.into_iter()
            .filter_map(|key| self.flush_key(&key).map(|text| (key, text)))
            .collect()
    }

    /// Flush a specific key and return the coalesced text
    fn flush_key(&self, key: &str) -> Option<String> {
        self.pending.remove(key).map(|(_, batch)| {
            if batch.messages.len() == 1 {
                batch.messages[0].text.clone()
            } else {
                // Combine multiple messages with newlines
                batch.messages.iter()
                    .map(|m| m.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
        })
    }

    /// Flush all pending batches (used on shutdown)
    pub fn flush_all(&self) -> Vec<(i64, String, String)> {
        let keys: Vec<String> = self.pending.iter().map(|e| e.key().clone()).collect();
        let mut results = Vec::new();

        for key in keys {
            if let Some((_, batch)) = self.pending.remove(&key) {
                if let Some(first) = batch.messages.first() {
                    let channel_id = first.channel_id;
                    let user_id = first.user_id.clone();
                    let text = if batch.messages.len() == 1 {
                        batch.messages[0].text.clone()
                    } else {
                        batch.messages.iter()
                            .map(|m| m.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    };
                    results.push((channel_id, user_id, text));
                }
            }
        }

        results
    }

    /// Get the number of pending batches
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Get the config
    pub fn config(&self) -> &CoalescerConfig {
        &self.config
    }
}
