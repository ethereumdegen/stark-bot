use ethers::core::k256::ecdsa::SigningKey;
use ethers::signers::{LocalWallet, Signer};
use std::env;
use std::path::{Path, PathBuf};

/// Environment variable names - single source of truth
pub mod env_vars {
    pub const LOGIN_ADMIN_PUBLIC_ADDRESS: &str = "LOGIN_ADMIN_PUBLIC_ADDRESS";
    pub const BURNER_WALLET_PRIVATE_KEY: &str = "BURNER_WALLET_BOT_PRIVATE_KEY";
    pub const PORT: &str = "PORT";
    pub const DATABASE_URL: &str = "DATABASE_URL";
    /// Explicit override for the bot's own public URL (e.g. "https://mybot.example.com").
    /// In Flash mode, the control plane sets this during provisioning.
    pub const PUBLIC_URL: &str = "STARK_PUBLIC_URL";
    /// Set to "false" or "0" to skip auto-restoring from keystore on boot.
    /// Default: true (auto-sync enabled).
    pub const AUTO_SYNC_FROM_KEYSTORE: &str = "AUTO_SYNC_FROM_KEYSTORE";
}

/// Default values
pub mod defaults {
    pub const PORT: u16 = 8080;
    pub const DATABASE_URL: &str = "./.db/stark.db";
    pub const WORKSPACE_DIR: &str = "workspace";
    pub const SKILLS_DIR: &str = "skills";
    pub const NOTES_DIR: &str = "notes";
    pub const SOUL_DIR: &str = "soul";
    pub const PUBLIC_DIR: &str = "public";
    pub const MEMORY_DIR: &str = "memory";
    pub const DISK_QUOTA_MB: u64 = 1024;
}

/// Returns the absolute path to the stark-backend directory.
/// Uses CARGO_MANIFEST_DIR at compile time, so it always resolves
/// to stark-backend/ regardless of the working directory at runtime.
pub fn backend_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Returns the absolute path to the monorepo root (parent of stark-backend/).
pub fn repo_root() -> PathBuf {
    backend_dir().parent().expect("backend_dir has no parent").to_path_buf()
}

/// Get the workspace directory
pub fn workspace_dir() -> String {
    backend_dir().join(defaults::WORKSPACE_DIR).to_string_lossy().to_string()
}

/// Get the bundled skills directory (repo_root/skills/ — read-only source)
pub fn bundled_skills_dir() -> String {
    repo_root().join(defaults::SKILLS_DIR).to_string_lossy().to_string()
}

/// Get the runtime skills directory (stark-backend/skills/ — mutable working copy)
pub fn runtime_skills_dir() -> String {
    backend_dir().join(defaults::SKILLS_DIR).to_string_lossy().to_string()
}

/// Deprecated alias — use bundled_skills_dir() or runtime_skills_dir()
pub fn skills_dir() -> String {
    bundled_skills_dir()
}

/// Get the bundled modules directory (repo_root/modules/ — read-only source)
pub fn bundled_modules_dir() -> PathBuf {
    repo_root().join("modules")
}

/// Get the runtime modules directory (stark-backend/modules/ — mutable working copy)
pub fn runtime_modules_dir() -> PathBuf {
    backend_dir().join("modules")
}

/// Get the bundled agents directory (config/agents/ — read-only source)
pub fn bundled_agents_dir() -> PathBuf {
    repo_root().join("config").join("agents")
}

/// Get the runtime agents directory (stark-backend/agents/ — mutable working copy)
pub fn runtime_agents_dir() -> PathBuf {
    backend_dir().join("agents")
}

/// Get the notes directory
pub fn notes_dir() -> String {
    backend_dir().join(defaults::NOTES_DIR).to_string_lossy().to_string()
}

/// Get the public files directory
pub fn public_dir() -> String {
    backend_dir().join(defaults::PUBLIC_DIR).to_string_lossy().to_string()
}

