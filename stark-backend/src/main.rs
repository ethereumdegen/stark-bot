use actix_cors::Cors;
use actix_files::{Files, NamedFile};
use actix_web::{middleware::Logger, web, App, HttpServer};
use dotenv::dotenv;
use std::sync::Arc;

mod ai;
mod ai_endpoint_config;
mod backup;
mod channels;
mod config;
mod context;
mod controllers;
mod db;
mod disk_quota;
mod discord_hooks;
mod domain_types;
mod execution;
mod gateway;
mod integrations;
mod middleware;
mod models;
mod qmd_memory;
mod scheduler;
mod skills;
mod tools;
mod siwa;
mod wallet;
mod x402;
mod erc8128;
mod eip8004;
mod hooks;
pub mod http;
mod tool_validators;
mod tx_queue;
mod web3;
mod keystore_client;
mod identity_client;
mod modules;
mod telemetry;

use channels::{ChannelManager, MessageDispatcher, SafeModeChannelRateLimiter};
use tx_queue::TxQueueManager;
use config::Config;
use db::Database;
use execution::ExecutionTracker;
use gateway::{events::EventBroadcaster, Gateway};
use hooks::HookManager;
use scheduler::{Scheduler, SchedulerConfig};
use skills::SkillRegistry;
use tools::ToolRegistry;
use wallet::WalletProvider;

pub struct AppState {
    pub db: Arc<Database>,
    pub config: Config,
    pub gateway: Arc<Gateway>,
    pub tool_registry: Arc<ToolRegistry>,
    pub skill_registry: Arc<SkillRegistry>,
    pub dispatcher: Arc<MessageDispatcher>,
    pub execution_tracker: Arc<ExecutionTracker>,
    pub scheduler: Arc<Scheduler>,
    pub channel_manager: Arc<ChannelManager>,
    pub broadcaster: Arc<EventBroadcaster>,
    pub hook_manager: Arc<HookManager>,
    pub tx_queue: Arc<TxQueueManager>,
    pub safe_mode_rate_limiter: SafeModeChannelRateLimiter,
    /// Wallet provider for x402 payments and transaction signing
    /// Either EnvWalletProvider (Standard mode) or FlashWalletProvider (Flash mode)
    /// None if no wallet is configured (graceful degradation - shows warning on login page)
    pub wallet_provider: Option<Arc<dyn WalletProvider>>,
    /// Disk quota manager for application-level disk usage enforcement
    pub disk_quota: Option<Arc<disk_quota::DiskQuotaManager>>,
    /// Handles for module background workers (keyed by module name).
    /// Used for hot-reload: abort old worker, spawn new one without restart.
    pub module_workers: Arc<tokio::sync::Mutex<std::collections::HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// Server start time for uptime calculation
    pub started_at: std::time::Instant,
    /// Telemetry store for querying execution spans and reward stats
    pub telemetry_store: Arc<telemetry::TelemetryStore>,
    /// Resource manager for versioned prompts and configs
    pub resource_manager: Arc<telemetry::ResourceManager>,
}

/// Auto-retrieve backup from keystore on fresh instance
///
/// This solves the common problem where starkbot is dockerized and
/// database state is lost on container updates. On boot, if we haven't
/// already retrieved from keystore, we attempt to restore state.
///
/// Conditions for auto-retrieval:
/// 1. Wallet address hasn't been auto-retrieved before (tracked in keystore_state)
/// 2. Local database appears fresh (no API keys, no mind nodes beyond trunk)
///
/// Retry logic: 3 attempts with exponential backoff (2s, 4s, 8s)
async fn auto_retrieve_from_keystore(db: &std::sync::Arc<db::Database>, private_key: &str) {
    auto_retrieve_from_keystore_with_retry(db, private_key, None).await;
}

/// Auto-retrieve with a wallet provider for keystore auth (Flash/Privy mode).
/// The private_key is still used for ECIES decryption; the wallet provider is used
/// for SIWE auth so the keystore sees the correct (Privy) wallet address.
async fn auto_retrieve_from_keystore_with_provider(
    db: &std::sync::Arc<db::Database>,
    private_key: &str,
    wallet_provider: &std::sync::Arc<dyn wallet::WalletProvider>,
) {
    auto_retrieve_from_keystore_with_retry(db, private_key, Some(wallet_provider.clone())).await;
}

