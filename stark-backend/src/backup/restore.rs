//! Unified cloud backup restore logic.
//!
//! Both the startup auto-restore (`main.rs`) and the manual API endpoint
//! (`controllers/api_keys.rs`) call [`restore_all`] so every resource type
//! is handled consistently.

use std::collections::HashMap;
use std::sync::Arc;

use crate::backup::BackupData;
use crate::channels::ChannelManager;
use crate::db::Database;
use crate::notes::store::NoteStore;
use crate::skills::SkillRegistry;

/// Counts of each resource type restored.
#[derive(Default)]
pub struct RestoreResult {
    pub api_keys: usize,
    pub impulse_nodes: usize,
    pub impulse_connections: usize,
    pub channels: usize,
    pub channel_settings: usize,
    pub cron_jobs: usize,
    pub skills: usize,
    pub agent_settings: usize,
    pub agent_subtypes: usize,
    pub modules: usize,
    pub special_roles: usize,
    pub special_role_assignments: usize,
    pub x402_limits: usize,
    pub memories: usize,
    pub notes: usize,
    pub kanban_items: usize,
    pub bot_settings: bool,
    pub heartbeat_config: bool,
    pub soul_document: bool,
    pub agent_identity: bool,
}

impl RestoreResult {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.api_keys > 0 { parts.push(format!("{} API keys", self.api_keys)); }
        if self.channels > 0 { parts.push(format!("{} channels", self.channels)); }
        if self.skills > 0 { parts.push(format!("{} skills", self.skills)); }
        if self.cron_jobs > 0 { parts.push(format!("{} cron jobs", self.cron_jobs)); }
        if self.agent_settings > 0 { parts.push(format!("{} agent settings", self.agent_settings)); }
        if self.agent_subtypes > 0 { parts.push(format!("{} agent subtypes", self.agent_subtypes)); }
        if self.modules > 0 { parts.push(format!("{} modules", self.modules)); }
        if self.impulse_nodes > 0 { parts.push(format!("{} impulse map nodes", self.impulse_nodes)); }
        if self.impulse_connections > 0 { parts.push(format!("{} impulse connections", self.impulse_connections)); }
        if self.channel_settings > 0 { parts.push(format!("{} channel settings", self.channel_settings)); }
        if self.special_roles > 0 { parts.push(format!("{} special roles", self.special_roles)); }
        if self.special_role_assignments > 0 { parts.push(format!("{} role assignments", self.special_role_assignments)); }
        if self.x402_limits > 0 { parts.push(format!("{} x402 payment limits", self.x402_limits)); }
        if self.memories > 0 { parts.push(format!("{} memories", self.memories)); }
        if self.notes > 0 { parts.push(format!("{} notes", self.notes)); }
        if self.kanban_items > 0 { parts.push(format!("{} kanban items", self.kanban_items)); }
        if self.bot_settings { parts.push("bot settings".to_string()); }
        if self.heartbeat_config { parts.push("heartbeat config".to_string()); }
        if self.soul_document { parts.push("soul document".to_string()); }
        if self.agent_identity { parts.push("agent identity".to_string()); }

        if parts.is_empty() {
            "No data restored".to_string()
        } else {
            format!("Restored {}", parts.join(", "))
        }
    }
}