/// Get the bot's own public URL (for constructing absolute URLs to /public/ files, etc.)
///
/// Set STARK_PUBLIC_URL to the instance's externally-reachable URL.
/// In Flash mode, the control plane should set this during provisioning.
/// Falls back to http://localhost:{PORT} if not set.
pub fn self_url() -> String {
    if let Ok(url) = env::var(env_vars::PUBLIC_URL) {
        return url.trim_end_matches('/').to_string();
    }

    // Fallback: localhost
    let port = env::var(env_vars::PORT)
        .unwrap_or_else(|_| defaults::PORT.to_string());
    format!("http://localhost:{}", port)
}

/// Get the bot config directory (inside stark-backend)
pub fn bot_config_dir() -> std::path::PathBuf {
    backend_dir().join("config")
}

/// Get the runtime bot_config.ron path
pub fn bot_config_path() -> std::path::PathBuf {
    bot_config_dir().join("bot_config.ron")
}

/// Get the runtime agent_preset.ron path (written by Flash control plane)
pub fn agent_preset_path() -> std::path::PathBuf {
    bot_config_dir().join("agent_preset.ron")
}

/// Get the seed bot_config.ron path (repo root config/)
pub fn bot_config_seed_path() -> std::path::PathBuf {
    repo_root().join("config").join("bot_config.ron")
}

/// Get the soul directory
pub fn soul_dir() -> String {
    backend_dir().join(defaults::SOUL_DIR).to_string_lossy().to_string()
}

/// Get the disk quota in megabytes (0 = disabled)
pub fn disk_quota_mb() -> u64 {
    defaults::DISK_QUOTA_MB
}

/// Get the burner wallet private key from environment (for tools)
pub fn burner_wallet_private_key() -> Option<String> {
    env::var(env_vars::BURNER_WALLET_PRIVATE_KEY).ok()
}

/// Derive the public address from a private key
fn derive_address_from_private_key(private_key: &str) -> Result<String, String> {
    let key_hex = private_key.strip_prefix("0x").unwrap_or(private_key);
    let key_bytes = hex::decode(key_hex)
        .map_err(|e| format!("Invalid private key hex: {}", e))?;

    let signing_key = SigningKey::from_bytes(key_bytes.as_slice().into())
        .map_err(|e| format!("Invalid private key: {}", e))?;

    let wallet = LocalWallet::from(signing_key);
    Ok(format!("{:?}", wallet.address()).to_lowercase())
}

#[derive(Clone)]
pub struct Config {
    pub login_admin_public_address: Option<String>,
    pub burner_wallet_private_key: Option<String>,
    pub port: u16,
    pub database_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        let burner_wallet_private_key = env::var(env_vars::BURNER_WALLET_PRIVATE_KEY).ok();

        // Try to get public address from env, or derive from private key (no panic if both missing)
        let login_admin_public_address = env::var(env_vars::LOGIN_ADMIN_PUBLIC_ADDRESS)
            .ok()
            .or_else(|| {
                burner_wallet_private_key.as_ref().and_then(|pk| {
                    derive_address_from_private_key(pk)
                        .map_err(|e| log::warn!("Failed to derive address from private key: {}", e))
                        .ok()
                })
            });

        Self {
            login_admin_public_address,
            burner_wallet_private_key,
            port: env::var(env_vars::PORT)
                .unwrap_or_else(|_| defaults::PORT.to_string())
                .parse()
                .expect("PORT must be a valid number"),
            database_url: env::var(env_vars::DATABASE_URL)
                .unwrap_or_else(|_| defaults::DATABASE_URL.to_string()),
        }
    }
}

