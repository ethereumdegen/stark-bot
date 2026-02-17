//! Special roles database operations (enriched safe mode)

use chrono::Utc;
use rusqlite::Result as SqliteResult;

use crate::models::{SpecialRole, SpecialRoleAssignment, SpecialRoleGrants};
use super::super::Database;

impl Database {
    /// List all special roles.
    pub fn list_special_roles(&self) -> SqliteResult<Vec<SpecialRole>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT name, allowed_tools, allowed_skills, description, created_at, updated_at
             FROM special_roles ORDER BY name"
        )?;
        let roles = stmt
            .query_map([], |row| {
                let tools_str: String = row.get(1)?;
                let skills_str: String = row.get(2)?;
                Ok(SpecialRole {
                    name: row.get(0)?,
                    allowed_tools: serde_json::from_str(&tools_str).unwrap_or_default(),
                    allowed_skills: serde_json::from_str(&skills_str).unwrap_or_default(),
                    description: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(roles)
    }

    /// Get a single special role by name.
    pub fn get_special_role(&self, name: &str) -> SqliteResult<Option<SpecialRole>> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT name, allowed_tools, allowed_skills, description, created_at, updated_at
             FROM special_roles WHERE name = ?1",
            [name],
            |row| {
                let tools_str: String = row.get(1)?;
                let skills_str: String = row.get(2)?;
                Ok(SpecialRole {
                    name: row.get(0)?,
                    allowed_tools: serde_json::from_str(&tools_str).unwrap_or_default(),
                    allowed_skills: serde_json::from_str(&skills_str).unwrap_or_default(),
                    description: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        );
        match result {
            Ok(role) => Ok(Some(role)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Insert or update a special role.
    pub fn upsert_special_role(&self, role: &SpecialRole) -> SqliteResult<()> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let tools_json = serde_json::to_string(&role.allowed_tools).unwrap_or_else(|_| "[]".to_string());
        let skills_json = serde_json::to_string(&role.allowed_skills).unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT INTO special_roles (name, allowed_tools, allowed_skills, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(name) DO UPDATE SET
                allowed_tools = excluded.allowed_tools,
                allowed_skills = excluded.allowed_skills,
                description = excluded.description,
                updated_at = excluded.updated_at",
            rusqlite::params![role.name, tools_json, skills_json, role.description, now],
        )?;
        Ok(())
    }

    /// Count total special roles.
    pub fn count_special_roles(&self) -> SqliteResult<i64> {
        let conn = self.conn();
        conn.query_row("SELECT COUNT(*) FROM special_roles", [], |row| row.get(0))
    }

    /// Count total special role assignments.
    pub fn count_special_role_assignments(&self) -> SqliteResult<i64> {
        let conn = self.conn();
        conn.query_row("SELECT COUNT(*) FROM special_role_assignments", [], |row| row.get(0))
    }

    /// Delete a special role by name. Returns true if a row was deleted.
    pub fn delete_special_role(&self, name: &str) -> SqliteResult<bool> {
        let conn = self.conn();
        let rows = conn.execute("DELETE FROM special_roles WHERE name = ?1", [name])?;
        Ok(rows > 0)
    }

    // --- Assignments ---

    /// List special role assignments, optionally filtered by role_name.
    pub fn list_special_role_assignments(&self, role_name: Option<&str>) -> SqliteResult<Vec<SpecialRoleAssignment>> {
        let conn = self.conn();
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match role_name {
            Some(name) => (
                "SELECT id, channel_type, user_id, special_role_name, created_at
                 FROM special_role_assignments WHERE special_role_name = ?1 ORDER BY id",
                vec![Box::new(name.to_string())],
            ),
            None => (
                "SELECT id, channel_type, user_id, special_role_name, created_at
                 FROM special_role_assignments ORDER BY id",
                vec![],
            ),
        };

        let mut stmt = conn.prepare(sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let assignments = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(SpecialRoleAssignment {
                    id: row.get(0)?,
                    channel_type: row.get(1)?,
                    user_id: row.get(2)?,
                    special_role_name: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(assignments)
    }

    /// Create a special role assignment. Returns the created assignment.
    pub fn create_special_role_assignment(
        &self,
        channel_type: &str,
        user_id: &str,
        special_role_name: &str,
    ) -> SqliteResult<SpecialRoleAssignment> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO special_role_assignments (channel_type, user_id, special_role_name, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![channel_type, user_id, special_role_name, now],
        )?;
        let id = conn.last_insert_rowid();
        Ok(SpecialRoleAssignment {
            id,
            channel_type: channel_type.to_string(),
            user_id: user_id.to_string(),
            special_role_name: special_role_name.to_string(),
            created_at: now,
        })
    }

    /// Delete a special role assignment by ID. Returns true if deleted.
    pub fn delete_special_role_assignment(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn();
        let rows = conn.execute("DELETE FROM special_role_assignments WHERE id = ?1", [id])?;
        Ok(rows > 0)
    }

    /// Delete a special role assignment by composite key.
    pub fn delete_special_role_assignment_by_key(
        &self,
        channel_type: &str,
        user_id: &str,
        role_name: &str,
    ) -> SqliteResult<bool> {
        let conn = self.conn();
        let rows = conn.execute(
            "DELETE FROM special_role_assignments WHERE channel_type = ?1 AND user_id = ?2 AND special_role_name = ?3",
            rusqlite::params![channel_type, user_id, role_name],
        )?;
        Ok(rows > 0)
    }

    /// Get special role grants for a user on a channel (hot-path for dispatcher).
    /// Returns at most one role's grants (UNIQUE constraint: one role per user/channel).
    pub fn get_special_role_grants(
        &self,
        channel_type: &str,
        user_id: &str,
    ) -> SqliteResult<SpecialRoleGrants> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT sr.name, sr.allowed_tools, sr.allowed_skills
             FROM special_role_assignments sra
             JOIN special_roles sr ON sr.name = sra.special_role_name
             WHERE sra.channel_type = ?1 AND sra.user_id = ?2",
            rusqlite::params![channel_type, user_id],
            |row| {
                let name: String = row.get(0)?;
                let tools_str: String = row.get(1)?;
                let skills_str: String = row.get(2)?;
                Ok((name, tools_str, skills_str))
            },
        );

        match result {
            Ok((name, tools_str, skills_str)) => {
                Ok(SpecialRoleGrants {
                    role_name: Some(name),
                    extra_tools: serde_json::from_str(&tools_str).unwrap_or_default(),
                    extra_skills: serde_json::from_str(&skills_str).unwrap_or_default(),
                })
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(SpecialRoleGrants::default()),
            Err(e) => Err(e),
        }
    }
}
