//! Memory store - FTS5-indexed markdown file storage
//!
//! The MemoryStore manages:
//! - Reading/writing markdown memory files
//! - FTS5 full-text search indexing
//! - Reindexing when files change

use super::file_ops;
use crate::disk_quota::DiskQuotaManager;
use chrono::{Local, NaiveDate};
use rusqlite::{params, Connection, Result as SqliteResult};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Search result from the memory store
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Relative file path (e.g., "MEMORY.md" or "user123/2024-01-15.md")
    pub file_path: String,
    /// Matching text snippet
    pub snippet: String,
    /// BM25 relevance score (lower is better in FTS5)
    pub score: f64,
}

/// Memory store wrapping SQLite FTS5 for markdown file indexing
pub struct MemoryStore {
    /// Path to the memory directory
    memory_dir: PathBuf,
    /// SQLite connection for FTS5 index
    conn: Mutex<Connection>,
    /// Optional disk quota manager for enforcing limits
    disk_quota: Mutex<Option<Arc<DiskQuotaManager>>>,
}

impl MemoryStore {
    /// Create a new memory store
    pub fn new(memory_dir: PathBuf, db_path: &str) -> SqliteResult<Self> {
        // Ensure memory directory exists
        std::fs::create_dir_all(&memory_dir).ok();

        // Open or create SQLite database
        let conn = Connection::open(db_path)?;

        // Create FTS5 table for indexing
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS qmd_memory_fts USING fts5(
                file_path,
                content,
                tokenize='porter'
            )",
            [],
        )?;

        let store = Self {
            memory_dir,
            conn: Mutex::new(conn),
            disk_quota: Mutex::new(None),
        };

        // Initial reindex
        store.reindex()?;