/// Restore everything from a [`BackupData`] payload.
///
/// Optional components (`skill_registry`, `channel_manager`, `notes_store`)
/// control post-restore actions:
/// - `skill_registry` → reload DB, set enabled state
/// - `channel_manager` → auto-start channels with `auto_start_on_boot`
/// - `notes_store` → FTS reindex after writing note files
pub async fn restore_all(
    db: &Arc<Database>,
    backup_data: &mut BackupData,
    skill_registry: Option<&Arc<SkillRegistry>>,
    channel_manager: Option<&Arc<ChannelManager>>,
    notes_store: Option<&Arc<NoteStore>>,
) -> Result<RestoreResult, String> {
    let mut result = RestoreResult::default();

    log::info!(
        "[Restore] Restoring backup v{} with {} items from {}",
        backup_data.version,
        backup_data.item_count(),
        backup_data.created_at.format("%Y-%m-%d %H:%M:%S")
    );

    // ── 1. API keys ─────────────────────────────────────────────────────
    for key in &backup_data.api_keys {
        if let Err(e) = db.upsert_api_key(&key.key_name, &key.key_value) {
            log::warn!("[Restore] Failed to restore key {}: {}", key.key_name, e);
        } else {
            result.api_keys += 1;
        }
    }
    if result.api_keys > 0 {
        log::info!("[Restore] Restored {} API keys", result.api_keys);
    }

    // ── 2. Impulse map ──────────────────────────────────────────────────
    // Clear existing nodes/connections
    match db.clear_impulse_nodes_for_restore() {
        Ok((nodes_deleted, connections_deleted)) => {
            if nodes_deleted > 0 || connections_deleted > 0 {
                log::info!("[Restore] Cleared {} nodes and {} connections for restore", nodes_deleted, connections_deleted);
            }
        }
        Err(e) => log::warn!("[Restore] Failed to clear impulse nodes for restore: {}", e),
    }

    // ID mapping for connections
    let mut old_to_new_id: HashMap<i64, i64> = HashMap::new();

    // Map trunk node
    let current_trunk = db.get_or_create_trunk_node().ok();
    if let Some(ref trunk) = current_trunk {
        for node in &backup_data.impulse_map_nodes {
            if node.is_trunk {
                old_to_new_id.insert(node.id, trunk.id);
                if !node.body.is_empty() {
                    let _ = db.update_impulse_node(trunk.id, &crate::db::tables::impulse_nodes::UpdateImpulseNodeRequest {
                        body: Some(node.body.clone()),
                        position_x: node.position_x,
                        position_y: node.position_y,
                    });
                }
                break;
            }
        }
    }

    // Create non-trunk nodes
    for node in &backup_data.impulse_map_nodes {
        if node.is_trunk { continue; }
        let request = crate::db::tables::impulse_nodes::CreateImpulseNodeRequest {
            body: Some(node.body.clone()),
            position_x: node.position_x,
            position_y: node.position_y,
            parent_id: None,
        };
        match db.create_impulse_node(&request) {
            Ok(new_node) => {
                old_to_new_id.insert(node.id, new_node.id);
                result.impulse_nodes += 1;
            }
            Err(e) => log::warn!("[Restore] Failed to restore impulse node: {}", e),
        }
    }
    if result.impulse_nodes > 0 {
        log::info!("[Restore] Restored {} impulse map nodes", result.impulse_nodes);
    }

    // Connections
    for conn in &backup_data.impulse_map_connections {
        if let (Some(&parent_id), Some(&child_id)) = (
            old_to_new_id.get(&conn.parent_id),
            old_to_new_id.get(&conn.child_id),
        ) {
            match db.create_impulse_node_connection(parent_id, child_id) {
                Ok(_) => result.impulse_connections += 1,
                Err(e) => {
                    if !e.to_string().contains("UNIQUE constraint") {
                        log::warn!("[Restore] Failed to restore connection: {}", e);
                    }
                }
            }
        }
    }
    if result.impulse_connections > 0 {
        log::info!("[Restore] Restored {} impulse map connections", result.impulse_connections);
    }

    // ── 3. Bot settings ─────────────────────────────────────────────────
    if let Some(settings) = &backup_data.bot_settings {
        let custom_rpc: Option<HashMap<String, String>> =
            settings.custom_rpc_endpoints.as_ref().and_then(|s| serde_json::from_str(s).ok());

        match db.update_bot_settings_full(
            Some(&settings.bot_name),
            Some(&settings.bot_email),
            Some(settings.web3_tx_requires_confirmation),
            settings.rpc_provider.as_deref(),
            custom_rpc.as_ref(),
            settings.max_tool_iterations,
            Some(settings.rogue_mode_enabled),
            settings.safe_mode_max_queries_per_10min,
            None, // Don't restore keystore_url - it's infrastructure config
            None,
            Some(settings.guest_dashboard_enabled),
            settings.theme_accent.as_deref(),
            None, // Don't restore proxy_url - it's infrastructure config
            None, // Don't restore kanban_auto_execute - keep current setting
            settings.whisper_server_url.as_deref(),
            settings.embeddings_server_url.as_deref(),
        ) {
            Ok(_) => { result.bot_settings = true; log::info!("[Restore] Restored bot settings"); }
            Err(e) => log::warn!("[Restore] Failed to restore bot settings: {}", e),
        }
    }

    // ── 4. Channels ─────────────────────────────────────────────────────
    // Clear existing channels/settings/cron jobs first (API endpoint path does this)
    let _ = db.clear_channel_settings_for_restore();
    let _ = db.clear_channels_for_restore();
    let _ = db.clear_cron_jobs_for_restore();

    let mut old_channel_to_new_id: HashMap<i64, i64> = HashMap::new();
    for channel in &backup_data.channels {
        match db.create_channel(&channel.channel_type, &channel.name, &channel.bot_token, channel.app_token.as_deref()) {
            Ok(new_channel) => {
                old_channel_to_new_id.insert(channel.id, new_channel.id);
                // Restore enabled state
                if channel.enabled {
                    let _ = db.set_channel_enabled(new_channel.id, true);
                }
                // Migrate legacy bot_token → channel setting
                if !channel.bot_token.is_empty() {
                    let setting_key = match channel.channel_type.as_str() {
                        "discord" => Some("discord_bot_token"),
                        "telegram" => Some("telegram_bot_token"),
                        "slack" => Some("slack_bot_token"),
                        _ => None,
                    };
                    if let Some(key) = setting_key {
                        let _ = db.set_channel_setting(new_channel.id, key, &channel.bot_token);
                    }
                }
                // Migrate legacy app_token → channel setting
                if let Some(ref app_token) = channel.app_token {
                    if !app_token.is_empty() && channel.channel_type == "slack" {
                        let _ = db.set_channel_setting(new_channel.id, "slack_app_token", app_token);
                    }
                }
                result.channels += 1;
            }
            Err(e) => {
                // Channel might already exist — map to existing
                if let Ok(existing) = db.list_channels() {
                    if let Some(found) = existing.iter().find(|c| c.name == channel.name && c.channel_type == channel.channel_type) {
                        old_channel_to_new_id.insert(channel.id, found.id);
                        log::debug!("[Restore] Channel {} already exists, mapping to existing", channel.name);
                    } else {
                        log::warn!("[Restore] Failed to restore channel {}: {}", channel.name, e);
                    }
                }
            }
        }
    }
    if result.channels > 0 {
        log::info!("[Restore] Restored {} channels", result.channels);
    }

    // ── 5. Channel settings ─────────────────────────────────────────────
    for setting in &backup_data.channel_settings {
        if let Some(&new_channel_id) = old_channel_to_new_id.get(&setting.channel_id) {
            match db.set_channel_setting(new_channel_id, &setting.setting_key, &setting.setting_value) {
                Ok(_) => result.channel_settings += 1,
                Err(e) => log::warn!("[Restore] Failed to restore channel setting: {}", e),
            }
        }
    }
    if result.channel_settings > 0 {
        log::info!("[Restore] Restored {} channel settings", result.channel_settings);
    }

    // ── 6. Cron jobs ────────────────────────────────────────────────────
    for job in &backup_data.cron_jobs {
        let mapped_channel_id = job.channel_id.and_then(|old_id| old_channel_to_new_id.get(&old_id).copied());
        match db.create_cron_job(
            &job.name,
            job.description.as_deref(),
            &job.schedule_type,
            &job.schedule_value,
            job.timezone.as_deref(),
            &job.session_mode,
            job.message.as_deref(),
            job.system_event.as_deref(),
            mapped_channel_id,
            job.deliver_to.as_deref(),
            job.deliver,
            job.model_override.as_deref(),
            job.thinking_level.as_deref(),
            job.timeout_seconds,
            job.delete_after_run,
        ) {
            Ok(_) => result.cron_jobs += 1,
            Err(e) => log::warn!("[Restore] Failed to restore cron job {}: {}", job.name, e),
        }
    }
    if result.cron_jobs > 0 {
        log::info!("[Restore] Restored {} cron jobs", result.cron_jobs);
    }

    // ── 7. Heartbeat config ─────────────────────────────────────────────
    if let Some(hb_config) = &backup_data.heartbeat_config {
        let mapped_channel_id = hb_config.channel_id.and_then(|old_id| old_channel_to_new_id.get(&old_id).copied());
        match db.get_or_create_heartbeat_config(mapped_channel_id) {
            Ok(existing) => {
                if let Err(e) = db.update_heartbeat_config(
                    existing.id,
                    Some(hb_config.interval_minutes),
                    Some(&hb_config.target),
                    hb_config.active_hours_start.as_deref(),
                    hb_config.active_hours_end.as_deref(),
                    hb_config.active_days.as_deref(),
                    Some(hb_config.enabled),
                ) {
                    log::warn!("[Restore] Failed to restore heartbeat config: {}", e);
                } else {
                    result.heartbeat_config = true;
                    log::info!("[Restore] Restored heartbeat config (enabled={})", hb_config.enabled);
                }
            }
            Err(e) => log::warn!("[Restore] Failed to create heartbeat config for restore: {}", e),
        }
    }

    // ── 8. Soul document ────────────────────────────────────────────────
    if let Some(soul_content) = &backup_data.soul_document {
        let soul_path = crate::config::soul_document_path();
        if let Some(parent) = soul_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&soul_path, soul_content) {
            Ok(_) => { result.soul_document = true; log::info!("[Restore] Restored soul document from backup (overrides template)"); }
            Err(e) => log::warn!("[Restore] Failed to restore soul document: {}", e),
        }
    }

    // ── 9. Agent identity ───────────────────────────────────────────────
    if let Some(ref ai) = backup_data.agent_identity {
        let conn = db.conn();
        let existing: i64 = conn
            .query_row("SELECT COUNT(*) FROM agent_identity", [], |r| r.get(0))
            .unwrap_or(0);
        if existing == 0 {
            match db.upsert_agent_identity(
                ai.agent_id,
                &ai.agent_registry,
                ai.chain_id,
                ai.name.as_deref(),
                ai.description.as_deref(),
                ai.image.as_deref(),
                ai.x402_support,
                ai.active,
                &ai.services_json,
                &ai.supported_trust_json,
                ai.registration_uri.as_deref(),
            ) {
                Ok(_) => {
                    result.agent_identity = true;
                    log::info!("[Restore] Restored agent identity (agent_id={}) from backup", ai.agent_id);
                }
                Err(e) => log::warn!("[Restore] Failed to restore agent identity: {}", e),
            }
        } else {
            result.agent_identity = true;
            log::info!("[Restore] Agent identity already exists locally, skipping restore from backup");
        }
    }

    // Legacy: identity_document → DB migration
    if !result.agent_identity {
        if let Some(identity_content) = &backup_data.identity_document {
            let existing: i64 = db.conn()
                .query_row("SELECT COUNT(*) FROM agent_identity", [], |r| r.get(0))
                .unwrap_or(0);
            if existing == 0 {
                if let Ok(reg) = serde_json::from_str::<crate::eip8004::types::RegistrationFile>(identity_content) {
                    let services_json = serde_json::to_string(&reg.services).unwrap_or_else(|_| "[]".to_string());
                    let supported_trust_json = serde_json::to_string(&reg.supported_trust).unwrap_or_else(|_| "[]".to_string());
                    match db.upsert_agent_identity(
                        0, "", 0,
                        Some(&reg.name), Some(&reg.description), reg.image.as_deref(),
                        reg.x402_support, reg.active,
                        &services_json, &supported_trust_json,
                        None,
                    ) {
                        Ok(_) => {
                            result.agent_identity = true;
                            log::info!("[Restore] Migrated legacy identity_document to DB");
                        }
                        Err(e) => log::warn!("[Restore] Failed to migrate legacy identity_document: {}", e),
                    }
                }
            }
        }
    }

    // ── 10. x402 payment limits ─────────────────────────────────────────
    for limit in &backup_data.x402_payment_limits {
        match db.set_x402_payment_limit(&limit.asset, &limit.max_amount, limit.decimals, &limit.display_name, limit.address.as_deref()) {
            Ok(_) => {
                crate::x402::payment_limits::set_limit(&limit.asset, &limit.max_amount, limit.decimals, &limit.display_name, limit.address.as_deref());
                result.x402_limits += 1;
            }
            Err(e) => log::warn!("[Restore] Failed to restore x402 payment limit for {}: {}", limit.asset, e),
        }
    }
    if result.x402_limits > 0 {
        log::info!("[Restore] Restored {} x402 payment limits", result.x402_limits);
    }

    // ── 11. Module data (generic module.restore_data()) ─────────────────
    {
        let module_registry = crate::modules::ModuleRegistry::new();

        // Backward-compat shim: legacy discord_registrations → module_data
        if !backup_data.discord_registrations.is_empty() && !backup_data.module_data.contains_key("discord_tipping") {
            log::info!("[Restore] Converting legacy discord_registrations to module_data format");
            let legacy_entries: Vec<serde_json::Value> = backup_data.discord_registrations.iter().map(|reg| {
                serde_json::json!({
                    "discord_user_id": reg.discord_user_id,
                    "discord_username": reg.discord_username,
                    "public_address": reg.public_address,
                    "registered_at": reg.registered_at,
                })
            }).collect();
            backup_data.module_data.insert("discord_tipping".to_string(), serde_json::Value::Array(legacy_entries));
        }

        for (module_name, data) in &backup_data.module_data {
            if let Some(module) = module_registry.get(module_name) {
                if !db.is_module_installed(module_name).unwrap_or(true) {
                    let _ = db.install_module(
                        module_name,
                        module.description(),
                        module.version(),
                        module.has_tools(),
                        module.has_dashboard(),
                    );
                }
                match module.restore_data(db, data).await {
                    Ok(()) => log::info!("[Restore] Restored module data for '{}'", module_name),
                    Err(e) => log::warn!("[Restore] Failed to restore module data for '{}': {}", module_name, e),
                }
            } else {
                log::warn!("[Restore] Skipping restore for unknown module '{}'", module_name);
            }
        }
    }

    // ── 12. Modules (folder files → disk) ───────────────────────────────
    if !backup_data.modules.is_empty() {
        let runtime_modules_dir = crate::config::runtime_modules_dir();
        std::fs::create_dir_all(&runtime_modules_dir).ok();

        for module_entry in &backup_data.modules {
            if module_entry.folder_files.is_empty() { continue; }
            let module_dir = runtime_modules_dir.join(&module_entry.name);

            // Semver check: only overwrite if backup version is newer (or module missing)
            if module_dir.exists() {
                let local_version = crate::config::extract_version_from_module_toml_pub(&module_dir);
                if let Some(ref local_v) = local_version {
                    if !module_entry.version.is_empty() && !crate::config::semver_is_newer(&module_entry.version, local_v) {
                        log::info!(
                            "[Restore] Skipping module '{}': local v{} >= backup v{}",
                            module_entry.name, local_v, module_entry.version
                        );
                        continue;
                    }
                    log::info!(
                        "[Restore] Upgrading module '{}' from v{} to v{} (backup is newer)",
                        module_entry.name, local_v, module_entry.version
                    );
                }
            } else {
                log::info!("[Restore] Restoring module '{}' v{} from backup", module_entry.name, module_entry.version);
            }

            for file_entry in &module_entry.folder_files {
                if file_entry.relative_path.contains("..") || file_entry.relative_path.contains('\0')
                    || file_entry.relative_path.starts_with('/') || file_entry.relative_path.starts_with('\\') {
                    log::warn!("[Restore] Skipping module file with unsafe path: {}", file_entry.relative_path);
                    continue;
                }
                let file_path = module_dir.join(&file_entry.relative_path);
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                if let Err(e) = std::fs::write(&file_path, &file_entry.content) {
                    log::warn!("[Restore] Failed to write module file {}/{}: {}", module_entry.name, file_entry.relative_path, e);
                }
            }
            result.modules += 1;
        }

        // Re-seed bundled modules (newer bundled versions take precedence)
        if let Err(e) = crate::config::seed_modules() {
            log::warn!("[Restore] Failed to re-seed bundled modules after restore: {}", e);
        }

        // Auto-install restored modules
        let module_registry = crate::modules::ModuleRegistry::new();
        for module_entry in &backup_data.modules {
            if let Some(module) = module_registry.get(&module_entry.name) {
                if !db.is_module_installed(&module_entry.name).unwrap_or(true) {
                    let _ = db.install_module(
                        &module_entry.name,
                        module.description(),
                        module.version(),
                        module.has_tools(),
                        module.has_dashboard(),
                    );
                    log::info!("[Restore] Auto-installed restored module '{}'", module_entry.name);
                }
                let _ = db.set_module_enabled(&module_entry.name, module_entry.enabled);
            }
        }

        if result.modules > 0 {
            log::info!("[Restore] Restored {} modules from backup", result.modules);
        }
    }

    // ── 13. Skills (folder files → disk) ────────────────────────────────
    {
        let runtime_skills_dir = std::path::PathBuf::from(crate::config::runtime_skills_dir());
        std::fs::create_dir_all(&runtime_skills_dir).ok();

        for skill_entry in &backup_data.skills {
            if !skill_entry.folder_files.is_empty() {
                // New folder-based format
                let skill_dir = runtime_skills_dir.join(&skill_entry.name);
                for file_entry in &skill_entry.folder_files {
                    if file_entry.relative_path.contains("..") || file_entry.relative_path.contains('\0')
                        || file_entry.relative_path.starts_with('/') || file_entry.relative_path.starts_with('\\') {
                        log::warn!("[Restore] Skipping skill file with unsafe path: {}", file_entry.relative_path);
                        continue;
                    }
                    let file_path = skill_dir.join(&file_entry.relative_path);
                    if let Some(parent) = file_path.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    if let Err(e) = std::fs::write(&file_path, &file_entry.content) {
                        log::warn!("[Restore] Failed to write skill file {}/{}: {}", skill_entry.name, file_entry.relative_path, e);
                    }
                }
                result.skills += 1;
            } else {
                // Legacy format: reconstruct folder
                let arguments: HashMap<String, crate::skills::types::SkillArgument> =
                    serde_json::from_str(&skill_entry.arguments).unwrap_or_default();
                let requires_api_keys: HashMap<String, crate::skills::types::SkillApiKey> =
                    serde_json::from_str(&skill_entry.requires_api_keys).unwrap_or_default();

                let parsed = crate::skills::ParsedSkill {
                    name: skill_entry.name.clone(),
                    description: skill_entry.description.clone(),
                    body: skill_entry.body.clone(),
                    version: skill_entry.version.clone(),
                    author: skill_entry.author.clone(),
                    homepage: skill_entry.homepage.clone(),
                    metadata: skill_entry.metadata.clone(),
                    requires_tools: skill_entry.requires_tools.clone(),
                    requires_binaries: skill_entry.requires_binaries.clone(),
                    arguments,
                    tags: skill_entry.tags.clone(),
                    subagent_type: skill_entry.subagent_type.clone(),
                    requires_api_keys,
                    scripts: skill_entry.scripts.iter().map(|s| crate::skills::ParsedScript {
                        name: s.name.clone(),
                        code: s.code.clone(),
                        language: s.language.clone(),
                    }).collect(),
                    abis: skill_entry.abis.iter().map(|a| crate::skills::ParsedAbi {
                        name: a.name.clone(),
                        content: a.content.clone(),
                    }).collect(),
                    presets_content: skill_entry.presets_content.clone(),
                    flows: skill_entry.flows.iter().map(|f| crate::skills::ParsedFlow {
                        name: f.name.clone(),
                        content: f.content.clone(),
                    }).collect(),
                };

                match crate::skills::write_skill_folder(&runtime_skills_dir, &parsed) {
                    Ok(()) => result.skills += 1,
                    Err(e) => log::warn!("[Restore] Failed to restore skill folder '{}': {}", skill_entry.name, e),
                }
            }
        }

        // Re-seed bundled skills (newer bundled versions take precedence)
        if let Err(e) = crate::config::seed_skills() {
            log::warn!("[Restore] Failed to re-seed skills after restore: {}", e);
        }

        // Reload skill registry if available (syncs disk → DB)
        if let Some(registry) = skill_registry {
            if result.skills > 0 {
                match registry.reload().await {
                    Ok(count) => log::info!("[Restore] Synced {} skills from disk after restore (ABIs/presets reloaded)", count),
                    Err(e) => log::warn!("[Restore] Failed to sync skills after restore: {}", e),
                }
            }
            // Restore enabled/disabled state from backup
            for skill_entry in &backup_data.skills {
                registry.set_enabled(&skill_entry.name, skill_entry.enabled);
            }
        }
    }
    if result.skills > 0 {
        log::info!("[Restore] Restored {} skills to disk", result.skills);
    }

    // ── 14. Agent subtypes (folder files → disk) ────────────────────────
    {
        let agents_dir = crate::config::runtime_agents_dir();
        std::fs::create_dir_all(&agents_dir).ok();
        for entry in &backup_data.agent_subtypes {
            if !entry.folder_files.is_empty() {
                let agent_folder = agents_dir.join(&entry.key);
                std::fs::create_dir_all(&agent_folder).ok();
                for file in &entry.folder_files {
                    let file_path = agent_folder.join(&file.relative_path);
                    if let Some(parent) = file_path.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    if let Err(e) = std::fs::write(&file_path, &file.content) {
                        log::warn!("[Restore] Failed to write agent file {}/{}: {}", entry.key, file.relative_path, e);
                    }
                }
                result.agent_subtypes += 1;
            } else {
                // Legacy format: reconstruct from fields
                let tool_groups: Vec<String> = serde_json::from_str(&entry.tool_groups_json).unwrap_or_default();
                let skill_tags: Vec<String> = serde_json::from_str(&entry.skill_tags_json).unwrap_or_default();
                let additional_tools: Vec<String> = serde_json::from_str(&entry.additional_tools_json).unwrap_or_default();
                let aliases: Vec<String> = serde_json::from_str(&entry.aliases_json).unwrap_or_default();
                let config = crate::ai::multi_agent::types::AgentSubtypeConfig {
                    key: entry.key.clone(),
                    version: String::new(),
                    label: entry.label.clone(),
                    emoji: entry.emoji.clone(),
                    description: entry.description.clone(),
                    tool_groups,
                    skill_tags,
                    additional_tools,
                    prompt: entry.prompt.clone(),
                    sort_order: entry.sort_order,
                    enabled: entry.enabled,
                    max_iterations: entry.max_iterations.unwrap_or(90) as u32,
                    skip_task_planner: entry.skip_task_planner.unwrap_or(false),
                    aliases,
                    hidden: entry.hidden.unwrap_or(false),
                    preferred_ai_model: entry.preferred_ai_model.clone(),
                    hooks: Vec::new(),
                };
                match crate::agents::loader::write_agent_folder(&agents_dir, &config) {
                    Ok(_) => result.agent_subtypes += 1,
                    Err(e) => log::warn!("[Restore] Failed to restore agent subtype '{}': {}", entry.key, e),
                }
            }
        }
        if result.agent_subtypes > 0 {
            log::info!("[Restore] Restored {} agent subtypes to disk", result.agent_subtypes);
            crate::agents::loader::reload_registry_from_disk();
        }
    }

    // ── 15. Agent settings ──────────────────────────────────────────────
    if !backup_data.agent_settings.is_empty() {
        if let Err(e) = db.disable_agent_settings() {
            log::warn!("[Restore] Failed to disable existing agent settings for restore: {}", e);
        }
        for entry in &backup_data.agent_settings {
            let payment_mode = if entry.payment_mode.is_empty() { "credits" } else { &entry.payment_mode };
            match db.save_agent_settings(
                entry.endpoint_name.as_deref(),
                &entry.endpoint,
                &entry.model_archetype,
                entry.model.as_deref(),
                entry.max_response_tokens,
                entry.max_context_tokens,
                entry.secret_key.as_deref(),
                payment_mode,
            ) {
                Ok(saved) => {
                    if !entry.enabled {
                        let _ = db.disable_agent_settings();
                    }
                    result.agent_settings += 1;
                    log::info!("[Restore] Restored agent settings: {:?} / {} ({})", saved.endpoint_name, saved.endpoint, saved.model_archetype);
                }
                Err(e) => log::warn!("[Restore] Failed to restore agent settings for {}: {}", entry.endpoint, e),
            }
        }
    }
    if result.agent_settings > 0 {
        log::info!("[Restore] Restored {} agent settings", result.agent_settings);
    }

    // ── 16. Kanban items ────────────────────────────────────────────────
    if !backup_data.kanban_items.is_empty() {
        // Clear existing kanban items
        if let Ok(existing) = db.list_kanban_items() {
            for item in existing {
                let _ = db.delete_kanban_item(item.id);
            }
        }

        for item in &backup_data.kanban_items {
            let request = crate::db::tables::kanban::CreateKanbanItemRequest {
                title: item.title.clone(),
                description: Some(item.description.clone()),
                priority: Some(item.priority),
            };
            match db.create_kanban_item(&request) {
                Ok(new_item) => {
                    let update_req = crate::db::tables::kanban::UpdateKanbanItemRequest {
                        status: Some(item.status.clone()),
                        result: item.result.clone(),
                        ..Default::default()
                    };
                    let _ = db.update_kanban_item(new_item.id, &update_req);
                    result.kanban_items += 1;
                }
                Err(e) => log::warn!("[Restore] Failed to restore kanban item: {}", e),
            }
        }
        if result.kanban_items > 0 {
            log::info!("[Restore] Restored {} kanban board items", result.kanban_items);
        }
    }

    // ── 17. Special roles ───────────────────────────────────────────────
    for entry in &backup_data.special_roles {
        let role = crate::models::SpecialRole {
            name: entry.name.clone(),
            allowed_tools: serde_json::from_str(&entry.allowed_tools_json).unwrap_or_default(),
            allowed_skills: serde_json::from_str(&entry.allowed_skills_json).unwrap_or_default(),
            description: entry.description.clone(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        match db.upsert_special_role(&role) {
            Ok(_) => result.special_roles += 1,
            Err(e) => log::warn!("[Restore] Failed to restore special role '{}': {}", entry.name, e),
        }
    }
    if result.special_roles > 0 {
        log::info!("[Restore] Restored {} special roles", result.special_roles);
    }

    // ── 18. Special role assignments ────────────────────────────────────
    for entry in &backup_data.special_role_assignments {
        match db.create_special_role_assignment(&entry.channel_type, &entry.user_id, &entry.special_role_name, entry.label.as_deref()) {
            Ok(_) => result.special_role_assignments += 1,
            Err(e) => log::warn!(
                "[Restore] Failed to restore special role assignment ({}/{} -> {}): {}",
                entry.channel_type, entry.user_id, entry.special_role_name, e
            ),
        }
    }
    if result.special_role_assignments > 0 {
        log::info!("[Restore] Restored {} special role assignments", result.special_role_assignments);
    }

    // ── 19. Notes ───────────────────────────────────────────────────────
    if !backup_data.notes.is_empty() {
        let notes_dir = std::path::PathBuf::from(crate::config::notes_dir());
        std::fs::create_dir_all(&notes_dir).ok();

        for note in &backup_data.notes {
            if note.relative_path.contains("..") {
                log::warn!("[Restore] Skipping suspicious note path: {}", note.relative_path);
                continue;
            }
            let target = notes_dir.join(&note.relative_path);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&target, &note.content) {
                Ok(_) => result.notes += 1,
                Err(e) => log::warn!("[Restore] Failed to restore note '{}': {}", note.relative_path, e),
            }
        }

        if result.notes > 0 {
            // Reindex FTS if notes_store available
            if let Some(store) = notes_store {
                if let Err(e) = store.reindex() {
                    log::warn!("[Restore] Failed to reindex notes after restore: {}", e);
                }
            }
            log::info!("[Restore] Restored {} notes", result.notes);
        }
    }

    // ── 20. Memories ────────────────────────────────────────────────────
    if let Some(ref memories) = backup_data.memories {
        match db.clear_memories_for_restore() {
            Ok(deleted) => {
                if deleted > 0 {
                    log::info!("[Restore] Cleared {} memories for restore", deleted);
                }
            }
            Err(e) => log::warn!("[Restore] Failed to clear memories for restore: {}", e),
        }

        for mem in memories {
            let insert_result = if !mem.created_at.is_empty() {
                db.insert_memory_with_created_at(
                    &mem.memory_type,
                    &mem.content,
                    mem.category.as_deref(),
                    mem.tags.as_deref(),
                    mem.importance.unwrap_or(5) as i64,
                    mem.identity_id.as_deref(),
                    None,
                    mem.entity_type.as_deref(),
                    mem.entity_name.as_deref(),
                    mem.source_type.as_deref(),
                    mem.log_date.as_deref(),
                    &mem.created_at,
                    mem.agent_subtype.as_deref(),
                )
            } else {
                db.insert_memory(
                    &mem.memory_type,
                    &mem.content,
                    mem.category.as_deref(),
                    mem.tags.as_deref(),
                    mem.importance.unwrap_or(5) as i64,
                    mem.identity_id.as_deref(),
                    None,
                    mem.entity_type.as_deref(),
                    mem.entity_name.as_deref(),
                    mem.source_type.as_deref(),
                    mem.log_date.as_deref(),
                    mem.agent_subtype.as_deref(),
                )
            };
            match insert_result {
                Ok(_) => result.memories += 1,
                Err(e) => log::warn!("[Restore] Failed to restore memory: {}", e),
            }
        }
        if result.memories > 0 {
            log::info!("[Restore] Restored {} memories (embeddings + associations will be recomputed)", result.memories);
            // Rebuild FTS index to ensure it's in sync after bulk restore
            if let Err(e) = db.rebuild_fts_index() {
                log::warn!("[Restore] Failed to rebuild FTS index after memory restore: {}", e);
            } else {
                log::info!("[Restore] FTS index rebuilt successfully after memory restore");
            }
        }
    }

    // ── 21. Tool configs (gogcli etc.) ──────────────────────────────────
    restore_tool_configs(backup_data);

    // ── 23. Auto-start channels ─────────────────────────────────────────
    if let Some(cm) = channel_manager {
        let mut auto_started = 0;
        for (_old_id, &new_id) in &old_channel_to_new_id {
            let should_auto_start = db
                .get_channel_setting(new_id, "auto_start_on_boot")
                .ok()
                .flatten()
                .map(|v| v == "true")
                .unwrap_or(false);

            if should_auto_start {
                if let Ok(Some(channel)) = db.get_channel(new_id) {
                    match cm.start_channel(channel).await {
                        Ok(_) => {
                            auto_started += 1;
                            log::info!("[Restore] Auto-started channel {}", new_id);
                        }
                        Err(e) => log::warn!("[Restore] Failed to auto-start channel {}: {}", new_id, e),
                    }
                }
            }
        }
        if auto_started > 0 {
            log::info!("[Restore] Auto-started {} channels after restore", auto_started);
        }
    } else {
        // No channel manager — just enable channels with auto_start_on_boot so gateway.start_enabled_channels() picks them up
        for &new_channel_id in old_channel_to_new_id.values() {
            let should_auto_start = db
                .get_channel_setting(new_channel_id, "auto_start_on_boot")
                .ok()
                .flatten()
                .map(|v| v == "true")
                .unwrap_or(false);

            if should_auto_start {
                if let Err(e) = db.set_channel_enabled(new_channel_id, true) {
                    log::warn!("[Restore] Failed to enable auto-start channel {}: {}", new_channel_id, e);
                }
            }
        }
    }

    log::info!("[Restore] Restore complete");
    Ok(result)
}

