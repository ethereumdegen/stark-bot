//! Bot settings database operations

use chrono::{DateTime, Utc};
use rusqlite::Result as SqliteResult;
use std::collections::HashMap;

use crate::models::{BotSettings, DEFAULT_MAX_TOOL_ITERATIONS, DEFAULT_SAFE_MODE_MAX_QUERIES_PER_10MIN};
use super::super::Database;

impl Database {
    /// Get bot settings (there's only one row)
    pub fn get_bot_settings(&self) -> SqliteResult<BotSettings> {
        if let Some(cached) = self.cache.get_bot_settings() {
            return Ok(cached);
        }

        let conn = self.conn();

        let result = conn.query_row(
            "SELECT id, bot_name, bot_email, web3_tx_requires_confirmation, rpc_provider, custom_rpc_endpoints, max_tool_iterations, rogue_mode_enabled, safe_mode_max_queries_per_10min, keystore_url, chat_session_memory_generation, guest_dashboard_enabled, theme_accent, proxy_url, kanban_auto_execute, created_at, updated_at, coalescing_enabled, coalescing_debounce_ms, coalescing_max_wait_ms, compaction_background_threshold, compaction_aggressive_threshold, compaction_emergency_threshold FROM bot_settings LIMIT 1",
            [],
            |row| {
                let web3_tx_confirmation: i64 = row.get(3)?;
                let rpc_provider: String = row.get::<_, Option<String>>(4)?.unwrap_or_else(|| "defirelay".to_string());
                let custom_rpc_endpoints_json: Option<String> = row.get(5)?;
                let max_tool_iterations: i32 = row.get::<_, Option<i32>>(6)?.unwrap_or(DEFAULT_MAX_TOOL_ITERATIONS);
                let rogue_mode_enabled: i64 = row.get::<_, Option<i64>>(7)?.unwrap_or(0);
                let safe_mode_max_queries: i32 = row.get::<_, Option<i32>>(8)?.unwrap_or(DEFAULT_SAFE_MODE_MAX_QUERIES_PER_10MIN);
                let keystore_url: Option<String> = row.get(9)?;
                let chat_session_memory_generation: i64 = row.get::<_, Option<i64>>(10)?.unwrap_or(1);
                let guest_dashboard_enabled: i64 = row.get::<_, Option<i64>>(11)?.unwrap_or(0);
                let theme_accent: Option<String> = row.get(12)?;
                let proxy_url: Option<String> = row.get(13)?;
                let kanban_auto_execute: i64 = row.get::<_, Option<i64>>(14)?.unwrap_or(1);
                let created_at_str: String = row.get(15)?;
                let updated_at_str: String = row.get(16)?;
                let coalescing_enabled: i64 = row.get::<_, Option<i64>>(17)?.unwrap_or(0);
                let coalescing_debounce_ms: u64 = row.get::<_, Option<u64>>(18)?.unwrap_or(1500);
                let coalescing_max_wait_ms: u64 = row.get::<_, Option<u64>>(19)?.unwrap_or(5000);
                let compaction_background_threshold: f64 = row.get::<_, Option<f64>>(20)?.unwrap_or(0.80);
                let compaction_aggressive_threshold: f64 = row.get::<_, Option<f64>>(21)?.unwrap_or(0.85);
                let compaction_emergency_threshold: f64 = row.get::<_, Option<f64>>(22)?.unwrap_or(0.95);

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
                    rogue_mode_enabled: rogue_mode_enabled != 0,
                    safe_mode_max_queries_per_10min: safe_mode_max_queries,
                    keystore_url,
                    chat_session_memory_generation: chat_session_memory_generation != 0,
                    guest_dashboard_enabled: guest_dashboard_enabled != 0,
                    theme_accent,
                    proxy_url,
                    kanban_auto_execute: kanban_auto_execute != 0,
                    coalescing_enabled: coalescing_enabled != 0,
                    coalescing_debounce_ms,
                    coalescing_max_wait_ms,
                    compaction_background_threshold,
                    compaction_aggressive_threshold,
                    compaction_emergency_threshold,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            },
        );

        let settings = match result {
            Ok(settings) => settings,
            Err(_) => BotSettings::default(),
        };
        self.cache.set_bot_settings(settings.clone());
        Ok(settings)
    }

    /// Update bot settings
    pub fn update_bot_settings(
        &self,
        bot_name: Option<&str>,
        bot_email: Option<&str>,
        web3_tx_requires_confirmation: Option<bool>,
    ) -> SqliteResult<BotSettings> {
        self.update_bot_settings_full(bot_name, bot_email, web3_tx_requires_confirmation, None, None, None, None, None, None, None, None, None, None, None)
    }

