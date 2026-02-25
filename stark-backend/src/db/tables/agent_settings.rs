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
            "SELECT id, endpoint_name, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, enabled, secret_key, created_at, updated_at, payment_mode
             FROM agent_settings WHERE enabled = 1 LIMIT 1",
        )?;

        let settings = stmt
            .query_row([], |row| Self::row_to_agent_settings(row))
            .ok();

        self.cache.set_active_agent_settings(settings.clone());
        Ok(settings)
    }

    /// Get agent settings by endpoint_name (preset key)
    pub fn get_agent_settings_by_endpoint_name(&self, endpoint_name: &str) -> SqliteResult<Option<AgentSettings>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, endpoint_name, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, enabled, secret_key, created_at, updated_at, payment_mode
             FROM agent_settings WHERE endpoint_name = ?1",
        )?;

        let settings = stmt
            .query_row([endpoint_name], |row| Self::row_to_agent_settings(row))
            .ok();

        Ok(settings)
    }

    /// Get agent settings by endpoint and model (for custom endpoints without endpoint_name)
    pub fn get_agent_settings_by_endpoint_and_model(&self, endpoint: &str, model: Option<&str>) -> SqliteResult<Option<AgentSettings>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, endpoint_name, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, enabled, secret_key, created_at, updated_at, payment_mode
             FROM agent_settings WHERE endpoint = ?1 AND (model = ?2 OR (?2 IS NULL AND model IS NULL))",
        )?;

        let settings = stmt
            .query_row(rusqlite::params![endpoint, model], |row| Self::row_to_agent_settings(row))
            .ok();

        Ok(settings)
    }

    /// List all agent settings
    pub fn list_agent_settings(&self) -> SqliteResult<Vec<AgentSettings>> {
        let conn = self.conn();

        let mut stmt = conn.prepare(
            "SELECT id, endpoint_name, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, enabled, secret_key, created_at, updated_at, payment_mode
             FROM agent_settings ORDER BY id",
        )?;

        let settings = stmt
            .query_map([], |row| Self::row_to_agent_settings(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(settings)
    }

    /// Save agent settings (upsert by endpoint_name if preset, or endpoint+model if custom, and set as the only enabled one)
    pub fn save_agent_settings(
        &self,
        endpoint_name: Option<&str>,
        endpoint: &str,
        model_archetype: &str,
        model: Option<&str>,
        max_response_tokens: i32,
        max_context_tokens: i32,
        secret_key: Option<&str>,
        payment_mode: &str,
    ) -> SqliteResult<AgentSettings> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();

        // Enforce minimum context tokens
        let max_context_tokens = max_context_tokens.max(MIN_CONTEXT_TOKENS);

        // First, disable all existing settings
        conn.execute("UPDATE agent_settings SET enabled = 0, updated_at = ?1", [&now])?;

        // Find existing row: by endpoint_name if preset, by endpoint+model if custom
        let existing: Option<i64> = if let Some(name) = endpoint_name {
            conn.query_row(
                "SELECT id FROM agent_settings WHERE endpoint_name = ?1",
                [name],
                |row| row.get(0),
            ).ok()
        } else {
            conn.query_row(
                "SELECT id FROM agent_settings WHERE endpoint_name IS NULL AND endpoint = ?1 AND (model = ?2 OR (?2 IS NULL AND model IS NULL))",
                rusqlite::params![endpoint, model],
                |row| row.get(0),
            ).ok()
        };

        if let Some(id) = existing {
            // Update existing
            conn.execute(
                "UPDATE agent_settings SET endpoint_name = ?1, endpoint = ?2, model_archetype = ?3, model = ?4, max_response_tokens = ?5, max_context_tokens = ?6, secret_key = ?7, enabled = 1, updated_at = ?8, payment_mode = ?10 WHERE id = ?9",
                rusqlite::params![endpoint_name, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, secret_key, &now, id, payment_mode],
            )?;
        } else {
            // Insert new
            conn.execute(
                "INSERT INTO agent_settings (endpoint_name, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, secret_key, enabled, created_at, updated_at, payment_mode)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10)",
                rusqlite::params![endpoint_name, endpoint, model_archetype, model, max_response_tokens, max_context_tokens, secret_key, &now, &now, payment_mode],
            )?;
        }

        drop(conn);
        self.cache.invalidate_agent_settings();

        // Return the saved settings
        if let Some(name) = endpoint_name {
            self.get_agent_settings_by_endpoint_name(name)
                .map(|opt| opt.unwrap())
        } else {
            self.get_agent_settings_by_endpoint_and_model(endpoint, model)
                .map(|opt| opt.unwrap())
        }
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
        let created_at_str: String = row.get(9)?;
        let updated_at_str: String = row.get(10)?;

        Ok(AgentSettings {
            id: row.get(0)?,
            endpoint_name: row.get(1)?,
            endpoint: row.get(2)?,
            model_archetype: row.get::<_, Option<String>>(3)?.unwrap_or_else(|| "kimi".to_string()),
            model: row.get(4)?,
            max_response_tokens: row.get::<_, Option<i32>>(5)?.unwrap_or(40000),
            max_context_tokens: row.get::<_, Option<i32>>(6)?.unwrap_or(DEFAULT_CONTEXT_TOKENS),
            enabled: row.get::<_, i32>(7)? != 0,
            secret_key: row.get(8)?,
            payment_mode: row.get::<_, Option<String>>(11)?.unwrap_or_else(|| "credits".to_string()),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
        })
    }
}
