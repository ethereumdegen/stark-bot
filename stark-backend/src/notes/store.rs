//! NoteStore — FTS5-indexed note storage
//!
//! Manages a separate `.notes.db` SQLite database with an FTS5 virtual table
//! indexing file_path, title, tags, and content for full-text search.

use super::{file_ops, frontmatter};
use crate::disk_quota::DiskQuotaManager;
use rusqlite::{params, Connection, Result as SqliteResult};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Search result from the note store
#[derive(Debug, Clone)]
pub struct NoteSearchResult {
    pub file_path: String,
    pub title: String,
    pub tags: String,
    pub snippet: String,
    pub score: f64,
}

/// NoteStore wrapping SQLite FTS5 for markdown note indexing
pub struct NoteStore {
    notes_dir: PathBuf,
    conn: Mutex<Connection>,
    disk_quota: Mutex<Option<Arc<DiskQuotaManager>>>,
}

impl NoteStore {
    /// Create a new note store, initializing the FTS5 table and performing initial reindex
    pub fn new(notes_dir: PathBuf, db_path: &str) -> SqliteResult<Self> {
        std::fs::create_dir_all(&notes_dir).ok();

        let conn = Connection::open(db_path)?;

        // Create FTS5 table with richer columns than memory
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
                file_path,
                title,
                tags,
                content,
                tokenize='porter'
            )",
            [],
        )?;

        let store = Self {
            notes_dir,
            conn: Mutex::new(conn),
            disk_quota: Mutex::new(None),
        };

        store.reindex()?;

        Ok(store)
    }

    /// Set the disk quota manager
    pub fn set_disk_quota(&self, dq: Arc<DiskQuotaManager>) {
        if let Ok(mut guard) = self.disk_quota.lock() {
            *guard = Some(dq);
        }
    }

    /// Get the notes directory path
    pub fn notes_dir(&self) -> &PathBuf {
        &self.notes_dir
    }

    /// Reindex all markdown files in the notes directory
    pub fn reindex(&self) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();

        conn.execute("DELETE FROM notes_fts", [])?;

        let files = file_ops::list_notes(&self.notes_dir).unwrap_or_default();

        let mut count = 0;
        for file_path in files {
            if let Ok(content) = file_ops::read_note(&file_path) {
                if content.is_empty() {
                    continue;
                }
                if let Some(rel_path) = file_ops::relative_path(&self.notes_dir, &file_path) {
                    let parsed = frontmatter::parse_note(&content);
                    let title = if parsed.frontmatter.title.is_empty() {
                        // Fall back to filename without extension
                        file_path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default()
                    } else {
                        parsed.frontmatter.title.clone()
                    };
                    let tags = parsed.all_tags.join(", ");

                    conn.execute(
                        "INSERT INTO notes_fts (file_path, title, tags, content) VALUES (?1, ?2, ?3, ?4)",
                        params![rel_path, title, tags, parsed.body],
                    )?;
                    count += 1;
                }
            }
        }

        log::info!("[NOTES] Indexed {} note files", count);
        Ok(count)
    }

    /// Full-text search across notes
    pub fn search(&self, query: &str, limit: i32) -> SqliteResult<Vec<NoteSearchResult>> {
        let conn = self.conn.lock().unwrap();

        let escaped_query = escape_fts5_query(query);
        if escaped_query.is_empty() {
            return Ok(vec![]);
        }

        let mut stmt = conn.prepare(
            "SELECT file_path, title, tags,
                    snippet(notes_fts, 3, '>>>', '<<<', '...', 64) as snippet,
                    bm25(notes_fts) as score
             FROM notes_fts
             WHERE notes_fts MATCH ?1
             ORDER BY score
             LIMIT ?2",
        )?;

        let results = stmt
            .query_map(params![escaped_query, limit], |row| {
                Ok(NoteSearchResult {
                    file_path: row.get(0)?,
                    title: row.get(1)?,
                    tags: row.get(2)?,
                    snippet: row.get(3)?,
                    score: row.get(4)?,
                })
            })?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(results)
    }

    /// Search notes by tag
    pub fn search_by_tag(&self, tag: &str, limit: i32) -> SqliteResult<Vec<NoteSearchResult>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT file_path, title, tags,
                    snippet(notes_fts, 3, '>>>', '<<<', '...', 32) as snippet,
                    bm25(notes_fts) as score
             FROM notes_fts
             WHERE tags MATCH ?1
             ORDER BY score
             LIMIT ?2",
        )?;

        let escaped = escape_fts5_query(tag);
        let results = stmt
            .query_map(params![escaped, limit], |row| {
                Ok(NoteSearchResult {
                    file_path: row.get(0)?,
                    title: row.get(1)?,
                    tags: row.get(2)?,
                    snippet: row.get(3)?,
                    score: row.get(4)?,
                })
            })?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(results)
    }

    /// List all unique tags across all notes
    pub fn list_tags(&self) -> SqliteResult<Vec<(String, usize)>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare("SELECT tags FROM notes_fts WHERE tags != ''")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

        let mut tag_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for row in rows {
            if let Ok(tags_str) = row {
                for tag in tags_str.split(',') {
                    let tag = tag.trim().to_lowercase();
                    if !tag.is_empty() {
                        *tag_counts.entry(tag).or_insert(0) += 1;
                    }
                }
            }
        }

        let mut tags: Vec<(String, usize)> = tag_counts.into_iter().collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Ok(tags)
    }

    /// Create a new note with auto-generated frontmatter
    pub fn create_note(
        &self,
        title: &str,
        content: &str,
        tags: &[String],
        aliases: &[String],
        note_type: &str,
        subdir: Option<&str>,
    ) -> Result<String, String> {
        // Check disk quota
        let estimated_size = title.len() + content.len() + 200; // rough frontmatter overhead
        if let Ok(guard) = self.disk_quota.lock() {
            if let Some(ref dq) = *guard {
                if let Err(e) = dq.check_quota(estimated_size as u64) {
                    return Err(e.to_string());
                }
            }
        }

        let slug = file_ops::slugify(title);
        let filename = format!("{}.md", slug);

        let rel_path = match subdir {
            Some(dir) => format!("{}/{}", dir, filename),
            None => filename,
        };

        let full_path = self.notes_dir.join(&rel_path);

        // Don't overwrite existing notes
        if full_path.exists() {
            return Err(format!("Note already exists: {}", rel_path));
        }

        let fm = frontmatter::generate_frontmatter(title, tags, aliases, note_type, None);
        let full_content = format!("{}\n\n# {}\n\n{}\n", fm, title, content);

        file_ops::write_note(&full_path, &full_content)
            .map_err(|e| format!("Failed to write note: {}", e))?;

        // Record write with disk quota
        if let Ok(guard) = self.disk_quota.lock() {
            if let Some(ref dq) = *guard {
                dq.record_write(full_content.len() as u64);
            }
        }

        // Update index
        self.index_file(&full_path).ok();

        Ok(rel_path)
    }

    /// Edit an existing note (replaces body, updates frontmatter timestamp)
    pub fn edit_note(&self, rel_path: &str, new_content: &str) -> Result<(), String> {
        let full_path = self.notes_dir.join(rel_path);
        if !full_path.exists() {
            return Err(format!("Note not found: {}", rel_path));
        }

        // Check disk quota
        if let Ok(guard) = self.disk_quota.lock() {
            if let Some(ref dq) = *guard {
                if let Err(e) = dq.check_quota(new_content.len() as u64) {
                    return Err(e.to_string());
                }
            }
        }

        let existing = file_ops::read_note(&full_path)
            .map_err(|e| format!("Failed to read note: {}", e))?;

        let parsed = frontmatter::parse_note(&existing);

        // Rebuild with existing frontmatter but updated timestamp and new body
        let fm = frontmatter::generate_frontmatter(
            &parsed.frontmatter.title,
            &parsed.frontmatter.tags,
            &parsed.frontmatter.aliases,
            &parsed.frontmatter.note_type,
            parsed.frontmatter.date.as_deref(),
        );
        let updated_fm = frontmatter::touch_updated(&fm);

        let full = format!("{}\n\n{}\n", updated_fm, new_content.trim());

        file_ops::write_note(&full_path, &full)
            .map_err(|e| format!("Failed to write note: {}", e))?;

        if let Ok(guard) = self.disk_quota.lock() {
            if let Some(ref dq) = *guard {
                dq.record_write(full.len() as u64);
            }
        }

        self.index_file(&full_path).ok();

        Ok(())
    }

    /// Read a note's content
    pub fn read_note(&self, rel_path: &str) -> Result<String, String> {
        let full_path = self.notes_dir.join(rel_path);
        file_ops::read_note(&full_path).map_err(|e| format!("Failed to read note: {}", e))
    }

    /// List all note files (relative paths)
    pub fn list_files(&self) -> std::io::Result<Vec<String>> {
        let files = file_ops::list_notes(&self.notes_dir)?;
        Ok(files
            .into_iter()
            .filter_map(|p| file_ops::relative_path(&self.notes_dir, &p))
            .collect())
    }

    /// Index or update a single file in the FTS index
    fn index_file(&self, file_path: &PathBuf) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();

        if let Some(rel_path) = file_ops::relative_path(&self.notes_dir, file_path) {
            if let Ok(content) = file_ops::read_note(file_path) {
                let parsed = frontmatter::parse_note(&content);
                let title = if parsed.frontmatter.title.is_empty() {
                    file_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                } else {
                    parsed.frontmatter.title.clone()
                };
                let tags = parsed.all_tags.join(", ");

                conn.execute(
                    "DELETE FROM notes_fts WHERE file_path = ?1",
                    params![rel_path],
                )?;
                conn.execute(
                    "INSERT INTO notes_fts (file_path, title, tags, content) VALUES (?1, ?2, ?3, ?4)",
                    params![rel_path, title, tags, parsed.body],
                )?;
            }
        }

        Ok(())
    }
}