        Ok(store)
    }

    /// Create memory store using an existing database connection
    pub fn with_connection(memory_dir: PathBuf, conn: Connection) -> SqliteResult<Self> {
        std::fs::create_dir_all(&memory_dir).ok();

        // Create FTS5 table if not exists
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS qmd_memory_fts USING fts5(
                file_path,
                content,
                tokenize='porter'
            )",
            [],
        )?;

        let store = Self {
            memory_dir,
            conn: Mutex::new(conn),
            disk_quota: Mutex::new(None),
        };

        store.reindex()?;
        Ok(store)
    }

    /// Set the disk quota manager for enforcing memory write limits
    pub fn set_disk_quota(&self, dq: Arc<DiskQuotaManager>) {
        if let Ok(mut guard) = self.disk_quota.lock() {
            *guard = Some(dq);
        }
    }

    /// Get the memory directory path
    pub fn memory_dir(&self) -> &PathBuf {
        &self.memory_dir
    }

    /// Reindex all markdown files in the memory directory
    pub fn reindex(&self) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();

        // Clear existing index
        conn.execute("DELETE FROM qmd_memory_fts", [])?;

        // List all markdown files
        let files = file_ops::list_memory_files(&self.memory_dir).unwrap_or_default();

        let mut count = 0;
        for file_path in files {
            if let Ok(content) = file_ops::read_file(&file_path) {
                if let Some(rel_path) = file_ops::relative_path(&self.memory_dir, &file_path) {
                    conn.execute(
                        "INSERT INTO qmd_memory_fts (file_path, content) VALUES (?1, ?2)",
                        params![rel_path, content],
                    )?;
                    count += 1;
                }
            }
        }

        log::info!("[QMD_MEMORY] Indexed {} memory files", count);
        Ok(count)
    }

    /// Search memories using BM25 full-text search
    pub fn search(&self, query: &str, limit: i32) -> SqliteResult<Vec<SearchResult>> {
        let conn = self.conn.lock().unwrap();

        // Escape and prepare query for FTS5
        let escaped_query = escape_fts5_query(query);

        let mut stmt = conn.prepare(
            "SELECT file_path, snippet(qmd_memory_fts, 1, '>>>', '<<<', '...', 64) as snippet, bm25(qmd_memory_fts) as score
             FROM qmd_memory_fts
             WHERE qmd_memory_fts MATCH ?1
             ORDER BY score
             LIMIT ?2"
        )?;

        let results = stmt
            .query_map(params![escaped_query, limit], |row| {
                Ok(SearchResult {
                    file_path: row.get(0)?,
                    snippet: row.get(1)?,
                    score: row.get(2)?,
                })
            })?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(results)
    }

    /// Get content of a specific memory file
    pub fn get_file(&self, relative_path: &str) -> std::io::Result<String> {
        let full_path = self.memory_dir.join(relative_path);
        file_ops::read_file(&full_path)
    }

    /// Append to today's daily log
    pub fn append_daily_log(
        &self,
        content: &str,
        identity_id: Option<&str>,
    ) -> std::io::Result<()> {
        // Per-append size cap (100KB)
        if content.len() > crate::disk_quota::MAX_MEMORY_APPEND_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Memory append rejected: content size ({} bytes) exceeds the per-append limit of 100KB.",
                    content.len()
                ),
            ));
        }

        // Check disk quota
        if let Ok(guard) = self.disk_quota.lock() {
            if let Some(ref dq) = *guard {
                if let Err(e) = dq.check_quota(content.len() as u64) {
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
                }
            }
        }

        let today = Local::now().date_naive();
        let path = file_ops::daily_log_path(&self.memory_dir, today, identity_id);

        // Ensure directory exists
        file_ops::ensure_memory_dirs(&self.memory_dir, identity_id)?;

        // Append with timestamp
        file_ops::append_to_file(&path, content)?;

        // Record write with disk quota
        if let Ok(guard) = self.disk_quota.lock() {
            if let Some(ref dq) = *guard {
                dq.record_write(content.len() as u64);
            }
        }

        // Update index for this file
        self.index_file(&path).ok();

        Ok(())
    }

    /// Append to the long-term memory file (MEMORY.md)
    pub fn append_long_term(
        &self,
        content: &str,
        identity_id: Option<&str>,
    ) -> std::io::Result<()> {
        // Per-append size cap (100KB)
        if content.len() > crate::disk_quota::MAX_MEMORY_APPEND_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Memory append rejected: content size ({} bytes) exceeds the per-append limit of 100KB.",
                    content.len()
                ),
            ));
        }

        // Check disk quota
        if let Ok(guard) = self.disk_quota.lock() {
            if let Some(ref dq) = *guard {
                if let Err(e) = dq.check_quota(content.len() as u64) {
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
                }
            }
        }

        let path = file_ops::long_term_path(&self.memory_dir, identity_id);

        // Ensure directory exists
        file_ops::ensure_memory_dirs(&self.memory_dir, identity_id)?;

        // Append with timestamp
        file_ops::append_to_file(&path, content)?;

        // Record write with disk quota
        if let Ok(guard) = self.disk_quota.lock() {
            if let Some(ref dq) = *guard {
                dq.record_write(content.len() as u64);
            }
        }

        // Update index for this file
        self.index_file(&path).ok();

        Ok(())
    }

    /// Get today's daily log content
    pub fn get_daily_log(&self, identity_id: Option<&str>) -> std::io::Result<String> {
        let today = Local::now().date_naive();
        let path = file_ops::daily_log_path(&self.memory_dir, today, identity_id);
        file_ops::read_file(&path)
    }

    /// Get long-term memory content
    pub fn get_long_term(&self, identity_id: Option<&str>) -> std::io::Result<String> {
        let path = file_ops::long_term_path(&self.memory_dir, identity_id);
        file_ops::read_file(&path)
    }

    /// Get daily log for a specific date
    pub fn get_daily_log_for_date(
        &self,
        date: NaiveDate,
        identity_id: Option<&str>,
    ) -> std::io::Result<String> {
        let path = file_ops::daily_log_path(&self.memory_dir, date, identity_id);
        file_ops::read_file(&path)
    }

    /// List all memory files
    pub fn list_files(&self) -> std::io::Result<Vec<String>> {
        let files = file_ops::list_memory_files(&self.memory_dir)?;
        Ok(files
            .into_iter()
            .filter_map(|p| file_ops::relative_path(&self.memory_dir, &p))
            .collect())
    }

    /// Index or update a single file in the FTS index
    fn index_file(&self, file_path: &PathBuf) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();

        if let Some(rel_path) = file_ops::relative_path(&self.memory_dir, file_path) {
            if let Ok(content) = file_ops::read_file(file_path) {
                // Delete existing entry
                conn.execute(
                    "DELETE FROM qmd_memory_fts WHERE file_path = ?1",
                    params![rel_path],
                )?;

                // Insert updated content
                conn.execute(
                    "INSERT INTO qmd_memory_fts (file_path, content) VALUES (?1, ?2)",
                    params![rel_path, content],
                )?;
            }
        }

        Ok(())
    }
}

