//! In-memory cache layer for hot-path database queries.
//!
//! Uses moka::sync::Cache to avoid hitting SQLite on every request for
//! nearly-static data like bot settings and API keys.

use std::time::Duration;

use moka::sync::Cache;

use crate::models::{ApiKey, BotSettings};

/// Short TTL for config data (bot settings, API keys)
const CONFIG_TTL: Duration = Duration::from_secs(300); // 5 min

/// In-memory cache for frequently-read, rarely-written database data.
pub struct DbCache {
    /// Singleton cache: key "bot_settings" → BotSettings
    bot_settings: Cache<&'static str, BotSettings>,

    /// Per-service: key = service_name → Option<ApiKey>
    api_keys: Cache<String, Option<ApiKey>>,
}

impl DbCache {
    /// Create a new cache with default TTLs and reasonable max capacities.
    pub fn new() -> Self {
        Self {
            bot_settings: Cache::builder()
                .time_to_live(CONFIG_TTL)
                .max_capacity(1)
                .build(),
            api_keys: Cache::builder()
                .time_to_live(CONFIG_TTL)
                .max_capacity(64)
                .build(),
        }
    }

    // ── Bot settings ────────────────────────────────────────

    pub fn get_bot_settings(&self) -> Option<BotSettings> {
        self.bot_settings.get(&"bot_settings")
    }

    pub fn set_bot_settings(&self, settings: BotSettings) {
        self.bot_settings.insert("bot_settings", settings);
    }

    pub fn invalidate_bot_settings(&self) {
        self.bot_settings.invalidate(&"bot_settings");
    }

    // ── API keys ────────────────────────────────────────────

    pub fn get_api_key(&self, service: &str) -> Option<Option<ApiKey>> {
        self.api_keys.get(&service.to_string())
    }

    pub fn set_api_key(&self, service: &str, key: Option<ApiKey>) {
        self.api_keys.insert(service.to_string(), key);
    }

    pub fn invalidate_api_key(&self, service: &str) {
        self.api_keys.invalidate(&service.to_string());
    }
}
