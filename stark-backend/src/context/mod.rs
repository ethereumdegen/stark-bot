//! Context management for session conversations
//!
//! This module provides:
//! - Token estimation for messages (content-aware)
//! - Context compaction (summarizing old messages when context grows too large)
//! - Sliding window compaction (incremental instead of all-at-once)
//! - Summary chaining (preserve context across compactions)
//! - Pre-compaction memory flush (AI extracts memories before summarization)
//! - Cross-session memory integration
//! - Session memory hooks (saving session summaries on reset)

pub mod tokenizer;

use crate::ai::{AiClient, Message, MessageRole};
use crate::config::MemoryConfig;
use crate::db::{ActiveSessionCache, Database};
use crate::models::SessionMessage;
use crate::models::session_message::MessageRole as DbMessageRole;
use chrono::Utc;
use std::sync::Arc;
pub use tokenizer::TokenEstimator;

/// Default context window size (Claude 3.5 Sonnet)
pub const DEFAULT_MAX_CONTEXT_TOKENS: i32 = 100_000;

/// Reserve tokens for system prompt and output
pub const DEFAULT_RESERVE_TOKENS: i32 = 20_000;

/// Minimum messages to keep after compaction
pub const MIN_KEEP_RECENT_MESSAGES: i32 = 5;

/// Default number of messages to keep after compaction
pub const DEFAULT_KEEP_RECENT_MESSAGES: i32 = 10;

/// Configuration for sliding window (incremental) compaction
#[derive(Debug, Clone)]
pub struct SlidingWindowConfig {
    /// Target number of tokens to free per compaction cycle
    pub target_free_tokens: i32,
    /// Minimum messages to always keep (safety floor)
    pub min_keep_messages: i32,
    /// Maximum messages to compact per cycle (caps batch size)
    pub max_compact_per_cycle: i32,
    /// Buffer tokens to trigger compaction early (before hitting hard limit)
    pub compaction_buffer: i32,
}

impl Default for SlidingWindowConfig {
    fn default() -> Self {
        Self {
            target_free_tokens: 20_000,    // Free ~20k tokens per cycle
            min_keep_messages: 5,           // Never remove below this
            max_compact_per_cycle: 30,      // Cap batch size
            compaction_buffer: 15_000,      // Trigger at 85k instead of 80k
        }
    }
}

/// Compaction urgency level based on context fullness
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionLevel {
    /// No compaction needed
    None,
    /// Background compaction — >80% full, compact 30%
    Background,
    /// Aggressive compaction — >85% full, compact 50%
    Aggressive,
    /// Emergency compaction — >95% full, hard-drop 50% synchronously
    Emergency,
}

impl std::fmt::Display for CompactionLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompactionLevel::None => write!(f, "none"),
            CompactionLevel::Background => write!(f, "background"),
            CompactionLevel::Aggressive => write!(f, "aggressive"),
            CompactionLevel::Emergency => write!(f, "emergency"),
        }
    }
}

/// Configuration for three-tier compaction thresholds
#[derive(Debug, Clone)]
pub struct ThreeTierCompactionConfig {
    /// Trigger background compaction above this percentage (default: 0.80)
    pub background_threshold: f64,
    /// Trigger aggressive compaction above this percentage (default: 0.85)
    pub aggressive_threshold: f64,
    /// Trigger emergency compaction above this percentage (default: 0.95)
    pub emergency_threshold: f64,
    /// Fraction of context to compact in background mode (default: 0.30)
    pub background_compact_ratio: f64,
    /// Fraction of context to compact in aggressive mode (default: 0.50)
    pub aggressive_compact_ratio: f64,
    /// Fraction of context to hard-drop in emergency mode (default: 0.50)
    pub emergency_drop_ratio: f64,
}

impl Default for ThreeTierCompactionConfig {
    fn default() -> Self {
        Self {
            background_threshold: 0.80,
            aggressive_threshold: 0.85,
            emergency_threshold: 0.95,
            background_compact_ratio: 0.30,
            aggressive_compact_ratio: 0.50,
            emergency_drop_ratio: 0.50,
        }
    }
}

/// Estimate token count for a string using content-aware estimation
/// This provides more accurate estimates than simple character counting
/// by considering content type (JSON, code, prose)
pub fn estimate_tokens(text: &str) -> i32 {
    TokenEstimator::ContentAware.estimate_text(text)
}

