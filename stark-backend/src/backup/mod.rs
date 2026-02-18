//! Backup module for starkbot
//!
//! Provides structures and utilities for backing up and restoring user data
//! to/from the keystore server.
//!
//! ## Schema resilience
//!
//! All structs use `#[serde(default)]` at the struct level so that:
//! - **Missing fields** in old backups get sensible defaults (deserialization never fails)
//! - **Unknown fields** from newer backups are silently ignored (serde default behavior)
//! This means you can freely add/remove fields without breaking existing backups.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Current backup format version
pub const BACKUP_VERSION: u32 = 1;

/// Complete backup data structure
///
/// This is the encrypted payload stored on the keystore server.
/// All data is serialized to JSON before encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupData {
    /// Backup format version for future migrations
    pub version: u32,
    /// When this backup was created
    pub created_at: DateTime<Utc>,
    /// Wallet address that created this backup
    pub wallet_address: String,
    /// API keys (always included)
    pub api_keys: Vec<ApiKeyEntry>,
    /// Mind map nodes
    pub mind_map_nodes: Vec<MindNodeEntry>,
    /// Mind map connections
    pub mind_map_connections: Vec<MindConnectionEntry>,
    /// Cron jobs (scheduled tasks)
    pub cron_jobs: Vec<CronJobEntry>,
    /// Heartbeat config (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_config: Option<HeartbeatConfigEntry>,
    /// Memories (optional - can be large)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memories: Option<Vec<MemoryEntry>>,
    /// Bot settings (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_settings: Option<BotSettingsEntry>,
    /// Channel settings (key-value configs per channel)
    pub channel_settings: Vec<ChannelSettingEntry>,
    /// Channels (with bot tokens)
    pub channels: Vec<ChannelEntry>,
    /// Soul document content (SOUL.md - agent's personality and truths)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soul_document: Option<String>,
    /// Identity document content (IDENTITY.json - EIP-8004 agent identity registration)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_document: Option<String>,
    /// Discord user registrations (LEGACY — kept for backward compat with old backups)
    /// New backups store this in module_data["discord_tipping"] instead.
    pub discord_registrations: Vec<DiscordRegistrationEntry>,
    /// Generic module data — each module stores its backup under its name
    pub module_data: HashMap<String, serde_json::Value>,
    /// Skills (custom agent skills)
    pub skills: Vec<SkillEntry>,
    /// AI model / agent settings (endpoint, archetype, tokens, etc.)
    pub agent_settings: Vec<AgentSettingsEntry>,
    /// On-chain agent identity registration (NFT token ID, tx hash, registry, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_identity: Option<AgentIdentityEntry>,
    /// x402 payment limits (per-call max amounts per token)
    pub x402_payment_limits: Vec<X402PaymentLimitEntry>,
    /// Kanban board items
    pub kanban_items: Vec<KanbanItemEntry>,
    /// Agent subtypes (configurable toolboxes)
    pub agent_subtypes: Vec<AgentSubtypeEntry>,
    /// Special roles (enriched safe mode)
    pub special_roles: Vec<SpecialRoleEntry>,
    /// Special role assignments (user→role mappings)
    pub special_role_assignments: Vec<SpecialRoleAssignmentEntry>,
    /// Tool config directories (e.g. gog CLI auth tokens)
    /// Maps a logical name (e.g. "gogcli") to a list of files with base64-encoded contents.
    pub tool_configs: HashMap<String, Vec<ToolConfigFileEntry>>,
}

/// Manual Default because DateTime<Utc> doesn't derive Default
impl Default for BackupData {
    fn default() -> Self {
        Self {
            version: 0,
            created_at: Utc::now(),
            wallet_address: String::new(),
            api_keys: Vec::new(),
            mind_map_nodes: Vec::new(),
            mind_map_connections: Vec::new(),
            cron_jobs: Vec::new(),
            heartbeat_config: None,
            memories: None,
            bot_settings: None,
            channel_settings: Vec::new(),
            channels: Vec::new(),
            soul_document: None,
            identity_document: None,
            discord_registrations: Vec::new(),
            module_data: HashMap::new(),
            skills: Vec::new(),
            agent_settings: Vec::new(),
            agent_identity: None,
            x402_payment_limits: Vec::new(),
            kanban_items: Vec::new(),
            agent_subtypes: Vec::new(),
            special_roles: Vec::new(),
            special_role_assignments: Vec::new(),
            tool_configs: HashMap::new(),
        }
    }
}