async fn auto_retrieve_from_keystore_with_retry(
    db: &std::sync::Arc<db::Database>,
    private_key: &str,
    wallet_provider: Option<std::sync::Arc<dyn wallet::WalletProvider>>,
) {
    const MAX_RETRIES: u32 = 3;
    const INITIAL_BACKOFF_SECS: u64 = 2;

    // Get wallet address - prefer wallet provider (correct in Flash/Privy mode)
    let wallet_address = if let Some(ref wp) = wallet_provider {
        wp.get_address().to_lowercase()
    } else {
        match keystore_client::get_wallet_address(private_key) {
            Ok(addr) => addr.to_lowercase(),
            Err(e) => {
                log::warn!("[Keystore] Failed to get wallet address: {}", e);
                return;
            }
        }
    };

    // Check if we've already done auto-retrieval for this wallet
    match db.has_keystore_auto_retrieved(&wallet_address) {
        Ok(true) => {
            log::debug!("[Keystore] Already auto-retrieved for wallet {}", wallet_address);
            return;
        }
        Ok(false) => {}
        Err(e) => {
            log::warn!("[Keystore] Failed to check auto-retrieval status: {}", e);
            return;
        }
    }

    // Additional check: only auto-retrieve if local state is truly fresh
    // (no API keys and only trunk node in mind map)
    let has_api_keys = db.list_api_keys().map(|k| !k.is_empty()).unwrap_or(false);
    let mind_node_count = db.list_mind_nodes().map(|n| n.len()).unwrap_or(0);

    if has_api_keys || mind_node_count > 1 {
        log::info!(
            "[Keystore] Local state exists (keys: {}, nodes: {}), skipping auto-retrieval",
            has_api_keys,
            mind_node_count
        );
        // Mark as retrieved so we don't check again
        let _ = db.mark_keystore_auto_retrieved(&wallet_address);
        let _ = db.record_auto_sync_result(
            &wallet_address,
            "skipped",
            "Local state already exists",
            None,
            None,
        );
        return;
    }

    log::info!("[Keystore] Fresh instance detected, attempting auto-retrieval for {}", wallet_address);

    // Retry loop with exponential backoff
    let mut last_error = String::new();
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let backoff = INITIAL_BACKOFF_SECS * (1 << (attempt - 1)); // 2s, 4s, 8s
            log::info!("[Keystore] Retry {} of {}, waiting {}s...", attempt + 1, MAX_RETRIES, backoff);
            tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
        }

        let get_result = if let Some(ref wp) = wallet_provider {
            keystore_client::KEYSTORE_CLIENT.get_keys_with_provider(wp).await
        } else {
            keystore_client::KEYSTORE_CLIENT.get_keys(private_key).await
        };
        match get_result {
            Ok(resp) => {
                if resp.success {
                    // Successfully got backup, restore it
                    if let Some(encrypted_data) = resp.encrypted_data {
                        match restore_backup_data(db, private_key, &encrypted_data).await {
                            Ok((key_count, node_count)) => {
                                log::info!("[Keystore] Auto-sync restored {} keys, {} nodes", key_count, node_count);
                                let _ = db.record_auto_sync_result(
                                    &wallet_address,
                                    "success",
                                    &format!("Restored {} API keys and {} mind map nodes", key_count, node_count),
                                    Some(key_count as i32),
                                    Some(node_count as i32),
                                );
                            }
                            Err(e) => {
                                log::error!("[Keystore] Failed to restore backup: {}", e);
                                let _ = db.record_auto_sync_result(
                                    &wallet_address,
                                    "error",
                                    &format!("Restore failed: {}", e),
                                    None,
                                    None,
                                );
                            }
                        }
                        let _ = db.mark_keystore_auto_retrieved(&wallet_address);
                        return;
                    } else {
                        // Server returned success but no data - treat as no backup
                        log::info!("[Keystore] Server returned success but no backup data");
                        let _ = db.mark_keystore_auto_retrieved(&wallet_address);
                        let _ = db.record_auto_sync_result(
                            &wallet_address,
                            "no_backup",
                            "Server returned success but no backup data was found.",
                            None,
                            None,
                        );
                        return;
                    }
                } else if let Some(error) = &resp.error {
                    if error.contains("No backup found") {
                        log::info!("[Keystore] No cloud backup found - starting fresh");
                        let _ = db.mark_keystore_auto_retrieved(&wallet_address);
                        let _ = db.record_auto_sync_result(
                            &wallet_address,
                            "no_backup",
                            "No cloud backup found. Use the API Keys page to backup your settings, or restore from another source.",
                            None,
                            None,
                        );
                        return;
                    }
                    last_error = error.clone();
                }
            }
            Err(e) => {
                last_error = e;
                log::warn!("[Keystore] Attempt {} failed: {}", attempt + 1, last_error);
            }
        }
    }

    log::error!("[Keystore] Auto-retrieval failed after {} attempts: {}", MAX_RETRIES, last_error);
    // Mark as attempted anyway to prevent repeated failures on every restart
    let _ = db.mark_keystore_auto_retrieved(&wallet_address);

    // Determine error type for user-friendly message
    let (status, message) = if last_error.contains("connection") || last_error.contains("timeout") || last_error.contains("Failed to connect") {
        ("server_error", format!("Could not connect to keystore server after {} attempts. Check your network connection and keystore URL settings.", MAX_RETRIES))
    } else {
        ("error", format!("Auto-sync failed: {}", last_error))
    };
    let _ = db.record_auto_sync_result(&wallet_address, status, &message, None, None);
}

