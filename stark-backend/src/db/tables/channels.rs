//! Channel database operations

use chrono::{DateTime, Utc};
use rusqlite::Result as SqliteResult;

use crate::models::Channel;
use super::super::Database;

impl Database {
    /// Create a new external channel
    pub fn create_channel(
        &self,
        channel_type: &str,
        name: &str,
        bot_token: &str,
        app_token: Option<&str>,
    ) -> SqliteResult<Channel> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO external_channels (channel_type, name, enabled, bot_token, app_token, created_at, updated_at)
             VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6)",
            rusqlite::params![channel_type, name, bot_token, app_token, &now, &now],
        )?;

        let id = conn.last_insert_rowid();

        Ok(Channel {
            id,
            channel_type: channel_type.to_string(),
            name: name.to_string(),
            enabled: false,
            bot_token: bot_token.to_string(),
            app_token: app_token.map(|s| s.to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }

    /// Get a channel by ID
    pub fn get_channel(&self, id: i64) -> SqliteResult<Option<Channel>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, channel_type, name, enabled, bot_token, app_token, created_at, updated_at
             FROM external_channels WHERE id = ?1",
        )?;

        let channel = stmt
            .query_row([id], |row| Self::row_to_channel(row))
            .ok();

        Ok(channel)
    }

    /// List all channels
    pub fn list_channels(&self) -> SqliteResult<Vec<Channel>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, channel_type, name, enabled, bot_token, app_token, created_at, updated_at
             FROM external_channels ORDER BY channel_type, name",
        )?;

        let channels = stmt
            .query_map([], |row| Self::row_to_channel(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(channels)
    }

    /// List only enabled channels
    pub fn list_enabled_channels(&self) -> SqliteResult<Vec<Channel>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, channel_type, name, enabled, bot_token, app_token, created_at, updated_at
             FROM external_channels WHERE enabled = 1 ORDER BY channel_type, name",
        )?;

        let channels = stmt
            .query_map([], |row| Self::row_to_channel(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(channels)
    }

    /// Update a channel
    pub fn update_channel(
        &self,
        id: i64,
        name: Option<&str>,
        enabled: Option<bool>,
        bot_token: Option<&str>,
        app_token: Option<Option<&str>>,
    ) -> SqliteResult<Option<Channel>> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Build dynamic update query
        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut param_idx = 2;

        if name.is_some() {
            updates.push(format!("name = ?{}", param_idx));
            param_idx += 1;
        }
        if enabled.is_some() {
            updates.push(format!("enabled = ?{}", param_idx));
            param_idx += 1;
        }
        if bot_token.is_some() {
            updates.push(format!("bot_token = ?{}", param_idx));
            param_idx += 1;
        }
        if app_token.is_some() {
            updates.push(format!("app_token = ?{}", param_idx));
            param_idx += 1;
        }

        let sql = format!(
            "UPDATE external_channels SET {} WHERE id = ?{}",
            updates.join(", "),
            param_idx
        );

        // Build params dynamically
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        if let Some(n) = name {
            params.push(Box::new(n.to_string()));
        }
        if let Some(e) = enabled {
            params.push(Box::new(if e { 1 } else { 0 }));
        }
        if let Some(t) = bot_token {
            params.push(Box::new(t.to_string()));
        }
        if let Some(at) = app_token {
            params.push(Box::new(at.map(|s| s.to_string())));
        }
        params.push(Box::new(id));

        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        conn.execute(&sql, params_ref.as_slice())?;

        drop(conn);
        self.get_channel(id)
    }

    /// Enable or disable a channel
    pub fn set_channel_enabled(&self, id: i64, enabled: bool) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let rows_affected = conn.execute(
            "UPDATE external_channels SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![if enabled { 1 } else { 0 }, &now, id],
        )?;

        Ok(rows_affected > 0)
    }

    /// Delete a channel
    pub fn delete_channel(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute(
            "DELETE FROM external_channels WHERE id = ?1",
            [id],
        )?;
        Ok(rows_affected > 0)
    }

    fn row_to_channel(row: &rusqlite::Row) -> rusqlite::Result<Channel> {
        let created_at_str: String = row.get(6)?;
        let updated_at_str: String = row.get(7)?;

        Ok(Channel {
            id: row.get(0)?,
            channel_type: row.get(1)?,
            name: row.get(2)?,
            enabled: row.get::<_, i32>(3)? != 0,
            bot_token: row.get(4)?,
            app_token: row.get(5)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
        })
    }
}