impl BackupData {
    /// Create a new backup with the current timestamp
    pub fn new(wallet_address: String) -> Self {
        Self {
            version: BACKUP_VERSION,
            created_at: Utc::now(),
            wallet_address,
            ..Default::default()
        }
    }

    /// Returns true if there's nothing meaningful to backup
    pub fn is_empty(&self) -> bool {
        self.item_count() == 0
    }

    /// Calculate total item count for progress reporting
    pub fn item_count(&self) -> usize {
        self.api_keys.len()
            + self.mind_map_nodes.len()
            + self.mind_map_connections.len()
            + self.cron_jobs.len()
            + self.memories.as_ref().map(|m| m.len()).unwrap_or(0)
            + if self.bot_settings.is_some() { 1 } else { 0 }
            + if self.heartbeat_config.is_some() { 1 } else { 0 }
            + self.channel_settings.len()
            + self.channels.len()
            + if self.soul_document.is_some() { 1 } else { 0 }
            + if self.identity_document.is_some() { 1 } else { 0 }
            + self.discord_registrations.len()
            + self.module_data.len()
            + self.skills.len()
            + self.agent_settings.len()
            + if self.agent_identity.is_some() { 1 } else { 0 }
            + self.x402_payment_limits.len()
            + self.kanban_items.len()
            + self.agent_subtypes.len()
            + self.special_roles.len()
            + self.special_role_assignments.len()
    }
}

/// API key entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiKeyEntry {
    pub key_name: String,
    pub key_value: String,
}

/// Mind map node entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MindNodeEntry {
    pub id: i64,
    pub body: String,
    pub position_x: Option<f64>,
    pub position_y: Option<f64>,
    pub is_trunk: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Mind map connection entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MindConnectionEntry {
    pub parent_id: i64,
    pub child_id: i64,
}

/// Cron job entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CronJobEntry {
    pub name: String,
    pub description: Option<String>,
    pub schedule_type: String,
    pub schedule_value: String,
    pub timezone: Option<String>,
    pub session_mode: String,
    pub message: Option<String>,
    pub system_event: Option<String>,
    pub channel_id: Option<i64>,
    pub deliver_to: Option<String>,
    pub deliver: bool,
    pub model_override: Option<String>,
    pub thinking_level: Option<String>,
    pub timeout_seconds: Option<i32>,
    pub delete_after_run: bool,
    pub status: String,
}

/// Heartbeat config entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HeartbeatConfigEntry {
    pub channel_id: Option<i64>,
    pub interval_minutes: i32,
    pub target: String,
    pub active_hours_start: Option<String>,
    pub active_hours_end: Option<String>,
    pub active_days: Option<String>,
    pub enabled: bool,
}

/// Memory entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryEntry {
    pub memory_type: String,
    pub content: String,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub importance: Option<i32>,
    pub identity_id: Option<String>,
    pub created_at: String,
}

/// Bot settings entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BotSettingsEntry {
    pub bot_name: String,
    pub bot_email: String,
    pub web3_tx_requires_confirmation: bool,
    pub rpc_provider: Option<String>,
    pub custom_rpc_endpoints: Option<String>,
    pub max_tool_iterations: Option<i32>,
    pub rogue_mode_enabled: bool,
    pub safe_mode_max_queries_per_10min: Option<i32>,
    pub guest_dashboard_enabled: bool,
    pub theme_accent: Option<String>,
}

/// Channel setting entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelSettingEntry {
    pub channel_id: i64,
    pub setting_key: String,
    pub setting_value: String,
}

/// Channel entry in backup (the actual channel with tokens)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelEntry {
    pub id: i64,
    pub channel_type: String,
    pub name: String,
    pub enabled: bool,
    pub bot_token: String,
    pub app_token: Option<String>,
}

/// Discord user registration entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordRegistrationEntry {
    pub discord_user_id: String,
    pub discord_username: Option<String>,
    pub public_address: String,
    pub registered_at: Option<String>,
}

