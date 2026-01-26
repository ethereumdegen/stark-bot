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
            "SELECT id, provider, endpoint, api_key, model, enabled, bot_name, bot_email, created_at, updated_at
             FROM agent_settings WHERE enabled = 1 LIMIT 1",
        )?;

        let settings = stmt
            .query_row([], |row| Self::row_to_agent_settings(row))
            .ok();

        Ok(settings)
    }

    /// Get agent settings by provider name
    pub fn get_agent_settings_by_provider(&self, provider: &str) -> SqliteResult<Option<AgentSettings>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, provider, endpoint, api_key, model, enabled, bot_name, bot_email, created_at, updated_at
             FROM agent_settings WHERE provider = ?1",
        )?;

        let settings = stmt
            .query_row([provider], |row| Self::row_to_agent_settings(row))
            .ok();

        Ok(settings)
    }

    /// List all agent settings
    pub fn list_agent_settings(&self) -> SqliteResult<Vec<AgentSettings>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, provider, endpoint, api_key, model, enabled, bot_name, bot_email, created_at, updated_at
             FROM agent_settings ORDER BY provider",
        )?;

        let settings = stmt
            .query_map([], |row| Self::row_to_agent_settings(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(settings)
    }

    /// Save agent settings (upsert by provider, and set as the only enabled one)
    pub fn save_agent_settings(
        &self,
        provider: &str,
        endpoint: &str,
        api_key: &str,
        model: &str,
        bot_name: Option<&str>,
        bot_email: Option<&str>,
    ) -> SqliteResult<AgentSettings> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let bot_name = bot_name.unwrap_or("StarkBot");
        let bot_email = bot_email.unwrap_or("starkbot@users.noreply.github.com");

        // First, disable all existing settings
        conn.execute("UPDATE agent_settings SET enabled = 0, updated_at = ?1", [&now])?;

        // Check if this provider already exists
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM agent_settings WHERE provider = ?1",
                [provider],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            // Update existing
            conn.execute(
                "UPDATE agent_settings SET endpoint = ?1, api_key = ?2, model = ?3, bot_name = ?4, bot_email = ?5, enabled = 1, updated_at = ?6 WHERE id = ?7",
                rusqlite::params![endpoint, api_key, model, bot_name, bot_email, &now, id],
            )?;
        } else {
            // Insert new
            conn.execute(
                "INSERT INTO agent_settings (provider, endpoint, api_key, model, bot_name, bot_email, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8)",
                rusqlite::params![provider, endpoint, api_key, model, bot_name, bot_email, &now, &now],
            )?;
        }

        drop(conn);

        // Return the saved settings
        self.get_agent_settings_by_provider(provider)
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
        let created_at_str: String = row.get(8)?;
        let updated_at_str: String = row.get(9)?;

        Ok(AgentSettings {
            id: row.get(0)?,
            provider: row.get(1)?,
            endpoint: row.get(2)?,
            api_key: row.get(3)?,
            model: row.get(4)?,
            enabled: row.get::<_, i32>(5)? != 0,
            bot_name: row.get(6)?,
            bot_email: row.get(7)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
        })
    }
}
