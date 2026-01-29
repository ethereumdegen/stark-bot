//! Chat session and session message database operations

use chrono::{DateTime, Timelike, Utc};
use rusqlite::Result as SqliteResult;

use crate::models::{ChatSession, MessageRole, ResetPolicy, SessionMessage, SessionScope};
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

        // Try to get existing session
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
                let conn = self.conn.lock().unwrap();
                conn.execute(
                    "UPDATE chat_sessions SET last_activity_at = ?1, updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![&now_str, session.id],
                )?;
                session.last_activity_at = now;
                session.updated_at = now;
            }

            return Ok(session);
        }

        // Create new session
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour,
             created_at, updated_at, last_activity_at, expires_at, context_tokens, max_context_tokens, compaction_id
             FROM chat_sessions WHERE id = ?1",
        )?;

        let session = stmt
            .query_row([id], |row| Self::row_to_chat_session(row))
            .ok();

        Ok(session)
    }

    /// List all chat sessions
    pub fn list_chat_sessions(&self) -> SqliteResult<Vec<ChatSession>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour,
             created_at, updated_at, last_activity_at, expires_at, context_tokens, max_context_tokens, compaction_id
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
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour,
             created_at, updated_at, last_activity_at, expires_at, context_tokens, max_context_tokens, compaction_id
             FROM chat_sessions WHERE session_key = ?1 AND is_active = 1",
        )?;

        let session = stmt
            .query_row([session_key], |row| Self::row_to_chat_session(row))
            .ok();

        Ok(session)
    }

    /// Reset a chat session (mark old as inactive, create new)
    pub fn reset_chat_session(&self, id: i64) -> SqliteResult<ChatSession> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Get the old session info
        let old_session: Option<(String, Option<String>, String, String, i64, String, String, Option<i32>, Option<i32>)> = conn
            .query_row(
                "SELECT session_key, agent_id, scope, channel_type, channel_id, platform_chat_id, reset_policy, idle_timeout_minutes, daily_reset_hour
                 FROM chat_sessions WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)),
            )
            .ok();

        let Some((session_key, agent_id, scope, channel_type, channel_id, platform_chat_id, reset_policy, idle_timeout, daily_hour)) = old_session else {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        };

        // Mark old session as inactive
        conn.execute(
            "UPDATE chat_sessions SET is_active = 0, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![&now, id],
        )?;

        // Create new session with same settings
        conn.execute(
            "INSERT INTO chat_sessions (session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour, created_at, updated_at, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, ?10, ?10, ?10)",
            rusqlite::params![
                &session_key,
                agent_id,
                &scope,
                &channel_type,
                channel_id,
                &platform_chat_id,
                &reset_policy,
                idle_timeout,
                daily_hour,
                &now,
            ],
        )?;

        let new_id = conn.last_insert_rowid();
        drop(conn);

        self.get_chat_session(new_id).map(|opt| opt.unwrap())
    }

    /// Update session reset policy
    pub fn update_session_reset_policy(
        &self,
        id: i64,
        reset_policy: ResetPolicy,
        idle_timeout_minutes: Option<i32>,
        daily_reset_hour: Option<i32>,
    ) -> SqliteResult<Option<ChatSession>> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE chat_sessions SET reset_policy = ?1, idle_timeout_minutes = ?2, daily_reset_hour = ?3, updated_at = ?4 WHERE id = ?5",
            rusqlite::params![reset_policy.as_str(), idle_timeout_minutes, daily_reset_hour, &now, id],
        )?;

        drop(conn);
        self.get_chat_session(id)
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
        let conn = self.conn.lock().unwrap();
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

    /// Get all messages for a session
    pub fn get_session_messages(&self, session_id: i64) -> SqliteResult<Vec<SessionMessage>> {
        let conn = self.conn.lock().unwrap();

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
        let conn = self.conn.lock().unwrap();

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
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM session_messages WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
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
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET context_tokens = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![context_tokens, &now, session_id],
        )?;
        Ok(())
    }

    /// Set the compaction ID for a session (after compaction occurs)
    pub fn set_session_compaction(&self, session_id: i64, compaction_id: i64) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE chat_sessions SET compaction_id = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![compaction_id, &now, session_id],
        )?;
        Ok(())
    }

    /// Get oldest messages for compaction (excludes most recent messages)
    pub fn get_messages_for_compaction(&self, session_id: i64, keep_recent: i32) -> SqliteResult<Vec<SessionMessage>> {
        let conn = self.conn.lock().unwrap();

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
        let conn = self.conn.lock().unwrap();

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
    pub fn get_session_compaction_summary(&self, session_id: i64) -> SqliteResult<Option<String>> {
        let conn = self.conn.lock().unwrap();

        // First get the compaction_id from the session
        let compaction_id: Option<i64> = conn.query_row(
            "SELECT compaction_id FROM chat_sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        ).ok().flatten();

        let Some(compaction_id) = compaction_id else {
            return Ok(None);
        };

        // Get the compaction memory content
        let content: Option<String> = conn.query_row(
            "SELECT content FROM memories WHERE id = ?1",
            [compaction_id],
            |row| row.get(0),
        ).ok();

        Ok(content)
    }
}