/// Estimate total tokens for a list of messages
/// Uses content-aware estimation with role overhead
pub fn estimate_messages_tokens(messages: &[SessionMessage]) -> i32 {
    let estimator = TokenEstimator::ContentAware;
    messages.iter()
        .map(|m| estimator.estimate_message(&m.content, &m.role))
        .sum()
}

/// Context manager for handling session context and compaction
pub struct ContextManager {
    db: Arc<Database>,
    /// Maximum context window size in tokens
    max_context_tokens: i32,
    /// Tokens to reserve for system prompt and output
    reserve_tokens: i32,
    /// Number of recent messages to keep after compaction
    keep_recent_messages: i32,
    /// Memory configuration
    memory_config: MemoryConfig,
    /// Configuration for sliding window compaction
    sliding_window_config: SlidingWindowConfig,
    /// Three-tier compaction thresholds (can be overridden from bot settings)
    compaction_config: ThreeTierCompactionConfig,
    /// Optional in-memory session cache for fast reads of context_tokens / max_context_tokens
    active_cache: Option<Arc<ActiveSessionCache>>,
}

impl ContextManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            max_context_tokens: DEFAULT_MAX_CONTEXT_TOKENS,
            reserve_tokens: DEFAULT_RESERVE_TOKENS,
            keep_recent_messages: DEFAULT_KEEP_RECENT_MESSAGES,
            memory_config: MemoryConfig::from_env(),
            sliding_window_config: SlidingWindowConfig::default(),
            compaction_config: ThreeTierCompactionConfig::default(),
            active_cache: None,
        }
    }

    /// Set the active session cache for fast session metadata reads
    pub fn with_active_cache(mut self, cache: Arc<ActiveSessionCache>) -> Self {
        self.active_cache = Some(cache);
        self
    }

    pub fn with_compaction_config(mut self, config: ThreeTierCompactionConfig) -> Self {
        self.compaction_config = config;
        self
    }

    pub fn with_max_context(mut self, tokens: i32) -> Self {
        self.max_context_tokens = tokens;
        self
    }

    pub fn with_reserve_tokens(mut self, tokens: i32) -> Self {
        self.reserve_tokens = tokens;
        self
    }

    pub fn with_keep_recent(mut self, count: i32) -> Self {
        self.keep_recent_messages = count.max(MIN_KEEP_RECENT_MESSAGES);
        self
    }

    pub fn with_memory_config(mut self, config: MemoryConfig) -> Self {
        self.memory_config = config;
        self
    }

    pub fn with_sliding_window_config(mut self, config: SlidingWindowConfig) -> Self {
        self.sliding_window_config = config;
        self
    }

    /// Get a session, preferring the in-memory cache over SQLite.
    fn get_session_cached(&self, session_id: i64) -> Option<crate::models::ChatSession> {
        if let Some(ref cache) = self.active_cache {
            if let Some(session) = cache.get_session(session_id) {
                return Some(session);
            }
        }
        self.db.get_chat_session(session_id).ok().flatten()
    }

    /// Update context tokens, writing to cache if available, otherwise to DB.
    fn set_context_tokens(&self, session_id: i64, tokens: i32) {
        if let Some(ref cache) = self.active_cache {
            if cache.contains(session_id) {
                cache.update_context_tokens(session_id, tokens);
                return;
            }
        }
        let _ = self.db.update_session_context_tokens(session_id, tokens);
    }

    /// Sync session's max_context_tokens with agent settings
    /// This ensures compaction triggers at the right threshold for the configured endpoint
    pub fn sync_max_context_tokens(&self, session_id: i64, agent_max_tokens: i32) {
        // Only update if different from current value
        if let Some(session) = self.get_session_cached(session_id) {
            if session.max_context_tokens != agent_max_tokens {
                log::info!(
                    "[CONTEXT] Syncing session {} max_context_tokens: {} -> {}",
                    session_id, session.max_context_tokens, agent_max_tokens
                );
                if let Err(e) = self.db.update_session_max_context_tokens(session_id, agent_max_tokens) {
                    log::error!("[CONTEXT] Failed to update max_context_tokens: {}", e);
                }
            }
        }
    }

    /// Check if compaction is needed for a session (original all-at-once threshold)
    pub fn needs_compaction(&self, session_id: i64) -> bool {
        if let Some(session) = self.get_session_cached(session_id) {
            let threshold = session.max_context_tokens - self.reserve_tokens;
            return session.context_tokens > threshold;
        }
        false
    }

    /// Get available context budget (after reserving tokens)
    pub fn get_context_budget(&self, session_id: i64) -> i32 {
        if let Some(session) = self.get_session_cached(session_id) {
            return session.max_context_tokens - self.reserve_tokens - session.context_tokens;
        }
        self.max_context_tokens - self.reserve_tokens
    }

    /// Build conversation context for AI, including compaction summary if present
    pub fn build_context(&self, session_id: i64, limit: i32) -> Vec<SessionMessage> {
        // Get recent messages
        let messages = self.db.get_recent_session_messages(session_id, limit)
            .unwrap_or_default();

        messages
    }

    /// Get compaction summary for a session (if any)
    pub fn get_compaction_summary(&self, session_id: i64) -> Option<String> {
        self.db.get_session_compaction_summary(session_id).ok().flatten()
    }

    /// Check if incremental (sliding window) compaction should occur
    /// Triggers earlier than full compaction to do smaller, less disruptive compactions
    pub fn needs_incremental_compaction(&self, session_id: i64) -> bool {
        if let Some(session) = self.get_session_cached(session_id) {
            // Trigger at (max - reserve - buffer) instead of (max - reserve)
            // e.g., at 85k instead of 80k for 100k context with 20k reserve and 15k buffer
            let threshold = session.max_context_tokens
                - self.reserve_tokens
                - self.sliding_window_config.compaction_buffer;
            return session.context_tokens > threshold;
        }
        false
    }

    /// Perform incremental compaction - compact only the oldest N messages
    /// This is less disruptive than full compaction as it preserves more recent context
    pub async fn compact_incremental(
        &self,
        session_id: i64,
        client: &AiClient,
        identity_id: Option<&str>,
    ) -> Result<i32, String> {
        // Calculate how many messages to compact to free target tokens
        let messages_to_compact = self.calculate_messages_to_compact(session_id)?;

        if messages_to_compact.is_empty() {
            log::info!("[INCREMENTAL_COMPACT] No messages to compact for session {}", session_id);
            return Ok(0);
        }

        let message_count = messages_to_compact.len() as i32;
        log::info!(
            "[INCREMENTAL_COMPACT] Compacting {} oldest messages for session {} (incremental)",
            message_count, session_id
        );

        // Skip pre-compaction flush — the compaction summary and
        // session_completion_memory are the intended memory sources.
        // The flush created noisy per-tool-result entries.

        // Generate a shorter summary for incremental compaction
        let summary = self.generate_incremental_summary(client, &messages_to_compact).await?;

        log::info!(
            "[INCREMENTAL_COMPACT] Generated summary ({} chars) for {} messages",
            summary.len(), message_count
        );

        // Chain with existing summary if present
        let chained_summary = self.chain_summaries(session_id, &summary)?;

        // Store the chained summary
        if let Err(e) = self.db.set_session_compaction_summary(session_id, &chained_summary) {
            log::warn!("[INCREMENTAL_COMPACT] Failed to store compaction summary: {}", e);
        }

        // Delete only the oldest N messages
        let deleted = self.db.delete_oldest_messages(session_id, message_count)
            .map_err(|e| format!("Failed to delete oldest messages: {}", e))?;

        log::info!("[INCREMENTAL_COMPACT] Deleted {} oldest messages for session {}", deleted, session_id);

        // Increment compaction generation
        if let Err(e) = self.db.increment_compaction_generation(session_id) {
            log::warn!("[INCREMENTAL_COMPACT] Failed to increment compaction generation: {}", e);
        }

        // Recalculate and update context tokens
        let remaining = self.db.get_session_messages(session_id).unwrap_or_default();
        let new_token_count = estimate_messages_tokens(&remaining) + estimate_tokens(&chained_summary);
        self.set_context_tokens(session_id, new_token_count);

        Ok(message_count)
    }

    /// Calculate which messages to compact to free target tokens
    fn calculate_messages_to_compact(&self, session_id: i64) -> Result<Vec<SessionMessage>, String> {
        let all_messages = self.db.get_session_messages(session_id)
            .map_err(|e| format!("Failed to get session messages: {}", e))?;

        if all_messages.len() as i32 <= self.sliding_window_config.min_keep_messages {
            return Ok(vec![]);
        }

        let target_tokens = self.sliding_window_config.target_free_tokens;
        let max_messages = self.sliding_window_config.max_compact_per_cycle;
        let min_keep = self.sliding_window_config.min_keep_messages as usize;

        // Calculate how many messages to compact
        let mut token_sum = 0i32;
        let mut count = 0usize;
        let max_compactable = all_messages.len().saturating_sub(min_keep);

        for msg in &all_messages {
            if count >= max_compactable {
                break;
            }
            if count >= max_messages as usize {
                break;
            }
            if token_sum >= target_tokens {
                break;
            }

            token_sum += estimate_tokens(&msg.content);
            count += 1;
        }

        Ok(all_messages.into_iter().take(count).collect())
    }

    /// Generate a shorter summary for incremental compaction
    async fn generate_incremental_summary(
        &self,
        client: &AiClient,
        messages: &[SessionMessage],
    ) -> Result<String, String> {
        let conversation_text = messages.iter()
            .map(|m| {
                let role = match m.role {
                    DbMessageRole::User => "User",
                    DbMessageRole::Assistant => "Assistant",
                    DbMessageRole::System => "System",
                    DbMessageRole::ToolCall => "Tool Call",
                    DbMessageRole::ToolResult => "Tool Result",
                };
                format!("{}: {}", role, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // Shorter prompt for incremental summaries - target ~200 words
        let summary_prompt = format!(
            "Summarize this conversation segment concisely (under 200 words). \
            Focus on: decisions made, facts learned, tasks started or completed. \
            Be factual and specific.\n\n\
            Conversation:\n{}\n\nSummary:",
            conversation_text
        );

        let summary_messages = vec![
            Message {
                role: MessageRole::System,
                content: "You summarize conversations accurately and concisely.".to_string(),
            },
            Message {
                role: MessageRole::User,
                content: summary_prompt,
            },
        ];

        client.generate_text(summary_messages).await
            .map_err(|e| format!("Failed to generate incremental summary: {}", e))
    }

    /// Chain a new summary with existing summary, preserving key context
    fn chain_summaries(&self, session_id: i64, new_summary: &str) -> Result<String, String> {
        let existing = self.db.get_session_compaction_summary(session_id)
            .map_err(|e| format!("Failed to get existing summary: {}", e))?;

        match existing {
            None => Ok(new_summary.to_string()),
            Some(prev) => {
                // Truncate previous summary to ~300 words to prevent unbounded growth
                let prev_limited = truncate_summary(&prev, 300);
                Ok(format!(
                    "## Previous Context\n{}\n\n## Recent Activity\n{}",
                    prev_limited, new_summary
                ))
            }
        }
    }

    /// Phase 1: Flush memories before compaction
    /// Gives the AI a "silent turn" to extract important memories from the conversation
    /// that would otherwise be lost during summarization.
    /// Writes extracted memories to the DB memories table.
    pub async fn flush_memories_before_compaction(
        &self,
        session_id: i64,
        client: &AiClient,
        identity_id: Option<&str>,
        messages_to_compact: &[SessionMessage],
        agent_subtype: Option<&str>,
    ) -> Result<usize, String> {
        if messages_to_compact.is_empty() {
            return Ok(0);
        }

        log::info!("[PRE_FLUSH] Starting memory flush for session {} ({} messages)",
            session_id, messages_to_compact.len());

        // Only extract memories from User/Assistant messages — skip raw ToolCall
        // and ToolResult entries so individual tool outputs don't pollute memory.
        // The final summary/output captured by save_session_completion_memory is
        // the intended memory source.
        let messages_filtered: Vec<&SessionMessage> = messages_to_compact.iter()
            .filter(|m| {
                matches!(m.role, DbMessageRole::User | DbMessageRole::Assistant)
            })
            .collect();

        if messages_filtered.is_empty() {
            log::info!("[PRE_FLUSH] No user/assistant messages to extract memories from");
            return Ok(0);
        }

        // Build conversation text
        let conversation_text = messages_filtered.iter()
            .map(|m| {
                let role = match m.role {
                    DbMessageRole::User => "User",
                    DbMessageRole::Assistant => "Assistant",
                    _ => unreachable!(),
                };
                format!("{}: {}", role, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // Prompt the AI to extract memories - simplified for markdown output
        let flush_prompt = format!(
            "Before this conversation history is summarized, extract any important information that should be remembered.\n\n\
            Format your response as markdown with sections:\n\
            ## Long-Term (facts, preferences, important info)\n\
            - bullet points\n\n\
            ## Daily Activity (what was done today)\n\
            - bullet points\n\n\
            Only extract genuinely important information. Don't save trivial details.\n\
            If nothing important needs to be saved, respond with just: NO_MEMORIES_NEEDED\n\n\
            Conversation to analyze:\n{}\n\n\
            Extract memories:",
            conversation_text
        );

        let flush_messages = vec![
            Message {
                role: MessageRole::System,
                content: "You are a memory extraction assistant. Extract important information from conversations and format it as markdown.".to_string(),
            },
            Message {
                role: MessageRole::User,
                content: flush_prompt,
            },
        ];

        let response = client.generate_text(flush_messages).await
            .map_err(|e| format!("Failed to generate memory flush: {}", e))?;

        if response.contains("NO_MEMORIES_NEEDED") {
            log::info!("[PRE_FLUSH] No memories to extract for session {}", session_id);
            return Ok(0);
        }

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let mut count = 0;

        // Extract long-term section
        if let Some(long_term_start) = response.find("## Long-Term") {
            let section_end = response[long_term_start..]
                .find("\n## ")
                .map(|i| long_term_start + i)
                .unwrap_or(response.len());
            let long_term_content = &response[long_term_start..section_end];

            if !long_term_content.trim().is_empty() {
                if let Err(e) = self.db.insert_memory(
                    "long_term",
                    long_term_content.trim(),
                    None, None, 5, identity_id, None, None, None,
                    Some("pre_compaction_flush"), None, agent_subtype,
                ) {
                    log::error!("[PRE_FLUSH] Failed to write long-term memory: {}", e);
                } else {
                    count += 1;
                    log::info!("[PRE_FLUSH] Wrote long-term memories");
                }
            }
        }

        // Extract daily activity section
        if let Some(daily_start) = response.find("## Daily") {
            let section_end = response[daily_start..]
                .find("\n## ")
                .map(|i| daily_start + i)
                .unwrap_or(response.len());
            let daily_content = &response[daily_start..section_end];

            if !daily_content.trim().is_empty() {
                if let Err(e) = self.db.insert_memory(
                    "daily_log",
                    daily_content.trim(),
                    None, None, 5, identity_id, None, None, None,
                    Some("pre_compaction_flush"), Some(&today), agent_subtype,
                ) {
                    log::error!("[PRE_FLUSH] Failed to write daily log: {}", e);
                } else {
                    count += 1;
                    log::info!("[PRE_FLUSH] Wrote daily activity");
                }
            }
        }

        log::info!("[PRE_FLUSH] Extracted {} memory sections for session {}", count, session_id);

        // Update last_flush_at timestamp
        if let Err(e) = self.db.update_session_last_flush(session_id) {
            log::warn!("[PRE_FLUSH] Failed to update last_flush_at: {}", e);
        }

        Ok(count)
    }

    /// Perform context compaction for a session
    /// Returns the number of messages compacted
    pub async fn compact_session(
        &self,
        session_id: i64,
        client: &AiClient,
        identity_id: Option<&str>,
        agent_subtype: Option<&str>,
    ) -> Result<i32, String> {
        // Get messages to compact (all except recent ones)
        let messages_to_compact = self.db.get_messages_for_compaction(session_id, self.keep_recent_messages)
            .map_err(|e| format!("Failed to get messages for compaction: {}", e))?;

        if messages_to_compact.is_empty() {
            log::info!("[COMPACTION] No messages to compact for session {}", session_id);
            return Ok(0);
        }

        let message_count = messages_to_compact.len() as i32;
        log::info!("[COMPACTION] Compacting {} messages for session {}", message_count, session_id);

        // Skip pre-compaction flush — the compaction summary and
        // session_completion_memory are the intended memory sources.
        // The flush created noisy per-tool-result entries.

        // Build the conversation text for summarization
        let conversation_text = messages_to_compact.iter()
            .map(|m| {
                let role = match m.role {
                    DbMessageRole::User => "User",
                    DbMessageRole::Assistant => "Assistant",
                    DbMessageRole::System => "System",
                    DbMessageRole::ToolCall => "Tool Call",
                    DbMessageRole::ToolResult => "Tool Result",
                };
                format!("{}: {}", role, m.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        // Generate summary using AI
        let summary_prompt = format!(
            "Summarize the following conversation history concisely. \
            Focus on: key topics discussed, important decisions made, user preferences learned, \
            and any tasks or commitments. Keep it factual and under 500 words.\n\n\
            Conversation:\n{}\n\nSummary:",
            conversation_text
        );

        let summary_messages = vec![
            Message {
                role: MessageRole::System,
                content: "You are a helpful assistant that summarizes conversations accurately and concisely.".to_string(),
            },
            Message {
                role: MessageRole::User,
                content: summary_prompt,
            },
        ];

        let summary = client.generate_text(summary_messages).await
            .map_err(|e| format!("Failed to generate compaction summary: {}", e))?;

        log::info!("[COMPACTION] Generated summary ({} chars) for session {}", summary.len(), session_id);

        // Write the compaction summary to DB as a daily_log memory
        {
            let summary_entry = format!("### Session Summary\n{}", summary);
            let today = Utc::now().format("%Y-%m-%d").to_string();
            if let Err(e) = self.db.insert_memory(
                "daily_log",
                &summary_entry,
                None, None, 5, identity_id, Some(session_id), None, None,
                Some("compaction_summary"), Some(&today), agent_subtype,
            ) {
                log::error!("[COMPACTION] Failed to write session summary to daily log: {}", e);
            }
        }

        // Store summary in session record for context building
        // (The session still needs a compaction summary for the conversation window)
        if let Err(e) = self.db.set_session_compaction_summary(session_id, &summary) {
            log::warn!("[COMPACTION] Failed to store compaction summary in session: {}", e);
        }

        // Delete the compacted messages
        let deleted = self.db.delete_compacted_messages(session_id, self.keep_recent_messages)
            .map_err(|e| format!("Failed to delete compacted messages: {}", e))?;

        log::info!("[COMPACTION] Deleted {} old messages for session {}", deleted, session_id);

        // Recalculate and update context tokens
        let remaining = self.db.get_session_messages(session_id).unwrap_or_default();
        let new_token_count = estimate_messages_tokens(&remaining) + estimate_tokens(&summary);
        self.set_context_tokens(session_id, new_token_count);

        Ok(message_count)
    }

    /// Update context tokens after adding a message
    pub fn update_context_tokens(&self, session_id: i64, message_tokens: i32) {
        if let Some(session) = self.get_session_cached(session_id) {
            let new_total = session.context_tokens + message_tokens;
            self.set_context_tokens(session_id, new_total);
        }
    }

    // ============================================
    // Cross-Session Memory Integration
    // ============================================

    /// Retrieve relevant memories from QMD store based on recent conversation
    /// Returns formatted memory context if enabled and memories are found
    pub fn retrieve_relevant_memories(
        &self,
        identity_id: Option<&str>,
        recent_messages: &[SessionMessage],
    ) -> Option<String> {
        if !self.memory_config.enable_cross_session_memory {
            return None;
        }

        // Build search query from last 3 user messages
        let query_terms: Vec<String> = recent_messages
            .iter()
            .filter(|m| m.role == DbMessageRole::User)
            .rev()  // Most recent first
            .take(3)
            .flat_map(|m| {
                // Extract meaningful words (skip very short/common words)
                m.content
                    .split_whitespace()
                    .filter(|w| w.len() > 3)
                    .take(10)
                    .map(|s| s.to_lowercase())
            })
            .collect();

        if query_terms.is_empty() {
            return None;
        }

        // Use prefix matching (word*) and OR for broader recall
        let query = query_terms.iter()
            .map(|w| format!("{}*", w))
            .collect::<Vec<_>>()
            .join(" OR ");
        log::debug!("[MEMORY_RETRIEVAL] Searching with query: {}", &query);

        let limit = self.memory_config.cross_session_memory_limit;
        match self.db.search_memories_fts(&query, identity_id, limit) {
            Ok(results) if !results.is_empty() => {
                log::info!(
                    "[MEMORY_RETRIEVAL] Found {} relevant memories for identity {:?}",
                    results.len(), identity_id
                );

                // Format as bullet points with content snippets
                let formatted = results
                    .iter()
                    .map(|(mem, _rank)| {
                        let snippet: String = if mem.content.chars().count() > 200 {
                            let truncated: String = mem.content.chars().take(200).collect();
                            format!("{}...", truncated)
                        } else {
                            mem.content.clone()
                        };
                        format!("- {}", snippet)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                Some(formatted)
            }
            Ok(_) => {
                log::debug!("[MEMORY_RETRIEVAL] No relevant memories found");
                None
            }
            Err(e) => {
                log::warn!("[MEMORY_RETRIEVAL] Search failed: {}", e);
                None
            }
        }
    }

    /// Build context with optional memory retrieval
    /// Returns (messages, combined_context_summary)
    /// The combined_context includes both compaction summary and cross-session memories
    pub fn build_context_with_memories(
        &self,
        session_id: i64,
        identity_id: Option<&str>,
        limit: i32,
    ) -> (Vec<SessionMessage>, Option<String>) {
        let messages = self.build_context(session_id, limit);
        let compaction_summary = self.get_compaction_summary(session_id);

        // Retrieve cross-session memories if enabled
        let memory_context = self.retrieve_relevant_memories(identity_id, &messages);

        // Combine summaries
        let combined = match (compaction_summary, memory_context) {
            (Some(c), Some(m)) => Some(format!(
                "## Session Context\n{}\n\n## Relevant Memories\n{}",
                c, m
            )),
            (Some(c), None) => Some(format!("## Session Context\n{}", c)),
            (None, Some(m)) => Some(format!("## Relevant Memories\n{}", m)),
            (None, None) => None,
        };

        (messages, combined)
    }

    /// Check the compaction urgency level based on current token usage
    pub fn check_compaction_level(&self, session_id: i64) -> CompactionLevel {
        let config = &self.compaction_config;

        let session = self.get_session_cached(session_id);
        let max_tokens = session.as_ref()
            .map(|s| s.max_context_tokens)
            .unwrap_or(self.max_context_tokens);
        let available = max_tokens - self.reserve_tokens;
        if available <= 0 {
            return CompactionLevel::Emergency;
        }

        let current = session
            .map(|s| s.context_tokens)
            .unwrap_or(0);
        let ratio = current as f64 / available as f64;

        if ratio >= config.emergency_threshold {
            CompactionLevel::Emergency
        } else if ratio >= config.aggressive_threshold {
            CompactionLevel::Aggressive
        } else if ratio >= config.background_threshold {
            CompactionLevel::Background
        } else {
            CompactionLevel::None
        }
    }

    /// Emergency compaction: synchronously hard-drop oldest 50% of messages
    pub fn compact_emergency(&self, session_id: i64) -> Result<usize, String> {
        let messages = self.db.get_session_messages(session_id)
            .map_err(|e| format!("Failed to get session messages: {}", e))?;

        if messages.len() <= MIN_KEEP_RECENT_MESSAGES as usize {
            return Ok(0);
        }

        let drop_count = ((messages.len() as f64 * self.compaction_config.emergency_drop_ratio) as usize)
            .min(messages.len().saturating_sub(MIN_KEEP_RECENT_MESSAGES as usize));

        if drop_count == 0 {
            return Ok(0);
        }

        // Delete the oldest messages in one batch
        let deleted = self.db.delete_oldest_messages(session_id, drop_count as i32)
            .map_err(|e| format!("Failed to delete oldest messages: {}", e))?;

        // Recalculate context_tokens from remaining messages
        let remaining = self.db.get_session_messages(session_id)
            .map_err(|e| format!("Failed to get remaining messages: {}", e))?;
        let new_token_count = estimate_messages_tokens(&remaining);
        self.set_context_tokens(session_id, new_token_count);

        log::info!(
            "[COMPACTION] Emergency: dropped {} of {} messages for session {} (tokens now {})",
            deleted, messages.len(), session_id, new_token_count
        );

        Ok(deleted as usize)
    }

    /// Tiered compaction: determine level and apply appropriate strategy
    pub async fn compact_tiered(
        &self,
        session_id: i64,
        client: &crate::ai::AiClient,
        identity_id: Option<&str>,
        agent_subtype: Option<&str>,
    ) -> Result<CompactionLevel, String> {
        let level = self.check_compaction_level(session_id);

        match level {
            CompactionLevel::None => Ok(CompactionLevel::None),
            CompactionLevel::Emergency => {
                self.compact_emergency(session_id)?;
                Ok(CompactionLevel::Emergency)
            }
            CompactionLevel::Aggressive | CompactionLevel::Background => {
                // Use existing compaction methods for non-emergency levels
                if let Err(e) = self.compact_session(session_id, client, identity_id, agent_subtype).await {
                    log::error!("[COMPACTION] {} compaction failed: {}, trying emergency", level, e);
                    self.compact_emergency(session_id)?;
                    Ok(CompactionLevel::Emergency)
                } else {
                    Ok(level)
                }
            }
        }
    }
}

/// Save session summary before reset (session memory hook)
/// Writes extracted summary to DB memories table.
pub async fn save_session_memory(
    db: &Arc<Database>,
    client: &AiClient,
    session_id: i64,
    identity_id: Option<&str>,
    message_limit: i32,
    agent_subtype: Option<&str>,
) -> Result<(), String> {
    // Get recent messages from the session
    let messages = db.get_recent_session_messages(session_id, message_limit)
        .map_err(|e| format!("Failed to get session messages: {}", e))?;

    if messages.is_empty() {
        return Err("No messages to summarize".to_string());
    }

    log::info!("[SESSION_MEMORY] Saving session memory for {} messages", messages.len());

    // Build conversation text (all roles — the AI summarizes into a single
    // TITLE+SUMMARY entry so individual tool results don't leak into memory)
    let conversation_text = messages.iter()
        .map(|m| {
            let role = match m.role {
                DbMessageRole::User => "User",
                DbMessageRole::Assistant => "Assistant",
                DbMessageRole::System => "System",
                DbMessageRole::ToolCall => "Tool Call",
                DbMessageRole::ToolResult => "Tool Result",
            };
            format!("{}: {}", role, m.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // Generate summary and title using AI
    let summary_prompt = format!(
        "Analyze this conversation and provide:\n\
        1. A short descriptive title (5-10 words)\n\
        2. A brief summary of the key points discussed\n\n\
        Format your response as:\n\
        TITLE: <title here>\n\
        SUMMARY: <summary here>\n\n\
        Conversation:\n{}",
        conversation_text
    );

    let ai_messages = vec![
        Message {
            role: MessageRole::System,
            content: "You summarize conversations concisely. Respond only with the requested TITLE and SUMMARY format.".to_string(),
        },
        Message {
            role: MessageRole::User,
            content: summary_prompt,
        },
    ];

    let response = client.generate_text(ai_messages).await
        .map_err(|e| format!("Failed to generate session summary: {}", e))?;

    // Parse title and summary from response
    let (title, summary) = parse_title_summary(&response);

    // Write to DB memories table
    let content = format!("### {}\n{}", title, summary);
    let today = Utc::now().format("%Y-%m-%d").to_string();
    db.insert_memory(
        "daily_log",
        &content,
        None, None, 5, identity_id, Some(session_id), None, None,
        Some("session_reset"), Some(&today), agent_subtype,
    ).map_err(|e| format!("Failed to write session summary: {}", e))?;
    log::info!("[SESSION_MEMORY] Saved session summary to daily log: {}", title);

    Ok(())
}

/// Truncate a summary to approximately max_words, breaking at word boundaries
fn truncate_summary(summary: &str, max_words: usize) -> String {
    let words: Vec<&str> = summary.split_whitespace().collect();
    if words.len() <= max_words {
        return summary.to_string();
    }

    let mut result = words[..max_words].join(" ");
    result.push_str("...");
    result
}

/// Parse title and summary from AI response
fn parse_title_summary(response: &str) -> (String, String) {
    let mut title = String::new();
    let mut summary = String::new();

    for line in response.lines() {
        let line = line.trim();
        if line.to_uppercase().starts_with("TITLE:") {
            title = line[6..].trim().to_string();
        } else if line.to_uppercase().starts_with("SUMMARY:") {
            summary = line[8..].trim().to_string();
        } else if !title.is_empty() && !line.to_uppercase().starts_with("SUMMARY:") && summary.is_empty() {
            // Multi-line handling - append to title if before summary
        } else if !summary.is_empty() {
            // Append to summary if we're past the SUMMARY: prefix
            if !summary.is_empty() {
                summary.push(' ');
            }
            summary.push_str(line);
        }
    }

    // Fallbacks
    if title.is_empty() {
        title = format!("Session {}", Utc::now().format("%Y-%m-%d %H:%M"));
    }
    if summary.is_empty() {
        summary = response.chars().take(500).collect();
    }

    (title, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        // Roughly 4 chars per token
        assert!(estimate_tokens("hello") >= 1);
        assert!(estimate_tokens("hello world") >= 2);

        // Longer text
        let long_text = "This is a longer piece of text that should estimate to roughly 10-15 tokens based on our heuristic.";
        let tokens = estimate_tokens(long_text);
        assert!(tokens >= 10 && tokens <= 50);
    }

    #[test]
    fn test_parse_title_summary() {
        let response = "TITLE: Discussion about Rust programming\nSUMMARY: User asked about ownership and borrowing in Rust.";
        let (title, summary) = parse_title_summary(response);
        assert_eq!(title, "Discussion about Rust programming");
        assert!(summary.contains("ownership"));
    }
}
