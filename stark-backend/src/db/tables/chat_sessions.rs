//! Chat session and session message database operations

use chrono::{DateTime, Timelike, Utc};
use rusqlite::Result as SqliteResult;

use crate::models::{ChatSession, CompletionStatus, MessageRole, ResetPolicy, SessionMessage, SessionScope};
use super::super::Database;

impl Database {
    // ============================================
    // Chat Session methods
    // ============================================

    /// Generate a session key from channel info
    fn generate_session_key(channel_type: &str, channel_id: i64, platform_chat_id: &str) -> String {
        format!("{}:{}:{}", channel_type, channel_id, platform_chat_id)
    }

    /// Get or create a chat session, handling reset policy
    pub fn get_or_create_chat_session(
        &self,
        channel_type: &str,
        channel_id: i64,
        platform_chat_id: &str,
        scope: SessionScope,
        agent_id: Option<&str>,
    ) -> SqliteResult<ChatSession> {
        let session_key = Self::generate_session_key(channel_type, channel_id, platform_chat_id);
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        // Try to get existing active session
        if let Some(mut session) = self.get_chat_session_by_key(&session_key)? {
            // Check if session needs reset based on policy
            let should_reset = match session.reset_policy {
                ResetPolicy::Daily => {
                    let reset_hour = session.daily_reset_hour.unwrap_or(0);
                    let last_activity = session.last_activity_at;
                    let last_day = last_activity.date_naive();
                    let today = now.date_naive();

                    if today > last_day {
                        // Check if we've passed the reset hour today
                        now.hour() >= reset_hour as u32
                    } else {
                        false
                    }
                }
                ResetPolicy::Idle => {
                    if let Some(timeout) = session.idle_timeout_minutes {
                        let idle_duration = now.signed_duration_since(session.last_activity_at);
                        idle_duration.num_minutes() > timeout as i64
                    } else {
                        false
                    }
                }
                ResetPolicy::Manual | ResetPolicy::Never => false,
            };

            if should_reset {
                // Reset the session
                self.reset_chat_session(session.id)?;
                session = self.get_chat_session(session.id)?.unwrap();
            } else {
                // Update last activity
                let conn = self.conn();
                conn.execute(
                    "UPDATE chat_sessions SET last_activity_at = ?1, updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![&now_str, session.id],
                )?;
                session.last_activity_at = now;
                session.updated_at = now;
            }

            return Ok(session);
        }

        // No active session found - check if there's an inactive one we can reactivate
        let conn = self.conn();
        let inactive_session_id: Option<i64> = conn.query_row(
            "SELECT id FROM chat_sessions WHERE session_key = ?1 AND is_active = 0 ORDER BY updated_at DESC LIMIT 1",
            [&session_key],
            |row| row.get(0),
        ).ok();

        if let Some(inactive_id) = inactive_session_id {
            // Reactivate the existing inactive session
            conn.execute(
                "UPDATE chat_sessions SET is_active = 1, last_activity_at = ?1, updated_at = ?1, completion_status = 'active' WHERE id = ?2",
                rusqlite::params![&now_str, inactive_id],
            )?;
            drop(conn);
            return self.get_chat_session(inactive_id).map(|opt| opt.unwrap());
        }

        // Create new session
        conn.execute(
            "INSERT INTO chat_sessions (session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour, created_at, updated_at, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, ?10, ?10, ?10)",
            rusqlite::params![
                &session_key,
                agent_id,
                scope.as_str(),
                channel_type,
                channel_id,
                platform_chat_id,
                ResetPolicy::default().as_str(),
                Option::<i32>::None,
                Some(0i32),
                &now_str,
            ],
        )?;

        let id = conn.last_insert_rowid();
        drop(conn);

        self.get_chat_session(id).map(|opt| opt.unwrap())
    }

