//! Agent settings database operations

use chrono::{DateTime, Utc};
use rusqlite::Result as SqliteResult;

use crate::models::{AgentSettings, MIN_CONTEXT_TOKENS, DEFAULT_CONTEXT_TOKENS};
use super::super::Database;

impl Database {
    /// Get the currently enabled agent settings (only one can be enabled)
    pub fn get_active_agent_settings(&self) -> SqliteResult<Option<AgentSettings>> {
        if let Some(cached) = self.cache.get_active_agent_settings() {
            return Ok(cached);
        }

        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, enabled, secret_key, created_at, updated_at
             FROM agent_settings WHERE enabled = 1 LIMIT 1",
        )?;

        let settings = stmt
            .query_row([], |row| Self::row_to_agent_settings(row))
            .ok();

        self.cache.set_active_agent_settings(settings.clone());
        Ok(settings)
    }

    /// Get agent settings by endpoint and model
    pub fn get_agent_settings_by_endpoint_and_model(&self, endpoint: &str, model: Option<&str>) -> SqliteResult<Option<AgentSettings>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, enabled, secret_key, created_at, updated_at
             FROM agent_settings WHERE endpoint = ?1 AND (model = ?2 OR (?2 IS NULL AND model IS NULL))",
        )?;

        let settings = stmt
            .query_row(rusqlite::params![endpoint, model], |row| Self::row_to_agent_settings(row))
            .ok();

        Ok(settings)
    }

    /// Get agent settings by endpoint (legacy, for backward compat)
    pub fn get_agent_settings_by_endpoint(&self, endpoint: &str) -> SqliteResult<Option<AgentSettings>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, enabled, secret_key, created_at, updated_at
             FROM agent_settings WHERE endpoint = ?1",
        )?;

        let settings = stmt
            .query_row([endpoint], |row| Self::row_to_agent_settings(row))
            .ok();

        Ok(settings)
    }

    /// List all agent settings
    pub fn list_agent_settings(&self) -> SqliteResult<Vec<AgentSettings>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, enabled, secret_key, created_at, updated_at
             FROM agent_settings ORDER BY id",
        )?;

        let settings = stmt
            .query_map([], |row| Self::row_to_agent_settings(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(settings)
    }

    /// Save agent settings (upsert by endpoint+model, and set as the only enabled one)
    pub fn save_agent_settings(
        &self,
        endpoint: &str,
        model_archetype: &str,
        model: Option<&str>,
        max_response_tokens: i32,
        max_context_tokens: i32,
        secret_key: Option<&str>,
    ) -> SqliteResult<AgentSettings> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();

        // Enforce minimum context tokens
        let max_context_tokens = max_context_tokens.max(MIN_CONTEXT_TOKENS);

        // First, disable all existing settings
        conn.execute("UPDATE agent_settings SET enabled = 0, updated_at = ?1", [&now])?;

        // Check if this endpoint+model already exists
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM agent_settings WHERE endpoint = ?1 AND (model = ?2 OR (?2 IS NULL AND model IS NULL))",
                rusqlite::params![endpoint, model],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            // Update existing
            conn.execute(
                "UPDATE agent_settings SET model_archetype = ?1, model = ?2, max_response_tokens = ?3, max_context_tokens = ?4, secret_key = ?5, enabled = 1, updated_at = ?6 WHERE id = ?7",
                rusqlite::params![model_archetype, model, max_response_tokens, max_context_tokens, secret_key, &now, id],
            )?;
        } else {
            // Insert new
            conn.execute(
                "INSERT INTO agent_settings (endpoint, model_archetype, model, max_response_tokens, max_context_tokens, secret_key, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8)",
                rusqlite::params![endpoint, model_archetype, model, max_response_tokens, max_context_tokens, secret_key, &now, &now],
            )?;
        }

        drop(conn);
        self.cache.invalidate_agent_settings();

        // Return the saved settings
        self.get_agent_settings_by_endpoint_and_model(endpoint, model)
            .map(|opt| opt.unwrap())
    }

    /// Disable all agent settings (no AI provider active)
    pub fn disable_agent_settings(&self) -> SqliteResult<()> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        conn.execute("UPDATE agent_settings SET enabled = 0, updated_at = ?1", [&now])?;
        self.cache.invalidate_agent_settings();
        Ok(())
    }

    fn row_to_agent_settings(row: &rusqlite::Row) -> rusqlite::Result<AgentSettings> {
        let created_at_str: String = row.get(8)?;
        let updated_at_str: String = row.get(9)?;

        Ok(AgentSettings {
            id: row.get(0)?,
            endpoint: row.get(1)?,
            model_archetype: row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "kimi".to_string()),
            model: row.get(3)?,
            max_response_tokens: row.get::<_, Option<i32>>(4)?.unwrap_or(40000),
            max_context_tokens: row.get::<_, Option<i32>>(5)?.unwrap_or(DEFAULT_CONTEXT_TOKENS),
            enabled: row.get::<_, i32>(6)? != 0,
            secret_key: row.get(7)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
        })
    }
}