    /// Update bot settings with all fields including RPC config and keystore URL
    pub fn update_bot_settings_full(
        &self,
        bot_name: Option<&str>,
        bot_email: Option<&str>,
        web3_tx_requires_confirmation: Option<bool>,
        rpc_provider: Option<&str>,
        custom_rpc_endpoints: Option<&HashMap<String, String>>,
        max_tool_iterations: Option<i32>,
        rogue_mode_enabled: Option<bool>,
        safe_mode_max_queries_per_10min: Option<i32>,
        keystore_url: Option<&str>,
        chat_session_memory_generation: Option<bool>,
        guest_dashboard_enabled: Option<bool>,
        theme_accent: Option<&str>,
        proxy_url: Option<&str>,
        kanban_auto_execute: Option<bool>,
    ) -> SqliteResult<BotSettings> {
        let conn = self.conn();
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
            if let Some(rogue_mode) = rogue_mode_enabled {
                conn.execute(
                    "UPDATE bot_settings SET rogue_mode_enabled = ?1, updated_at = ?2",
                    rusqlite::params![if rogue_mode { 1 } else { 0 }, &now],
                )?;
            }
            if let Some(max_queries) = safe_mode_max_queries_per_10min {
                conn.execute(
                    "UPDATE bot_settings SET safe_mode_max_queries_per_10min = ?1, updated_at = ?2",
                    rusqlite::params![max_queries, &now],
                )?;
            }
            if let Some(url) = keystore_url {
                // Empty string means reset to default (NULL)
                let url_value: Option<&str> = if url.is_empty() { None } else { Some(url) };
                conn.execute(
                    "UPDATE bot_settings SET keystore_url = ?1, updated_at = ?2",
                    rusqlite::params![url_value, &now],
                )?;
            }
            if let Some(enabled) = chat_session_memory_generation {
                conn.execute(
                    "UPDATE bot_settings SET chat_session_memory_generation = ?1, updated_at = ?2",
                    rusqlite::params![if enabled { 1 } else { 0 }, &now],
                )?;
            }
            if let Some(enabled) = guest_dashboard_enabled {
                conn.execute(
                    "UPDATE bot_settings SET guest_dashboard_enabled = ?1, updated_at = ?2",
                    rusqlite::params![if enabled { 1 } else { 0 }, &now],
                )?;
            }
            if let Some(accent) = theme_accent {
                let accent_value: Option<&str> = if accent.is_empty() { None } else { Some(accent) };
                conn.execute(
                    "UPDATE bot_settings SET theme_accent = ?1, updated_at = ?2",
                    rusqlite::params![accent_value, &now],
                )?;
            }
            if let Some(url) = proxy_url {
                let url_value: Option<&str> = if url.is_empty() { None } else { Some(url) };
                conn.execute(
                    "UPDATE bot_settings SET proxy_url = ?1, updated_at = ?2",
                    rusqlite::params![url_value, &now],
                )?;
            }
            if let Some(enabled) = kanban_auto_execute {
                conn.execute(
                    "UPDATE bot_settings SET kanban_auto_execute = ?1, updated_at = ?2",
                    rusqlite::params![if enabled { 1 } else { 0 }, &now],
                )?;
            }
        } else {
            // Insert new
            let name = bot_name.unwrap_or("StarkBot");
            let email = bot_email.unwrap_or("starkbot@users.noreply.github.com");
            let confirmation = web3_tx_requires_confirmation.unwrap_or(false);
            let provider = rpc_provider.unwrap_or("defirelay");
            let max_iterations = max_tool_iterations.unwrap_or(DEFAULT_MAX_TOOL_ITERATIONS);
            let rogue_mode = rogue_mode_enabled.unwrap_or(false);
            let safe_mode_queries = safe_mode_max_queries_per_10min.unwrap_or(DEFAULT_SAFE_MODE_MAX_QUERIES_PER_10MIN);
            let endpoints_json = custom_rpc_endpoints
                .map(|e| serde_json::to_string(e).unwrap_or_else(|_| "{}".to_string()));
            // Empty string means no custom URL (use default)
            let keystore_url_value: Option<&str> = keystore_url.filter(|u| !u.is_empty());
            let session_memory = chat_session_memory_generation.unwrap_or(true);
            let guest_dashboard = guest_dashboard_enabled.unwrap_or(false);
            let theme_accent_value: Option<&str> = theme_accent.filter(|u| !u.is_empty());
            let proxy_url_value: Option<&str> = proxy_url.filter(|u| !u.is_empty());
            let kanban_auto = kanban_auto_execute.unwrap_or(true);
            conn.execute(
                "INSERT INTO bot_settings (bot_name, bot_email, web3_tx_requires_confirmation, rpc_provider, custom_rpc_endpoints, max_tool_iterations, rogue_mode_enabled, safe_mode_max_queries_per_10min, keystore_url, chat_session_memory_generation, guest_dashboard_enabled, theme_accent, proxy_url, kanban_auto_execute, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                rusqlite::params![name, email, if confirmation { 1 } else { 0 }, provider, endpoints_json, max_iterations, if rogue_mode { 1 } else { 0 }, safe_mode_queries, keystore_url_value, if session_memory { 1 } else { 0 }, if guest_dashboard { 1 } else { 0 }, theme_accent_value, proxy_url_value, if kanban_auto { 1 } else { 0 }, &now, &now],
            )?;
        }

        drop(conn);
        self.cache.invalidate_bot_settings();
        self.get_bot_settings()
    }
}
