//! Agent subtypes database operations

use chrono::Utc;
use rusqlite::Result as SqliteResult;

use crate::ai::multi_agent::types::AgentSubtypeConfig;
use super::super::Database;

impl Database {
    /// List all agent subtypes, ordered by sort_order.
    pub fn list_agent_subtypes(&self) -> SqliteResult<Vec<AgentSubtypeConfig>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT key, label, emoji, description, tool_groups_json, skill_tags_json, prompt, sort_order, enabled, max_iterations, additional_tools_json, skip_task_planner, aliases_json, hidden
             FROM agent_subtypes ORDER BY sort_order, key"
        )?;

        let configs: Vec<AgentSubtypeConfig> = stmt
            .query_map([], |row| {
                let tool_groups_str: String = row.get(4)?;
                let skill_tags_str: String = row.get(5)?;
                let additional_tools_str: String = row.get::<_, String>(10).unwrap_or_else(|_| "[]".to_string());
                let aliases_str: String = row.get::<_, String>(12).unwrap_or_else(|_| "[]".to_string());
                Ok(AgentSubtypeConfig {
                    key: row.get(0)?,
                    label: row.get(1)?,
                    emoji: row.get(2)?,
                    description: row.get(3)?,
                    tool_groups: serde_json::from_str(&tool_groups_str).unwrap_or_default(),
                    skill_tags: serde_json::from_str(&skill_tags_str).unwrap_or_default(),
                    additional_tools: serde_json::from_str(&additional_tools_str).unwrap_or_default(),
                    prompt: row.get(6)?,
                    sort_order: row.get(7)?,
                    enabled: row.get::<_, i32>(8)? != 0,
                    max_iterations: row.get::<_, i64>(9).unwrap_or(90) as u32,
                    skip_task_planner: row.get::<_, i32>(11).unwrap_or(0) != 0,
                    aliases: serde_json::from_str(&aliases_str).unwrap_or_default(),
                    hidden: row.get::<_, i32>(13).unwrap_or(0) != 0,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(configs)
    }

    /// Get a single agent subtype by key.
    pub fn get_agent_subtype(&self, key: &str) -> SqliteResult<Option<AgentSubtypeConfig>> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT key, label, emoji, description, tool_groups_json, skill_tags_json, prompt, sort_order, enabled, max_iterations, additional_tools_json, skip_task_planner, aliases_json, hidden
             FROM agent_subtypes WHERE key = ?1",
            [key],
            |row| {
                let tool_groups_str: String = row.get(4)?;
                let skill_tags_str: String = row.get(5)?;
                let additional_tools_str: String = row.get::<_, String>(10).unwrap_or_else(|_| "[]".to_string());
                let aliases_str: String = row.get::<_, String>(12).unwrap_or_else(|_| "[]".to_string());
                Ok(AgentSubtypeConfig {
                    key: row.get(0)?,
                    label: row.get(1)?,
                    emoji: row.get(2)?,
                    description: row.get(3)?,
                    tool_groups: serde_json::from_str(&tool_groups_str).unwrap_or_default(),
                    skill_tags: serde_json::from_str(&skill_tags_str).unwrap_or_default(),
                    additional_tools: serde_json::from_str(&additional_tools_str).unwrap_or_default(),
                    prompt: row.get(6)?,
                    sort_order: row.get(7)?,
                    enabled: row.get::<_, i32>(8)? != 0,
                    max_iterations: row.get::<_, i64>(9).unwrap_or(90) as u32,
                    skip_task_planner: row.get::<_, i32>(11).unwrap_or(0) != 0,
                    aliases: serde_json::from_str(&aliases_str).unwrap_or_default(),
                    hidden: row.get::<_, i32>(13).unwrap_or(0) != 0,
                })
            },
        );
        match result {
            Ok(config) => Ok(Some(config)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Insert or update an agent subtype.
    pub fn upsert_agent_subtype(&self, config: &AgentSubtypeConfig) -> SqliteResult<()> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let tool_groups_json = serde_json::to_string(&config.tool_groups).unwrap_or_else(|_| "[]".to_string());
        let skill_tags_json = serde_json::to_string(&config.skill_tags).unwrap_or_else(|_| "[]".to_string());
        let additional_tools_json = serde_json::to_string(&config.additional_tools).unwrap_or_else(|_| "[]".to_string());
        let aliases_json = serde_json::to_string(&config.aliases).unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT INTO agent_subtypes (key, label, emoji, description, tool_groups_json, skill_tags_json, additional_tools_json, prompt, sort_order, enabled, max_iterations, skip_task_planner, aliases_json, hidden, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
             ON CONFLICT(key) DO UPDATE SET
                label = excluded.label,
                emoji = excluded.emoji,
                description = excluded.description,
                tool_groups_json = excluded.tool_groups_json,
                skill_tags_json = excluded.skill_tags_json,
                additional_tools_json = excluded.additional_tools_json,
                prompt = excluded.prompt,
                sort_order = excluded.sort_order,
                enabled = excluded.enabled,
                max_iterations = excluded.max_iterations,
                skip_task_planner = excluded.skip_task_planner,
                aliases_json = excluded.aliases_json,
                hidden = excluded.hidden,
                updated_at = excluded.updated_at",
            rusqlite::params![
                config.key,
                config.label,
                config.emoji,
                config.description,
                tool_groups_json,
                skill_tags_json,
                additional_tools_json,
                config.prompt,
                config.sort_order,
                config.enabled as i32,
                config.max_iterations as i64,
                config.skip_task_planner as i32,
                aliases_json,
                config.hidden as i32,
                now,
            ],
        )?;
        Ok(())
    }

    /// Delete an agent subtype by key.
    pub fn delete_agent_subtype(&self, key: &str) -> SqliteResult<bool> {
        let conn = self.conn();
        let rows = conn.execute("DELETE FROM agent_subtypes WHERE key = ?1", [key])?;
        Ok(rows > 0)
    }

    /// Count total agent subtypes.
    pub fn count_agent_subtypes(&self) -> SqliteResult<i64> {
        let conn = self.conn();
        conn.query_row("SELECT COUNT(*) FROM agent_subtypes", [], |row| row.get(0))
    }
}
