//! Database operations for memory_associations table
//! Manages typed connections between memories (knowledge graph)

use crate::db::Database;
use serde::{Deserialize, Serialize};

/// Association record from database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAssociationRow {
    pub id: i64,
    pub source_memory_id: i64,
    pub target_memory_id: i64,
    pub association_type: String,
    pub strength: f64,
    pub metadata: Option<String>,
    pub created_at: String,
}

/// Graph statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGraphStats {
    pub total_associations: i64,
    pub unique_memories: i64,
    pub avg_strength: f64,
    pub type_counts: std::collections::HashMap<String, i64>,
}

impl Database {
    /// Create a new association between two memories
    pub fn create_memory_association(
        &self,
        source_memory_id: i64,
        target_memory_id: i64,
        association_type: &str,
        strength: f64,
        metadata: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO memory_associations (source_memory_id, target_memory_id, association_type, strength, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            rusqlite::params![source_memory_id, target_memory_id, association_type, strength, metadata],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get associations for a memory (both directions)
    pub fn get_memory_associations(&self, memory_id: i64) -> Result<Vec<MemoryAssociationRow>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, source_memory_id, target_memory_id, association_type, strength, metadata, created_at
             FROM memory_associations
             WHERE source_memory_id = ?1 OR target_memory_id = ?1
             ORDER BY strength DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![memory_id], |row| {
            Ok(MemoryAssociationRow {
                id: row.get(0)?,
                source_memory_id: row.get(1)?,
                target_memory_id: row.get(2)?,
                association_type: row.get(3)?,
                strength: row.get(4)?,
                metadata: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Get associations of a specific type for a memory
    pub fn get_memory_associations_by_type(
        &self,
        memory_id: i64,
        association_type: &str,
    ) -> Result<Vec<MemoryAssociationRow>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, source_memory_id, target_memory_id, association_type, strength, metadata, created_at
             FROM memory_associations
             WHERE (source_memory_id = ?1 OR target_memory_id = ?1) AND association_type = ?2
             ORDER BY strength DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![memory_id, association_type], |row| {
            Ok(MemoryAssociationRow {
                id: row.get(0)?,
                source_memory_id: row.get(1)?,
                target_memory_id: row.get(2)?,
                association_type: row.get(3)?,
                strength: row.get(4)?,
                metadata: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Delete an association
    pub fn delete_memory_association(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let count = conn.execute(
            "DELETE FROM memory_associations WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(count > 0)
    }

    /// Count associations for a memory
    pub fn count_memory_associations(&self, memory_id: i64) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.query_row(
            "SELECT COUNT(*) FROM memory_associations
             WHERE source_memory_id = ?1 OR target_memory_id = ?1",
            rusqlite::params![memory_id],
            |row| row.get(0),
        )
    }

    /// Get graph statistics
    pub fn get_memory_graph_stats(&self) -> Result<MemoryGraphStats, rusqlite::Error> {
        let conn = self.conn();
        let total_associations: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_associations",
            [],
            |row| row.get(0),
        )?;

        let unique_memories: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT memory_id) FROM (
                SELECT source_memory_id AS memory_id FROM memory_associations
                UNION
                SELECT target_memory_id AS memory_id FROM memory_associations
            )",
            [],
            |row| row.get(0),
        )?;

        let avg_strength: f64 = conn.query_row(
            "SELECT COALESCE(AVG(strength), 0.0) FROM memory_associations",
            [],
            |row| row.get(0),
        )?;

        // Count associations by type
        let mut type_counts = std::collections::HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT association_type, COUNT(*) FROM memory_associations GROUP BY association_type",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            if let Ok((atype, count)) = row {
                type_counts.insert(atype, count);
            }
        }

        Ok(MemoryGraphStats {
            total_associations,
            unique_memories,
            avg_strength,
            type_counts,
        })
    }

    /// Get memories ranked by association count (for graph search component of hybrid search)
    pub fn get_memories_by_association_count(&self, limit: i32) -> Result<Vec<(i64, i64)>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT memory_id, COUNT(*) as cnt FROM (
                SELECT source_memory_id AS memory_id FROM memory_associations
                UNION ALL
                SELECT target_memory_id AS memory_id FROM memory_associations
            ) GROUP BY memory_id ORDER BY cnt DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect()
    }

    /// Check if an association already exists between two memories
    pub fn association_exists(
        &self,
        source_id: i64,
        target_id: i64,
        association_type: &str,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_associations
             WHERE ((source_memory_id = ?1 AND target_memory_id = ?2)
                OR (source_memory_id = ?2 AND target_memory_id = ?1))
             AND association_type = ?3",
            rusqlite::params![source_id, target_id, association_type],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}