/// Skill entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub body: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    pub enabled: bool,
    pub requires_tools: Vec<String>,
    pub requires_binaries: Vec<String>,
    /// Arguments serialized as JSON string
    pub arguments: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    /// requires_api_keys serialized as JSON string
    #[serde(default)]
    pub requires_api_keys: String,
    pub scripts: Vec<SkillScriptEntry>,
}

/// Skill script entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillScriptEntry {
    pub name: String,
    pub code: String,
    pub language: String,
}

/// AI model / agent settings entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettingsEntry {
    pub endpoint: String,
    pub model_archetype: String,
    pub model: Option<String>,
    pub max_response_tokens: i32,
    pub max_context_tokens: i32,
    pub enabled: bool,
    /// Secret key is included so the user doesn't have to re-enter API keys after restore.
    /// The entire backup payload is already encrypted with ECIES — this is not stored in plaintext.
    pub secret_key: Option<String>,
}

/// On-chain agent identity registration entry in backup (full metadata — DB is single source of truth)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentIdentityEntry {
    pub agent_id: i64,
    pub agent_registry: String,
    pub chain_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub x402_support: bool,
    pub active: bool,
    pub services_json: String,
    pub supported_trust_json: String,
    pub registration_uri: Option<String>,
}

/// x402 payment limit entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct X402PaymentLimitEntry {
    pub asset: String,
    pub max_amount: String,
    pub decimals: u8,
    pub display_name: String,
    pub address: Option<String>,
}

/// Kanban board item entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct KanbanItemEntry {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: i32,
    pub session_id: Option<i64>,
    pub result: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Agent subtype entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSubtypeEntry {
    pub key: String,
    pub label: String,
    pub emoji: String,
    pub description: String,
    pub tool_groups_json: String,
    pub skill_tags_json: String,
    pub additional_tools_json: String,
    pub prompt: String,
    pub sort_order: i32,
    pub enabled: bool,
    pub max_iterations: Option<u32>,
    pub skip_task_planner: Option<bool>,
    pub aliases_json: String,
}

/// Special role entry in backup (enriched safe mode)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpecialRoleEntry {
    pub name: String,
    pub allowed_tools_json: String,
    pub allowed_skills_json: String,
    pub description: Option<String>,
}

/// Special role assignment entry in backup
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SpecialRoleAssignmentEntry {
    pub channel_type: String,
    pub user_id: String,
    pub special_role_name: String,
    pub label: Option<String>,
}

/// A file from a tool's config directory, stored as base64
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolConfigFileEntry {
    /// Relative path within the config directory (e.g. "keyring/token:default:user@gmail.com.json")
    pub relative_path: String,
    /// Base64-encoded file contents
    pub content_b64: String,
}

/// Options for what to include in a backup
#[derive(Debug, Clone, Default)]
pub struct BackupOptions {
    /// Include memories (can be large)
    pub include_memories: bool,
    /// Include bot settings
    pub include_bot_settings: bool,
    /// Maximum number of memories to include (0 = unlimited)
    pub max_memories: usize,
}

impl BackupOptions {
    /// Backup everything
    pub fn full() -> Self {
        Self {
            include_memories: true,
            include_bot_settings: true,
            max_memories: 0,
        }
    }

    /// Minimal backup (API keys and mind map only)
    pub fn minimal() -> Self {
        Self {
            include_memories: false,
            include_bot_settings: false,
            max_memories: 0,
        }
    }
}

