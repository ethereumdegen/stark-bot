//! Agent settings database operations

use chrono::{DateTime, Utc};
use rusqlite::Result as SqliteResult;

use crate::models::AgentSettings;
use super::super::Database;

impl Database {
    /// Get the currently enabled agent settings (only one can be enabled)
    pub fn get_active_agent_settings(&self) -> SqliteResult<Option<AgentSettings>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, endpoint, model_archetype, max_tokens, enabled, created_at, updated_at
             FROM agent_settings WHERE enabled = 1 LIMIT 1",
        )?;

        let settings = stmt
            .query_row([], |row| Self::row_to_agent_settings(row))
            .ok();

        Ok(settings)
    }

    /// Get agent settings by endpoint
    pub fn get_agent_settings_by_endpoint(&self, endpoint: &str) -> SqliteResult<Option<AgentSettings>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, endpoint, model_archetype, max_tokens, enabled, created_at, updated_at
             FROM agent_settings WHERE endpoint = ?1",
        )?;

        let settings = stmt
            .query_row([endpoint], |row| Self::row_to_agent_settings(row))
            .ok();

        Ok(settings)
    }

    /// List all agent settings
    pub fn list_agent_settings(&self) -> SqliteResult<Vec<AgentSettings>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, endpoint, model_archetype, max_tokens, enabled, created_at, updated_at
             FROM agent_settings ORDER BY id",
        )?;

        let settings = stmt
            .query_map([], |row| Self::row_to_agent_settings(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(settings)
    }

    /// Save agent settings (upsert by endpoint, and set as the only enabled one)
    pub fn save_agent_settings(
        &self,
        endpoint: &str,
        model_archetype: &str,
        max_tokens: i32,
    ) -> SqliteResult<AgentSettings> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // First, disable all existing settings
        conn.execute("UPDATE agent_settings SET enabled = 0, updated_at = ?1", [&now])?;

        // Check if this endpoint already exists
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM agent_settings WHERE endpoint = ?1",
                [endpoint],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            // Update existing
            conn.execute(
                "UPDATE agent_settings SET model_archetype = ?1, max_tokens = ?2, enabled = 1, updated_at = ?3 WHERE id = ?4",
                rusqlite::params![model_archetype, max_tokens, &now, id],
            )?;
        } else {
            // Insert new
            conn.execute(
                "INSERT INTO agent_settings (endpoint, model_archetype, max_tokens, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4, ?5)",
                rusqlite::params![endpoint, model_archetype, max_tokens, &now, &now],
            )?;
        }

        drop(conn);

        // Return the saved settings
        self.get_agent_settings_by_endpoint(endpoint)
            .map(|opt| opt.unwrap())
    }

    /// Disable all agent settings (no AI provider active)
    pub fn disable_agent_settings(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute("UPDATE agent_settings SET enabled = 0, updated_at = ?1", [&now])?;
        Ok(())
    }

    fn row_to_agent_settings(row: &rusqlite::Row) -> rusqlite::Result<AgentSettings> {
        let created_at_str: String = row.get(5)?;
        let updated_at_str: String = row.get(6)?;

        Ok(AgentSettings {
            id: row.get(0)?,
            endpoint: row.get(1)?,
            model_archetype: row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "kimi".to_string()),
            max_tokens: row.get::<_, Option<i32>>(3)?.unwrap_or(40000),
            enabled: row.get::<_, i32>(4)? != 0,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
        })
    }
}
