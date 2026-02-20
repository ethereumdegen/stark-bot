//! Database operations for skill_associations table
//! Manages typed connections between skills (skill knowledge graph)

use crate::db::Database;
use serde::{Deserialize, Serialize};

/// Association record from database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAssociationRow {
    pub id: i64,
    pub source_skill_id: i64,
    pub target_skill_id: i64,
    pub association_type: String,
    pub strength: f64,
    pub metadata: Option<String>,
    pub created_at: String,
}

impl Database {
    /// Create a new association between two skills
    pub fn create_skill_association(
        &self,
        source_skill_id: i64,
        target_skill_id: i64,
        association_type: &str,
        strength: f64,
        metadata: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO skill_associations (source_skill_id, target_skill_id, association_type, strength, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            rusqlite::params![source_skill_id, target_skill_id, association_type, strength, metadata],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get associations for a skill (both directions)
    pub fn get_skill_associations(&self, skill_id: i64) -> Result<Vec<SkillAssociationRow>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, source_skill_id, target_skill_id, association_type, strength, metadata, created_at
             FROM skill_associations
             WHERE source_skill_id = ?1 OR target_skill_id = ?1
             ORDER BY strength DESC"
        )?;
        let rows = stmt.query_map(rusqlite::params![skill_id], |row| {
            Ok(SkillAssociationRow {
                id: row.get(0)?,
                source_skill_id: row.get(1)?,
                target_skill_id: row.get(2)?,
                association_type: row.get(3)?,
                strength: row.get(4)?,
                metadata: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Delete an association
    pub fn delete_skill_association(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let count = conn.execute(
            "DELETE FROM skill_associations WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(count > 0)
    }

    /// List all skill associations (for graph endpoint)
    pub fn list_all_skill_associations(&self) -> Result<Vec<SkillAssociationRow>, rusqlite::Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, source_skill_id, target_skill_id, association_type, strength, metadata, created_at
             FROM skill_associations
             ORDER BY strength DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SkillAssociationRow {
                id: row.get(0)?,
                source_skill_id: row.get(1)?,
                target_skill_id: row.get(2)?,
                association_type: row.get(3)?,
                strength: row.get(4)?,
                metadata: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Check if an association exists between two skills (either direction)
    pub fn skill_association_exists(
        &self,
        source_id: i64,
        target_id: i64,
    ) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM skill_associations
             WHERE (source_skill_id = ?1 AND target_skill_id = ?2)
                OR (source_skill_id = ?2 AND target_skill_id = ?1)",
            rusqlite::params![source_id, target_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Delete all associations involving a specific skill
    pub fn delete_skill_associations_for(&self, skill_id: i64) -> Result<usize, rusqlite::Error> {
        let conn = self.conn();
        let count = conn.execute(
            "DELETE FROM skill_associations WHERE source_skill_id = ?1 OR target_skill_id = ?1",
            rusqlite::params![skill_id],
        )?;
        Ok(count)
    }

    /// Delete all skill associations (for rebuild)
    pub fn delete_all_skill_associations(&self) -> Result<usize, rusqlite::Error> {
        let conn = self.conn();
        let count = conn.execute("DELETE FROM skill_associations", [])?;
        Ok(count)
    }
}