/// Collect all backup data from the database into a BackupData struct.
///
/// This is the core data-gathering logic extracted from the backup endpoint
/// so it can be reused by both the HTTP handler and the cloud_backup tool.
pub async fn collect_backup_data(
    db: &crate::db::Database,
    wallet_address: String,
) -> BackupData {
    let mut backup = BackupData::new(wallet_address);

    // API keys
    if let Ok(keys) = db.list_api_keys_with_values() {
        backup.api_keys = keys
            .iter()
            .map(|(name, value)| ApiKeyEntry {
                key_name: name.clone(),
                key_value: value.clone(),
            })
            .collect();
    }

    // Mind map nodes
    if let Ok(nodes) = db.list_mind_nodes() {
        backup.mind_map_nodes = nodes
            .iter()
            .map(|n| MindNodeEntry {
                id: n.id,
                body: n.body.clone(),
                position_x: n.position_x,
                position_y: n.position_y,
                is_trunk: n.is_trunk,
                created_at: n.created_at.to_rfc3339(),
                updated_at: n.updated_at.to_rfc3339(),
            })
            .collect();
    }

    // Mind map connections
    if let Ok(connections) = db.list_mind_node_connections() {
        backup.mind_map_connections = connections
            .iter()
            .map(|c| MindConnectionEntry {
                parent_id: c.parent_id,
                child_id: c.child_id,
            })
            .collect();
    }

    // Bot settings
    if let Ok(settings) = db.get_bot_settings() {
        let custom_rpc_json = settings
            .custom_rpc_endpoints
            .as_ref()
            .and_then(|h| serde_json::to_string(h).ok());

        backup.bot_settings = Some(BotSettingsEntry {
            bot_name: settings.bot_name.clone(),
            bot_email: settings.bot_email.clone(),
            web3_tx_requires_confirmation: settings.web3_tx_requires_confirmation,
            rpc_provider: Some(settings.rpc_provider.clone()),
            custom_rpc_endpoints: custom_rpc_json,
            max_tool_iterations: Some(settings.max_tool_iterations),
            rogue_mode_enabled: settings.rogue_mode_enabled,
            safe_mode_max_queries_per_10min: Some(settings.safe_mode_max_queries_per_10min),
            guest_dashboard_enabled: settings.guest_dashboard_enabled,
            theme_accent: settings.theme_accent.clone(),
        });
    }

    // Cron jobs
    if let Ok(jobs) = db.list_cron_jobs() {
        backup.cron_jobs = jobs
            .iter()
            .map(|j| CronJobEntry {
                name: j.name.clone(),
                description: j.description.clone(),
                schedule_type: j.schedule_type.clone(),
                schedule_value: j.schedule_value.clone(),
                timezone: j.timezone.clone(),
                session_mode: j.session_mode.clone(),
                message: j.message.clone(),
                system_event: j.system_event.clone(),
                channel_id: j.channel_id,
                deliver_to: j.deliver_to.clone(),
                deliver: j.deliver,
                model_override: j.model_override.clone(),
                thinking_level: j.thinking_level.clone(),
                timeout_seconds: j.timeout_seconds,
                delete_after_run: j.delete_after_run,
                status: j.status.clone(),
            })
            .collect();
    }

    // Heartbeat config
    if let Ok(configs) = db.list_heartbeat_configs() {
        if let Some(config) = configs.into_iter().next() {
            backup.heartbeat_config = Some(HeartbeatConfigEntry {
                channel_id: config.channel_id,
                interval_minutes: config.interval_minutes,
                target: config.target.clone(),
                active_hours_start: config.active_hours_start.clone(),
                active_hours_end: config.active_hours_end.clone(),
                active_days: config.active_days.clone(),
                enabled: config.enabled,
            });
        }
    }

    // Channel settings
    if let Ok(settings) = db.get_all_channel_settings() {
        backup.channel_settings = settings
            .iter()
            .map(|s| ChannelSettingEntry {
                channel_id: s.channel_id,
                setting_key: s.setting_key.clone(),
                setting_value: s.setting_value.clone(),
            })
            .collect();
    }

    // Channels (non-safe-mode only)
    if let Ok(channels) = db.list_channels_for_backup() {
        backup.channels = channels
            .iter()
            .map(|c| ChannelEntry {
                id: c.id,
                channel_type: c.channel_type.clone(),
                name: c.name.clone(),
                enabled: c.enabled,
                bot_token: c.bot_token.clone(),
                app_token: c.app_token.clone(),
            })
            .collect();
    }

    // Soul document
    let soul_path = crate::config::soul_document_path();
    if let Ok(content) = std::fs::read_to_string(&soul_path) {
        backup.soul_document = Some(content);
    }

    // Agent identity
    if let Some(row) = db.get_agent_identity_full() {
        backup.agent_identity = Some(AgentIdentityEntry {
            agent_id: row.agent_id,
            agent_registry: row.agent_registry,
            chain_id: row.chain_id,
            name: row.name,
            description: row.description,
            image: row.image,
            x402_support: row.x402_support,
            active: row.active,
            services_json: row.services_json,
            supported_trust_json: row.supported_trust_json,
            registration_uri: row.registration_uri,
        });
    }

    // Module data
    {
        let module_registry = crate::modules::ModuleRegistry::new();
        let installed = db.list_installed_modules().unwrap_or_default();
        for entry in &installed {
            if let Some(module) = module_registry.get(&entry.module_name) {
                if let Some(data) = module.backup_data(db).await {
                    backup.module_data.insert(entry.module_name.clone(), data);
                }
            }
        }
    }

    // Skills
    if let Ok(skills) = db.list_skills() {
        for skill in skills {
            let skill_id = skill.id.unwrap_or(0);
            let scripts = db
                .get_skill_scripts(skill_id)
                .unwrap_or_default()
                .into_iter()
                .map(|s| SkillScriptEntry {
                    name: s.name,
                    code: s.code,
                    language: s.language,
                })
                .collect();

            backup.skills.push(SkillEntry {
                name: skill.name,
                description: skill.description,
                body: skill.body,
                version: skill.version,
                author: skill.author,
                homepage: skill.homepage,
                metadata: skill.metadata,
                enabled: skill.enabled,
                requires_tools: skill.requires_tools.clone(),
                requires_binaries: skill.requires_binaries.clone(),
                arguments: serde_json::to_string(&skill.arguments).unwrap_or_default(),
                tags: skill.tags,
                subagent_type: skill.subagent_type,
                requires_api_keys: serde_json::to_string(&skill.requires_api_keys)
                    .unwrap_or_default(),
                scripts,
            });
        }
    }

    // Agent settings
    if let Ok(settings) = db.list_agent_settings() {
        backup.agent_settings = settings
            .iter()
            .map(|s| AgentSettingsEntry {
                endpoint: s.endpoint.clone(),
                model_archetype: s.model_archetype.clone(),
                model: s.model.clone(),
                max_response_tokens: s.max_response_tokens,
                max_context_tokens: s.max_context_tokens,
                enabled: s.enabled,
                secret_key: s.secret_key.clone(),
            })
            .collect();
    }

    // x402 payment limits
    if let Ok(limits) = db.get_all_x402_payment_limits() {
        backup.x402_payment_limits = limits
            .iter()
            .map(|l| X402PaymentLimitEntry {
                asset: l.asset.clone(),
                max_amount: l.max_amount.clone(),
                decimals: l.decimals,
                display_name: l.display_name.clone(),
                address: l.address.clone(),
            })
            .collect();
    }

    // Kanban items
    if let Ok(items) = db.list_kanban_items() {
        backup.kanban_items = items
            .iter()
            .map(|i| KanbanItemEntry {
                id: i.id,
                title: i.title.clone(),
                description: i.description.clone(),
                status: i.status.clone(),
                priority: i.priority,
                session_id: i.session_id,
                result: i.result.clone(),
                created_at: i.created_at.to_rfc3339(),
                updated_at: i.updated_at.to_rfc3339(),
            })
            .collect();
    }

    // Agent subtypes
    if let Ok(subtypes) = db.list_agent_subtypes() {
        backup.agent_subtypes = subtypes
            .iter()
            .map(|s| AgentSubtypeEntry {
                key: s.key.clone(),
                label: s.label.clone(),
                emoji: s.emoji.clone(),
                description: s.description.clone(),
                tool_groups_json: serde_json::to_string(&s.tool_groups).unwrap_or_else(|_| "[]".to_string()),
                skill_tags_json: serde_json::to_string(&s.skill_tags).unwrap_or_else(|_| "[]".to_string()),
                additional_tools_json: serde_json::to_string(&s.additional_tools).unwrap_or_else(|_| "[]".to_string()),
                prompt: s.prompt.clone(),
                sort_order: s.sort_order,
                enabled: s.enabled,
                max_iterations: Some(s.max_iterations),
                skip_task_planner: Some(s.skip_task_planner),
                aliases_json: serde_json::to_string(&s.aliases).unwrap_or_else(|_| "[]".to_string()),
            })
            .collect();
    }

    // Special roles
    if let Ok(roles) = db.list_special_roles() {
        backup.special_roles = roles
            .iter()
            .map(|r| SpecialRoleEntry {
                name: r.name.clone(),
                allowed_tools_json: serde_json::to_string(&r.allowed_tools).unwrap_or_else(|_| "[]".to_string()),
                allowed_skills_json: serde_json::to_string(&r.allowed_skills).unwrap_or_else(|_| "[]".to_string()),
                description: r.description.clone(),
            })
            .collect();
    }

    // Special role assignments
    if let Ok(assignments) = db.list_special_role_assignments(None) {
        backup.special_role_assignments = assignments
            .iter()
            .map(|a| SpecialRoleAssignmentEntry {
                channel_type: a.channel_type.clone(),
                user_id: a.user_id.clone(),
                special_role_name: a.special_role_name.clone(),
                label: a.label.clone(),
            })
            .collect();
    }

    // Tool config directories (gog CLI tokens, etc.)
    collect_tool_configs(&mut backup);

    backup
}