    /// Get a chat session by ID
    pub fn get_chat_session(&self, id: i64) -> SqliteResult<Option<ChatSession>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour,
             created_at, updated_at, last_activity_at, expires_at, context_tokens, max_context_tokens, compaction_id, completion_status, safe_mode, special_role_name
             FROM chat_sessions WHERE id = ?1",
        )?;

        let session = stmt
            .query_row([id], |row| Self::row_to_chat_session(row))
            .ok();

        Ok(session)
    }

    /// List all chat sessions
    pub fn list_chat_sessions(&self) -> SqliteResult<Vec<ChatSession>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour,
             created_at, updated_at, last_activity_at, expires_at, context_tokens, max_context_tokens, compaction_id, completion_status, safe_mode, special_role_name
             FROM chat_sessions ORDER BY last_activity_at DESC LIMIT 100",
        )?;

        let sessions = stmt
            .query_map([], |row| Self::row_to_chat_session(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(sessions)
    }

    /// Get a chat session by session key
    pub fn get_chat_session_by_key(&self, session_key: &str) -> SqliteResult<Option<ChatSession>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour,
             created_at, updated_at, last_activity_at, expires_at, context_tokens, max_context_tokens, compaction_id, completion_status, safe_mode, special_role_name
             FROM chat_sessions WHERE session_key = ?1 AND is_active = 1",
        )?;

        let session = stmt
            .query_row([session_key], |row| Self::row_to_chat_session(row))
            .ok();

        Ok(session)
    }

    /// Get the latest active session for a channel (without creating)
    /// Used by gateway channels to get previous session before creating a new one
    pub fn get_latest_session_for_channel(
        &self,
        channel_type: &str,
        channel_id: i64,
    ) -> SqliteResult<Option<ChatSession>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour,
             created_at, updated_at, last_activity_at, expires_at, context_tokens, max_context_tokens, compaction_id, completion_status, safe_mode, special_role_name
             FROM chat_sessions
             WHERE channel_type = ?1 AND channel_id = ?2 AND is_active = 1
             ORDER BY last_activity_at DESC LIMIT 1",
        )?;

        let session = stmt
            .query_row(rusqlite::params![channel_type, channel_id], |row| Self::row_to_chat_session(row))
            .ok();

        Ok(session)
    }

    /// Create a new session for gateway channels (Discord, Telegram)
    /// Always creates a fresh session with a unique key
    pub fn create_gateway_session(
        &self,
        channel_type: &str,
        channel_id: i64,
        scope: SessionScope,
        agent_id: Option<&str>,
    ) -> SqliteResult<ChatSession> {
        let conn = self.conn();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        // Generate unique platform_chat_id with timestamp to ensure fresh session
        let timestamp = now.timestamp_millis();
        let platform_chat_id = format!("gateway-{}", timestamp);
        let session_key = Self::generate_session_key(channel_type, channel_id, &platform_chat_id);

        // Create new session
        conn.execute(
            "INSERT INTO chat_sessions (session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour, created_at, updated_at, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, ?10, ?10, ?10)",
            rusqlite::params![
                &session_key,
                agent_id,
                scope.as_str(),
                channel_type,
                channel_id,
                &platform_chat_id,
                ResetPolicy::default().as_str(),
                Option::<i32>::None,
                Some(0i32),
                &now_str,
            ],
        )?;

        let id = conn.last_insert_rowid();
        drop(conn);

        self.get_chat_session(id).map(|opt| opt.unwrap())
    }

    /// Mark a session as inactive (used when creating a new gateway session)
    pub fn deactivate_session(&self, session_id: i64) -> SqliteResult<()> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET is_active = 0, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![&now, session_id],
        )?;
        Ok(())
    }

    /// Reset a chat session (mark old as inactive, create new)
    pub fn reset_chat_session(&self, id: i64) -> SqliteResult<ChatSession> {
        let conn = self.conn();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        // Get the old session info
        let old_session: Option<(String, Option<String>, String, String, i64, String, String, Option<i32>, Option<i32>)> = conn
            .query_row(
                "SELECT session_key, agent_id, scope, channel_type, channel_id, platform_chat_id, reset_policy, idle_timeout_minutes, daily_reset_hour
                 FROM chat_sessions WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)),
            )
            .ok();

        let Some((old_session_key, agent_id, scope, channel_type, channel_id, platform_chat_id, reset_policy, idle_timeout, daily_hour)) = old_session else {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        };

        // Archive the old session: give it a unique key so get_or_create_chat_session
        // won't find and reactivate it (which would cause a session_id mismatch with
        // the frontend's cached dbSessionId).
        let archived_key = format!("{}:archived-{}", old_session_key, now.timestamp_millis());
        conn.execute(
            "UPDATE chat_sessions SET is_active = 0, session_key = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![&archived_key, &now_str, id],
        )?;

        // Delete agent context for the old session
        conn.execute(
            "DELETE FROM agent_contexts WHERE session_id = ?1",
            rusqlite::params![id],
        )?;

        // Create new session with the original key so get_or_create_chat_session finds it
        conn.execute(
            "INSERT INTO chat_sessions (session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour, created_at, updated_at, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, ?10, ?10, ?10)",
            rusqlite::params![
                &old_session_key,
                agent_id,
                &scope,
                &channel_type,
                channel_id,
                &platform_chat_id,
                &reset_policy,
                idle_timeout,
                daily_hour,
                &now_str,
            ],
        )?;

        let new_id = conn.last_insert_rowid();
        drop(conn);

        self.get_chat_session(new_id).map(|opt| opt.unwrap())
    }

    /// Delete a chat session and all its messages
    pub fn delete_chat_session(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn();

        // Delete agent context for the session
        conn.execute(
            "DELETE FROM agent_contexts WHERE session_id = ?1",
            rusqlite::params![id],
        )?;

        // Delete sub_agents for the session (FK constraint lacks ON DELETE CASCADE)
        conn.execute(
            "DELETE FROM sub_agents WHERE parent_session_id = ?1 OR session_id = ?1",
            rusqlite::params![id],
        )?;

        // Delete the session (messages are cascade deleted via FK constraint)
        let deleted = conn.execute(
            "DELETE FROM chat_sessions WHERE id = ?1",
            rusqlite::params![id],
        )?;

        Ok(deleted > 0)
    }

    /// Delete all chat sessions and their messages
    /// Returns (deleted_count, channel_ids) where channel_ids can be used to cancel agents
    pub fn delete_all_chat_sessions(&self) -> SqliteResult<(i64, Vec<i64>)> {
        let conn = self.conn();

        // Get all unique channel_ids first (for cancelling agents)
        let mut stmt = conn.prepare("SELECT DISTINCT channel_id FROM chat_sessions")?;
        let channel_ids: Vec<i64> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        // Delete all agent contexts
        conn.execute("DELETE FROM agent_contexts", [])?;

        // Delete all sub_agents (FK constraint lacks ON DELETE CASCADE)
        conn.execute("DELETE FROM sub_agents", [])?;

        // Count sessions before deleting
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM chat_sessions", [], |row| row.get(0))?;

        // Delete all sessions (messages are cascade deleted via FK constraint)
        conn.execute("DELETE FROM chat_sessions", [])?;

        Ok((count, channel_ids))
    }

    /// Update session reset policy
    pub fn update_session_reset_policy(
        &self,
        id: i64,
        reset_policy: ResetPolicy,
        idle_timeout_minutes: Option<i32>,
        daily_reset_hour: Option<i32>,
    ) -> SqliteResult<Option<ChatSession>> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE chat_sessions SET reset_policy = ?1, idle_timeout_minutes = ?2, daily_reset_hour = ?3, updated_at = ?4 WHERE id = ?5",
            rusqlite::params![reset_policy.as_str(), idle_timeout_minutes, daily_reset_hour, &now, id],
        )?;

        drop(conn);
        self.get_chat_session(id)
    }

    /// Set safe_mode flag on a session (called when force_safe_mode message is processed)
    pub fn set_session_safe_mode(&self, id: i64) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE chat_sessions SET safe_mode = 1, updated_at = ?1 WHERE id = ?2 AND safe_mode = 0",
            rusqlite::params![Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    /// Set the special_role_name on a session (called when special role enrichment is applied)
    pub fn set_session_special_role(&self, id: i64, role_name: &str) -> SqliteResult<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE chat_sessions SET special_role_name = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![role_name, Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    fn row_to_chat_session(row: &rusqlite::Row) -> rusqlite::Result<ChatSession> {
        let created_at_str: String = row.get(11)?;
        let updated_at_str: String = row.get(12)?;
        let last_activity_str: String = row.get(13)?;
        let expires_at_str: Option<String> = row.get(14)?;
        let scope_str: String = row.get(3)?;
        let reset_policy_str: String = row.get(8)?;

        Ok(ChatSession {
            id: row.get(0)?,
            session_key: row.get(1)?,
            agent_id: row.get(2)?,
            scope: SessionScope::from_str(&scope_str).unwrap_or_default(),
            channel_type: row.get(4)?,
            channel_id: row.get(5)?,
            platform_chat_id: row.get(6)?,
            is_active: row.get::<_, i32>(7)? != 0,
            reset_policy: ResetPolicy::from_str(&reset_policy_str).unwrap_or_default(),
            idle_timeout_minutes: row.get(9)?,
            daily_reset_hour: row.get(10)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
            last_activity_at: DateTime::parse_from_rfc3339(&last_activity_str)
                .unwrap()
                .with_timezone(&Utc),
            expires_at: expires_at_str.map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .unwrap()
                    .with_timezone(&Utc)
            }),
            context_tokens: row.get(15).unwrap_or(0),
            max_context_tokens: row.get(16).unwrap_or(100000),
            compaction_id: row.get(17).ok(),
            completion_status: {
                let status_str: String = row.get(18).unwrap_or_else(|_| "active".to_string());
                CompletionStatus::from_str(&status_str).unwrap_or_default()
            },
            safe_mode: row.get::<_, i32>(19).unwrap_or(0) != 0,
            special_role_name: row.get::<_, Option<String>>(20).unwrap_or(None),
        })
    }

    // ============================================
    // Session Message methods
    // ============================================

    /// Add a message to a session
    pub fn add_session_message(
        &self,
        session_id: i64,
        role: MessageRole,
        content: &str,
        user_id: Option<&str>,
        user_name: Option<&str>,
        platform_message_id: Option<&str>,
        tokens_used: Option<i32>,
    ) -> SqliteResult<SessionMessage> {
        let conn = self.conn();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        conn.execute(
            "INSERT INTO session_messages (session_id, role, content, user_id, user_name, platform_message_id, tokens_used, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                session_id,
                role.as_str(),
                content,
                user_id,
                user_name,
                platform_message_id,
                tokens_used,
                &now_str,
            ],
        )?;

        let id = conn.last_insert_rowid();

        Ok(SessionMessage {
            id,
            session_id,
            role,
            content: content.to_string(),
            user_id: user_id.map(|s| s.to_string()),
            user_name: user_name.map(|s| s.to_string()),
            platform_message_id: platform_message_id.map(|s| s.to_string()),
            tokens_used,
            created_at: now,
        })
    }

    /// Batch insert multiple session messages in a single transaction.
    /// Much faster than individual inserts when saving tool call/result pairs.
    pub fn add_session_messages_batch(
        &self,
        messages: &[(i64, MessageRole, String, Option<String>, Option<String>)],
    ) -> SqliteResult<()> {
        if messages.is_empty() {
            return Ok(());
        }
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO session_messages (session_id, role, content, user_id, user_name, platform_message_id, tokens_used, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6)",
            )?;
            let now_str = Utc::now().to_rfc3339();
            for (session_id, role, content, _user_id, user_name) in messages {
                stmt.execute(rusqlite::params![
                    session_id,
                    role.as_str(),
                    content,
                    Option::<&str>::None,
                    user_name.as_deref(),
                    &now_str,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Get all messages for a session
    pub fn get_session_messages(&self, session_id: i64) -> SqliteResult<Vec<SessionMessage>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, user_id, user_name, platform_message_id, tokens_used, created_at
             FROM session_messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;

        let messages = stmt
            .query_map([session_id], |row| Self::row_to_session_message(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(messages)
    }

    /// Get recent messages for a session (limited)
    pub fn get_recent_session_messages(&self, session_id: i64, limit: i32) -> SqliteResult<Vec<SessionMessage>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, user_id, user_name, platform_message_id, tokens_used, created_at
             FROM session_messages WHERE session_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;

        let mut messages: Vec<SessionMessage> = stmt
            .query_map(rusqlite::params![session_id, limit], |row| Self::row_to_session_message(row))?
            .filter_map(|r| r.ok())
            .collect();

        // Reverse to get chronological order
        messages.reverse();
        Ok(messages)
    }

    /// Count messages in a session
    pub fn count_session_messages(&self, session_id: i64) -> SqliteResult<i64> {
        let conn = self.conn();
        conn.query_row(
            "SELECT COUNT(*) FROM session_messages WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
    }

    /// Get the first user message for a session (for showing initial query)
    pub fn get_first_user_message(&self, session_id: i64) -> SqliteResult<Option<String>> {
        let conn = self.conn();
        conn.query_row(
            "SELECT content FROM session_messages
             WHERE session_id = ?1 AND role = 'user'
             ORDER BY created_at ASC LIMIT 1",
            [session_id],
            |row| row.get(0),
        ).map(Some).or_else(|e| {
            if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                Ok(None)
            } else {
                Err(e)
            }
        })
    }

    /// List heartbeat sessions with their associated impulse node IDs
    /// Parses the node ID from the heartbeat message content
    pub fn list_heartbeat_sessions(&self, limit: i32) -> SqliteResult<Vec<(ChatSession, Option<i64>)>> {
        let conn = self.conn();

        // Get heartbeat sessions ordered by most recent first
        let mut stmt = conn.prepare(
            "SELECT id, session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour,
             created_at, updated_at, last_activity_at, expires_at, context_tokens, max_context_tokens, compaction_id, completion_status, safe_mode, special_role_name
             FROM chat_sessions
             WHERE channel_type = 'heartbeat'
             ORDER BY created_at DESC
             LIMIT ?1",
        )?;

        let sessions: Vec<ChatSession> = stmt
            .query_map([limit], |row| Self::row_to_chat_session(row))?
            .filter_map(|r| r.ok())
            .collect();

        drop(stmt);

        // For each session, try to extract the impulse node ID from the first message
        let mut results = Vec::new();
        for session in sessions {
            let node_id = self.extract_heartbeat_node_id(&conn, session.id);
            results.push((session, node_id));
        }

        Ok(results)
    }

    /// Extract the impulse node ID from a heartbeat session's first message
    /// Looks for pattern "Current Position: Node #X" in the message
    fn extract_heartbeat_node_id(&self, conn: &super::super::DbConn, session_id: i64) -> Option<i64> {
        let content: Option<String> = conn.query_row(
            "SELECT content FROM session_messages
             WHERE session_id = ?1 AND role = 'user'
             ORDER BY created_at ASC LIMIT 1",
            [session_id],
            |row| row.get(0),
        ).ok();

        content.and_then(|text| {
            // Parse "Current Position: Node #123" pattern
            if let Some(start) = text.find("Current Position: Node #") {
                let after_hash = start + "Current Position: Node #".len();
                let rest = &text[after_hash..];
                // Find the end of the number (space, comma, or parenthesis)
                let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
                rest[..end].parse::<i64>().ok()
            } else {
                None
            }
        })
    }

    fn row_to_session_message(row: &rusqlite::Row) -> rusqlite::Result<SessionMessage> {
        let created_at_str: String = row.get(8)?;
        let role_str: String = row.get(2)?;

        Ok(SessionMessage {
            id: row.get(0)?,
            session_id: row.get(1)?,
            role: MessageRole::from_str(&role_str).unwrap_or(MessageRole::User),
            content: row.get(3)?,
            user_id: row.get(4)?,
            user_name: row.get(5)?,
            platform_message_id: row.get(6)?,
            tokens_used: row.get(7)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
        })
    }

    // ============================================
    // Context Management methods (compaction)
    // ============================================

    /// Update the context token count for a session
    pub fn update_session_context_tokens(&self, session_id: i64, context_tokens: i32) -> SqliteResult<()> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET context_tokens = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![context_tokens, &now, session_id],
        )?;
        Ok(())
    }

    /// Update the max context tokens limit for a session (for dynamic compaction thresholds)
    pub fn update_session_max_context_tokens(&self, session_id: i64, max_context_tokens: i32) -> SqliteResult<()> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET max_context_tokens = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![max_context_tokens, &now, session_id],
        )?;
        Ok(())
    }

    /// Get oldest messages for compaction (excludes most recent messages)
    pub fn get_messages_for_compaction(&self, session_id: i64, keep_recent: i32) -> SqliteResult<Vec<SessionMessage>> {
        let conn = self.conn();

        // Get total count first
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_messages WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )?;

        let to_compact = (total as i32).saturating_sub(keep_recent);
        if to_compact <= 0 {
            return Ok(vec![]);
        }

        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, user_id, user_name, platform_message_id, tokens_used, created_at
             FROM session_messages WHERE session_id = ?1 ORDER BY created_at ASC LIMIT ?2",
        )?;

        let messages = stmt
            .query_map(rusqlite::params![session_id, to_compact], |row| {
                let created_at_str: String = row.get(8)?;
                let role_str: String = row.get(2)?;

                Ok(SessionMessage {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: MessageRole::from_str(&role_str).unwrap_or(MessageRole::User),
                    content: row.get(3)?,
                    user_id: row.get(4)?,
                    user_name: row.get(5)?,
                    platform_message_id: row.get(6)?,
                    tokens_used: row.get(7)?,
                    created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(messages)
    }

    /// Delete old messages after compaction (keeps the most recent messages)
    pub fn delete_compacted_messages(&self, session_id: i64, keep_recent: i32) -> SqliteResult<i32> {
        let conn = self.conn();

        // Get IDs of messages to delete (all except the most recent)
        let deleted = conn.execute(
            "DELETE FROM session_messages WHERE session_id = ?1 AND id NOT IN (
                SELECT id FROM session_messages WHERE session_id = ?1 ORDER BY created_at DESC LIMIT ?2
            )",
            rusqlite::params![session_id, keep_recent],
        )?;

        Ok(deleted as i32)
    }

    /// Get the compaction summary for a session (if any)
    /// Now reads from the compaction_summary column directly instead of memories table.
    pub fn get_session_compaction_summary(&self, session_id: i64) -> SqliteResult<Option<String>> {
        let conn = self.conn();

        let summary: Option<String> = conn.query_row(
            "SELECT compaction_summary FROM chat_sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        ).ok().flatten();

        Ok(summary)
    }

    /// Set the compaction summary text for a session
    pub fn set_session_compaction_summary(&self, session_id: i64, summary: &str) -> SqliteResult<()> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET compaction_summary = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![summary, &now, session_id],
        )?;
        Ok(())
    }

    /// Update the last_flush_at timestamp for a session (Phase 1: pre-compaction flush)
    pub fn update_session_last_flush(&self, session_id: i64) -> SqliteResult<()> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET last_flush_at = ?1, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![&now, session_id],
        )?;
        Ok(())
    }

    /// Get the last flush timestamp for a session
    pub fn get_session_last_flush(&self, session_id: i64) -> SqliteResult<Option<chrono::DateTime<Utc>>> {
        let conn = self.conn();
        let flush_str: Option<String> = conn.query_row(
            "SELECT last_flush_at FROM chat_sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        ).ok().flatten();

        Ok(flush_str.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))
        }))
    }

    // ============================================
    // Sliding Window Compaction methods
    // ============================================

    /// Get the oldest N messages for incremental compaction
    pub fn get_oldest_messages(&self, session_id: i64, limit: i32) -> SqliteResult<Vec<SessionMessage>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, user_id, user_name, platform_message_id, tokens_used, created_at
             FROM session_messages WHERE session_id = ?1 ORDER BY created_at ASC LIMIT ?2",
        )?;

        let messages = stmt
            .query_map(rusqlite::params![session_id, limit], |row| Self::row_to_session_message(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(messages)
    }

    /// Delete the oldest N messages from a session
    pub fn delete_oldest_messages(&self, session_id: i64, count: i32) -> SqliteResult<i32> {
        let conn = self.conn();

        // Delete oldest N messages by ID
        let deleted = conn.execute(
            "DELETE FROM session_messages WHERE id IN (
                SELECT id FROM session_messages WHERE session_id = ?1 ORDER BY created_at ASC LIMIT ?2
            )",
            rusqlite::params![session_id, count],
        )?;

        Ok(deleted as i32)
    }

    /// Increment the compaction generation counter for a session
    pub fn increment_compaction_generation(&self, session_id: i64) -> SqliteResult<()> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET compaction_generation = compaction_generation + 1, last_compaction_at = ?1, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![&now, session_id],
        )?;
        Ok(())
    }

    /// Get the compaction generation for a session
    pub fn get_compaction_generation(&self, session_id: i64) -> SqliteResult<i32> {
        let conn = self.conn();
        conn.query_row(
            "SELECT COALESCE(compaction_generation, 0) FROM chat_sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
    }

    // ============================================
    // Completion Status methods (Task Planner)
    // ============================================

    /// Update the completion status of a session
    pub fn update_session_completion_status(&self, session_id: i64, status: CompletionStatus) -> SqliteResult<()> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET completion_status = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![status.as_str(), &now, session_id],
        )?;
        Ok(())
    }

    /// Get the completion status of a session
    pub fn get_session_completion_status(&self, session_id: i64) -> SqliteResult<Option<CompletionStatus>> {
        let conn = self.conn();
        conn.query_row(
            "SELECT completion_status FROM chat_sessions WHERE id = ?1",
            [session_id],
            |row| {
                let status_str: String = row.get(0)?;
                Ok(CompletionStatus::from_str(&status_str).unwrap_or_default())
            },
        ).map(Some).or_else(|e| {
            if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                Ok(None)
            } else {
                Err(e)
            }
        })
    }

    /// Find and mark stale sessions as failed.
    ///
    /// Sessions with `completion_status = 'active'` and `updated_at` older than
    /// `stale_minutes` are considered stuck (e.g., due to a panic or dropped future)
    /// and are marked as `failed` so they don't block the UI or appear as running.
    ///
    /// Returns the number of sessions cleaned up.
    pub fn cleanup_stale_active_sessions(&self, stale_minutes: i64) -> SqliteResult<usize> {
        let conn = self.conn();
        let cutoff = (Utc::now() - chrono::Duration::minutes(stale_minutes)).to_rfc3339();
        let count = conn.execute(
            "UPDATE chat_sessions
             SET completion_status = 'failed', updated_at = ?1
             WHERE completion_status = 'active'
               AND updated_at < ?2",
            rusqlite::params![&Utc::now().to_rfc3339(), &cutoff],
        )?;
        Ok(count)
    }
}
