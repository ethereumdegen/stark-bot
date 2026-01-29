//! Heartbeat configuration database operations

use chrono::Utc;
use rusqlite::Result as SqliteResult;

use crate::models::HeartbeatConfig;
use super::super::Database;

impl Database {
    /// Get or create heartbeat config for a channel (or global if channel_id is None)
    pub fn get_or_create_heartbeat_config(&self, channel_id: Option<i64>) -> SqliteResult<HeartbeatConfig> {
        let conn = self.conn.lock().unwrap();

        // Try to get existing config
        let existing = if let Some(cid) = channel_id {
            conn.query_row(
                "SELECT id, channel_id, interval_minutes, target, active_hours_start, active_hours_end,
                        active_days, enabled, last_beat_at, next_beat_at, created_at, updated_at
                 FROM heartbeat_configs WHERE channel_id = ?1",
                [cid],
                |row| self.map_heartbeat_config_row(row),
            ).ok()
        } else {
            conn.query_row(
                "SELECT id, channel_id, interval_minutes, target, active_hours_start, active_hours_end,
                        active_days, enabled, last_beat_at, next_beat_at, created_at, updated_at
                 FROM heartbeat_configs WHERE channel_id IS NULL",
                [],
                |row| self.map_heartbeat_config_row(row),
            ).ok()
        };

        if let Some(config) = existing {
            return Ok(config);
        }

        // Create new config
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO heartbeat_configs (channel_id, interval_minutes, target, enabled, created_at, updated_at)
             VALUES (?1, 30, 'last', 1, ?2, ?2)",
            rusqlite::params![channel_id, now],
        )?;

        let id = conn.last_insert_rowid();

        Ok(HeartbeatConfig {
            id,
            channel_id,
            interval_minutes: 30,
            target: "last".to_string(),
            active_hours_start: None,
            active_hours_end: None,
            active_days: None,
            enabled: true,
            last_beat_at: None,
            next_beat_at: None,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    fn map_heartbeat_config_row(&self, row: &rusqlite::Row) -> SqliteResult<HeartbeatConfig> {
        Ok(HeartbeatConfig {
            id: row.get(0)?,
            channel_id: row.get(1)?,
            interval_minutes: row.get(2)?,
            target: row.get(3)?,
            active_hours_start: row.get(4)?,
            active_hours_end: row.get(5)?,
            active_days: row.get(6)?,
            enabled: row.get::<_, i32>(7)? != 0,
            last_beat_at: row.get(8)?,
            next_beat_at: row.get(9)?,
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
        })
    }

    /// Update heartbeat config
    pub fn update_heartbeat_config(
        &self,
        id: i64,
        interval_minutes: Option<i32>,
        target: Option<&str>,
        active_hours_start: Option<&str>,
        active_hours_end: Option<&str>,
        active_days: Option<&str>,
        enabled: Option<bool>,
    ) -> SqliteResult<HeartbeatConfig> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut param_index = 2;

        if interval_minutes.is_some() { updates.push(format!("interval_minutes = ?{}", param_index)); param_index += 1; }
        if target.is_some() { updates.push(format!("target = ?{}", param_index)); param_index += 1; }
        if active_hours_start.is_some() { updates.push(format!("active_hours_start = ?{}", param_index)); param_index += 1; }
        if active_hours_end.is_some() { updates.push(format!("active_hours_end = ?{}", param_index)); param_index += 1; }
        if active_days.is_some() { updates.push(format!("active_days = ?{}", param_index)); param_index += 1; }
        if enabled.is_some() { updates.push(format!("enabled = ?{}", param_index)); param_index += 1; }

        let query = format!(
            "UPDATE heartbeat_configs SET {} WHERE id = ?{}",
            updates.join(", "),
            param_index
        );

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];
        if let Some(v) = interval_minutes { params.push(Box::new(v)); }
        if let Some(v) = target { params.push(Box::new(v.to_string())); }
        if let Some(v) = active_hours_start { params.push(Box::new(v.to_string())); }
        if let Some(v) = active_hours_end { params.push(Box::new(v.to_string())); }
        if let Some(v) = active_days { params.push(Box::new(v.to_string())); }
        if let Some(v) = enabled { params.push(Box::new(v as i32)); }
        params.push(Box::new(id));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        conn.execute(&query, params_refs.as_slice())?;

        conn.query_row(
            "SELECT id, channel_id, interval_minutes, target, active_hours_start, active_hours_end,
                    active_days, enabled, last_beat_at, next_beat_at, created_at, updated_at
             FROM heartbeat_configs WHERE id = ?1",
            [id],
            |row| self.map_heartbeat_config_row(row),
        )
    }

    /// Update heartbeat last run time
    pub fn update_heartbeat_last_beat(&self, id: i64, last_beat_at: &str, next_beat_at: &str) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE heartbeat_configs SET last_beat_at = ?1, next_beat_at = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![last_beat_at, next_beat_at, now, id],
        )?;

        Ok(())
    }

    /// List all heartbeat configs
    pub fn list_heartbeat_configs(&self) -> SqliteResult<Vec<HeartbeatConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, channel_id, interval_minutes, target, active_hours_start, active_hours_end,
                    active_days, enabled, last_beat_at, next_beat_at, created_at, updated_at
             FROM heartbeat_configs ORDER BY id"
        )?;

        let configs: Vec<HeartbeatConfig> = stmt
            .query_map([], |row| self.map_heartbeat_config_row(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(configs)
    }

    /// Get enabled heartbeat configs that are due to run
    pub fn list_due_heartbeat_configs(&self) -> SqliteResult<Vec<HeartbeatConfig>> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let mut stmt = conn.prepare(
            "SELECT id, channel_id, interval_minutes, target, active_hours_start, active_hours_end,
                    active_days, enabled, last_beat_at, next_beat_at, created_at, updated_at
             FROM heartbeat_configs
             WHERE enabled = 1 AND (next_beat_at IS NULL OR next_beat_at <= ?1)
             ORDER BY next_beat_at ASC"
        )?;

        let configs: Vec<HeartbeatConfig> = stmt
            .query_map([&now], |row| self.map_heartbeat_config_row(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(configs)
    }
}
