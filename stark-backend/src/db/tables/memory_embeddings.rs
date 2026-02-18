//! Database operations for memory_embeddings table
//! Stores vector embeddings for memories (cosine similarity search)

use crate::db::Database;

impl Database {
    /// Upsert an embedding for a memory
    pub fn upsert_memory_embedding(
        &self,
        memory_id: i64,
        embedding: &[f32],
        model: &str,
        dimensions: i32,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        let blob = embedding_to_blob(embedding);
        conn.execute(
            "INSERT INTO memory_embeddings (memory_id, embedding, model, dimensions, created_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))
             ON CONFLICT(memory_id) DO UPDATE SET
                embedding = excluded.embedding,
                model = excluded.model,
                dimensions = excluded.dimensions,
                updated_at = datetime('now')",
            rusqlite::params![memory_id, blob, model, dimensions],
        )?;
        Ok(())
    }

    /// Get embedding for a specific memory
    pub fn get_memory_embedding(&self, memory_id: i64) -> Result<Option<(Vec<f32>, String, i32)>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT embedding, model, dimensions FROM memory_embeddings WHERE memory_id = ?1"
        )?;
        let result = stmt.query_row(rusqlite::params![memory_id], |row| {
            let blob: Vec<u8> = row.get(0)?;
            let model: String = row.get(1)?;
            let dimensions: i32 = row.get(2)?;
            Ok((blob_to_embedding(&blob), model, dimensions))
        });
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get all embeddings (for brute-force vector search)
    pub fn list_memory_embeddings(&self) -> Result<Vec<(i64, Vec<f32>)>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT memory_id, embedding FROM memory_embeddings"
        )?;
        let rows = stmt.query_map([], |row| {
            let memory_id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((memory_id, blob_to_embedding(&blob)))
        })?;
        rows.collect()
    }

    /// Delete embedding for a memory
    pub fn delete_memory_embedding(&self, memory_id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let count = conn.execute(
            "DELETE FROM memory_embeddings WHERE memory_id = ?1",
            rusqlite::params![memory_id],
        )?;
        Ok(count > 0)
    }

    /// Count total embeddings
    pub fn count_memory_embeddings(&self) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.query_row(
            "SELECT COUNT(*) FROM memory_embeddings",
            [],
            |row| row.get(0),
        )
    }

    /// List memory IDs that have no embedding yet
    pub fn list_memories_without_embeddings(&self, limit: i32) -> Result<Vec<i64>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT m.id FROM memories m
             LEFT JOIN memory_embeddings me ON m.id = me.memory_id
             WHERE me.memory_id IS NULL
             ORDER BY m.created_at DESC
             LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit], |row| row.get(0))?;
        rows.collect()
    }
}

/// Convert f32 slice to bytes for SQLite BLOB storage (little-endian)
fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Convert bytes from SQLite BLOB back to f32 vector
fn blob_to_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}