/// Escape special characters for FTS5 query
fn escape_fts5_query(query: &str) -> String {
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }

    let escaped: Vec<String> = words
        .iter()
        .map(|word| {
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

    escaped.join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_note_store_create_and_search() {
        let dir = tempdir().unwrap();
        let notes_dir = dir.path().join("notes");
        let db_path = dir.path().join("test.db");

        let store =
            NoteStore::new(notes_dir.clone(), db_path.to_str().unwrap()).expect("Failed to create store");

        let path = store
            .create_note(
                "x402 Payment Protocol",
                "This note is about the x402 payment protocol for crypto.",
                &["crypto".to_string(), "payments".to_string()],
                &["x402".to_string()],
                "note",
                None,
            )
            .expect("Failed to create note");

        assert_eq!(path, "x402-payment-protocol.md");

        // File should exist
        assert!(notes_dir.join("x402-payment-protocol.md").exists());

        // Search should find it
        let results = store.search("payment protocol", 10).expect("Failed to search");
        assert!(!results.is_empty());
        assert!(results[0].title.contains("x402"));
    }

    #[test]
    fn test_note_store_create_in_subdir() {
        let dir = tempdir().unwrap();
        let notes_dir = dir.path().join("notes");
        let db_path = dir.path().join("test.db");

        let store =
            NoteStore::new(notes_dir.clone(), db_path.to_str().unwrap()).expect("Failed to create store");

        let path = store
            .create_note("My Idea", "A great idea.", &[], &[], "idea", Some("ideas"))
            .expect("Failed to create note");

        assert_eq!(path, "ideas/my-idea.md");
        assert!(notes_dir.join("ideas/my-idea.md").exists());
    }

    #[test]
    fn test_note_store_edit() {
        let dir = tempdir().unwrap();
        let notes_dir = dir.path().join("notes");
        let db_path = dir.path().join("test.db");

        let store =
            NoteStore::new(notes_dir.clone(), db_path.to_str().unwrap()).expect("Failed to create store");

        let path = store
            .create_note("Editable Note", "Original content.", &[], &[], "note", None)
            .expect("Failed to create note");

        store
            .edit_note(&path, "# Editable Note\n\nUpdated content with more detail.")
            .expect("Failed to edit note");

        let content = store.read_note(&path).expect("Failed to read note");
        assert!(content.contains("Updated content"));
    }

    #[test]
    fn test_note_store_tags() {
        let dir = tempdir().unwrap();
        let notes_dir = dir.path().join("notes");
        let db_path = dir.path().join("test.db");

        let store =
            NoteStore::new(notes_dir.clone(), db_path.to_str().unwrap()).expect("Failed to create store");

        store
            .create_note("Note A", "Content A", &["rust".to_string(), "web3".to_string()], &[], "note", None)
            .expect("Failed to create note");
        store
            .create_note("Note B", "Content B", &["rust".to_string()], &[], "note", None)
            .expect("Failed to create note");

        let tags = store.list_tags().expect("Failed to list tags");
        assert!(tags.iter().any(|(t, count)| t == "rust" && *count == 2));
        assert!(tags.iter().any(|(t, count)| t == "web3" && *count == 1));
    }

    #[test]
    fn test_note_store_duplicate_prevention() {
        let dir = tempdir().unwrap();
        let notes_dir = dir.path().join("notes");
        let db_path = dir.path().join("test.db");

        let store =
            NoteStore::new(notes_dir.clone(), db_path.to_str().unwrap()).expect("Failed to create store");

        store
            .create_note("Unique Note", "Content", &[], &[], "note", None)
            .expect("Failed to create note");

        let result = store.create_note("Unique Note", "Different content", &[], &[], "note", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }
}