/// Configuration for QMD memory system (file-based markdown memory)
#[derive(Clone, Debug)]
pub struct MemoryConfig {
    /// Directory for memory markdown files (default: ./memory)
    pub memory_dir: String,
    /// Reindex interval in seconds (default: 300 = 5 minutes)
    pub reindex_interval_secs: u64,
    /// Enable pre-compaction memory flush (AI extracts memories before summarization)
    pub enable_pre_compaction_flush: bool,
    /// Enable cross-session memory sharing (same identity across channels)
    pub enable_cross_session_memory: bool,
    /// Maximum number of cross-session memories to include
    pub cross_session_memory_limit: i32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            memory_dir: backend_dir().join(defaults::MEMORY_DIR).to_string_lossy().to_string(),
            reindex_interval_secs: 300,
            enable_pre_compaction_flush: true,
            enable_cross_session_memory: true,
            cross_session_memory_limit: 5,
        }
    }
}

impl MemoryConfig {
    /// Get the path to the memory FTS database
    pub fn memory_db_path(&self) -> String {
        format!("{}/.memory.db", self.memory_dir)
    }
}

/// Get the memory configuration
pub fn memory_config() -> MemoryConfig {
    MemoryConfig::default()
}

/// Configuration for Notes system (Obsidian-compatible markdown notes with FTS5)
#[derive(Clone, Debug)]
pub struct NotesConfig {
    /// Directory for notes markdown files (default: ./notes)
    pub notes_dir: String,
    /// Reindex interval in seconds (default: 300 = 5 minutes)
    pub reindex_interval_secs: u64,
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            notes_dir: backend_dir().join(defaults::NOTES_DIR).to_string_lossy().to_string(),
            reindex_interval_secs: 300,
        }
    }
}

impl NotesConfig {
    /// Get the path to the notes FTS database
    pub fn notes_db_path(&self) -> String {
        format!("{}/.notes.db", self.notes_dir)
    }
}

/// Get the notes configuration
pub fn notes_config() -> NotesConfig {
    NotesConfig::default()
}

/// Get the path to SOUL.md in the soul directory
pub fn soul_document_path() -> PathBuf {
    PathBuf::from(soul_dir()).join("SOUL.md")
}

/// Get the path to IDENTITY.json in the soul directory
pub fn identity_document_path() -> PathBuf {
    PathBuf::from(soul_dir()).join("IDENTITY.json")
}

/// Get the path to GUIDELINES.md in the soul directory
pub fn guidelines_document_path() -> PathBuf {
    PathBuf::from(soul_dir()).join("GUIDELINES.md")
}

/// Get the path to the soul_template directory at the repo root
fn soul_template_dir() -> PathBuf {
    repo_root().join("soul_template")
}

/// Find the template SOUL.md in soul_template/
fn find_original_soul() -> Option<PathBuf> {
    let path = soul_template_dir().join("SOUL.md");
    if path.exists() { Some(path) } else { None }
}

/// Find the template GUIDELINES.md in soul_template/
fn find_original_guidelines() -> Option<PathBuf> {
    let path = soul_template_dir().join("GUIDELINES.md");
    if path.exists() { Some(path) } else { None }
}

/// Extract semver (major, minor, patch) from a version string like "1.2.3" or "1.2.3-beta"
fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    // Strip pre-release suffix (e.g. "1.2.3-beta.1" → "1.2.3")
    let version = version.split('-').next().unwrap_or(version);
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() >= 3 {
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        let patch = parts[2].parse().ok()?;
        Some((major, minor, patch))
    } else if parts.len() == 2 {
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        Some((major, minor, 0))
    } else if parts.len() == 1 {
        let major = parts[0].parse().ok()?;
        Some((major, 0, 0))
    } else {
        None
    }
}

/// Compare two semver strings. Returns true if `a` is newer than `b`.
pub fn semver_is_newer(a: &str, b: &str) -> bool {
    match (parse_semver(a), parse_semver(b)) {
        (Some(va), Some(vb)) => va > vb,
        _ => false,
    }
}

