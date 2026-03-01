use actix_cors::Cors;
use actix_files::{Files, NamedFile};
use actix_web::{middleware::Logger, web, App, HttpServer};
use dotenv::dotenv;
use std::sync::Arc;

mod agents;
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
mod notes;
mod persona_hooks;
mod scheduler;
mod skills;
mod tools;
mod memory;
mod siwa;
mod wallet;
mod x402;
mod credits_session;
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
use db::{ActiveSessionCache, Database};
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
    /// Hybrid search engine (FTS + vector + graph)
    pub hybrid_search: Option<Arc<memory::HybridSearchEngine>>,
    /// Concrete remote embedding generator for live URL updates
    pub remote_embedding_generator: Option<Arc<memory::embeddings::RemoteEmbeddingGenerator>>,
    /// Bearer token for internal module-to-backend API calls (e.g. wallet signing proxy)
    pub internal_token: String,
    /// In-memory cache for active session metadata (shared with dispatcher for admin invalidation)
    pub active_cache: Arc<ActiveSessionCache>,
    /// Credits session client for Bearer-token auth (reduces Privy signing to ~1/hour)
    pub credits_session: Option<Arc<credits_session::CreditsSessionClient>>,
}

/// Auto-retrieve backup from keystore on fresh instance
///
/// This solves the common problem where starkbot is dockerized and
/// database state is lost on container updates. On boot, if we haven't
/// already retrieved from keystore, we attempt to restore state.
///
/// Conditions for auto-retrieval:
/// 1. Wallet address hasn't been auto-retrieved before (tracked in keystore_state)
/// 2. Local database appears fresh (no API keys, no impulse nodes beyond trunk)
///
/// Retry logic: 3 attempts with exponential backoff (2s, 4s, 8s)
async fn load_keystore_state_from_cloud(
    db: &std::sync::Arc<db::Database>,
    wallet_provider: &std::sync::Arc<dyn wallet::WalletProvider>,
    broadcaster: &std::sync::Arc<gateway::events::EventBroadcaster>,
) {
    const MAX_RETRIES: u32 = 3;
    const INITIAL_BACKOFF_SECS: u64 = 2;

    let wallet_address = wallet_provider.get_address().to_lowercase();

    // Helper to broadcast a system event to live logs
    let emit = |status: &str, message: &str| {
        broadcaster.broadcast(gateway::protocol::GatewayEvent::new(
            format!("system.keystore_{}", status),
            serde_json::json!({
                "status": status,
                "message": message,
                "wallet": &wallet_address,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        ));
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
    // (no API keys and only trunk node in impulse map)
    let has_api_keys = db.list_api_keys().map(|k| !k.is_empty()).unwrap_or(false);
    let impulse_node_count = db.list_impulse_nodes().map(|n| n.len()).unwrap_or(0);

    if has_api_keys || impulse_node_count > 1 {
        log::info!(
            "[Keystore] Local state exists (keys: {}, nodes: {}), skipping auto-retrieval",
            has_api_keys,
            impulse_node_count
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
        emit("skipped", "Local state already exists, skipping cloud restore");
        return;
    }

    log::info!("[Keystore] Fresh instance detected, attempting auto-retrieval for {}", wallet_address);
    emit("started", "Fresh instance detected, attempting cloud backup restore...");

    // Retry loop with exponential backoff
    let mut last_error = String::new();
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let backoff = INITIAL_BACKOFF_SECS * (1 << (attempt - 1)); // 2s, 4s, 8s
            log::info!("[Keystore] Retry {} of {}, waiting {}s...", attempt + 1, MAX_RETRIES, backoff);
            tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
        }

        let get_result = keystore_client::KEYSTORE_CLIENT
            .get_keys_with_provider(wallet_provider)
            .await;
        match get_result {
            Ok(resp) => {
                if resp.success {
                    // Successfully got backup, restore it
                    if let Some(encrypted_data) = resp.encrypted_data {
                        let encryption_key = match wallet_provider.get_encryption_key().await {
                            Ok(k) => k,
                            Err(e) => {
                                log::error!("[Keystore] Failed to get encryption key: {}", e);
                                let _ = db.record_auto_sync_result(
                                    &wallet_address,
                                    "error",
                                    &format!("Failed to get encryption key: {}", e),
                                    None,
                                    None,
                                );
                                let _ = db.mark_keystore_auto_retrieved(&wallet_address);
                                return;
                            }
                        };
                        let mut backup_data = match keystore_client::decrypt_backup_data(&encryption_key, &encrypted_data) {
                            Ok(b) => b,
                            Err(e) => {
                                log::error!("[Keystore] Failed to decrypt backup: {}", e);
                                let _ = db.record_auto_sync_result(
                                    &wallet_address,
                                    "error",
                                    &format!("Decrypt failed: {}", e),
                                    None,
                                    None,
                                );
                                let _ = db.mark_keystore_auto_retrieved(&wallet_address);
                                return;
                            }
                        };
                        match backup::restore::restore_all(db, &mut backup_data, None, None, None).await {
                            Ok(restore_result) => {
                                let summary = restore_result.summary();
                                log::info!("[Keystore] Auto-sync: {}", summary);
                                let _ = db.record_auto_sync_result(
                                    &wallet_address,
                                    "success",
                                    &summary,
                                    Some(restore_result.api_keys as i32),
                                    Some(restore_result.impulse_nodes as i32),
                                );
                                emit("success", &format!("Cloud backup restored: {}", summary));
                            }
                            Err(e) => {
                                log::error!("[Keystore] Failed to restore backup: {}", e);
                                let msg = format!("Restore failed: {}", e);
                                let _ = db.record_auto_sync_result(
                                    &wallet_address,
                                    "error",
                                    &msg,
                                    None,
                                    None,
                                );
                                emit("error", &msg);
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
                        emit("no_backup", "No cloud backup data found");
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
                        emit("no_backup", "No cloud backup found — starting fresh");
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
    emit(status, &message);
}

/// Return the directory where hyperpacks are stored on disk.
fn hyperpacks_dir() -> std::path::PathBuf {
    crate::config::backend_dir().join("hyperpacks")
}

/// Derive a directory name for a hyperpack from its path config.
fn hyperpack_dir_name(path: &crate::models::bot_config::HyperPackPath) -> String {
    use crate::models::bot_config::HyperPackPath;
    match path {
        HyperPackPath::Git { url, .. } => {
            // Use the repo name from the URL (strip .git suffix)
            let name = url.rsplit('/').next().unwrap_or("unknown");
            name.trim_end_matches(".git").to_string()
        }
        HyperPackPath::WebServer { hyperpack_name, .. } => hyperpack_name.clone(),
    }
}

/// Load hyperpacks declared in `bot_config.ron`.
///
/// Downloads each hyperpack into `stark-backend/hyperpacks/<name>/`.
/// Git sources are cloned; WebServer sources are fetched as zip archives
/// and extracted.
///
/// Called once during the background boot sequence, after the keystore state
/// has been restored (so any config synced from the cloud is already on disk).
async fn load_hyperpacks(broadcaster: &std::sync::Arc<gateway::events::EventBroadcaster>) {
    use crate::models::bot_config::HyperPackPath;

    let config = crate::models::BotConfig::load();

    if config.hyperpacks.is_empty() {
        log::debug!("[Hyperpacks] No hyperpacks declared in bot_config.ron");
        return;
    }

    // Load API key from agent preset (if available, for authenticated downloads)
    let api_key = crate::models::bot_config::AgentPreset::load()
        .and_then(|p| p.api_key);

    let base_dir = hyperpacks_dir();
    if let Err(e) = std::fs::create_dir_all(&base_dir) {
        log::error!("[Hyperpacks] Failed to create hyperpacks dir: {}", e);
        return;
    }

    log::info!(
        "[Hyperpacks] Resolving {} hyperpack(s) from bot_config.ron → {}",
        config.hyperpacks.len(),
        base_dir.display()
    );

    broadcaster.broadcast(gateway::protocol::GatewayEvent::new(
        "system.hyperpacks_resolving",
        serde_json::json!({
            "count": config.hyperpacks.len(),
            "packs": config.hyperpacks.iter().map(|p| {
                match &p.path {
                    HyperPackPath::Git { url, commit } => serde_json::json!({
                        "source": "git",
                        "url": url,
                        "commit": commit,
                    }),
                    HyperPackPath::WebServer { host, hyperpack_name, version } => serde_json::json!({
                        "source": "registry",
                        "host": host,
                        "name": hyperpack_name,
                        "version": version,
                    }),
                }
            }).collect::<Vec<_>>(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    ));

    let mut results: Vec<serde_json::Value> = Vec::new();

    for pack in &config.hyperpacks {
        let dir_name = hyperpack_dir_name(&pack.path);
        let dest = base_dir.join(&dir_name);

        let result = match &pack.path {
            HyperPackPath::Git { url, commit } => {
                fetch_hyperpack_git(url, commit.as_deref(), &dest).await
            }
            HyperPackPath::WebServer { host, hyperpack_name, version } => {
                fetch_hyperpack_registry(host, hyperpack_name, version.as_deref(), &dest, api_key.as_deref()).await
            }
        };

        match &result {
            Ok(()) => {
                log::info!("[Hyperpacks] {} — ready at {}", dir_name, dest.display());
                results.push(serde_json::json!({ "name": dir_name, "status": "ok" }));
            }
            Err(e) => {
                log::error!("[Hyperpacks] {} — failed: {}", dir_name, e);
                results.push(serde_json::json!({ "name": dir_name, "status": "error", "error": e.to_string() }));
            }
        }
    }

    broadcaster.broadcast(gateway::protocol::GatewayEvent::new(
        "system.hyperpacks_resolved",
        serde_json::json!({
            "count": config.hyperpacks.len(),
            "results": results,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    ));

    // Run install scripts for all successfully downloaded packs
    install_hyperpacks(&base_dir, &config.hyperpacks, broadcaster).await;
}

/// Clone or update a git-sourced hyperpack.
async fn fetch_hyperpack_git(
    url: &str,
    commit: Option<&str>,
    dest: &std::path::Path,
) -> Result<(), String> {
    if dest.join(".git").exists() {
        // Already cloned — fetch + reset to desired state
        log::info!("[Hyperpacks] git pull existing clone at {}", dest.display());
        let output = tokio::process::Command::new("git")
            .args(["fetch", "--all"])
            .current_dir(dest)
            .output()
            .await
            .map_err(|e| format!("git fetch failed: {}", e))?;
        if !output.status.success() {
            return Err(format!("git fetch failed: {}", String::from_utf8_lossy(&output.stderr)));
        }

        let reset_target = commit.unwrap_or("origin/HEAD");
        let output = tokio::process::Command::new("git")
            .args(["reset", "--hard", reset_target])
            .current_dir(dest)
            .output()
            .await
            .map_err(|e| format!("git reset failed: {}", e))?;
        if !output.status.success() {
            return Err(format!("git reset to {} failed: {}", reset_target, String::from_utf8_lossy(&output.stderr)));
        }
    } else {
        // Fresh clone
        log::info!("[Hyperpacks] git clone {} → {}", url, dest.display());
        let output = tokio::process::Command::new("git")
            .args(["clone", url, &dest.to_string_lossy()])
            .output()
            .await
            .map_err(|e| format!("git clone failed: {}", e))?;
        if !output.status.success() {
            return Err(format!("git clone failed: {}", String::from_utf8_lossy(&output.stderr)));
        }

        // Checkout specific commit if requested
        if let Some(sha) = commit {
            let output = tokio::process::Command::new("git")
                .args(["checkout", sha])
                .current_dir(dest)
                .output()
                .await
                .map_err(|e| format!("git checkout failed: {}", e))?;
            if !output.status.success() {
                return Err(format!("git checkout {} failed: {}", sha, String::from_utf8_lossy(&output.stderr)));
            }
        }
    }

    Ok(())
}

/// Fetch a hyperpack from a registry (zip download), extract into dest.
async fn fetch_hyperpack_registry(
    host: &str,
    name: &str,
    version: Option<&str>,
    dest: &std::path::Path,
    api_key: Option<&str>,
) -> Result<(), String> {
    let version_segment = version.unwrap_or("latest");
    let url = format!("{}/api/v1/packs/{}/{}/download", host.trim_end_matches('/'), name, version_segment);

    log::info!("[Hyperpacks] Downloading {} @ {} from {}", name, version_segment, url);

    let client = crate::http::shared_client();
    let mut request = client.get(&url);
    if let Some(key) = api_key {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("HTTP request to {} failed: {}", url, e))?;

    if !response.status().is_success() {
        return Err(format!("Registry returned HTTP {} for {}", response.status(), url));
    }

    let bytes = response.bytes().await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    // Clean destination and recreate
    if dest.exists() {
        std::fs::remove_dir_all(dest)
            .map_err(|e| format!("Failed to remove old hyperpack dir: {}", e))?;
    }
    std::fs::create_dir_all(dest)
        .map_err(|e| format!("Failed to create hyperpack dir: {}", e))?;

    // Try zip first, fall back to tarball
    if bytes.len() >= 4 && bytes[0..4] == [0x50, 0x4B, 0x03, 0x04] {
        // ZIP magic bytes
        extract_zip(&bytes, dest)?;
    } else {
        // Assume gzipped tarball
        extract_tarball(&bytes, dest)?;
    }

    Ok(())
}

/// Extract a zip archive into the destination directory.
fn extract_zip(data: &[u8], dest: &std::path::Path) -> Result<(), String> {
    use std::io::Read;
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| format!("Failed to open zip archive: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)
            .map_err(|e| format!("Failed to read zip entry {}: {}", i, e))?;

        let out_path = dest.join(file.mangled_name());

        if file.is_dir() {
            std::fs::create_dir_all(&out_path)
                .map_err(|e| format!("Failed to create dir {}: {}", out_path.display(), e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent dir: {}", e))?;
            }
            let mut out_file = std::fs::File::create(&out_path)
                .map_err(|e| format!("Failed to create file {}: {}", out_path.display(), e))?;
            std::io::copy(&mut file, &mut out_file)
                .map_err(|e| format!("Failed to write file {}: {}", out_path.display(), e))?;

            // Preserve executable permissions on unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode)).ok();
                }
            }
        }
    }

    Ok(())
}

/// Extract a gzipped tarball into the destination directory.
fn extract_tarball(data: &[u8], dest: &std::path::Path) -> Result<(), String> {
    use std::io::Read;
    let gz = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(dest)
        .map_err(|e| format!("Failed to extract tarball: {}", e))?;
    Ok(())
}

/// Copy a directory's children into a target directory (merging, not replacing).
/// e.g. copy_subdir_contents("hyperpacks/foo/skills", "skills") copies each
/// skill folder from the hyperpack into stark-backend/skills/.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<u32, String> {
    let mut count = 0u32;
    if !src.is_dir() {
        return Ok(0);
    }
    let entries = std::fs::read_dir(src)
        .map_err(|e| format!("Failed to read {}: {}", src.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir entry error: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)
                .map_err(|e| format!("Failed to create {}: {}", dst_path.display(), e))?;
            count += copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy {} → {}: {}", src_path.display(), dst_path.display(), e))?;
            count += 1;
        }
    }
    Ok(count)
}

/// Install all downloaded hyperpacks.
///
/// For each hyperpack:
/// 1. Copy `skills/` contents → `stark-backend/skills/`
/// 2. Copy `agents/` contents → `stark-backend/agents/`
/// 3. Copy `modules/` contents → `stark-backend/modules/`
/// 4. Run `install.sh` if present
async fn install_hyperpacks(
    base_dir: &std::path::Path,
    packs: &[crate::models::HyperPack],
    broadcaster: &std::sync::Arc<gateway::events::EventBroadcaster>,
) {
    let bd = crate::config::backend_dir();
    let skills_dir = bd.join("skills");
    let agents_dir = bd.join("agents");
    let modules_dir = bd.join("modules");

    let mut results: Vec<serde_json::Value> = Vec::new();

    for pack in packs {
        let dir_name = hyperpack_dir_name(&pack.path);
        let pack_dir = base_dir.join(&dir_name);

        if !pack_dir.exists() {
            log::warn!("[Hyperpacks] {} — directory missing, skipping install", dir_name);
            continue;
        }

        broadcaster.broadcast(gateway::protocol::GatewayEvent::new(
            "system.hyperpack_installing",
            serde_json::json!({
                "name": dir_name,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        ));

        let mut copied = serde_json::Map::new();

        // Copy skills/, agents/, modules/ from the hyperpack into the main dirs
        for (subdir, target) in [("skills", &skills_dir), ("agents", &agents_dir), ("modules", &modules_dir)] {
            let src = pack_dir.join(subdir);
            if src.is_dir() {
                std::fs::create_dir_all(target).ok();
                match copy_dir_recursive(&src, target) {
                    Ok(n) => {
                        log::info!("[Hyperpacks] {} — copied {} file(s) from {}/", dir_name, n, subdir);
                        copied.insert(subdir.to_string(), serde_json::json!(n));
                    }
                    Err(e) => {
                        log::error!("[Hyperpacks] {} — failed to copy {}/: {}", dir_name, subdir, e);
                        copied.insert(subdir.to_string(), serde_json::json!({ "error": e }));
                    }
                }
            }
        }

        // Run install.sh if present
        let install_script = pack_dir.join("install.sh");
        if install_script.exists() {
            log::info!("[Hyperpacks] {} — running install.sh", dir_name);

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&install_script, std::fs::Permissions::from_mode(0o755)).ok();
            }

            let output = tokio::process::Command::new("bash")
                .arg("install.sh")
                .current_dir(&pack_dir)
                .output()
                .await;

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);

                    if out.status.success() {
                        log::info!("[Hyperpacks] {} — install.sh completed successfully", dir_name);
                        if !stdout.is_empty() {
                            log::info!("[Hyperpacks] {} stdout: {}", dir_name, stdout.trim());
                        }
                        results.push(serde_json::json!({
                            "name": dir_name, "status": "ok", "copied": copied, "install_sh": "ok"
                        }));
                    } else {
                        log::error!(
                            "[Hyperpacks] {} — install.sh exited with code {:?}\nstdout: {}\nstderr: {}",
                            dir_name, out.status.code(), stdout.trim(), stderr.trim()
                        );
                        results.push(serde_json::json!({
                            "name": dir_name, "status": "error", "copied": copied,
                            "install_sh": { "exit_code": out.status.code(), "stderr": stderr.trim().chars().take(500).collect::<String>() }
                        }));
                    }
                }
                Err(e) => {
                    log::error!("[Hyperpacks] {} — failed to run install.sh: {}", dir_name, e);
                    results.push(serde_json::json!({
                        "name": dir_name, "status": "error", "copied": copied, "install_sh": { "error": e.to_string() }
                    }));
                }
            }
        } else {
            log::debug!("[Hyperpacks] {} — no install.sh", dir_name);
            results.push(serde_json::json!({
                "name": dir_name, "status": "ok", "copied": copied
            }));
        }
    }

    broadcaster.broadcast(gateway::protocol::GatewayEvent::new(
        "system.hyperpacks_installed",
        serde_json::json!({
            "count": results.len(),
            "results": results,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    ));
}

/// Re-sync skills from disk after hyperpacks have been installed.
async fn sync_skills(skill_registry: &std::sync::Arc<skills::SkillRegistry>) {
    log::info!("[Hyperpacks] Re-syncing skills from disk");
    match skill_registry.sync_to_db().await {
        Ok(count) => log::info!("[Hyperpacks] Synced {} skills from disk", count),
        Err(e) => log::error!("[Hyperpacks] Failed to sync skills: {}", e),
    }
}

/// Re-sync agents from disk after hyperpacks have been installed.
fn sync_agents() {
    log::info!("[Hyperpacks] Re-syncing agents from disk");
    agents::loader::reload_registry_from_disk();
}

/// Re-sync modules from disk after hyperpacks have been installed.
/// Registers any new modules that appeared on disk into the DB.
fn sync_modules(db: &std::sync::Arc<db::Database>) {
    log::info!("[Hyperpacks] Re-syncing modules from disk");
    let module_registry = modules::ModuleRegistry::new();
    for module in module_registry.available_modules() {
        let name = module.name();
        if !db.is_module_installed(name).unwrap_or(true) {
            match db.install_module(
                name,
                module.description(),
                module.version(),
                module.has_tools(),
                module.has_dashboard(),
            ) {
                Ok(_) => log::info!("[Hyperpacks] Installed module '{}' from hyperpack", name),
                Err(e) => log::warn!("[Hyperpacks] Failed to install module '{}': {}", name, e),
            }
        }
    }
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

/// Auto-start module service binaries as child processes.
///
/// Discovers modules from `~/.starkbot/modules/`, assigns each a free port,
/// and spawns them in the background. stdout/stderr are inherited so logs appear
/// in the same terminal. Child processes are killed when the parent exits.
///
/// Port assignment priority:
/// 1. Explicit env var already set (e.g. WALLET_MONITOR_PORT=9100) — respected as-is
/// 2. Port already in use (module running externally) — skipped, env var set so starkbot can reach it
/// 3. Otherwise — OS assigns a free port, passed to child via MODULE_PORT
fn start_module_services(db: &Database) {
    // Load API keys from database (with env fallback) to pass to child services
    let mut api_key_envs: Vec<(String, String)> = Vec::new();
    let alchemy_key = db.get_api_key("ALCHEMY_API_KEY").ok().flatten()
        .map(|k| k.api_key)
        .or_else(|| std::env::var("ALCHEMY_API_KEY").ok().filter(|v| !v.is_empty()));
    if let Some(key) = alchemy_key {
        log::info!("[MODULE] ALCHEMY_API_KEY found — will pass to module services");
        api_key_envs.push(("ALCHEMY_API_KEY".to_string(), key));
    }

    // Twitter API keys (OAuth 1.0a) — used by twitter_watcher module
    for twitter_key in &["TWITTER_CONSUMER_KEY", "TWITTER_CONSUMER_SECRET",
                         "TWITTER_ACCESS_TOKEN", "TWITTER_ACCESS_TOKEN_SECRET"] {
        let val = db.get_api_key(twitter_key).ok().flatten()
            .map(|k| k.api_key)
            .or_else(|| std::env::var(twitter_key).ok().filter(|v| !v.is_empty()));
        if let Some(v) = val {
            api_key_envs.push((twitter_key.to_string(), v));
        }
    }

    // All module services are discovered dynamically from ~/.starkbot/modules/
    let dynamic_services = modules::loader::get_dynamic_service_binaries();
    for svc in &dynamic_services {
        // Only start services for modules that are enabled in the database
        if !db.is_module_enabled(&svc.name).unwrap_or(false) {
            log::info!("[MODULE] {} is disabled — skipping service start", svc.name);
            continue;
        }

        // A module must have either a command or a binary to start
        let has_command = svc.command.is_some();
        if !has_command && !svc.binary_path.exists() {
            log::debug!(
                "[MODULE] Dynamic module '{}' has no command or service binary at {} — skipping",
                svc.name, svc.binary_path.display()
            );
            continue;
        }

        // Determine the port: check explicit env var first, then check if
        // default port is already in use, otherwise find a free port.
        let explicit_port = svc.port_env_var.as_ref()
            .and_then(|var| std::env::var(var).ok())
            .and_then(|s| s.parse::<u16>().ok());

        let port = if let Some(p) = explicit_port {
            // User explicitly set the port env var — use it
            p
        } else if std::net::TcpStream::connect(format!("127.0.0.1:{}", svc.default_port)).is_ok() {
            // Default port is occupied (module likely running externally) — use it
            log::info!(
                "[MODULE] {} already running on default port {} — skipping start",
                svc.name, svc.default_port
            );
            set_module_port_env(&svc, svc.default_port);
            modules::port_registry::register(&svc.name, svc.default_port);
            continue;
        } else {
            // Find a free port from the OS
            match find_free_port() {
                Some(p) => p,
                None => {
                    log::error!("[MODULE] Failed to find free port for '{}' — skipping", svc.name);
                    continue;
                }
            }
        };

        // If the chosen port is already in use (explicit env case), skip starting
        if explicit_port.is_some() && std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            log::info!("[MODULE] {} already running on port {} — skipping start", svc.name, port);
            set_module_port_env(&svc, port);
            modules::port_registry::register(&svc.name, port);
            continue;
        }

        // Pass relevant API keys + port + internal signing token to child services
        let mut envs: Vec<(String, String)> = api_key_envs.clone();
        envs.push(("MODULE_PORT".to_string(), port.to_string()));
        // Internal token for module→backend API calls (wallet signing proxy)
        if let Ok(token) = std::env::var("STARKBOT_INTERNAL_TOKEN") {
            envs.push(("STARKBOT_INTERNAL_TOKEN".to_string(), token));
        }
        // Self URL so modules can call back to the backend
        envs.push(("STARKBOT_SELF_URL".to_string(), config::self_url()));
        if let Some(ref port_var) = svc.port_env_var {
            envs.push((port_var.clone(), port.to_string()));
        }

        let env_refs: Vec<(&str, &str)> = envs.iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        if let Some(ref command) = svc.command {
            start_service_command(command, &svc.module_dir, &svc.name, port, &env_refs);
        } else {
            start_service_binary(&svc.binary_path, &svc.name, port, &env_refs);
        }

        // Set env vars in parent process so manifest.service_url() resolves correctly
        // when DynamicModule makes RPC calls to this service.
        set_module_port_env(&svc, port);

        // Register in the port registry so local_rpc can resolve module names to ports
        modules::port_registry::register(&svc.name, port);
    }
}

/// Set the port/URL env vars in the parent process so manifest.service_url()
/// can resolve the correct URL for this module's service.
fn set_module_port_env(svc: &modules::loader::DynamicServiceInfo, port: u16) {
    // SAFETY: Called during single-threaded startup before any module tools are invoked.
    // No concurrent reads of these env vars at this point.
    unsafe {
        if let Some(ref port_var) = svc.port_env_var {
            std::env::set_var(port_var, port.to_string());
        }
        if let Some(ref url_var) = svc.url_env_var {
            std::env::set_var(url_var, format!("http://127.0.0.1:{}", port));
        }
    }
}

/// Ask the OS for a free TCP port by binding to port 0.
fn find_free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|addr| addr.port())
}

/// Start a single service binary.
/// The caller is responsible for checking port availability before calling this.
fn start_service_binary(exe_path: &std::path::Path, name: &str, port: u16, envs: &[(&str, &str)]) {
    let mut cmd = std::process::Command::new(exe_path);
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    for (key, value) in envs {
        cmd.env(key, value);
    }

    match cmd.spawn() {
        Ok(mut child) => {
            log::info!("[MODULE] Started {} (port {})", name, port);
            modules::service_logs::spawn_log_capture_threads(
                name,
                child.stdout.take(),
                child.stderr.take(),
            );
        }
        Err(e) => {
            log::error!("[MODULE] Failed to start {}: {}", name, e);
        }
    }
}

/// Start a service via a shell command (e.g. "uv run service.py").
/// The command is run from the module directory with `sh -c`.
fn start_service_command(command: &str, cwd: &std::path::Path, name: &str, port: u16, envs: &[(&str, &str)]) {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(command);
    cmd.current_dir(cwd);
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    for (key, value) in envs {
        cmd.env(key, value);
    }

    match cmd.spawn() {
        Ok(mut child) => {
            log::info!("[MODULE] Started {} via `{}` (port {}, cwd={})", name, command, port, cwd.display());
            modules::service_logs::spawn_log_capture_threads(
                name,
                child.stdout.take(),
                child.stderr.take(),
            );
        }
        Err(e) => {
            log::error!("[MODULE] Failed to start {} via `{}`: {}", name, command, e);
        }
    }
}

/// Migrate QMD markdown memory files into the DB `memories` table.
/// Parses identity from subdirectory, date from filename, and splits entries at `## HH:MM` headers.
fn migrate_qmd_memories_to_db(
    db: &db::Database,
    memory_dir: &std::path::Path,
) -> Result<usize, String> {
    use std::fs;

    // Inline list: recursively find all .md files in the memory directory
    fn list_md_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
        if !dir.exists() { return Ok(()); }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                list_md_files(&path, out)?;
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    list_md_files(memory_dir, &mut files)
        .map_err(|e| format!("Failed to list memory files: {}", e))?;

    if files.is_empty() {
        return Ok(0);
    }

    let mut count = 0usize;

    for file_path in &files {
        let rel = file_path
            .strip_prefix(memory_dir)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let content = match fs::read_to_string(file_path) {
            Ok(c) if !c.trim().is_empty() => c,
            _ => continue,
        };

        // Determine identity_id from subdirectory (e.g. "user123/MEMORY.md" -> Some("user123"))
        let parts: Vec<&str> = rel.split('/').collect();
        let (identity_id, filename) = if parts.len() >= 2 {
            (Some(parts[0].to_string()), parts.last().unwrap().to_string())
        } else {
            (None, parts[0].to_string())
        };

        let identity_ref = identity_id.as_deref();

        // Determine memory_type and log_date from filename
        let (memory_type, log_date) = if filename == "MEMORY.md" {
            ("long_term", None)
        } else if let Some(date) = filename.strip_suffix(".md")
            .and_then(|stem| chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()) {
            ("daily_log", Some(date.format("%Y-%m-%d").to_string()))
        } else {
            // Unknown file type, skip
            continue;
        };

        // Split content at "## HH:MM" timestamp headers into individual entries
        let entries = split_qmd_entries(&content);

        for entry in &entries {
            if entry.trim().is_empty() {
                continue;
            }
            if let Err(e) = db.insert_memory(
                memory_type,
                entry,
                None,                         // category
                None,                         // tags
                5,                            // importance (default)
                identity_ref,
                None,                         // session_id
                None,                         // entity_type
                None,                         // entity_name
                Some("qmd_migration"),        // source_type
                log_date.as_deref(),
                None,                         // agent_subtype (not available in migration)
            ) {
                log::warn!("[MIGRATION] Failed to insert entry from {}: {}", rel, e);
            } else {
                count += 1;
            }
        }
    }

    Ok(count)
}

/// Split QMD markdown content at `## HH:MM` timestamp headers into individual entries.
/// If no headers are found, returns the entire content as a single entry.
fn split_qmd_entries(content: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut current = String::new();

    for line in content.lines() {
        // Match "## HH:MM" pattern (timestamp header)
        if line.starts_with("## ")
            && line.len() >= 8
            && line.chars().nth(3).map(|c| c.is_ascii_digit()).unwrap_or(false)
        {
            // Flush previous entry
            if !current.trim().is_empty() {
                entries.push(current.trim().to_string());
            }
            current = String::new();
            // Skip the timestamp header itself — content follows
            continue;
        }
        current.push_str(line);
        current.push('\n');
    }

    // Flush last entry
    if !current.trim().is_empty() {
        entries.push(current.trim().to_string());
    }

    // If nothing was split (no timestamp headers), return whole content
    if entries.is_empty() && !content.trim().is_empty() {
        entries.push(content.trim().to_string());
    }

    entries
}

/// Ensure a CLI gateway external channel exists with the given token.
/// Called on startup when CLI_GATEWAY_TOKEN env var is set.
/// Creates the channel if it doesn't exist, or updates the token if it changed.
fn ensure_cli_gateway_channel(db: &std::sync::Arc<db::Database>, token: &str) {
    const CLI_CHANNEL_NAME: &str = "cli-gateway";

    // Check if a channel named "cli-gateway" already exists
    match db.list_channels() {
        Ok(channels) => {
            for ch in &channels {
                if ch.name == CLI_CHANNEL_NAME && ch.channel_type == "external_channel" {
                    // Channel exists — check if token matches
                    if ch.bot_token == token {
                        log::debug!("CLI gateway channel already exists with correct token");
                    } else {
                        // Update the token
                        match db.update_channel(ch.id, None, None, Some(token), None) {
                            Ok(_) => log::info!("Updated CLI gateway channel token"),
                            Err(e) => log::warn!("Failed to update CLI gateway channel token: {}", e),
                        }
                    }

                    // Ensure the API token setting matches (this is what the gateway validates)
                    let _ = db.set_channel_setting(ch.id, "external_channel_api_token", token);
                    // Ensure auto_start_on_boot is enabled
                    let _ = db.set_channel_setting(ch.id, "auto_start_on_boot", "true");
                    let _ = db.set_channel_enabled(ch.id, true);
                    return;
                }
            }
        }
        Err(e) => {
            log::warn!("Failed to list channels for CLI gateway setup: {}", e);
        }
    }

    // Channel doesn't exist — create it
    match db.create_channel_with_safe_mode("external_channel", CLI_CHANNEL_NAME, token, None, true) {
        Ok(ch) => {
            // Enable it, set the API token, and auto_start_on_boot
            let _ = db.set_channel_enabled(ch.id, true);
            let _ = db.set_channel_setting(ch.id, "external_channel_api_token", token);
            let _ = db.set_channel_setting(ch.id, "auto_start_on_boot", "true");
            log::info!("Created CLI gateway channel (id: {})", ch.id);
        }
        Err(e) => {
            log::error!("Failed to create CLI gateway channel: {}", e);
        }
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
    log::info!("Starkbot v{}", env!("CARGO_PKG_VERSION"));
    log::info!("Using config directory: {:?}", config_dir);
    log::info!("Loading presets from config directory");
    tools::presets::load_presets(config_dir);
    log::info!("Loading token configs from config directory");
    tools::builtin::cryptocurrency::token_lookup::load_tokens(config_dir);
    log::info!("Loading network configs from config directory");
    tools::builtin::cryptocurrency::network_lookup::load_networks(config_dir);
    log::info!("Loading RPC provider configs from config directory");
    tools::rpc_config::load_rpc_providers(config_dir);
    log::info!("Loading AI endpoint presets from inference router");
    ai_endpoint_config::load_ai_endpoints().await;
    log::info!("Loading x402 payment limit defaults from config directory");
    x402::payment_limits::load_defaults(config_dir);

    let mut config = Config::from_env();
    let port = config.port;

    // Initialize workspace directory and copy SOUL.md
    log::info!("Initializing workspace");
    if let Err(e) = config::initialize_workspace() {
        log::error!("Failed to initialize workspace: {}", e);
    }
    log::info!("Public URL (self_url): {}", config::self_url());

    // Initialize disk quota manager
    let disk_quota_mb = config::disk_quota_mb();
    let disk_quota: Option<Arc<disk_quota::DiskQuotaManager>> = if disk_quota_mb > 0 {
        let tracked_dirs = vec![
            std::path::PathBuf::from(config::workspace_dir()),
            std::path::PathBuf::from(config::notes_config().notes_dir),
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

    // Load RPC configuration into the unified resolver so ALL codepaths
    // (tools, eip8004, x402 signer, etc.) share the same resolution logic.
    {
        let alchemy_key = db.get_api_key("ALCHEMY_API_KEY").ok().flatten()
            .map(|k| k.api_key)
            .or_else(|| std::env::var("ALCHEMY_API_KEY").ok().filter(|v| !v.is_empty()));
        if let Some(key) = alchemy_key {
            log::info!("[rpc_config] Alchemy API key loaded — Tier 1 RPC available");
            tools::rpc_config::set_alchemy_api_key(key);
        }

        if let Ok(settings) = db.get_bot_settings() {
            if let Some(endpoints) = settings.custom_rpc_endpoints {
                tools::rpc_config::set_custom_rpc_endpoints(endpoints);
            }
        }
    }

    // Load agent subtypes from agents/ folders (disk-based, no DB).
    {
        let agents_dir = config::runtime_agents_dir();
        std::fs::create_dir_all(&agents_dir).ok();
        let configs = agents::loader::load_agents_from_directory(&agents_dir)
            .unwrap_or_else(|e| { log::error!("Failed to load agents: {}", e); vec![] });
        ai::multi_agent::types::load_subtype_registry(configs);
    }

    // Initialize keystore URL (must be before auto-retrieve)
    // Priority: 1. bot_config.ron keystore_server_url, 2. KEYSTORE_URL env var, 3. default
    let env_keystore_url = std::env::var("KEYSTORE_URL").ok().filter(|s| !s.is_empty());
    let cfg_keystore_url = crate::models::BotConfig::load().services.keystore_server_url.filter(|s| !s.is_empty());

    if let Some(url) = cfg_keystore_url.or(env_keystore_url) {
        log::info!("Using custom keystore URL: {}", url);
        keystore_client::KEYSTORE_CLIENT.set_base_url(&url).await;
    }

    // Auto-retrieve from keystore (restore state from cloud backup on fresh instance)
    // This runs before channel auto-start so restored channels can start
    // NOTE: Flash mode auto-retrieval happens later, after deriving the backup key from wallet signature
    let is_flash_mode = std::env::var("FLASH_KEYSTORE_URL").is_ok();

    // Initialize Module Registry (compile-time plugin registry)
    let module_registry = modules::ModuleRegistry::new();

    // Auto-install all bundled modules that aren't already in the DB.
    // Only kv_store is enabled by default — all others start disabled
    // and must be explicitly enabled by the user. The enabled state is
    // persisted in the keystore cloud backup so it survives resets.
    for module in module_registry.available_modules() {
        let name = module.name();
        if !db.is_module_installed(name).unwrap_or(true) {
            let default_enabled = name == "kv_store";
            match db.install_module(
                name,
                module.description(),
                module.version(),
                module.has_tools(),
                module.has_dashboard(),
            ) {
                Ok(_) => {
                    if !default_enabled {
                        let _ = db.set_module_enabled(name, false);
                    }
                    log::info!("[MODULE] Auto-installed {} (enabled={})", name, default_enabled);
                }
                Err(e) => log::warn!("[MODULE] Failed to auto-install {}: {}", name, e),
            }
        }
    }

    // Generate internal token early so child module services can use it for
    // backend API calls (wallet signing proxy, hooks, etc.).
    if std::env::var("STARKBOT_INTERNAL_TOKEN").is_err() {
        let mut buf = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut buf);
        let token = hex::encode(buf);
        // SAFETY: Called during single-threaded startup before any modules are spawned.
        unsafe { std::env::set_var("STARKBOT_INTERNAL_TOKEN", &token); }
        log::info!("Generated STARKBOT_INTERNAL_TOKEN for module communication");
    }

    // Auto-start module service binaries as child processes.
    // Only starts services for modules that are enabled in the database.
    // Set DISABLE_MODULE_SERVICES=1 to skip auto-start entirely.
    if std::env::var("DISABLE_MODULE_SERVICES").map(|v| v == "1" || v == "true").unwrap_or(false) {
        log::info!("[MODULE] Module service auto-start disabled via DISABLE_MODULE_SERVICES");
    } else {
        start_module_services(&db);
    }

    // Initialize Tool Registry with built-in tools + installed module tools
    log::info!("Initializing tool registry");
    let mut tool_registry_mut = tools::create_default_registry();

    // Register tools from ALL built-in modules unconditionally.
    // Tool visibility is controlled by subtype groups + skill requires_tools,
    // not by module install/enable state. This prevents skills from silently
    // failing when they reference a module tool that isn't installed yet.
    for module in module_registry.available_modules() {
        if module.has_tools() {
            for tool in module.create_tools() {
                log::info!("[MODULE] Registered tool: {} (from {})", tool.name(), module.name());
                tool_registry_mut.register(tool);
            }
        }
    }

    let tool_registry = Arc::new(tool_registry_mut);
    log::info!("Registered {} tools", tool_registry.len());

    // Initialize Skill Registry (disk-primary, DB is synced index)
    log::info!("Initializing skill registry");
    let skill_registry = Arc::new(skills::create_default_registry(db.clone()));

    // Sync skills from disk to database
    let skill_count = skill_registry.sync_to_db().await.unwrap_or_else(|e| {
        log::warn!("Failed to sync skills from disk: {}", e);
        0
    });
    log::info!("Synced {} skills from disk, {} total in database", skill_count, skill_registry.len());

    // Load skills from enabled modules (write to disk if missing, then sync to DB)
    {
        let installed_modules = db.list_installed_modules().unwrap_or_default();
        for entry in &installed_modules {
            if entry.enabled {
                if let Some(module) = module_registry.get(&entry.module_name) {
                    // Prefer skill_dir (full skill folder), fall back to skill_content (legacy)
                    if let Some(skill_dir) = module.skill_dir() {
                        match skill_registry.create_skill_from_module_dir(skill_dir).await {
                            Ok(s) => log::info!("[MODULE] Loaded skill '{}' from module '{}' (skill dir)", s.name, entry.module_name),
                            Err(e) => log::warn!("[MODULE] Failed to load skill dir from '{}': {}", entry.module_name, e),
                        }
                    } else if let Some(skill_md) = module.skill_content() {
                        match skill_registry.create_skill_from_markdown(skill_md) {
                            Ok(s) => log::info!("[MODULE] Loaded skill '{}' from module '{}'", s.name, entry.module_name),
                            Err(e) => log::warn!("[MODULE] Failed to load skill from '{}': {}", entry.module_name, e),
                        }
                    }
                }
            }
        }
    }

    // Load skill ABIs and presets from DB into in-memory indexes
    web3::load_all_abis_from_db(&db);
    tools::presets::load_all_skill_presets_from_db(&db);

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
                // Extract and save agent_preset before the Arc cast erases the concrete type
                if let Some(preset_json) = provider.agent_preset().cloned() {
                    match models::bot_config::AgentPreset::save_from_json(&preset_json) {
                        Ok(()) => log::info!("Saved agent_preset.ron from control plane"),
                        Err(e) => log::warn!("Failed to save agent_preset: {}", e),
                    }

                    // Fetch hyperpacks API key from flash control plane and inject into preset
                    match provider.fetch_hyperpacks_api_key().await {
                        Ok(key) => {
                            if let Some(mut preset) = models::bot_config::AgentPreset::load() {
                                preset.api_key = Some(key);
                                match preset.save() {
                                    Ok(()) => log::info!("Injected hyperpacks API key into agent_preset.ron"),
                                    Err(e) => log::warn!("Failed to save API key to agent_preset: {}", e),
                                }
                            }
                        }
                        Err(e) => log::warn!("Could not fetch hyperpacks API key: {}", e),
                    }
                }
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

    // Flash mode: ECIES encryption key is now derived on-demand via
    // wallet_provider.get_encryption_key() — no startup derivation needed.

    // Create credits session client for Bearer-token auth (reduces signing to ~1/hour)
    let credits_session: Option<Arc<credits_session::CreditsSessionClient>> = if let Some(ref wp) = wallet_provider {
        // Determine the inference endpoint base URL from active settings
        let base_url = db.get_active_agent_settings()
            .ok()
            .flatten()
            .and_then(|s| {
                if s.endpoint.contains("defirelay.com") {
                    s.endpoint.find("/api/")
                        .or_else(|| s.endpoint.find("/chat"))
                        .map(|idx| s.endpoint[..idx].to_string())
                        .or_else(|| Some(s.endpoint.trim_end_matches('/').to_string()))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "https://inference.defirelay.com".to_string());

        log::info!("Credits session client initialized (base_url: {}, wallet: {})", base_url, wp.get_address());
        Some(Arc::new(credits_session::CreditsSessionClient::new(wp.clone(), &base_url)))
    } else {
        None
    };

    // Initialize Gateway with tool registry, wallet provider, tx_queue, and skill registry for channels
    log::info!("Initializing Gateway");
    let gateway = Arc::new(Gateway::new_with_tools_wallet_and_tx_queue(
        db.clone(),
        tool_registry.clone(),
        wallet_provider.clone(),
        Some(tx_queue.clone()),
        Some(skill_registry.clone()),
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

    // Create embedding generator for hybrid search + association loop
    let embeddings_server_url = crate::models::BotConfig::load()
        .services.embeddings_server_url
        .unwrap_or_else(|| crate::models::DEFAULT_EMBEDDINGS_SERVER_URL.to_string());
    log::info!(
        "HybridSearchEngine: using remote embeddings server at {}",
        embeddings_server_url
    );
    let remote_embedding_generator = Arc::new(memory::embeddings::RemoteEmbeddingGenerator::new(
        embeddings_server_url,
    ));
    let embedding_generator: Arc<dyn memory::EmbeddingGenerator + Send + Sync> =
        remote_embedding_generator.clone();

    // Create hybrid search engine (FTS + vector + graph)
    let hybrid_search_engine: Option<Arc<memory::HybridSearchEngine>> =
        Some(Arc::new(memory::HybridSearchEngine::new(
            db.clone(),
            embedding_generator.clone(),
        )));

    // One-time migration: import QMD markdown files into the DB memories table.
    // This runs once; afterward the memory/ directory is renamed to memory.migrated/.
    {
        let memory_dir = config::memory_config().memory_dir;
        let memory_path = std::path::Path::new(&memory_dir);
        if memory_path.exists() && memory_path.is_dir() {
            match migrate_qmd_memories_to_db(&db, memory_path) {
                Ok(count) if count > 0 => {
                    log::info!("[MIGRATION] Migrated {} QMD memory entries to DB", count);
                    // Rename to prevent re-migration
                    let migrated_path = memory_path.with_extension("migrated");
                    if let Err(e) = std::fs::rename(memory_path, &migrated_path) {
                        log::warn!("[MIGRATION] Failed to rename memory/ to memory.migrated/: {}", e);
                    } else {
                        log::info!("[MIGRATION] Renamed {} -> {}", memory_dir, migrated_path.display());
                    }
                }
                Ok(_) => {
                    log::info!("[MIGRATION] No QMD memory files to migrate");
                }
                Err(e) => {
                    log::error!("[MIGRATION] QMD migration failed: {}", e);
                }
            }
        }
    }

    // Rebuild FTS index on startup to ensure it's in sync with the memories table.
    // This is cheap (takes <100ms for typical memory counts) and guarantees search works
    // even if the index got out of sync from a restore, crash, or schema migration.
    match db.rebuild_fts_index() {
        Ok(()) => {
            let count = db.count_memories().unwrap_or(0);
            log::info!("[FTS] Rebuilt FTS index on startup ({} memories)", count);
        }
        Err(e) => log::warn!("[FTS] Failed to rebuild FTS index on startup: {}", e),
    }

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
    if let Some(ref engine) = hybrid_search_engine {
        dispatcher_builder = dispatcher_builder.with_hybrid_search(engine.clone());
    }
    if let Some(ref dq) = disk_quota {
        dispatcher_builder = dispatcher_builder.with_disk_quota(dq.clone());
        // Also wire disk quota into the NoteStore
        if let Some(ref store) = dispatcher_builder.notes_store() {
            store.set_disk_quota(dq.clone());
        }
    }
    if let Some(ref cs) = credits_session {
        dispatcher_builder = dispatcher_builder.with_credits_session(cs.clone());
    }
    let dispatcher = Arc::new(dispatcher_builder);

    // Get broadcaster and channel_manager for the /ws route
    let broadcaster = gateway.broadcaster();
    let channel_manager = gateway.channel_manager();

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
        Some(skill_registry.clone()),
    ));

    // Start scheduler background task
    let scheduler_handle = Arc::clone(&scheduler);
    let (scheduler_shutdown_tx, scheduler_shutdown_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        scheduler_handle.start(scheduler_shutdown_rx).await;
    });

    // Spawn background association loop (auto-discovers memory connections via embeddings)
    {
        let db_loop = db.clone();
        let emb_loop = embedding_generator.clone();
        let config = memory::association_loop::AssociationLoopConfig::default();
        let _assoc_handle = memory::association_loop::spawn_association_loop(db_loop, emb_loop, config);
        log::info!("Background association loop spawned");
    }

    // One-time skill embedding backfill (generates embeddings for any skills missing them)
    {
        let db_emb = db.clone();
        let emb_gen = embedding_generator.clone();
        tokio::spawn(async move {
            // Small delay to let other startup tasks finish
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            match crate::skills::embeddings::backfill_skill_embeddings(&db_emb, &emb_gen).await {
                Ok(0) => log::debug!("[SKILL-EMB] All skills already have embeddings"),
                Ok(n) => log::info!("[SKILL-EMB] Startup backfill: generated {} skill embeddings", n),
                Err(e) => log::warn!("[SKILL-EMB] Startup backfill failed: {}", e),
            }
        });
    }

    // Spawn background memory decay/pruning task (runs every 6 hours)
    {
        let db_decay = db.clone();
        tokio::spawn(async move {
            let config = memory::decay::DecayConfig::default();
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(6 * 3600)).await;
                match memory::decay::run_decay_pass(&db_decay, &config) {
                    Ok((updated, pruned)) => {
                        log::info!("[DECAY] Pass complete: {} updated, {} pruned", updated, pruned);
                    }
                    Err(e) => {
                        log::error!("[DECAY] Pass failed: {}", e);
                    }
                }
            }
        });
        log::info!("Background memory decay task spawned (every 6h)");
    }

    // Spawn slow network-dependent init in background so HTTP server starts immediately
    {
        let db_bg = db.clone();
        let gateway_bg = gateway.clone();
        let wallet_provider_bg = wallet_provider.clone();
        let broadcaster_bg = broadcaster.clone();
        let skill_registry_bg = skill_registry.clone();
        tokio::spawn(async move {
            // Keystore auto-retrieve (works in both Standard and Flash mode via wallet provider)
            let auto_sync = std::env::var(crate::config::env_vars::AUTO_SYNC_FROM_KEYSTORE)
                .map(|v| v != "false" && v != "0")
                .unwrap_or(true);
            if auto_sync {
                if let Some(ref wp) = wallet_provider_bg {
                    load_keystore_state_from_cloud(&db_bg, wp, &broadcaster_bg).await;
                }
            } else {
                log::info!("AUTO_SYNC_FROM_KEYSTORE=false — skipping keystore restore");
            }

            // Resolve hyperpacks declared in bot_config.ron
            load_hyperpacks(&broadcaster_bg).await;

            // Re-sync skills, agents, and modules that hyperpacks may have installed
            sync_skills(&skill_registry_bg).await;
            sync_agents();
            sync_modules(&db_bg);

            // Auto-create CLI gateway channel if CLI_GATEWAY_TOKEN env var is set
            // This enables direct CLI-to-instance communication without manual channel setup
            if let Ok(cli_token) = std::env::var("CLI_GATEWAY_TOKEN") {
                if !cli_token.is_empty() {
                    ensure_cli_gateway_channel(&db_bg, &cli_token);
                }
            }

            // Start enabled channels (after keystore so restored channels are available)
            log::info!("Starting enabled channels (background)");
            gateway_bg.start_enabled_channels().await;
            log::info!("All enabled channels started");
        });
    }

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

    // Spawn stale session cleanup task — marks sessions stuck in 'active' as 'failed'.
    // This catches sessions left behind by panics, dropped futures, or missed finalization.
    {
        let db_cleanup = db.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(120));
            interval.tick().await; // skip immediate tick
            loop {
                interval.tick().await;
                match db_cleanup.cleanup_stale_active_sessions(10) {
                    Ok(0) => {} // nothing to clean
                    Ok(count) => {
                        log::warn!(
                            "[SESSION_CLEANUP] Marked {} stale active session(s) as failed (>10 min without update)",
                            count
                        );
                    }
                    Err(e) => {
                        log::error!("[SESSION_CLEANUP] Failed to clean up stale sessions: {}", e);
                    }
                }
                // FIFO: delete oldest inactive sessions when total exceeds 500
                match db_cleanup.cleanup_excess_sessions(500) {
                    Ok(0) => {}
                    Ok(count) => {
                        log::info!("[SESSION_CLEANUP] Deleted {} excess session(s) (FIFO cap: 500)", count);
                    }
                    Err(e) => {
                        log::error!("[SESSION_CLEANUP] Failed to clean up excess sessions: {}", e);
                    }
                }
            }
        });
    }

    // Module workers are now managed by standalone services — no workers to spawn here.
    // Keep an empty map in AppState for API compatibility.
    let module_workers = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<String, tokio::task::JoinHandle<()>>::new()));

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

    // Clones needed for shutdown handler (before HttpServer moves db)
    let shutdown_db = db.clone();
    let shutdown_cache = dispatcher.active_cache().clone();

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
    let credits_session = credits_session.clone();
    let disk_q = disk_quota.clone();
    let mod_workers = module_workers.clone();
    let hybrid_search_engine = hybrid_search_engine.clone();
    let frontend_dist = frontend_dist.to_string();
    let dev_mode = dev_mode;
    // Internal token for module-to-backend API calls (wallet signing proxy, etc.)
    // Token is generated early in startup (before module services are spawned).
    let internal_token = std::env::var("STARKBOT_INTERNAL_TOKEN")
        .expect("STARKBOT_INTERNAL_TOKEN should have been set during startup");

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
                hybrid_search: hybrid_search_engine.clone(),
                remote_embedding_generator: Some(Arc::clone(&remote_embedding_generator)),
                internal_token: internal_token.clone(),
                active_cache: disp.active_cache().clone(),
                credits_session: credits_session.clone(),
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
            .configure(controllers::notes::config)
            .configure(controllers::tx_queue::config)
            .configure(controllers::broadcasted_transactions::config)
            .configure(controllers::impulse_map::config)
            .configure(controllers::kanban::config)
            .configure(controllers::modules::config)
            .configure(controllers::memory::config)
            .configure(controllers::system::config)
            .configure(controllers::well_known::config)
            .configure(controllers::x402::config)
            .configure(controllers::x402_limits::config)
            .configure(controllers::telemetry::config)
            .configure(controllers::agent_subtypes::config)
            .configure(controllers::special_roles::config)
            .configure(controllers::external_channel::config)
            .configure(controllers::internal_wallet::config)
            .configure(controllers::transcribe::config)
            .configure(controllers::hooks_api::config)
            // Public ext proxy — must be before the SPA catch-all
            .configure(controllers::ext::config)
            .configure(controllers::public_files::config)
            // WebSocket Gateway route (same port as HTTP, required for single-port platforms)
            .route("/ws", web::get().to(gateway::actix_ws::ws_handler));

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

        // Flush active session cache to SQLite before shutdown
        log::info!("Flushing active session cache...");
        shutdown_cache.flush_all_dirty(&shutdown_db);

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
