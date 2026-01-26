//! Memory database operations (daily logs, long-term memories, session summaries)

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::Result as SqliteResult;

use crate::models::{Memory, MemorySearchResult, MemoryType};
use super::super::Database;

impl Database {
    /// Create a memory (daily_log, long_term, session_summary, or compaction)
    pub fn create_memory(
        &self,
        memory_type: MemoryType,
        content: &str,
        category: Option<&str>,
        tags: Option<&str>,
        importance: i32,
        identity_id: Option<&str>,
        session_id: Option<i64>,
        source_channel_type: Option<&str>,
        source_message_id: Option<&str>,
        log_date: Option<NaiveDate>,
        expires_at: Option<DateTime<Utc>>,
    ) -> SqliteResult<Memory> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let log_date_str = log_date.map(|d| d.to_string());
        let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());

        conn.execute(
            "INSERT INTO memories (memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12)",
            rusqlite::params![
                memory_type.as_str(),
                content,
                category,
                tags,
                importance,
                identity_id,
                session_id,
                source_channel_type,
                source_message_id,
                log_date_str,
                &now_str,
                expires_at_str,
            ],
        )?;

        let id = conn.last_insert_rowid();

        Ok(Memory {
            id,
            memory_type,
            content: content.to_string(),
            category: category.map(|s| s.to_string()),
            tags: tags.map(|s| s.to_string()),
            importance,
            identity_id: identity_id.map(|s| s.to_string()),
            session_id,
            source_channel_type: source_channel_type.map(|s| s.to_string()),
            source_message_id: source_message_id.map(|s| s.to_string()),
            log_date,
            created_at: now,
            updated_at: now,
            expires_at,
        })
    }

    /// Search memories using FTS5
    pub fn search_memories(
        &self,
        query: &str,
        memory_type: Option<MemoryType>,
        identity_id: Option<&str>,
        category: Option<&str>,
        min_importance: Option<i32>,
        limit: i32,
    ) -> SqliteResult<Vec<MemorySearchResult>> {
        let conn = self.conn.lock().unwrap();

        // Build the query with filters
        let mut sql = String::from(
            "SELECT m.id, m.memory_type, m.content, m.category, m.tags, m.importance, m.identity_id,
             m.session_id, m.source_channel_type, m.source_message_id, m.log_date,
             m.created_at, m.updated_at, m.expires_at, bm25(memories_fts) as rank
             FROM memories m
             JOIN memories_fts ON m.id = memories_fts.rowid
             WHERE memories_fts MATCH ?1",
        );

        let mut conditions: Vec<String> = Vec::new();
        if memory_type.is_some() {
            conditions.push("m.memory_type = ?2".to_string());
        }
        if identity_id.is_some() {
            conditions.push(format!("m.identity_id = ?{}", if memory_type.is_some() { 3 } else { 2 }));
        }
        if category.is_some() {
            let idx = 2 + (memory_type.is_some() as usize) + (identity_id.is_some() as usize);
            conditions.push(format!("m.category = ?{}", idx));
        }
        if min_importance.is_some() {
            let idx = 2 + (memory_type.is_some() as usize) + (identity_id.is_some() as usize) + (category.is_some() as usize);
            conditions.push(format!("m.importance >= ?{}", idx));
        }

        if !conditions.is_empty() {
            sql.push_str(" AND ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(" ORDER BY rank LIMIT ?");
        let limit_idx = 2 + (memory_type.is_some() as usize) + (identity_id.is_some() as usize)
            + (category.is_some() as usize) + (min_importance.is_some() as usize);
        sql = sql.replace("LIMIT ?", &format!("LIMIT ?{}", limit_idx));

        let mut stmt = conn.prepare(&sql)?;

        // Build params dynamically
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(query.to_string())];
        if let Some(mt) = memory_type {
            params.push(Box::new(mt.as_str().to_string()));
        }
        if let Some(iid) = identity_id {
            params.push(Box::new(iid.to_string()));
        }
        if let Some(cat) = category {
            params.push(Box::new(cat.to_string()));
        }
        if let Some(mi) = min_importance {
            params.push(Box::new(mi));
        }
        params.push(Box::new(limit));

        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let results = stmt
            .query_map(params_ref.as_slice(), |row| {
                let memory = Self::row_to_memory(row)?;
                let rank: f64 = row.get(14)?;
                Ok(MemorySearchResult {
                    memory: memory.into(),
                    rank,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// Get today's daily logs
    pub fn get_todays_daily_logs(&self, identity_id: Option<&str>) -> SqliteResult<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();
        let today = Utc::now().date_naive().to_string();

        let sql = if identity_id.is_some() {
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories WHERE memory_type = 'daily_log' AND log_date = ?1 AND identity_id = ?2 ORDER BY created_at ASC"
        } else {
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories WHERE memory_type = 'daily_log' AND log_date = ?1 ORDER BY created_at ASC"
        };

        let mut stmt = conn.prepare(sql)?;

        let memories: Vec<Memory> = if let Some(iid) = identity_id {
            stmt.query_map(rusqlite::params![&today, iid], |row| Self::row_to_memory(row))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map([&today], |row| Self::row_to_memory(row))?
                .filter_map(|r| r.ok())
                .collect()
        };

        Ok(memories)
    }

    /// Get long-term memories for an identity
    pub fn get_long_term_memories(&self, identity_id: Option<&str>, min_importance: Option<i32>, limit: i32) -> SqliteResult<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();
        let min_imp = min_importance.unwrap_or(0);

        let sql = if identity_id.is_some() {
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories WHERE memory_type = 'long_term' AND identity_id = ?1 AND importance >= ?2
             ORDER BY importance DESC, created_at DESC LIMIT ?3"
        } else {
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories WHERE memory_type = 'long_term' AND importance >= ?1
             ORDER BY importance DESC, created_at DESC LIMIT ?2"
        };

        let mut stmt = conn.prepare(sql)?;

        let memories: Vec<Memory> = if let Some(iid) = identity_id {
            stmt.query_map(rusqlite::params![iid, min_imp, limit], |row| Self::row_to_memory(row))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![min_imp, limit], |row| Self::row_to_memory(row))?
                .filter_map(|r| r.ok())
                .collect()
        };

        Ok(memories)
    }

    /// Get session summaries (past conversation summaries)
    pub fn get_session_summaries(&self, identity_id: Option<&str>, limit: i32) -> SqliteResult<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();

        let sql = if identity_id.is_some() {
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories WHERE memory_type = 'session_summary' AND identity_id = ?1
             ORDER BY created_at DESC LIMIT ?2"
        } else {
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories WHERE memory_type = 'session_summary'
             ORDER BY created_at DESC LIMIT ?1"
        };

        let mut stmt = conn.prepare(sql)?;

        let memories: Vec<Memory> = if let Some(iid) = identity_id {
            stmt.query_map(rusqlite::params![iid, limit], |row| Self::row_to_memory(row))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map([limit], |row| Self::row_to_memory(row))?
                .filter_map(|r| r.ok())
                .collect()
        };

        Ok(memories)
    }

    /// List all memories
    pub fn list_memories(&self) -> SqliteResult<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories ORDER BY created_at DESC LIMIT 100",
        )?;

        let memories = stmt
            .query_map([], |row| Self::row_to_memory(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(memories)
    }

    /// Delete a memory
    pub fn delete_memory(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute("DELETE FROM memories WHERE id = ?1", [id])?;
        Ok(rows_affected > 0)
    }

    /// Cleanup expired memories
    pub fn cleanup_expired_memories(&self) -> SqliteResult<i64> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let rows_affected = conn.execute(
            "DELETE FROM memories WHERE expires_at IS NOT NULL AND expires_at < ?1",
            [&now],
        )?;
        Ok(rows_affected as i64)
    }

    fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
        let created_at_str: String = row.get(11)?;
        let updated_at_str: String = row.get(12)?;
        let expires_at_str: Option<String> = row.get(13)?;
        let log_date_str: Option<String> = row.get(10)?;
        let memory_type_str: String = row.get(1)?;

        Ok(Memory {
            id: row.get(0)?,
            memory_type: MemoryType::from_str(&memory_type_str).unwrap_or(MemoryType::DailyLog),
            content: row.get(2)?,
            category: row.get(3)?,
            tags: row.get(4)?,
            importance: row.get(5)?,
            identity_id: row.get(6)?,
            session_id: row.get(7)?,
            source_channel_type: row.get(8)?,
            source_message_id: row.get(9)?,
            log_date: log_date_str.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
            expires_at: expires_at_str.map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .unwrap()
                    .with_timezone(&Utc)
            }),
        })
    }
}
