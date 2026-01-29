//! Tool configuration and execution logging database operations

use chrono::Utc;
use rusqlite::Result as SqliteResult;

use crate::tools::{ToolConfig, ToolExecution, ToolProfile};
use super::super::Database;

impl Database {
    /// Get global tool config (channel_id = NULL)
    pub fn get_global_tool_config(&self) -> SqliteResult<Option<ToolConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, channel_id, profile, allow_list, deny_list, allowed_groups, denied_groups
             FROM tool_configs WHERE channel_id IS NULL"
        )?;

        let config = stmt
            .query_row([], |row| {
                let allow_list: String = row.get(3)?;
                let deny_list: String = row.get(4)?;
                let allowed_groups: String = row.get(5)?;
                let denied_groups: String = row.get(6)?;
                let profile_str: String = row.get(2)?;

                Ok(ToolConfig {
                    id: row.get(0)?,
                    channel_id: row.get(1)?,
                    profile: ToolProfile::from_str(&profile_str).unwrap_or_default(),
                    allow_list: serde_json::from_str(&allow_list).unwrap_or_default(),
                    deny_list: serde_json::from_str(&deny_list).unwrap_or_default(),
                    allowed_groups: serde_json::from_str(&allowed_groups).unwrap_or_default(),
                    denied_groups: serde_json::from_str(&denied_groups).unwrap_or_default(),
                })
            })
            .ok();

        Ok(config)
    }

    /// Get tool config for a specific channel
    pub fn get_channel_tool_config(&self, channel_id: i64) -> SqliteResult<Option<ToolConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, channel_id, profile, allow_list, deny_list, allowed_groups, denied_groups
             FROM tool_configs WHERE channel_id = ?1"
        )?;

        let config = stmt
            .query_row([channel_id], |row| {
                let allow_list: String = row.get(3)?;
                let deny_list: String = row.get(4)?;
                let allowed_groups: String = row.get(5)?;
                let denied_groups: String = row.get(6)?;
                let profile_str: String = row.get(2)?;

                Ok(ToolConfig {
                    id: row.get(0)?,
                    channel_id: row.get(1)?,
                    profile: ToolProfile::from_str(&profile_str).unwrap_or_default(),
                    allow_list: serde_json::from_str(&allow_list).unwrap_or_default(),
                    deny_list: serde_json::from_str(&deny_list).unwrap_or_default(),
                    allowed_groups: serde_json::from_str(&allowed_groups).unwrap_or_default(),
                    denied_groups: serde_json::from_str(&denied_groups).unwrap_or_default(),
                })
            })
            .ok();

        Ok(config)
    }

    /// Get effective tool config for a channel (falls back to global if channel config doesn't exist)
    pub fn get_effective_tool_config(&self, channel_id: Option<i64>) -> SqliteResult<ToolConfig> {
        if let Some(cid) = channel_id {
            if let Some(config) = self.get_channel_tool_config(cid)? {
                return Ok(config);
            }
        }

        Ok(self.get_global_tool_config()?.unwrap_or_default())
    }

    /// Save tool config (upsert)
    pub fn save_tool_config(&self, config: &ToolConfig) -> SqliteResult<i64> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let profile_str = match &config.profile {
            ToolProfile::None => "none",
            ToolProfile::Minimal => "minimal",
            ToolProfile::Standard => "standard",
            ToolProfile::Messaging => "messaging",
            ToolProfile::Full => "full",
            ToolProfile::Custom => "custom",
        };

        let allow_list_json = serde_json::to_string(&config.allow_list).unwrap_or_default();
        let deny_list_json = serde_json::to_string(&config.deny_list).unwrap_or_default();
        let allowed_groups_json = serde_json::to_string(&config.allowed_groups).unwrap_or_default();
        let denied_groups_json = serde_json::to_string(&config.denied_groups).unwrap_or_default();

        if config.channel_id.is_some() {
            conn.execute(
                "INSERT INTO tool_configs (channel_id, profile, allow_list, deny_list, allowed_groups, denied_groups, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(channel_id) DO UPDATE SET
                    profile = excluded.profile,
                    allow_list = excluded.allow_list,
                    deny_list = excluded.deny_list,
                    allowed_groups = excluded.allowed_groups,
                    denied_groups = excluded.denied_groups,
                    updated_at = excluded.updated_at",
                rusqlite::params![
                    config.channel_id,
                    profile_str,
                    allow_list_json,
                    deny_list_json,
                    allowed_groups_json,
                    denied_groups_json,
                    now
                ],
            )?;
        } else {
            // Global config (channel_id = NULL) - need special handling
            conn.execute(
                "DELETE FROM tool_configs WHERE channel_id IS NULL",
                [],
            )?;
            conn.execute(
                "INSERT INTO tool_configs (channel_id, profile, allow_list, deny_list, allowed_groups, denied_groups, created_at, updated_at)
                 VALUES (NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                rusqlite::params![
                    profile_str,
                    allow_list_json,
                    deny_list_json,
                    allowed_groups_json,
                    denied_groups_json,
                    now
                ],
            )?;
        }

        Ok(conn.last_insert_rowid())
    }

    /// Log a tool execution
    pub fn log_tool_execution(&self, execution: &ToolExecution) -> SqliteResult<i64> {
        let conn = self.conn.lock().unwrap();
        let params_json = serde_json::to_string(&execution.parameters).unwrap_or_default();

        conn.execute(
            "INSERT INTO tool_executions (channel_id, session_id, tool_name, parameters, success, result, duration_ms, executed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                execution.channel_id,
                None::<i64>, // session_id could be added if needed
                execution.tool_name,
                params_json,
                execution.success as i32,
                execution.result,
                execution.duration_ms,
                execution.executed_at
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Get tool execution history for a channel
    pub fn get_tool_execution_history(
        &self,
        channel_id: i64,
        limit: i32,
        offset: i32,
    ) -> SqliteResult<Vec<ToolExecution>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, channel_id, tool_name, parameters, success, result, duration_ms, executed_at
             FROM tool_executions WHERE channel_id = ?1 ORDER BY executed_at DESC LIMIT ?2 OFFSET ?3"
        )?;

        let executions: Vec<ToolExecution> = stmt
            .query_map(rusqlite::params![channel_id, limit, offset], |row| {
                let params_str: String = row.get(3)?;
                Ok(ToolExecution {
                    id: row.get(0)?,
                    channel_id: row.get(1)?,
                    tool_name: row.get(2)?,
                    parameters: serde_json::from_str(&params_str).unwrap_or_default(),
                    success: row.get::<_, i32>(4)? != 0,
                    result: row.get(5)?,
                    duration_ms: row.get(6)?,
                    executed_at: row.get(7)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(executions)
    }

    /// Get all tool execution history
    pub fn get_all_tool_execution_history(
        &self,
        limit: i32,
        offset: i32,
    ) -> SqliteResult<Vec<ToolExecution>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, channel_id, tool_name, parameters, success, result, duration_ms, executed_at
             FROM tool_executions ORDER BY executed_at DESC LIMIT ?1 OFFSET ?2"
        )?;

        let executions: Vec<ToolExecution> = stmt
            .query_map(rusqlite::params![limit, offset], |row| {
                let params_str: String = row.get(3)?;
                Ok(ToolExecution {
                    id: row.get(0)?,
                    channel_id: row.get(1)?,
                    tool_name: row.get(2)?,
                    parameters: serde_json::from_str(&params_str).unwrap_or_default(),
                    success: row.get::<_, i32>(4)? != 0,
                    result: row.get(5)?,
                    duration_ms: row.get(6)?,
                    executed_at: row.get(7)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(executions)
    }
}
