//! Database operations for the `memories` table
//! Core CRUD for the structured memory system (SQL-backed).

use crate::db::Database;

/// Standard SELECT columns for memory queries
const MEMORY_SELECT_COLS: &str =
    "id, memory_type, content, category, tags, importance,
     identity_id, session_id, entity_type, entity_name,
     source_type, log_date, created_at, updated_at, last_accessed, agent_subtype";

/// Table-qualified SELECT columns for JOIN queries (avoids ambiguous column names with FTS)
const MEMORY_SELECT_COLS_QUALIFIED: &str =
    "memories.id, memories.memory_type, memories.content, memories.category, memories.tags, memories.importance,
     memories.identity_id, memories.session_id, memories.entity_type, memories.entity_name,
     memories.source_type, memories.log_date, memories.created_at, memories.updated_at, memories.last_accessed, memories.agent_subtype";

/// A row from the `memories` table
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryRow {
    pub id: i64,
    pub memory_type: String,
    pub content: String,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub importance: i64,
    pub identity_id: Option<String>,
    pub session_id: Option<i64>,
    pub entity_type: Option<String>,
    pub entity_name: Option<String>,
    pub source_type: Option<String>,
    pub log_date: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed: Option<String>,
    pub agent_subtype: Option<String>,
}

/// Parse a MemoryRow from a rusqlite::Row using the standard column order.
fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<MemoryRow> {
    Ok(MemoryRow {
        id: row.get(0)?,
        memory_type: row.get(1)?,
        content: row.get(2)?,
        category: row.get(3)?,
        tags: row.get(4)?,
        importance: row.get::<_, Option<f64>>(5)?.map(|v| v.round() as i64).unwrap_or(5),
        identity_id: row.get(6)?,
        session_id: row.get(7)?,
        entity_type: row.get(8)?,
        entity_name: row.get(9)?,
        source_type: row.get(10)?,
        log_date: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        last_accessed: row.get(14)?,
        agent_subtype: row.get(15)?,
    })
}

