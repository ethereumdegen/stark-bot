//! Bot settings database operations

use chrono::{DateTime, Utc};
use rusqlite::Result as SqliteResult;
use std::collections::HashMap;

use crate::models::{BotSettings, DEFAULT_MAX_TOOL_ITERATIONS};
use super::super::Database;

impl Database {
    /// Get bot settings (there's only one row)
    pub fn get_bot_settings(&self) -> SqliteResult<BotSettings> {
        let conn = self.conn.lock().unwrap();

        let result = conn.query_row(
            "SELECT id, bot_name, bot_email, web3_tx_requires_confirmation, rpc_provider, custom_rpc_endpoints, max_tool_iterations, created_at, updated_at FROM bot_settings LIMIT 1",
            [],
            |row| {
                let web3_tx_confirmation: i64 = row.get(3)?;
                let rpc_provider: String = row.get::<_, Option<String>>(4)?.unwrap_or_else(|| "defirelay".to_string());
                let custom_rpc_endpoints_json: Option<String> = row.get(5)?;
                let max_tool_iterations: i32 = row.get::<_, Option<i32>>(6)?.unwrap_or(DEFAULT_MAX_TOOL_ITERATIONS);
                let created_at_str: String = row.get(7)?;
                let updated_at_str: String = row.get(8)?;

                let custom_rpc_endpoints: Option<HashMap<String, String>> = custom_rpc_endpoints_json
                    .and_then(|json| serde_json::from_str(&json).ok());

                Ok(BotSettings {
                    id: row.get(0)?,
                    bot_name: row.get(1)?,
                    bot_email: row.get(2)?,
                    web3_tx_requires_confirmation: web3_tx_confirmation != 0,
                    rpc_provider,
                    custom_rpc_endpoints,
                    max_tool_iterations,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            },
        );

        match result {
            Ok(settings) => Ok(settings),
            Err(_) => Ok(BotSettings::default()),
        }
    }

    /// Update bot settings
    pub fn update_bot_settings(
        &self,
        bot_name: Option<&str>,
        bot_email: Option<&str>,
        web3_tx_requires_confirmation: Option<bool>,
    ) -> SqliteResult<BotSettings> {
        self.update_bot_settings_full(bot_name, bot_email, web3_tx_requires_confirmation, None, None, None)
    }

    /// Update bot settings with all fields including RPC config
    pub fn update_bot_settings_full(
        &self,
        bot_name: Option<&str>,
        bot_email: Option<&str>,
        web3_tx_requires_confirmation: Option<bool>,
        rpc_provider: Option<&str>,
        custom_rpc_endpoints: Option<&HashMap<String, String>>,
        max_tool_iterations: Option<i32>,
    ) -> SqliteResult<BotSettings> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Check if settings exist
        let exists: bool = conn
            .query_row("SELECT COUNT(*) FROM bot_settings", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|c| c > 0)
            .unwrap_or(false);

        if exists {
            // Update existing
            if let Some(name) = bot_name {
                conn.execute(
                    "UPDATE bot_settings SET bot_name = ?1, updated_at = ?2",
                    [name, &now],
                )?;
            }
            if let Some(email) = bot_email {
                conn.execute(
                    "UPDATE bot_settings SET bot_email = ?1, updated_at = ?2",
                    [email, &now],
                )?;
            }
            if let Some(requires_confirmation) = web3_tx_requires_confirmation {
                conn.execute(
                    "UPDATE bot_settings SET web3_tx_requires_confirmation = ?1, updated_at = ?2",
                    rusqlite::params![if requires_confirmation { 1 } else { 0 }, &now],
                )?;
            }
            if let Some(provider) = rpc_provider {
                conn.execute(
                    "UPDATE bot_settings SET rpc_provider = ?1, updated_at = ?2",
                    [provider, &now],
                )?;
            }
            if let Some(endpoints) = custom_rpc_endpoints {
                let endpoints_json = serde_json::to_string(endpoints).unwrap_or_else(|_| "{}".to_string());
                conn.execute(
                    "UPDATE bot_settings SET custom_rpc_endpoints = ?1, updated_at = ?2",
                    [&endpoints_json, &now],
                )?;
            }
            if let Some(max_iterations) = max_tool_iterations {
                conn.execute(
                    "UPDATE bot_settings SET max_tool_iterations = ?1, updated_at = ?2",
                    rusqlite::params![max_iterations, &now],
                )?;
            }
        } else {
            // Insert new
            let name = bot_name.unwrap_or("StarkBot");
            let email = bot_email.unwrap_or("starkbot@users.noreply.github.com");
            let confirmation = web3_tx_requires_confirmation.unwrap_or(false);
            let provider = rpc_provider.unwrap_or("defirelay");
            let max_iterations = max_tool_iterations.unwrap_or(DEFAULT_MAX_TOOL_ITERATIONS);
            let endpoints_json = custom_rpc_endpoints
                .map(|e| serde_json::to_string(e).unwrap_or_else(|_| "{}".to_string()));
            conn.execute(
                "INSERT INTO bot_settings (bot_name, bot_email, web3_tx_requires_confirmation, rpc_provider, custom_rpc_endpoints, max_tool_iterations, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![name, email, if confirmation { 1 } else { 0 }, provider, endpoints_json, max_iterations, &now, &now],
            )?;
        }

        drop(conn);
        self.get_bot_settings()
    }
}
