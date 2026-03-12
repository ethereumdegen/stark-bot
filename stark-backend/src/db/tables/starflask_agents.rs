//! CRUD operations for starflask_agents and starflask_command_log tables.

use crate::db::Database;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarflaskAgent {
    pub id: i64,
    pub capability: String,
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub pack_hashes: Vec<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarflaskCommandLog {
    pub id: i64,
    pub capability: String,
    pub session_id: Option<String>,
    pub message: String,
    pub status: String,
    pub result: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

impl Database {
    pub fn get_starflask_agent(&self, capability: &str) -> Result<Option<StarflaskAgent>, String> {
        let conn = self.conn();
        match conn.query_row(
            "SELECT id, capability, agent_id, name, description, pack_hashes, status, created_at, updated_at
             FROM starflask_agents WHERE capability = ?1",
            [capability],
            |row| {
                let hashes_str: String = row.get(5)?;
                let pack_hashes: Vec<String> = serde_json::from_str(&hashes_str).unwrap_or_default();
                Ok(StarflaskAgent {
                    id: row.get(0)?,
                    capability: row.get(1)?,
                    agent_id: row.get(2)?,
                    name: row.get(3)?,
                    description: row.get(4)?,
                    pack_hashes,
                    status: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        ) {
            Ok(agent) => Ok(Some(agent)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn list_starflask_agents(&self) -> Result<Vec<StarflaskAgent>, String> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, capability, agent_id, name, description, pack_hashes, status, created_at, updated_at
                 FROM starflask_agents ORDER BY capability",
            )
            .map_err(|e| e.to_string())?;
        let agents = stmt
            .query_map([], |row| {
                let hashes_str: String = row.get(5)?;
                let pack_hashes: Vec<String> = serde_json::from_str(&hashes_str).unwrap_or_default();
                Ok(StarflaskAgent {
                    id: row.get(0)?,
                    capability: row.get(1)?,
                    agent_id: row.get(2)?,
                    name: row.get(3)?,
                    description: row.get(4)?,
                    pack_hashes,
                    status: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(agents)
    }

    pub fn upsert_starflask_agent(
        &self,
        capability: &str,
        agent_id: &Uuid,
        name: &str,
        description: &str,
        pack_hashes: &[String],
        status: &str,
    ) -> Result<(), String> {
        self.upsert_starflask_agent_str(capability, &agent_id.to_string(), name, description, pack_hashes, status)
    }

    pub fn upsert_starflask_agent_str(
        &self,
        capability: &str,
        agent_id: &str,
        name: &str,
        description: &str,
        pack_hashes: &[String],
        status: &str,
    ) -> Result<(), String> {
        let conn = self.conn();
        let hashes_json = serde_json::to_string(pack_hashes).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO starflask_agents (capability, agent_id, name, description, pack_hashes, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'), datetime('now'))
             ON CONFLICT(capability) DO UPDATE SET
             agent_id = ?2, name = ?3, description = ?4, pack_hashes = ?5, status = ?6, updated_at = datetime('now')",
            rusqlite::params![capability, agent_id, name, description, hashes_json, status],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_starflask_agent(&self, capability: &str) -> Result<bool, String> {
        let conn = self.conn();
        let rows = conn
            .execute("DELETE FROM starflask_agents WHERE capability = ?1", [capability])
            .map_err(|e| e.to_string())?;
        Ok(rows > 0)
    }

    pub fn log_starflask_command(
        &self,
        capability: &str,
        session_id: Option<&str>,
        message: &str,
    ) -> Result<i64, String> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO starflask_command_log (capability, session_id, message, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'pending', datetime('now'), datetime('now'))",
            rusqlite::params![capability, session_id, message],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn complete_starflask_command(
        &self,
        command_id: i64,
        status: &str,
        result: &serde_json::Value,
    ) -> Result<(), String> {
        let conn = self.conn();
        let result_str = serde_json::to_string(result).unwrap_or_else(|_| "null".to_string());
        conn.execute(
            "UPDATE starflask_command_log SET status = ?1, result = ?2, updated_at = datetime('now') WHERE id = ?3",
            rusqlite::params![status, result_str, command_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_starflask_commands(&self, limit: u32) -> Result<Vec<StarflaskCommandLog>, String> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, capability, session_id, message, status, result, created_at, updated_at
                 FROM starflask_command_log ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let commands = stmt
            .query_map([limit], |row| {
                let result_str: Option<String> = row.get(5)?;
                let result = result_str.and_then(|s| serde_json::from_str(&s).ok());
                Ok(StarflaskCommandLog {
                    id: row.get(0)?,
                    capability: row.get(1)?,
                    session_id: row.get(2)?,
                    message: row.get(3)?,
                    status: row.get(4)?,
                    result,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(commands)
    }
}