/// Escape special characters for FTS5 query
fn escape_fts5_query(query: &str) -> String {
    // Split into words and join with OR for multi-word queries
    let words: Vec<&str> = query.split_whitespace().collect();

    if words.is_empty() {
        return String::new();
    }

    // Escape each word (wrap in quotes if it contains special chars)
    let escaped: Vec<String> = words
        .iter()
        .map(|word| {
            // If word contains FTS5 special characters, quote it
            if word
                .chars()
                .any(|c| matches!(c, '"' | '*' | ':' | '^' | '(' | ')' | '+' | '-'))
            {
                format!("\"{}\"", word.replace('"', "\"\""))
            } else {
                word.to_string()
            }
        })
        .collect();

    // Join with OR for broader matching
    escaped.join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_escape_fts5_query() {
        assert_eq!(escape_fts5_query("hello"), "hello");
        assert_eq!(escape_fts5_query("hello world"), "hello OR world");
        assert_eq!(escape_fts5_query("user:test"), "\"user:test\"");
    }

    #[test]
    fn test_memory_store_basic() {
        let dir = tempdir().unwrap();
        let mem_dir = dir.path().join("memory");
        let db_path = dir.path().join("test.db");

        let store =
            MemoryStore::new(mem_dir.clone(), db_path.to_str().unwrap()).expect("Failed to create store");

        // Append some content
        store
            .append_long_term("User prefers dark mode", None)
            .expect("Failed to append");

        // Read it back
        let content = store.get_long_term(None).expect("Failed to read");
        assert!(content.contains("dark mode"));

        // Search for it
        let results = store.search("dark mode", 10).expect("Failed to search");
        assert!(!results.is_empty());
        assert!(results[0].file_path.contains("MEMORY.md"));
    }

    #[test]
    fn test_daily_log() {
        let dir = tempdir().unwrap();
        let mem_dir = dir.path().join("memory");
        let db_path = dir.path().join("test.db");

        let store =
            MemoryStore::new(mem_dir.clone(), db_path.to_str().unwrap()).expect("Failed to create store");

        store
            .append_daily_log("Worked on feature X", None)
            .expect("Failed to append");

        let content = store.get_daily_log(None).expect("Failed to read");
        assert!(content.contains("feature X"));
    }

    #[test]
    fn test_identity_isolation() {
        let dir = tempdir().unwrap();
        let mem_dir = dir.path().join("memory");
        let db_path = dir.path().join("test.db");

        let store =
            MemoryStore::new(mem_dir.clone(), db_path.to_str().unwrap()).expect("Failed to create store");

        // Write for user1
        store
            .append_long_term("User1 likes coffee", Some("user1"))
            .expect("Failed to append");

        // Write for user2
        store
            .append_long_term("User2 likes tea", Some("user2"))
            .expect("Failed to append");

        // Each user should have their own MEMORY.md
        let user1_mem = store.get_long_term(Some("user1")).expect("Failed to read");
        let user2_mem = store.get_long_term(Some("user2")).expect("Failed to read");

        assert!(user1_mem.contains("coffee"));
        assert!(!user1_mem.contains("tea"));
        assert!(user2_mem.contains("tea"));
        assert!(!user2_mem.contains("coffee"));
    }
}
