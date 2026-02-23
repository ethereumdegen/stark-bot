//! In-memory cache for active session metadata and agent context.
//!
//! During message dispatch, session metadata and agent context are read/written
//! many times per turn. This cache holds them in a DashMap so the hot path
//! never touches SQLite. Dirty entries are flushed to the database on:
//!   1. Session completion (flush_and_evict)
//!   2. A periodic background timer (flush_all_dirty)
//!   3. Graceful shutdown

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::ai::multi_agent::types::AgentContext;
use crate::models::{ChatSession, CompletionStatus};

use super::Database;

/// A single cached session entry.
pub struct CachedSession {
    pub session: ChatSession,
    pub agent_context: Option<AgentContext>,
    pub dirty: bool,
    pub last_access: Instant,
}

/// Thread-safe in-memory cache for active sessions, keyed by session_id.
pub struct ActiveSessionCache {
    entries: DashMap<i64, CachedSession>,
    max_entries: usize,
}

impl ActiveSessionCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: DashMap::with_capacity(max_entries),
            max_entries,
        }
    }

    /// Insert or replace a session in the cache.
    pub fn load_session(&self, session: ChatSession) {
        let session_id = session.id;
        if let Some(mut entry) = self.entries.get_mut(&session_id) {
            entry.session = session;
            entry.last_access = Instant::now();
        } else {
            self.entries.insert(session_id, CachedSession {
                session,
                agent_context: None,
                dirty: false,
                last_access: Instant::now(),
            });
        }
    }

    /// Clone the cached session (if present).
    pub fn get_session(&self, session_id: i64) -> Option<ChatSession> {
        self.entries.get(&session_id).map(|e| {
            e.session.clone()
        })
    }

    /// Clone the cached agent context (if present).
    pub fn get_agent_context(&self, session_id: i64) -> Option<AgentContext> {
        self.entries.get(&session_id).and_then(|e| {
            e.agent_context.clone()
        })
    }

    /// Mutate the cached session in-place, marking it dirty.
    pub fn update_session<F: FnOnce(&mut ChatSession)>(&self, session_id: i64, f: F) {
        if let Some(mut entry) = self.entries.get_mut(&session_id) {
            f(&mut entry.session);
            entry.dirty = true;
            entry.last_access = Instant::now();
        }
    }

    /// Replace the cached agent context, marking dirty.
    pub fn save_agent_context(&self, session_id: i64, ctx: &AgentContext) {
        if let Some(mut entry) = self.entries.get_mut(&session_id) {
            entry.agent_context = Some(ctx.clone());
            entry.dirty = true;
            entry.last_access = Instant::now();
        }
    }

    /// Read completion status from the cache.
    pub fn get_completion_status(&self, session_id: i64) -> Option<CompletionStatus> {
        self.entries.get(&session_id).map(|e| e.session.completion_status)
    }

    /// Update completion status in the cache.
    pub fn update_completion_status(&self, session_id: i64, status: CompletionStatus) {
        self.update_session(session_id, |s| {
            s.completion_status = status;
        });
    }

    /// Update context_tokens in the cache.
    pub fn update_context_tokens(&self, session_id: i64, tokens: i32) {
        self.update_session(session_id, |s| {
            s.context_tokens = tokens;
        });
    }

    /// Load an agent context into the cache without marking dirty (for initial DB load).
    pub fn load_agent_context(&self, session_id: i64, ctx: AgentContext) {
        if let Some(mut entry) = self.entries.get_mut(&session_id) {
            entry.agent_context = Some(ctx);
            // Don't mark dirty — this is a load from DB, not a mutation
        }
    }

    /// Check if a session is present in the cache.
    pub fn contains(&self, session_id: i64) -> bool {
        self.entries.contains_key(&session_id)
    }

    /// Flush dirty state to SQLite, then remove the entry from the cache.
    pub fn flush_and_evict(&self, session_id: i64, db: &Database) {
        if let Some((_, entry)) = self.entries.remove(&session_id) {
            if entry.dirty {
                Self::flush_entry(session_id, &entry, db);
            }
        }
    }

    /// Iterate all entries and flush any that are dirty.
    pub fn flush_all_dirty(&self, db: &Database) {
        for entry in self.entries.iter() {
            if entry.dirty {
                Self::flush_entry(*entry.key(), &entry, db);
            }
        }
        // After flushing, clear dirty flags
        for mut entry in self.entries.iter_mut() {
            if entry.dirty {
                entry.dirty = false;
            }
        }
    }

    /// Remove from cache without flushing (e.g. after admin delete).
    pub fn force_evict(&self, session_id: i64) {
        self.entries.remove(&session_id);
    }

    /// Remove all entries without flushing (e.g. delete-all-sessions).
    pub fn force_evict_all(&self) {
        self.entries.clear();
    }

    /// Spawn a background task that periodically flushes dirty entries.
    pub fn start_background_flusher(
        self: &Arc<Self>,
        db: Arc<Database>,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        let cache = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await; // skip immediate tick
            loop {
                ticker.tick().await;
                let dirty_count = cache.entries.iter().filter(|e| e.dirty).count();
                if dirty_count > 0 {
                    log::debug!(
                        "[ACTIVE_CACHE] Flushing {} dirty entries to SQLite",
                        dirty_count
                    );
                    cache.flush_all_dirty(&db);
                }
                // Evict stale entries beyond max_entries (LRU by last_access)
                cache.evict_stale();
            }
        })
    }

    /// Write a single entry's dirty state to the database.
    fn flush_entry(session_id: i64, entry: &CachedSession, db: &Database) {
        // Flush completion status and context_tokens via update
        if let Err(e) = db.update_session_completion_status(
            session_id,
            entry.session.completion_status,
        ) {
            log::warn!(
                "[ACTIVE_CACHE] Failed to flush completion_status for session {}: {}",
                session_id, e
            );
        }
        if let Err(e) = db.update_session_context_tokens(
            session_id,
            entry.session.context_tokens,
        ) {
            log::warn!(
                "[ACTIVE_CACHE] Failed to flush context_tokens for session {}: {}",
                session_id, e
            );
        }
        // Flush agent context if present
        if let Some(ref ctx) = entry.agent_context {
            if let Err(e) = db.save_agent_context(session_id, ctx) {
                log::warn!(
                    "[ACTIVE_CACHE] Failed to flush agent_context for session {}: {}",
                    session_id, e
                );
            }
        }
    }

    /// Evict oldest entries when cache exceeds max_entries.
    fn evict_stale(&self) {
        if self.entries.len() <= self.max_entries {
            return;
        }
        // Collect (session_id, last_access) pairs
        let mut entries: Vec<(i64, Instant)> = self
            .entries
            .iter()
            .map(|e| (*e.key(), e.last_access))
            .collect();
        // Sort by last_access ascending (oldest first)
        entries.sort_by_key(|&(_, t)| t);
        let to_evict = entries.len() - self.max_entries;
        for (session_id, _) in entries.into_iter().take(to_evict) {
            // Don't flush on eviction — the background flusher handles dirty entries
            // and evict_stale only removes entries that haven't been accessed recently.
            // If they were dirty, flush_all_dirty() already cleared them.
            self.entries.remove(&session_id);
        }
    }
}