/// Collect config files from tool directories that need to persist across restarts.
///
/// Currently supports:
/// - `gogcli`: ~/.config/gogcli/ (Google Workspace CLI auth tokens)
fn collect_tool_configs(backup: &mut BackupData) {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    let home = match std::env::var("HOME") {
        Ok(h) => std::path::PathBuf::from(h),
        Err(_) => return,
    };

    // gogcli: ~/.config/gogcli/
    let gogcli_dir = home.join(".config").join("gogcli");
    if gogcli_dir.exists() {
        let mut files = Vec::new();
        if let Ok(entries) = collect_dir_files_recursive(&gogcli_dir, &gogcli_dir) {
            for (rel_path, content) in entries {
                files.push(ToolConfigFileEntry {
                    relative_path: rel_path,
                    content_b64: engine.encode(&content),
                });
            }
        }
        if !files.is_empty() {
            log::info!("[Backup] Collected {} gogcli config files", files.len());
            backup.tool_configs.insert("gogcli".to_string(), files);
        }
    }
}

/// Recursively collect all files in a directory, returning (relative_path, contents) pairs.
fn collect_dir_files_recursive(
    base: &std::path::Path,
    dir: &std::path::Path,
) -> Result<Vec<(String, Vec<u8>)>, std::io::Error> {
    let mut results = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            results.extend(collect_dir_files_recursive(base, &path)?);
        } else if path.is_file() {
            // Skip files larger than 1MB (safety limit for backup payload)
            if let Ok(metadata) = path.metadata() {
                if metadata.len() > 1_048_576 {
                    log::warn!("[Backup] Skipping large tool config file: {} ({} bytes)", path.display(), metadata.len());
                    continue;
                }
            }
            if let Ok(content) = std::fs::read(&path) {
                let rel = path.strip_prefix(base)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !rel.is_empty() {
                    results.push((rel, content));
                }
            }
        }
    }
    Ok(results)
}