impl Database {
    /// Insert a new memory and return its ID.
    /// FTS index is updated automatically via database triggers.
    /// Content is automatically redacted for PII/secrets before insertion.
    pub fn insert_memory(
        &self,
        memory_type: &str,
        content: &str,
        category: Option<&str>,
        tags: Option<&str>,
        importance: i64,
        identity_id: Option<&str>,
        session_id: Option<i64>,
        entity_type: Option<&str>,
        entity_name: Option<&str>,
        source_type: Option<&str>,
        log_date: Option<&str>,
        agent_subtype: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        // Redact secrets/PII before persisting
        let redaction = crate::memory::redaction::redact_content(content);
        if redaction.redaction_count > 0 {
            log::warn!(
                "Redacted {} secret(s) from memory: {:?}",
                redaction.redaction_count,
                redaction.redacted_types
            );
        }
        let content = &redaction.content;

        let conn = self.conn();
        conn.execute(
            "INSERT INTO memories (
                memory_type, content, category, tags, importance,
                identity_id, session_id, entity_type, entity_name,
                source_type, log_date, agent_subtype, created_at, updated_at, last_accessed
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9,
                ?10, ?11, ?12, datetime('now'), datetime('now'), datetime('now')
            )",
            rusqlite::params![
                memory_type, content, category, tags, importance,
                identity_id, session_id, entity_type, entity_name,
                source_type, log_date, agent_subtype,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Insert a memory with a specific created_at timestamp (for restore).
    /// Preserves the original creation date from the backup.
    pub fn insert_memory_with_created_at(
        &self,
        memory_type: &str,
        content: &str,
        category: Option<&str>,
        tags: Option<&str>,
        importance: i64,
        identity_id: Option<&str>,
        session_id: Option<i64>,
        entity_type: Option<&str>,
        entity_name: Option<&str>,
        source_type: Option<&str>,
        log_date: Option<&str>,
        created_at: &str,
        agent_subtype: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO memories (
                memory_type, content, category, tags, importance,
                identity_id, session_id, entity_type, entity_name,
                source_type, log_date, agent_subtype, created_at, updated_at, last_accessed
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9,
                ?10, ?11, ?12, ?13, datetime('now'), datetime('now')
            )",
            rusqlite::params![
                memory_type, content, category, tags, importance,
                identity_id, session_id, entity_type, entity_name,
                source_type, log_date, agent_subtype, created_at,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// List all memories (for backup export).
    /// Returns lightweight rows without embeddings or associations.
    pub fn list_all_memories(&self) -> Result<Vec<MemoryRow>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            &format!("SELECT {} FROM memories ORDER BY id", MEMORY_SELECT_COLS)
        )?;
        let rows = stmt.query_map([], |row| row_to_memory(row))?;
        rows.collect()
    }

    /// Clear all memories for restore.
    /// Explicitly deletes embeddings + associations first because SQLite FK
    /// cascading requires `PRAGMA foreign_keys = ON` (which is not always set).
    pub fn clear_memories_for_restore(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn();
        conn.execute("DELETE FROM memory_embeddings", [])?;
        conn.execute("DELETE FROM memory_associations", [])?;
        let deleted = conn.execute("DELETE FROM memories", [])?;
        Ok(deleted)
    }

    /// Count total memories in the table.
    pub fn count_memories(&self) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
    }

    /// Touch a memory's last_accessed timestamp (for decay tracking).
    pub fn touch_memory(&self, memory_id: i64) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE memories SET last_accessed = datetime('now') WHERE id = ?1",
            rusqlite::params![memory_id],
        )?;
        Ok(())
    }

    /// Get a single memory by ID.
    pub fn get_memory(&self, memory_id: i64) -> Result<Option<MemoryRow>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            &format!("SELECT {} FROM memories WHERE id = ?1", MEMORY_SELECT_COLS)
        )?;
        let result = stmt.query_row(rusqlite::params![memory_id], |row| row_to_memory(row));
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    // ====================================================================
    // Query helpers for the unified memory system
    // ====================================================================

    /// Fetch recent long_term memories (non-superseded), ordered by created_at DESC.
    /// If identity_id is Some, filters to that identity; if None, returns all identities.
    pub fn get_long_term_memories(
        &self,
        identity_id: Option<&str>,
        limit: i32,
    ) -> Result<Vec<MemoryRow>, rusqlite::Error> {
        let conn = self.conn();
        let (sql, params) = match identity_id {
            Some(id) => (
                format!(
                    "SELECT {} FROM memories
                     WHERE memory_type = 'long_term' AND superseded_by IS NULL AND identity_id = ?1
                     ORDER BY created_at DESC LIMIT ?2",
                    MEMORY_SELECT_COLS
                ),
                vec![
                    Box::new(id.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(limit),
                ],
            ),
            None => (
                format!(
                    "SELECT {} FROM memories
                     WHERE memory_type = 'long_term' AND superseded_by IS NULL
                     ORDER BY created_at DESC LIMIT ?1",
                    MEMORY_SELECT_COLS
                ),
                vec![Box::new(limit) as Box<dyn rusqlite::types::ToSql>],
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| row_to_memory(row))?;
        rows.collect()
    }

    /// Fetch daily_log entries for a specific date (YYYY-MM-DD).
    pub fn get_daily_log_memories(
        &self,
        date: &str,
        identity_id: Option<&str>,
        limit: i32,
    ) -> Result<Vec<MemoryRow>, rusqlite::Error> {
        let conn = self.conn();
        let (sql, params) = match identity_id {
            Some(id) => (
                format!(
                    "SELECT {} FROM memories
                     WHERE memory_type = 'daily_log' AND log_date = ?1 AND identity_id = ?2
                     ORDER BY created_at ASC LIMIT ?3",
                    MEMORY_SELECT_COLS
                ),
                vec![
                    Box::new(date.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(id.to_string()),
                    Box::new(limit),
                ],
            ),
            None => (
                format!(
                    "SELECT {} FROM memories
                     WHERE memory_type = 'daily_log' AND log_date = ?1
                     ORDER BY created_at ASC LIMIT ?2",
                    MEMORY_SELECT_COLS
                ),
                vec![
                    Box::new(date.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(limit),
                ],
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| row_to_memory(row))?;
        rows.collect()
    }

    /// Convenience: fetch today's daily log entries.
    pub fn get_today_daily_log(
        &self,
        identity_id: Option<&str>,
        limit: i32,
    ) -> Result<Vec<MemoryRow>, rusqlite::Error> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.get_daily_log_memories(&today, identity_id, limit)
    }

    /// FTS5 full-text search against the existing `memories_fts` virtual table.
    /// Returns matching memories with BM25 rank score (lower = better match).
    pub fn search_memories_fts(
        &self,
        query: &str,
        identity_id: Option<&str>,
        limit: i32,
    ) -> Result<Vec<(MemoryRow, f64)>, rusqlite::Error> {
        let conn = self.conn();
        let (sql, params) = match identity_id {
            Some(id) => (
                format!(
                    "SELECT {cols}, bm25(memories_fts) as rank
                     FROM memories
                     JOIN memories_fts ON memories.id = memories_fts.rowid
                     WHERE memories_fts MATCH ?1 AND memories.identity_id = ?2
                     ORDER BY rank
                     LIMIT ?3",
                    cols = MEMORY_SELECT_COLS_QUALIFIED
                ),
                vec![
                    Box::new(query.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(id.to_string()),
                    Box::new(limit),
                ],
            ),
            None => (
                format!(
                    "SELECT {cols}, bm25(memories_fts) as rank
                     FROM memories
                     JOIN memories_fts ON memories.id = memories_fts.rowid
                     WHERE memories_fts MATCH ?1
                     ORDER BY rank
                     LIMIT ?2",
                    cols = MEMORY_SELECT_COLS_QUALIFIED
                ),
                vec![
                    Box::new(query.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(limit),
                ],
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let memory = row_to_memory(row)?;
            let rank: f64 = row.get(16)?; // rank is after the 16 standard columns
            Ok((memory, rank))
        })?;
        rows.collect()
    }

    /// List distinct dates that have daily_log entries (for calendar display).
    pub fn list_memory_dates(
        &self,
        identity_id: Option<&str>,
    ) -> Result<Vec<String>, rusqlite::Error> {
        let conn = self.conn();
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match identity_id {
            Some(id) => (
                "SELECT DISTINCT log_date FROM memories
                 WHERE memory_type = 'daily_log' AND log_date IS NOT NULL AND identity_id = ?1
                 ORDER BY log_date DESC".to_string(),
                vec![Box::new(id.to_string())],
            ),
            None => (
                "SELECT DISTINCT log_date FROM memories
                 WHERE memory_type = 'daily_log' AND log_date IS NOT NULL
                 ORDER BY log_date DESC".to_string(),
                vec![],
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    /// List distinct identity_ids in memories (for filter dropdowns).
    pub fn list_memory_identities(&self) -> Result<Vec<String>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT identity_id FROM memories WHERE identity_id IS NOT NULL ORDER BY identity_id"
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    // ====================================================================
    // Duplicate detection & merge
    // ====================================================================

    /// Find similar memories using FTS5.
    /// Extracts the first few significant words from content and searches for matches.
    /// Returns (MemoryRow, BM25 rank) pairs sorted by relevance.
    pub fn find_similar_memories_fts(
        &self,
        content: &str,
        memory_type: Option<&str>,
        identity_id: Option<&str>,
        limit: i32,
    ) -> Result<Vec<(MemoryRow, f64)>, rusqlite::Error> {
        // Extract first few significant words for the FTS query
        let words: Vec<&str> = content
            .split_whitespace()
            .filter(|w| w.len() > 2) // skip short words
            .take(8)
            .collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        // Quote each token to prevent FTS5 operator interpretation
        let fts_query: String = words
            .iter()
            .map(|w| {
                let escaped = w.replace('"', "\"\"");
                format!("\"{}\"", escaped)
            })
            .collect::<Vec<_>>()
            .join(" ");

        let conn = self.conn();
        let mut conditions = vec!["memories_fts MATCH ?1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(fts_query)];
        let mut idx = 2;

        if let Some(mt) = memory_type {
            conditions.push(format!("memories.memory_type = ?{}", idx));
            params.push(Box::new(mt.to_string()));
            idx += 1;
        }
        if let Some(iid) = identity_id {
            conditions.push(format!("memories.identity_id = ?{}", idx));
            params.push(Box::new(iid.to_string()));
            idx += 1;
        }
        conditions.push(format!("memories.superseded_by IS NULL"));

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT {cols}, bm25(memories_fts) as rank
             FROM memories
             JOIN memories_fts ON memories.id = memories_fts.rowid
             WHERE {where_clause}
             ORDER BY rank
             LIMIT ?{idx}",
            cols = MEMORY_SELECT_COLS_QUALIFIED,
            where_clause = where_clause,
            idx = idx,
        );
        params.push(Box::new(limit));

        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let memory = row_to_memory(row)?;
            let rank: f64 = row.get(16)?;
            Ok((memory, rank))
        })?;
        rows.collect()
    }

    /// Merge two memories into a new one.
    ///
    /// 1. Fetch both memories
    /// 2. Combine content per strategy
    /// 3. Union tags, take max importance
    /// 4. Insert new memory
    /// 5. Set `superseded_by` on both originals
    /// 6. Transfer associations to the new memory
    /// 7. Return new memory ID
    pub fn merge_memories(
        &self,
        id_a: i64,
        id_b: i64,
        strategy: &MergeStrategy,
    ) -> Result<i64, rusqlite::Error> {
        let mem_a = self.get_memory(id_a)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
        let mem_b = self.get_memory(id_b)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;

        // Determine merged content
        let merged_content = match strategy {
            MergeStrategy::Append => {
                format!("{}\n\n---\n\n{}", mem_a.content, mem_b.content)
            }
            MergeStrategy::ReplaceWithNewer => {
                // Newer = later created_at
                if mem_b.created_at >= mem_a.created_at {
                    mem_b.content.clone()
                } else {
                    mem_a.content.clone()
                }
            }
            MergeStrategy::Custom(text) => text.clone(),
        };

        // Union tags
        let merged_tags = match (&mem_a.tags, &mem_b.tags) {
            (Some(a), Some(b)) => {
                let mut all: Vec<&str> = a.split(',').chain(b.split(',')).collect();
                all.sort_unstable();
                all.dedup();
                Some(all.join(","))
            }
            (Some(a), None) => Some(a.clone()),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        };

        // Take max importance
        let importance = mem_a.importance.max(mem_b.importance);

        // Use the newer memory's metadata for type, identity, etc.
        let newer = if mem_b.created_at >= mem_a.created_at { &mem_b } else { &mem_a };

        // Insert new merged memory
        let new_id = self.insert_memory(
            &newer.memory_type,
            &merged_content,
            newer.category.as_deref(),
            merged_tags.as_deref(),
            importance,
            newer.identity_id.as_deref(),
            newer.session_id,
            newer.entity_type.as_deref(),
            newer.entity_name.as_deref(),
            Some("merge"),
            newer.log_date.as_deref(),
            newer.agent_subtype.as_deref(),
        )?;

        let conn = self.conn();

        // Mark both originals as superseded
        conn.execute(
            "UPDATE memories SET superseded_by = ?1 WHERE id IN (?2, ?3)",
            rusqlite::params![new_id, id_a, id_b],
        )?;

        // Transfer associations: point source/target from old IDs to new ID
        conn.execute(
            "UPDATE memory_associations SET source_memory_id = ?1 WHERE source_memory_id IN (?2, ?3)",
            rusqlite::params![new_id, id_a, id_b],
        )?;
        conn.execute(
            "UPDATE memory_associations SET target_memory_id = ?1 WHERE target_memory_id IN (?2, ?3)",
            rusqlite::params![new_id, id_a, id_b],
        )?;

        // Clean up any self-referencing associations created by the transfer
        conn.execute(
            "DELETE FROM memory_associations WHERE source_memory_id = target_memory_id",
            [],
        )?;

        Ok(new_id)
    }

    // ====================================================================
    // Filtered listing (for export)
    // ====================================================================

    /// List memories with optional filters for export.
    pub fn list_memories_filtered(
        &self,
        memory_type: Option<&str>,
        identity_id: Option<&str>,
        date_from: Option<&str>,
        date_to: Option<&str>,
    ) -> Result<Vec<MemoryRow>, rusqlite::Error> {
        let conn = self.conn();
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1;

        if let Some(mt) = memory_type {
            conditions.push(format!("memory_type = ?{}", idx));
            params.push(Box::new(mt.to_string()));
            idx += 1;
        }
        if let Some(iid) = identity_id {
            conditions.push(format!("identity_id = ?{}", idx));
            params.push(Box::new(iid.to_string()));
            idx += 1;
        }
        if let Some(df) = date_from {
            conditions.push(format!("created_at >= ?{}", idx));
            params.push(Box::new(df.to_string()));
            idx += 1;
        }
        if let Some(dt) = date_to {
            conditions.push(format!("created_at <= ?{}", idx));
            params.push(Box::new(dt.to_string()));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT {} FROM memories {} ORDER BY id",
            MEMORY_SELECT_COLS, where_clause
        );

        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| row_to_memory(row))?;
        rows.collect()
    }

    /// Get memory statistics aggregated from the DB.
    pub fn get_memory_stats(
        &self,
    ) -> Result<MemoryStats, rusqlite::Error> {
        let conn = self.conn();
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))?;
        let daily_log_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE memory_type = 'daily_log'", [], |r| r.get(0)
        )?;
        let long_term_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE memory_type = 'long_term'", [], |r| r.get(0)
        )?;
        let identities = self.list_memory_identities().unwrap_or_default();
        let date_range = conn.query_row(
            "SELECT MIN(log_date), MAX(log_date) FROM memories WHERE log_date IS NOT NULL",
            [],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
        ).unwrap_or((None, None));

        Ok(MemoryStats {
            total_memories: total,
            daily_log_count,
            long_term_count,
            identity_count: identities.len() as i64,
            identities,
            earliest_date: date_range.0,
            latest_date: date_range.1,
        })
    }
}

/// Aggregated memory statistics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryStats {
    pub total_memories: i64,
    pub daily_log_count: i64,
    pub long_term_count: i64,
    pub identity_count: i64,
    pub identities: Vec<String>,
    pub earliest_date: Option<String>,
    pub latest_date: Option<String>,
}