/// Recursively copy a directory and all its contents (skips symlinks)
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        // Skip symlinks to prevent infinite recursion and symlink escape
        if file_type.is_symlink() {
            log::warn!("Skipping symlink during copy: {:?}", entry.path());
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Extract the version from a module directory's `module.toml` (public for backup).
pub fn extract_version_from_module_toml_pub(dir: &Path) -> Option<String> {
    extract_version_from_module_toml(dir)
}

/// Extract the version from a module directory's `module.toml`.
fn extract_version_from_module_toml(dir: &Path) -> Option<String> {
    let toml_path = dir.join("module.toml");
    let content = std::fs::read_to_string(&toml_path).ok()?;
    // Quick parse: find 'version = "x.y.z"' in [module] section
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version") {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix('=') {
                let version = rest.trim().trim_matches('"').trim_matches('\'').to_string();
                if !version.is_empty() {
                    return Some(version);
                }
            }
        }
    }
    None
}

/// Initialize the workspace, notes, and soul directories
/// This should be called at startup before any agent processing begins
/// SOUL.md is copied from the original to the soul directory only if it doesn't exist,
/// preserving agent modifications and user edits across restarts
pub fn initialize_workspace() -> std::io::Result<()> {
    let workspace = workspace_dir();
    let workspace_path = Path::new(&workspace);

    // Create workspace directory if it doesn't exist
    std::fs::create_dir_all(workspace_path)?;

    // Create notes directory if it doesn't exist
    let notes = notes_dir();
    let notes_path = Path::new(&notes);
    std::fs::create_dir_all(notes_path)?;

    // Create public files directory if it doesn't exist
    let public = public_dir();
    let public_path = Path::new(&public);
    std::fs::create_dir_all(public_path)?;
    log::info!("Public files directory: {:?}", public_path);

    // Create soul directory if it doesn't exist
    let soul = soul_dir();
    let soul_path = Path::new(&soul);
    std::fs::create_dir_all(soul_path)?;

    // Copy SOUL.md from repo root to soul directory only if it doesn't exist
    // This preserves agent modifications across restarts
    let soul_document = soul_path.join("SOUL.md");
    if !soul_document.exists() {
        if let Some(original_soul) = find_original_soul() {
            log::info!(
                "Initializing SOUL.md from {:?} to {:?}",
                original_soul,
                soul_document
            );
            std::fs::copy(&original_soul, &soul_document)?;
        } else {
            log::warn!("Original SOUL.md not found - soul directory will not have a soul document");
        }
    } else {
        log::info!("Using existing soul document at {:?}", soul_document);
    }

    // Copy GUIDELINES.md from repo root to soul directory only if it doesn't exist
    // GUIDELINES.md contains operational/business guidelines (vs SOUL.md for personality/culture)
    let guidelines_document = soul_path.join("GUIDELINES.md");
    if !guidelines_document.exists() {
        if let Some(original_guidelines) = find_original_guidelines() {
            log::info!(
                "Initializing GUIDELINES.md from {:?} to {:?}",
                original_guidelines,
                guidelines_document
            );
            std::fs::copy(&original_guidelines, &guidelines_document)?;
        } else {
            log::debug!("Original GUIDELINES.md not found - no operational guidelines will be loaded");
        }
    } else {
        log::info!("Using existing guidelines document at {:?}", guidelines_document);
    }

    // Create bot config directory and seed bot_config.ron if it doesn't exist
    let bot_cfg_dir = bot_config_dir();
    std::fs::create_dir_all(&bot_cfg_dir)?;

    let bot_cfg_path = bot_config_path();
    if !bot_cfg_path.exists() {
        let seed = bot_config_seed_path();
        if seed.exists() {
            log::info!(
                "Initializing bot_config.ron from {:?} to {:?}",
                seed,
                bot_cfg_path
            );
            std::fs::copy(&seed, &bot_cfg_path)?;
        } else {
            log::debug!("Seed bot_config.ron not found at {:?}, skipping", seed);
        }
    } else {
        log::info!("Using existing bot_config.ron at {:?}", bot_cfg_path);
    }

    Ok(())
}
