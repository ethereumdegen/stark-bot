//! Bot configuration model backed by a RON file.
//!
//! Manages bot-wide settings including name, operating mode, heartbeat
//! configuration, and hyperpacks.  Loaded/saved from `config/bot_config.ron`.

use serde::{Deserialize, Serialize};

/// Operating mode for the bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatingMode {
    /// Autonomous actions without user confirmation.
    Rogue,
    /// Requires user confirmation for sensitive operations.
    Partner,
}

impl OperatingMode {
    pub fn is_rogue(self) -> bool {
        matches!(self, OperatingMode::Rogue)
    }
}

/// Top-level bot configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub bot_name: String,
    pub operating_mode: OperatingMode,
    pub heartbeat: HeartbeatFileConfig,
    pub hyperpacks: Vec<HyperPack>,
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: i32,
    #[serde(default = "default_max_response_tokens")]
    pub max_response_tokens: i32,
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: i32,
    #[serde(default = "default_safe_mode_max_queries")]
    pub safe_mode_max_queries_per_10min: i32,
    #[serde(default)]
    pub guest_dashboard_enabled: bool,
    #[serde(default = "default_true")]
    pub session_memory_log: bool,
    #[serde(default)]
    pub compaction: CompactionConfig,
    #[serde(default)]
    pub services: ServicesConfig,
}

/// Compaction threshold configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    #[serde(default = "default_background_threshold")]
    pub background_threshold: f64,
    #[serde(default = "default_aggressive_threshold")]
    pub aggressive_threshold: f64,
    #[serde(default = "default_emergency_threshold")]
    pub emergency_threshold: f64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            background_threshold: 0.80,
            aggressive_threshold: 0.85,
            emergency_threshold: 0.95,
        }
    }
}

/// External service URLs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicesConfig {
    #[serde(default)]
    pub whisper_server_url: Option<String>,
    #[serde(default)]
    pub embeddings_server_url: Option<String>,
    #[serde(default)]
    pub http_proxy_url: Option<String>,
    #[serde(default)]
    pub keystore_server_url: Option<String>,
}

impl Default for ServicesConfig {
    fn default() -> Self {
        Self {
            whisper_server_url: None,
            embeddings_server_url: None,
            http_proxy_url: None,
            keystore_server_url: None,
        }
    }
}

fn default_max_tool_iterations() -> i32 { 100 }
fn default_max_response_tokens() -> i32 { 40000 }
fn default_max_context_tokens() -> i32 { 100000 }
fn default_safe_mode_max_queries() -> i32 { 5 }
fn default_true() -> bool { true }
fn default_background_threshold() -> f64 { 0.80 }
fn default_aggressive_threshold() -> f64 { 0.85 }
fn default_emergency_threshold() -> f64 { 0.95 }

/// Heartbeat scheduling settings (config only — no runtime state).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatFileConfig {
    pub enabled: bool,
    pub interval_minutes: i32,
    pub active_hours_start: Option<String>,
    pub active_hours_end: Option<String>,
    pub active_days: Option<String>,
}

/// Source path for a hyperpack dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HyperPackPath {
    /// Fetch from a git repository.
    Git {
        url: String,
        /// Pin to a specific commit hash. None = HEAD of default branch.
        commit: Option<String>,
    },
    /// Fetch from a hyperpack registry (e.g. hyperpacks.org).
    WebServer {
        host: String,
        hyperpack_name: String,
        /// Semver version constraint (e.g. "^0.8.1", "1.2.0"). None = latest.
        version: Option<String>,
    },
}

/// A hyperpack dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperPack {
    pub path: HyperPackPath,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            bot_name: "StarkBot".to_string(),
            operating_mode: OperatingMode::Partner,
            heartbeat: HeartbeatFileConfig::default(),
            hyperpacks: Vec::new(),
            max_tool_iterations: 100,
            max_response_tokens: 40000,
            max_context_tokens: 100000,
            safe_mode_max_queries_per_10min: 5,
            guest_dashboard_enabled: false,
            session_memory_log: true,
            compaction: CompactionConfig::default(),
            services: ServicesConfig::default(),
        }
    }
}

impl Default for HeartbeatFileConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: 60,
            active_hours_start: None,
            active_hours_end: None,
            active_days: None,
        }
    }
}

impl BotConfig {
    /// Load from the runtime config path, falling back to `Default` on any error.
    pub fn load() -> Self {
        let path = crate::config::bot_config_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                match ron::from_str::<BotConfig>(&content) {
                    Ok(config) => config,
                    Err(e) => {
                        log::warn!("Failed to parse bot_config.ron: {} — using defaults", e);
                        Self::default()
                    }
                }
            }
            Err(e) => {
                log::debug!("Could not read bot_config.ron ({}), using defaults", e);
                Self::default()
            }
        }
    }

    /// Serialize to pretty RON and write to the runtime config path.
    pub fn save(&self) -> Result<(), String> {
        let path = crate::config::bot_config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config dir: {}", e))?;
        }
        let pretty = ron::ser::PrettyConfig::default();
        let content = ron::ser::to_string_pretty(self, pretty)
            .map_err(|e| format!("Failed to serialize bot config: {}", e))?;
        std::fs::write(&path, content)
            .map_err(|e| format!("Failed to write bot_config.ron: {}", e))?;
        Ok(())
    }
}