/// Strategy for merging two memories.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MergeStrategy {
    /// Join both contents with a separator
    Append,
    /// Keep only the newer memory's content
    ReplaceWithNewer,
    /// Use caller-provided content
    Custom(String),
}

#[cfg(test)]
mod tests {
    use crate::db::Database;

    fn setup_db() -> Database {
        Database::new(":memory:").expect("in-memory db")
    }

    #[test]
    fn test_fts_no_identity_filter_finds_null_identity_memories() {
        let db = setup_db();
        // Insert a memory with NULL identity (standard mode behavior)
        db.insert_memory(
            "daily_log", "I created an Excalidraw file with shapes and arrows",
            None, None, 5, None, None, None, None,
            Some("session_completion"), Some("2026-02-23"), None,
        ).unwrap();

        // Search with identity_id = None should find it
        let results = db.search_memories_fts("excalidraw", None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].0.content.contains("Excalidraw"));
    }

    #[test]
    fn test_fts_identity_filter_excludes_null_identity_memories() {
        let db = setup_db();
        // Insert a memory with NULL identity
        db.insert_memory(
            "daily_log", "I created an Excalidraw file with shapes",
            None, None, 5, None, None, None, None,
            Some("session_completion"), Some("2026-02-23"), None,
        ).unwrap();

        // Search with a specific identity_id should NOT find NULL-identity memories
        let results = db.search_memories_fts("excalidraw", Some("some-uuid"), 10).unwrap();
        assert_eq!(results.len(), 0, "identity-filtered search should not match NULL-identity memories");
    }

    #[test]
    fn test_fts_safe_mode_only_finds_safemode_memories() {
        let db = setup_db();
        // Insert a safemode memory
        db.insert_memory(
            "long_term", "The user prefers dark mode in the UI",
            None, None, 7, Some("safemode"), None, None, None,
            None, None, None,
        ).unwrap();
        // Insert a standard memory (NULL identity)
        db.insert_memory(
            "daily_log", "Discussed dark mode implementation details",
            None, None, 5, None, None, None, None,
            Some("session_completion"), Some("2026-02-23"), None,
        ).unwrap();

        // Safe mode search: only safemode identity
        let safe_results = db.search_memories_fts("dark mode", Some("safemode"), 10).unwrap();
        assert_eq!(safe_results.len(), 1);
        assert_eq!(safe_results[0].0.identity_id.as_deref(), Some("safemode"));

        // Standard mode search: no identity filter, finds ALL
        let all_results = db.search_memories_fts("dark mode", None, 10).unwrap();
        assert_eq!(all_results.len(), 2, "standard mode should find all memories regardless of identity");
    }

    #[test]
    fn test_fts_standard_mode_finds_all_identities() {
        let db = setup_db();
        // Insert memories with different identities
        db.insert_memory(
            "daily_log", "Built a trading bot for DeFi",
            None, None, 5, Some("uuid-web-user"), None, None, None,
            None, None, None,
        ).unwrap();
        db.insert_memory(
            "daily_log", "Deployed the trading bot to production",
            None, None, 5, Some("uuid-discord-user"), None, None, None,
            None, None, None,
        ).unwrap();
        db.insert_memory(
            "daily_log", "Trading bot performance metrics look good",
            None, None, 5, None, None, None, None, // NULL identity
            None, None, None,
        ).unwrap();

        // Standard mode (None identity filter) should find all 3
        let results = db.search_memories_fts("trading bot", None, 10).unwrap();
        assert_eq!(results.len(), 3, "standard mode should find memories from all identities including NULL");
    }

    #[test]
    fn test_fts_prefix_matching_finds_plural_and_singular() {
        let db = setup_db();
        // Insert a memory with "excalidraw" (singular)
        db.insert_memory(
            "daily_log", "I created an Excalidraw file with shapes and arrows",
            None, None, 5, None, None, None, None,
            None, None, None,
        ).unwrap();

        // Exact match works
        let results = db.search_memories_fts("excalidraw", None, 10).unwrap();
        assert_eq!(results.len(), 1, "exact match should work");

        // Plural (excalidraws) does NOT match singular without prefix
        let results = db.search_memories_fts("excalidraws", None, 10).unwrap();
        assert_eq!(results.len(), 0, "plural should not match singular without prefix");

        // Prefix match (excalidraw*) matches the singular form
        let results = db.search_memories_fts("excalidraw*", None, 10).unwrap();
        assert_eq!(results.len(), 1, "prefix match should find singular");

        // OR query with prefix matching (simulating the fixed query builder)
        let results = db.search_memories_fts("excalidraws* OR shapes*", None, 10).unwrap();
        assert_eq!(results.len(), 1, "OR with prefix should find the memory via shapes*");
    }
}