/// Restore tool config directories from backup (e.g. gogcli auth tokens).
fn restore_tool_configs(backup_data: &BackupData) {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    let home = match std::env::var("HOME") {
        Ok(h) => std::path::PathBuf::from(h),
        Err(_) => {
            log::warn!("[Restore] HOME not set, skipping tool config restore");
            return;
        }
    };

    for (tool_name, files) in &backup_data.tool_configs {
        let base_dir = match tool_name.as_str() {
            "gogcli" => home.join(".config").join("gogcli"),
            other => {
                log::warn!("[Restore] Unknown tool config '{}', skipping", other);
                continue;
            }
        };

        let mut restored = 0;
        for file_entry in files {
            if file_entry.relative_path.contains("..") {
                log::warn!("[Restore] Skipping suspicious path in tool config: {}", file_entry.relative_path);
                continue;
            }

            let target = base_dir.join(&file_entry.relative_path);
            if let Some(parent) = target.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    log::warn!("[Restore] Failed to create dir for tool config {}: {}", file_entry.relative_path, e);
                    continue;
                }
            }

            match engine.decode(&file_entry.content_b64) {
                Ok(content) => {
                    match std::fs::write(&target, &content) {
                        Ok(_) => restored += 1,
                        Err(e) => log::warn!("[Restore] Failed to write tool config {}: {}", file_entry.relative_path, e),
                    }
                }
                Err(e) => log::warn!("[Restore] Failed to decode tool config {}: {}", file_entry.relative_path, e),
            }
        }

        if restored > 0 {
            log::info!("[Restore] Restored {} config files for tool '{}'", restored, tool_name);
        }
    }
}