/// Encrypt data using ECIES with the public key derived from private key.
///
/// Used for encrypting backup data before storing on the keystore server.
pub fn encrypt_with_private_key(private_key: &str, data: &str) -> Result<String, String> {
    use ecies::{encrypt, PublicKey, SecretKey};

    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex).map_err(|e| format!("Invalid private key hex: {}", e))?;

    let secret_key =
        SecretKey::parse_slice(&pk_bytes).map_err(|e| format!("Invalid private key: {:?}", e))?;
    let public_key = PublicKey::from_secret_key(&secret_key);

    let encrypted = encrypt(&public_key.serialize(), data.as_bytes())
        .map_err(|e| format!("Encryption failed: {:?}", e))?;

    Ok(hex::encode(encrypted))
}

/// Decrypt data using ECIES with the private key.
///
/// Used for decrypting backup data retrieved from the keystore server.
pub fn decrypt_with_private_key(private_key: &str, encrypted_hex: &str) -> Result<String, String> {
    use ecies::{decrypt, SecretKey};

    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex).map_err(|e| format!("Invalid private key hex: {}", e))?;

    let encrypted =
        hex::decode(encrypted_hex).map_err(|e| format!("Invalid encrypted data: {}", e))?;

    let secret_key =
        SecretKey::parse_slice(&pk_bytes).map_err(|e| format!("Invalid private key: {:?}", e))?;

    let decrypted = decrypt(&secret_key.serialize(), &encrypted)
        .map_err(|e| format!("Decryption failed: {:?}", e))?;

    String::from_utf8(decrypted).map_err(|e| format!("Invalid UTF-8 in decrypted data: {}", e))
}