/// Restore backup data from encrypted payload (used by both auto-retrieval and manual restore)
/// Returns (key_count, node_count) on success
async fn restore_backup_data(
    db: &std::sync::Arc<db::Database>,
    private_key: &str,
    encrypted_data: &str,
) -> Result<(usize, usize), String> {
    let mut backup_data = keystore_client::decrypt_backup_data(private_key, encrypted_data)?;

    log::info!(
        "[Keystore] Restoring backup v{} with {} items from {}",
        backup_data.version,
        backup_data.item_count(),
        backup_data.created_at.format("%Y-%m-%d %H:%M:%S")
    );

    // Restore API keys
    let mut restored_keys = 0;
    for key in &backup_data.api_keys {
        if let Err(e) = db.upsert_api_key(&key.key_name, &key.key_value) {
            log::warn!("[Keystore] Failed to restore key {}: {}", key.key_name, e);
        } else {
            restored_keys += 1;
        }
    }
    if restored_keys > 0 {
        log::info!("[Keystore] Restored {} API keys", restored_keys);
    }

    // Clear existing mind nodes and connections before restore
    match db.clear_mind_nodes_for_restore() {
        Ok((nodes_deleted, connections_deleted)) => {
            if nodes_deleted > 0 || connections_deleted > 0 {
                log::info!("[Keystore] Cleared {} nodes and {} connections for restore", nodes_deleted, connections_deleted);
            }
        }
        Err(e) => log::warn!("[Keystore] Failed to clear mind nodes for restore: {}", e),
    }

    // Restore mind map nodes (create ID mapping for connections)
    let mut old_to_new_id: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    let mut restored_nodes = 0;

    // First, get or create trunk and map backup trunk ID to it
    let current_trunk = db.get_or_create_trunk_node().ok();
    if let Some(ref trunk) = current_trunk {
        for node in &backup_data.mind_map_nodes {
            if node.is_trunk {
                old_to_new_id.insert(node.id, trunk.id);
                if !node.body.is_empty() {
                    let _ = db.update_mind_node(trunk.id, &db::tables::mind_nodes::UpdateMindNodeRequest {
                        body: Some(node.body.clone()),
                        position_x: node.position_x,
                        position_y: node.position_y,
                    });
                }
                break;
            }
        }
    }

    for node in &backup_data.mind_map_nodes {
        if node.is_trunk {
            // Already handled above
            continue;
        }

        let request = db::tables::mind_nodes::CreateMindNodeRequest {
            body: Some(node.body.clone()),
            position_x: node.position_x,
            position_y: node.position_y,
            parent_id: None,
        };
        match db.create_mind_node(&request) {
            Ok(new_node) => {
                old_to_new_id.insert(node.id, new_node.id);
                restored_nodes += 1;
            }
            Err(e) => log::warn!("[Keystore] Failed to restore mind node: {}", e),
        }
    }
    if restored_nodes > 0 {
        log::info!("[Keystore] Restored {} mind map nodes", restored_nodes);
    }

    // Restore mind map connections using ID mapping
    let mut restored_connections = 0;
    for conn in &backup_data.mind_map_connections {
        if let (Some(&parent_id), Some(&child_id)) = (
            old_to_new_id.get(&conn.parent_id),
            old_to_new_id.get(&conn.child_id),
        ) {
            match db.create_mind_node_connection(parent_id, child_id) {
                Ok(_) => restored_connections += 1,
                Err(e) => {
                    if !e.to_string().contains("UNIQUE constraint") {
                        log::warn!("[Keystore] Failed to restore connection: {}", e);
                    }
                }
            }
        }
    }
    if restored_connections > 0 {
        log::info!("[Keystore] Restored {} mind map connections", restored_connections);
    }

    // Restore bot settings if present
    if let Some(settings) = &backup_data.bot_settings {
        let custom_rpc: Option<std::collections::HashMap<String, String>> =
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
        ) {
            Ok(_) => log::info!("[Keystore] Restored bot settings"),
            Err(e) => log::warn!("[Keystore] Failed to restore bot settings: {}", e),
        }
    }

    // Restore channels FIRST (with bot tokens) - need ID mapping for cron jobs, heartbeat, and channel settings
    let mut old_channel_to_new_id: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    let mut restored_channels = 0;
    for channel in &backup_data.channels {
        match db.create_channel(&channel.channel_type, &channel.name, &channel.bot_token, channel.app_token.as_deref()) {
            Ok(new_channel) => {
                old_channel_to_new_id.insert(channel.id, new_channel.id);
                // Migrate legacy bot_token column → channel setting (backwards compat)
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
                if let Some(ref app_token) = channel.app_token {
                    if !app_token.is_empty() && channel.channel_type == "slack" {
                        let _ = db.set_channel_setting(new_channel.id, "slack_app_token", app_token);
                    }
                }
                restored_channels += 1;
            }
            Err(e) => {
                // Channel might already exist with same name/token - try to find it
                if let Ok(existing) = db.list_enabled_channels() {
                    if let Some(found) = existing.iter().find(|c| c.name == channel.name && c.channel_type == channel.channel_type) {
                        old_channel_to_new_id.insert(channel.id, found.id);
                        log::debug!("[Keystore] Channel {} already exists, mapping to existing", channel.name);
                    } else {
                        log::warn!("[Keystore] Failed to restore channel {}: {}", channel.name, e);
                    }
                }
            }
        }
    }
    if restored_channels > 0 {
        log::info!("[Keystore] Restored {} channels", restored_channels);
    }

    // Restore channel settings using channel ID mapping
    let mut restored_channel_settings = 0;
    for setting in &backup_data.channel_settings {
        if let Some(&new_channel_id) = old_channel_to_new_id.get(&setting.channel_id) {
            match db.set_channel_setting(new_channel_id, &setting.setting_key, &setting.setting_value) {
                Ok(_) => restored_channel_settings += 1,
                Err(e) => log::warn!("[Keystore] Failed to restore channel setting: {}", e),
            }
        }
    }
    if restored_channel_settings > 0 {
        log::info!("[Keystore] Restored {} channel settings", restored_channel_settings);
    }

    // Enable channels that have auto_start_on_boot=true (so gateway.start_enabled_channels() will start them)
    let mut auto_enabled_channels = 0;
    for &new_channel_id in old_channel_to_new_id.values() {
        let should_auto_start = db
            .get_channel_setting(new_channel_id, "auto_start_on_boot")
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false);

        if should_auto_start {
            if let Err(e) = db.set_channel_enabled(new_channel_id, true) {
                log::warn!("[Keystore] Failed to enable auto-start channel {}: {}", new_channel_id, e);
            } else {
                auto_enabled_channels += 1;
            }
        }
    }
    if auto_enabled_channels > 0 {
        log::info!("[Keystore] Enabled {} channels with auto_start_on_boot", auto_enabled_channels);
    }

    // Restore cron jobs (with mapped channel IDs)
    let mut restored_cron_jobs = 0;
    for job in &backup_data.cron_jobs {
        // Map old channel_id to new channel_id
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
            Ok(_) => restored_cron_jobs += 1,
            Err(e) => log::warn!("[Keystore] Failed to restore cron job {}: {}", job.name, e),
        }
    }
    if restored_cron_jobs > 0 {
        log::info!("[Keystore] Restored {} cron jobs", restored_cron_jobs);
    }

    // Restore heartbeat config if present (with mapped channel ID)
    if let Some(hb_config) = &backup_data.heartbeat_config {
        // Map old channel_id to new channel_id
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
                    log::warn!("[Keystore] Failed to restore heartbeat config: {}", e);
                } else {
                    log::info!("[Keystore] Restored heartbeat config (enabled={})", hb_config.enabled);
                }
            }
            Err(e) => log::warn!("[Keystore] Failed to create heartbeat config for restore: {}", e),
        }
    }

    // Restore soul document if present in backup AND no local copy exists
    // (preserves agent modifications and user edits across restarts)
    if let Some(soul_content) = &backup_data.soul_document {
        let soul_path = config::soul_document_path();
        if soul_path.exists() {
            log::info!("[Keystore] Soul document already exists locally, skipping restore from backup");
        } else {
            // Ensure soul directory exists
            if let Some(parent) = soul_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&soul_path, soul_content) {
                Ok(_) => log::info!("[Keystore] Restored soul document from backup"),
                Err(e) => log::warn!("[Keystore] Failed to restore soul document: {}", e),
            }
        }
    }

    // Legacy: if old backup has identity_document but no agent_identity entry,
    // parse and migrate to DB (only if no DB row exists yet)
    if backup_data.agent_identity.is_none() {
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
                        Ok(_) => log::info!("[Keystore] Migrated legacy identity_document to DB"),
                        Err(e) => log::warn!("[Keystore] Failed to migrate legacy identity_document: {}", e),
                    }
                }
            }
        }
    }

    // Restore x402 payment limits
    let mut restored_x402_limits = 0;
    for limit in &backup_data.x402_payment_limits {
        match db.set_x402_payment_limit(&limit.asset, &limit.max_amount, limit.decimals, &limit.display_name, limit.address.as_deref()) {
            Ok(_) => {
                // Also update the in-memory global
                crate::x402::payment_limits::set_limit(&limit.asset, &limit.max_amount, limit.decimals, &limit.display_name, limit.address.as_deref());
                restored_x402_limits += 1;
            }
            Err(e) => log::warn!("[Keystore] Failed to restore x402 payment limit for {}: {}", limit.asset, e),
        }
    }
    if restored_x402_limits > 0 {
        log::info!("[Keystore] Restored {} x402 payment limits", restored_x402_limits);
    }

    // Restore agent identity from backup (with full metadata)
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
                    log::info!(
                        "[Keystore] Restored agent identity (agent_id={}) from backup",
                        ai.agent_id
                    );
                }
                Err(e) => {
                    log::warn!("[Keystore] Failed to restore agent identity: {}", e);
                }
            }
        } else {
            log::info!("[Keystore] Agent identity already exists locally, skipping restore from backup");
        }
    }

    // Restore module data (generic module restore)
    {
        let module_registry = modules::ModuleRegistry::new();

        // Backward-compat shim: convert legacy discord_registrations to module_data format
        if !backup_data.discord_registrations.is_empty() && !backup_data.module_data.contains_key("discord_tipping") {
            log::info!("[Keystore] Converting legacy discord_registrations to module_data format");
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
                // Ensure tables exist
                if module.has_db_tables() {
                    let conn = db.conn();
                    if let Err(e) = module.init_tables(&conn) {
                        log::warn!("[Keystore] Failed to init tables for module '{}': {}", module_name, e);
                        continue;
                    }
                }
                // Auto-install if not already installed
                if !db.is_module_installed(module_name).unwrap_or(true) {
                    let required_keys = module.required_api_keys();
                    let key_strs: Vec<&str> = required_keys.iter().copied().collect();
                    let _ = db.install_module(
                        module_name,
                        module.description(),
                        module.version(),
                        module.has_db_tables(),
                        module.has_tools(),
                        module.has_worker(),
                        &key_strs,
                    );
                }
                match module.restore_data(db, data) {
                    Ok(()) => log::info!("[Keystore] Restored module data for '{}'", module_name),
                    Err(e) => log::warn!("[Keystore] Failed to restore module data for '{}': {}", module_name, e),
                }
            } else {
                log::warn!("[Keystore] Skipping restore for unknown module '{}'", module_name);
            }
        }
    }

    // Restore skills (version-aware: won't downgrade bundled skills that have newer versions on disk)
    let mut restored_skills = 0;
    for skill_entry in &backup_data.skills {
        let now = chrono::Utc::now().to_rfc3339();
        let arguments: std::collections::HashMap<String, skills::types::SkillArgument> =
            serde_json::from_str(&skill_entry.arguments).unwrap_or_default();

        let db_skill = skills::DbSkill {
            id: None,
            name: skill_entry.name.clone(),
            description: skill_entry.description.clone(),
            body: skill_entry.body.clone(),
            version: skill_entry.version.clone(),
            author: skill_entry.author.clone(),
            homepage: skill_entry.homepage.clone(),
            metadata: skill_entry.metadata.clone(),
            enabled: skill_entry.enabled,
            requires_tools: skill_entry.requires_tools.clone(),
            requires_binaries: skill_entry.requires_binaries.clone(),
            arguments,
            tags: skill_entry.tags.clone(),
            subagent_type: skill_entry.subagent_type.clone(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        match db.create_skill(&db_skill) {
            Ok(skill_id) => {
                for script in &skill_entry.scripts {
                    let db_script = skills::DbSkillScript {
                        id: None,
                        skill_id,
                        name: script.name.clone(),
                        code: script.code.clone(),
                        language: script.language.clone(),
                        created_at: now.clone(),
                    };
                    if let Err(e) = db.create_skill_script(&db_script) {
                        log::warn!("[Keystore] Failed to restore script '{}' for skill '{}': {}", script.name, skill_entry.name, e);
                    }
                }
                restored_skills += 1;
            }
            Err(e) => {
                log::warn!("[Keystore] Failed to restore skill '{}': {}", skill_entry.name, e);
            }
        }
    }
    if restored_skills > 0 {
        log::info!("[Keystore] Restored {} skills", restored_skills);
    }

    // Restore agent settings (AI model configurations)
    let mut restored_agent_settings = 0;
    if !backup_data.agent_settings.is_empty() {
        if let Err(e) = db.disable_agent_settings() {
            log::warn!("[Keystore] Failed to disable existing agent settings for restore: {}", e);
        }
        for entry in &backup_data.agent_settings {
            match db.save_agent_settings(
                &entry.endpoint,
                &entry.model_archetype,
                entry.max_response_tokens,
                entry.max_context_tokens,
                entry.secret_key.as_deref(),
            ) {
                Ok(saved) => {
                    if !entry.enabled {
                        let _ = db.disable_agent_settings();
                    }
                    restored_agent_settings += 1;
                    log::info!("[Keystore] Restored agent settings: {} ({})", saved.endpoint, saved.model_archetype);
                }
                Err(e) => {
                    log::warn!("[Keystore] Failed to restore agent settings for {}: {}", entry.endpoint, e);
                }
            }
        }
    }
    if restored_agent_settings > 0 {
        log::info!("[Keystore] Restored {} agent settings", restored_agent_settings);
    }

    log::info!("[Keystore] Restore complete");
    Ok((restored_keys, restored_nodes))
}

/// SPA fallback handler - serves index.html for client-side routing
async fn spa_fallback() -> actix_web::Result<NamedFile> {
    // Check both possible locations for frontend dist
    if std::path::Path::new("./stark-frontend/dist/index.html").exists() {
        Ok(NamedFile::open("./stark-frontend/dist/index.html")?)
    } else {
        Ok(NamedFile::open("../stark-frontend/dist/index.html")?)
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    // Load presets and tokens from config directory
    // Check ./config first, then ../config (for running from subdirectory)
    let config_dir = if std::path::Path::new("./config").exists() {
        std::path::Path::new("./config")
    } else if std::path::Path::new("../config").exists() {
        std::path::Path::new("../config")
    } else {
        panic!("Config directory not found in ./config or ../config");
    };
    log::info!("Using config directory: {:?}", config_dir);
    log::info!("Loading presets from config directory");
    tools::presets::load_presets(config_dir);
    log::info!("Loading token configs from config directory");
    tools::builtin::cryptocurrency::token_lookup::load_tokens(config_dir);
    log::info!("Loading network configs from config directory");
    tools::builtin::cryptocurrency::network_lookup::load_networks(config_dir);
    log::info!("Loading RPC provider configs from config directory");
    tools::rpc_config::load_rpc_providers(config_dir);
    log::info!("Loading AI endpoint presets from config directory");
    ai_endpoint_config::load_ai_endpoints(config_dir);
    log::info!("Loading x402 payment limit defaults from config directory");
    x402::payment_limits::load_defaults(config_dir);

    let mut config = Config::from_env();
    let port = config.port;

    // Initialize workspace directory and copy SOUL.md
    log::info!("Initializing workspace");
    if let Err(e) = config::initialize_workspace() {
        log::error!("Failed to initialize workspace: {}", e);
    }

    // Initialize disk quota manager
    let disk_quota_mb = config::disk_quota_mb();
    let disk_quota: Option<Arc<disk_quota::DiskQuotaManager>> = if disk_quota_mb > 0 {
        let tracked_dirs = vec![
            std::path::PathBuf::from(config::workspace_dir()),
            std::path::PathBuf::from(config::journal_dir()),
            std::path::PathBuf::from(config::memory_config().memory_dir),
            std::path::PathBuf::from(config::soul_dir()),
            // Include the database directory
            {
                let db_url = std::env::var("DATABASE_URL")
                    .unwrap_or_else(|_| config::defaults::DATABASE_URL.to_string());
                let db_path = std::path::PathBuf::from(&db_url);
                db_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| config::backend_dir().join(".db"))
            },
        ];
        let manager = Arc::new(disk_quota::DiskQuotaManager::new(Some(disk_quota_mb), tracked_dirs));
        log::info!("{}", manager.status_line());
        Some(manager)
    } else {
        log::info!("Disk quota: disabled");
        None
    };

    log::info!("Initializing database at {}", config.database_url);
    let db = Database::new(&config.database_url).expect("Failed to initialize database");
    let db = Arc::new(db);

    // Override x402 payment limit defaults with any user-configured values from DB
    match db.get_all_x402_payment_limits() {
        Ok(limits) => {
            for l in &limits {
                x402::payment_limits::set_limit(&l.asset, &l.max_amount, l.decimals, &l.display_name, l.address.as_deref());
            }
            if !limits.is_empty() {
                log::info!("Loaded {} x402 payment limits from database", limits.len());
            }
        }
        Err(e) => log::warn!("Failed to load x402 payment limits from DB: {}", e),
    }

    // Initialize keystore URL (must be before auto-retrieve)
    // Priority: 1. bot_settings.keystore_url, 2. KEYSTORE_URL env var, 3. default
    let env_keystore_url = std::env::var("KEYSTORE_URL").ok().filter(|s| !s.is_empty());
    let db_keystore_url = db.get_bot_settings().ok().and_then(|s| s.keystore_url).filter(|s| !s.is_empty());

    if let Some(url) = db_keystore_url.or(env_keystore_url) {
        log::info!("Using custom keystore URL: {}", url);
        keystore_client::KEYSTORE_CLIENT.set_base_url(&url).await;
    }

    // Auto-retrieve from keystore (restore state from cloud backup on fresh instance)
    // This runs before channel auto-start so restored channels can start
    // NOTE: Flash mode auto-retrieval happens later, after deriving the backup key from wallet signature
    let is_flash_mode = std::env::var("FLASH_KEYSTORE_URL").is_ok();
    if !is_flash_mode {
        if let Some(ref private_key) = config.burner_wallet_private_key {
            auto_retrieve_from_keystore(&db, private_key).await;
        }
    }

    // Initialize Module Registry (compile-time plugin registry)
    let module_registry = modules::ModuleRegistry::new();

    // Auto-migration: if discord_user_profiles table exists but discord_tipping module
    // is not installed, auto-install it so existing deployments keep tipping on upgrade.
    {
        let conn = db.conn();
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='discord_user_profiles'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(false);

        if table_exists && !db.is_module_installed("discord_tipping").unwrap_or(true) {
            log::info!("[MODULE] Auto-migrating: discord_user_profiles table found, installing discord_tipping module");
            if let Some(module) = module_registry.get("discord_tipping") {
                let required_keys = module.required_api_keys();
                let key_strs: Vec<&str> = required_keys.iter().copied().collect();
                match db.install_module(
                    "discord_tipping",
                    module.description(),
                    module.version(),
                    module.has_db_tables(),
                    module.has_tools(),
                    module.has_worker(),
                    &key_strs,
                ) {
                    Ok(_) => log::info!("[MODULE] Auto-installed discord_tipping module (migration from hardcoded table)"),
                    Err(e) => log::warn!("[MODULE] Failed to auto-install discord_tipping: {}", e),
                }
            }
        }
    }

    // Initialize Tool Registry with built-in tools + installed module tools
    log::info!("Initializing tool registry");
    let mut tool_registry_mut = tools::create_default_registry();

    // Register tools from installed & enabled modules
    let installed_modules = db.list_installed_modules().unwrap_or_default();
    for module_entry in &installed_modules {
        if module_entry.enabled {
            if let Some(module) = module_registry.get(&module_entry.module_name) {
                for tool in module.create_tools() {
                    log::info!("[MODULE] Registered tool: {} (from {})", tool.name(), module_entry.module_name);
                    tool_registry_mut.register(tool);
                }
            }
        }
    }

    let tool_registry = Arc::new(tool_registry_mut);
    log::info!("Registered {} tools", tool_registry.len());

    // Initialize Skill Registry (database-backed)
    log::info!("Initializing skill registry");
    let skill_registry = Arc::new(skills::create_default_registry(db.clone()));

    // Load file-based skills into database (for backward compatibility)
    let skill_count = skill_registry.load_all().await.unwrap_or_else(|e| {
        log::warn!("Failed to load skills from disk: {}", e);
        0
    });
    log::info!("Loaded {} skills from disk, {} total in database", skill_count, skill_registry.len());

    // Initialize Transaction Queue Manager with DB for persistent broadcast history
    // NOTE: Must be created before Gateway so channels can use it for web3 transactions
    log::info!("Initializing transaction queue manager");
    let tx_queue = Arc::new(TxQueueManager::with_db(db.clone()));

    // Initialize Wallet Provider
    // Flash mode: Uses FlashWalletProvider which proxies signing to Privy via Flash backend
    // Standard mode: Uses EnvWalletProvider which signs locally with raw private key
    // If neither is configured, wallet_provider will be None (graceful degradation)
    log::info!("Initializing wallet provider");
    let wallet_provider: Option<Arc<dyn wallet::WalletProvider>> = if is_flash_mode {
        // Flash mode - wallet managed by Privy via Flash control plane
        // BURNER_WALLET_BOT_PRIVATE_KEY is ignored in this mode
        log::info!("Flash mode: initializing FlashWalletProvider (Privy embedded wallet)...");
        match wallet::FlashWalletProvider::new().await {
            Ok(provider) => {
                log::info!("Flash wallet provider initialized: {} (mode: {})",
                    provider.get_address(), provider.mode_name());
                Some(Arc::new(provider) as Arc<dyn wallet::WalletProvider>)
            }
            Err(e) => {
                log::error!("Failed to create Flash wallet provider: {}", e);
                None
            }
        }
    } else if let Some(ref pk) = config.burner_wallet_private_key {
        // Standard mode - use raw private key from environment
        log::info!("Standard mode: initializing EnvWalletProvider...");
        match wallet::EnvWalletProvider::from_private_key(pk) {
            Ok(provider) => {
                log::info!("Wallet provider initialized: {} (mode: {})",
                    provider.get_address(), provider.mode_name());
                Some(Arc::new(provider) as Arc<dyn wallet::WalletProvider>)
            }
            Err(e) => {
                log::warn!("Failed to create wallet provider: {}. Wallet features disabled.", e);
                None
            }
        }
    } else {
        log::warn!("No wallet configured - set FLASH_KEYSTORE_URL (Flash/Privy mode) or BURNER_WALLET_BOT_PRIVATE_KEY (Standard mode)");
        None
    };

    // Flash mode: derive a deterministic backup key from the Flash wallet signature
    // This key is used ONLY for ECIES encryption/decryption of backup data.
    // Keystore auth (SIWE) and x402 payments use the actual Privy wallet provider.
    if is_flash_mode {
        if let Some(ref wp) = wallet_provider {
            match wp.sign_message(b"starkbot-backup-key-v1").await {
                Ok(sig) => {
                    let sig_bytes = sig.to_vec();
                    let derived_key = ethers::utils::keccak256(&sig_bytes);
                    config.burner_wallet_private_key = Some(hex::encode(derived_key));
                    log::info!("Flash mode: derived backup encryption key from wallet signature");

                    // Auto-retrieval: use wallet provider for keystore auth, derived key for decryption
                    if let Some(ref private_key) = config.burner_wallet_private_key {
                        auto_retrieve_from_keystore_with_provider(&db, private_key, wp).await;
                    }
                }
                Err(e) => {
                    log::error!("Flash mode: failed to derive backup key: {}. Cloud backup will be unavailable.", e);
                }
            }
        }
    }

    // Initialize Gateway with tool registry, wallet provider, and tx_queue for channels
    log::info!("Initializing Gateway");
    let gateway = Arc::new(Gateway::new_with_tools_wallet_and_tx_queue(
        db.clone(),
        tool_registry.clone(),
        wallet_provider.clone(),
        Some(tx_queue.clone()),
    ));

    // Initialize Execution Tracker for progress display
    log::info!("Initializing execution tracker");
    let execution_tracker = Arc::new(ExecutionTracker::new(gateway.broadcaster().clone()));

    // Initialize Hook Manager
    log::info!("Initializing hook manager");
    let hook_manager = Arc::new(HookManager::new());
    log::info!("Hook manager initialized");

    // Initialize Tool Validator Registry
    log::info!("Initializing tool validator registry");
    let validator_registry = Arc::new(tool_validators::create_default_registry());
    log::info!("Registered {} tool validators", validator_registry.len());

    // Create the shared MessageDispatcher for all message processing
    log::info!("Initializing message dispatcher");
    let mut dispatcher_builder = MessageDispatcher::new_with_wallet_and_skills(
            db.clone(),
            gateway.broadcaster().clone(),
            tool_registry.clone(),
            execution_tracker.clone(),
            wallet_provider.clone(),
            Some(skill_registry.clone()),
        ).with_hook_manager(hook_manager.clone())
         .with_validator_registry(validator_registry.clone())
         .with_tx_queue(tx_queue.clone());
    if let Some(ref dq) = disk_quota {
        dispatcher_builder = dispatcher_builder.with_disk_quota(dq.clone());
        // Also wire disk quota into the MemoryStore for memory append limits
        if let Some(ref store) = dispatcher_builder.memory_store() {
            store.set_disk_quota(dq.clone());
        }
    }
    let dispatcher = Arc::new(dispatcher_builder);

    // Get broadcaster and channel_manager for the /ws route
    let broadcaster = gateway.broadcaster();
    let channel_manager = gateway.channel_manager();

    // Start enabled channels
    log::info!("Starting enabled channels");
    gateway.start_enabled_channels().await;

    // Initialize and start the scheduler
    log::info!("Initializing scheduler");
    let scheduler_config = SchedulerConfig::default();
    let scheduler = Arc::new(Scheduler::new(
        db.clone(),
        dispatcher.clone(),
        gateway.broadcaster().clone(),
        execution_tracker.clone(),
        scheduler_config,
        wallet_provider.clone(),
    ));

    // Start scheduler background task
    let scheduler_handle = Arc::clone(&scheduler);
    let (scheduler_shutdown_tx, scheduler_shutdown_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        scheduler_handle.start(scheduler_shutdown_rx).await;
    });

    // Spawn disk quota background scan task (re-scan every 60s, broadcast warnings via gateway)
    if let Some(ref dq) = disk_quota {
        let dq_clone = dq.clone();
        let bc_clone = broadcaster.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.tick().await; // skip immediate tick
            // Hysteresis: track last broadcast level to avoid spamming
            let mut last_level: Option<String> = None;
            loop {
                interval.tick().await;
                let _usage = dq_clone.refresh();
                let pct = dq_clone.usage_percentage();
                let used = dq_clone.usage_bytes();
                let quota = dq_clone.quota_bytes();
                let remaining = dq_clone.remaining_bytes();

                let (level, message) = if pct >= 95 {
                    ("critical", "Storage is critically full. Clean up now to avoid write failures.")
                } else if pct >= 85 {
                    ("high", "Storage is 85% full. Writes may start failing soon.")
                } else if pct >= 70 {
                    ("warning", "Storage is 70% full. Consider cleaning up old files.")
                } else {
                    ("ok", "")
                };

                // Log at appropriate levels
                match level {
                    "critical" => log::error!("[DISK_QUOTA] CRITICAL: {} — consider cleaning up files", dq_clone.status_line()),
                    "high" => log::warn!("[DISK_QUOTA] HIGH: {}", dq_clone.status_line()),
                    "warning" => log::warn!("[DISK_QUOTA] WARNING: {}", dq_clone.status_line()),
                    _ => log::debug!("[DISK_QUOTA] {}", dq_clone.status_line()),
                }

                // Only broadcast when crossing a new threshold (hysteresis)
                let should_broadcast = match (&last_level, level) {
                    (None, "ok") => false, // Don't broadcast ok on first tick if already fine
                    (None, _) => true,     // First time crossing a threshold
                    (Some(prev), curr) => prev.as_str() != curr, // Level changed
                };

                if should_broadcast {
                    let event_data = serde_json::json!({
                        "percentage": pct,
                        "used_bytes": used,
                        "quota_bytes": quota,
                        "remaining_bytes": remaining,
                        "level": level,
                        "message": message,
                    });
                    bc_clone.broadcast(crate::gateway::protocol::GatewayEvent::custom(
                        "disk_quota.warning",
                        event_data,
                    ));
                    last_level = Some(level.to_string());
                }
            }
        });
    }

    // Spawn workers for installed & enabled modules (track handles for hot-reload)
    let module_workers = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<String, tokio::task::JoinHandle<()>>::new()));
    {
        let mut workers = module_workers.lock().await;
        for entry in &installed_modules {
            if entry.enabled {
                if let Some(module) = module_registry.get(&entry.module_name) {
                    if let Some(handle) = module.spawn_worker(db.clone(), broadcaster.clone(), dispatcher.clone()) {
                        log::info!("[MODULE] Started worker for: {}", entry.module_name);
                        workers.insert(entry.module_name.clone(), handle);
                    }
                }
            }
        }
    }

    // Determine frontend dist path (check both locations)
    // Set DISABLE_FRONTEND=1 to disable static file serving (for separate dev server)
    let frontend_dist = if std::env::var("DISABLE_FRONTEND").map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false) {
        log::info!("Frontend serving disabled via DISABLE_FRONTEND env var");
        ""
    } else if std::path::Path::new("./stark-frontend/dist").exists() {
        "./stark-frontend/dist"
    } else if std::path::Path::new("../stark-frontend/dist").exists() {
        "../stark-frontend/dist"
    } else {
        log::warn!("Frontend dist not found in ./stark-frontend/dist or ../stark-frontend/dist - static file serving disabled");
        ""
    };

    let dev_mode = std::env::var("STARKBOT_DEV").map(|v| v == "true" || v == "1").unwrap_or(false);
    if dev_mode {
        log::warn!("⚠️  DEV MODE ENABLED — /api/dev/chat is accessible without auth");
    }

    log::info!("Starting StarkBot server on port {}", port);
    log::info!("WebSocket Gateway available at /ws");
    log::info!("Scheduler started with cron and heartbeat support");
    if !frontend_dist.is_empty() {
        log::info!("Serving frontend from: {}", frontend_dist);
    }

    // Initialize safe mode channel rate limiter
    log::info!("Initializing safe mode channel rate limiter");
    let safe_mode_rate_limiter = SafeModeChannelRateLimiter::new(db.clone());

    let tool_reg = tool_registry.clone();
    let skill_reg = skill_registry.clone();
    let disp = dispatcher.clone();
    let exec_tracker = execution_tracker.clone();
    let sched = scheduler.clone();
    let bcast = broadcaster.clone();
    let chan_mgr = channel_manager.clone();
    let hook_mgr = hook_manager.clone();
    let tx_q = tx_queue.clone();
    let safe_mode_rl = safe_mode_rate_limiter.clone();
    let wallet_prov = wallet_provider.clone();
    let disk_q = disk_quota.clone();
    let mod_workers = module_workers.clone();
    let frontend_dist = frontend_dist.to_string();
    let dev_mode = dev_mode;

    let server = HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        let mut app = App::new()
            .app_data(web::Data::new(AppState {
                db: Arc::clone(&db),
                config: config.clone(),
                gateway: Arc::clone(&gateway),
                tool_registry: Arc::clone(&tool_reg),
                skill_registry: Arc::clone(&skill_reg),
                dispatcher: Arc::clone(&disp),
                execution_tracker: Arc::clone(&exec_tracker),
                scheduler: Arc::clone(&sched),
                channel_manager: Arc::clone(&chan_mgr),
                broadcaster: Arc::clone(&bcast),
                hook_manager: Arc::clone(&hook_mgr),
                tx_queue: Arc::clone(&tx_q),
                safe_mode_rate_limiter: safe_mode_rl.clone(),
                wallet_provider: wallet_prov.clone(),
                disk_quota: disk_q.clone(),
                module_workers: Arc::clone(&mod_workers),
                started_at: std::time::Instant::now(),
                telemetry_store: Arc::new(telemetry::TelemetryStore::new(Arc::clone(&db))),
                resource_manager: Arc::new(telemetry::ResourceManager::new(Arc::clone(&db))),
            }))
            .app_data(web::Data::new(Arc::clone(&sched)))
            // WebSocket data for /ws route
            .app_data(web::Data::new(Arc::clone(&db)))
            .app_data(web::Data::new(Arc::clone(&chan_mgr)))
            .app_data(web::Data::new(Arc::clone(&bcast)))
            .app_data(web::Data::new(Arc::clone(&tx_q)))
            .app_data(web::Data::new(wallet_prov.clone()))
            .wrap(Logger::default())
            .wrap(cors)
            .configure(controllers::health::config_routes)
            .configure(controllers::auth::config)
            .configure(controllers::dashboard::config)
            .configure(controllers::chat::config)
            .configure(controllers::api_keys::config)
            .configure(controllers::channels::config)
            .configure(controllers::agent_settings::configure)
            .configure(controllers::sessions::config)
            .configure(controllers::identity::config)
            .configure(controllers::tools::config)
            .configure(controllers::skills::config)
            .configure(controllers::cron::config)
            .configure(controllers::gmail::config)
            .configure(controllers::payments::config)
            .configure(controllers::eip8004::config)
            .configure(controllers::files::config)
            .configure(controllers::intrinsic::config)
            .configure(controllers::journal::config)
            .configure(controllers::tx_queue::config)
            .configure(controllers::broadcasted_transactions::config)
            .configure(controllers::mindmap::config)
            .configure(controllers::kanban::config)
            .configure(controllers::modules::config)
            .configure(controllers::memory::config)
            .configure(controllers::system::config)
            .configure(controllers::well_known::config)
            .configure(controllers::x402_limits::config)
            .configure(controllers::telemetry::config)
            // WebSocket Gateway route (same port as HTTP, required for single-port platforms)
            .route("/ws", web::get().to(gateway::actix_ws::ws_handler));

        if dev_mode {
            app = app.configure(controllers::dev_chat::config);
        }

        // Serve static files only if frontend dist exists
        if !frontend_dist.is_empty() {
            app = app.service(
                Files::new("/", frontend_dist.clone())
                    .index_file("index.html")
                    .default_handler(actix_web::web::to(spa_fallback))
            );
        }

        app
    })
    .bind(("0.0.0.0", port))?
    .run();

    // Get server handle for graceful shutdown
    let server_handle = server.handle();

    // Clone channel_manager for shutdown handler
    let shutdown_channel_manager = channel_manager.clone();

    // Spawn Ctrl+C handler
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        log::info!("Received Ctrl+C, shutting down...");

        // Stop all running channels with timeout (Discord, Telegram, Slack, etc.)
        log::info!("Stopping all channels...");
        let channel_stop = shutdown_channel_manager.stop_all();
        if tokio::time::timeout(std::time::Duration::from_secs(5), channel_stop).await.is_err() {
            log::warn!("Timeout waiting for channels to stop, continuing shutdown...");
        }

        // Signal scheduler to stop
        let _ = scheduler_shutdown_tx.send(());

        // Stop the HTTP server with timeout
        log::info!("Stopping HTTP server...");
        let server_stop = server_handle.stop(true);
        if tokio::time::timeout(std::time::Duration::from_secs(5), server_stop).await.is_err() {
            log::warn!("Timeout waiting for HTTP server to stop, forcing exit...");
        }

        log::info!("Shutdown complete");
    });

    server.await
}
