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
    pub const WORKSPACE_DIR: &str = "STARK_WORKSPACE_DIR";
    pub const SKILLS_DIR: &str = "STARK_SKILLS_DIR";
    pub const RUNTIME_SKILLS_DIR: &str = "STARK_RUNTIME_SKILLS_DIR";
    pub const NOTES_DIR: &str = "STARK_NOTES_DIR";
    pub const NOTES_REINDEX_INTERVAL_SECS: &str = "STARK_NOTES_REINDEX_INTERVAL_SECS";
    pub const SOUL_DIR: &str = "STARK_SOUL_DIR";
    pub const PUBLIC_DIR: &str = "STARK_PUBLIC_DIR";
    /// Explicit override for the bot's own public URL (e.g. "https://mybot.example.com")
    pub const PUBLIC_URL: &str = "STARK_PUBLIC_URL";
    // Disk quota (0 = disabled)
    pub const DISK_QUOTA_MB: &str = "STARK_DISK_QUOTA_MB";
    // QMD Memory configuration (simplified file-based memory system)
    pub const MEMORY_DIR: &str = "STARK_MEMORY_DIR";
    pub const MEMORY_REINDEX_INTERVAL_SECS: &str = "STARK_MEMORY_REINDEX_INTERVAL_SECS";
    // Legacy: still used by context manager
    pub const MEMORY_ENABLE_PRE_COMPACTION_FLUSH: &str = "STARK_MEMORY_ENABLE_PRE_COMPACTION_FLUSH";
    pub const MEMORY_ENABLE_CROSS_SESSION: &str = "STARK_MEMORY_ENABLE_CROSS_SESSION";
    pub const MEMORY_CROSS_SESSION_LIMIT: &str = "STARK_MEMORY_CROSS_SESSION_LIMIT";
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

/// Resolve a default sub-directory relative to the repo root.
/// If the env var is set, use that as-is; otherwise join the default name onto repo_root().
fn resolve_dir(env_var: &str, default_name: &str) -> String {
    env::var(env_var).unwrap_or_else(|_| {
        repo_root().join(default_name).to_string_lossy().to_string()
    })
}

/// Resolve a default sub-directory relative to the backend directory.
/// Use for dirs that live inside stark-backend/ (e.g. memory).
fn resolve_backend_dir(env_var: &str, default_name: &str) -> String {
    env::var(env_var).unwrap_or_else(|_| {
        backend_dir().join(default_name).to_string_lossy().to_string()
    })
}

/// Get the workspace directory from environment or default
pub fn workspace_dir() -> String {
    resolve_backend_dir(env_vars::WORKSPACE_DIR, defaults::WORKSPACE_DIR)
}

/// Get the bundled skills directory (repo_root/skills/ — read-only source)
pub fn bundled_skills_dir() -> String {
    resolve_dir(env_vars::SKILLS_DIR, defaults::SKILLS_DIR)
}

/// Get the runtime skills directory (stark-backend/skills/ — mutable working copy)
pub fn runtime_skills_dir() -> String {
    resolve_backend_dir(env_vars::RUNTIME_SKILLS_DIR, defaults::SKILLS_DIR)
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
    if let Ok(dir) = std::env::var("STARKBOT_MODULES_DIR") {
        return PathBuf::from(dir);
    }
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

/// Extract the version from an agent directory's agent.md frontmatter.
fn extract_version_from_agent_dir(dir: &Path) -> Option<String> {
    let agent_md = dir.join("agent.md");
    let content = std::fs::read_to_string(&agent_md).ok()?;
    if !content.starts_with("---") {
        return None;
    }
    for line in content.lines().skip(1) {
        if line.trim() == "---" {
            break;
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version:") {
            let version = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !version.is_empty() {
                return Some(version);
            }
        }
    }
    None
}

/// Seed runtime agents directory from bundled agents.
/// Copies agent folders from bundled_agents_dir() to runtime_agents_dir()
/// only if the runtime copy is missing or has an older semver version.
pub fn seed_agents() -> std::io::Result<()> {
    let bundled = bundled_agents_dir();
    let runtime = runtime_agents_dir();

    if !bundled.exists() {
        log::info!("Bundled agents directory {:?} does not exist, skipping seed", bundled);
        return Ok(());
    }

    std::fs::create_dir_all(&runtime)?;

    let entries = std::fs::read_dir(&bundled)?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::warn!("Failed to read bundled agent entry: {}", e);
                continue;
            }
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                log::warn!("Failed to get file type for '{}': {}", name_str, e);
                continue;
            }
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        if name_str.starts_with('.') || name_str.starts_with('_') {
            continue;
        }

        // Must have agent.md to be a valid agent
        if !entry.path().join("agent.md").exists() {
            continue;
        }

        let runtime_agent = runtime.join(&name);

        let should_copy = if runtime_agent.exists() {
            let bundled_version = extract_version_from_agent_dir(&entry.path());
            let runtime_version = extract_version_from_agent_dir(&runtime_agent);

            match (bundled_version, runtime_version) {
                (Some(bv), Some(rv)) => {
                    if semver_is_newer(&bv, &rv) {
                        log::info!(
                            "Upgrading agent '{}' from v{} to v{} (bundled is newer)",
                            name_str, rv, bv
                        );
                        true
                    } else {
                        false
                    }
                }
                (Some(bv), None) => {
                    log::info!(
                        "Upgrading agent '{}' (bundled has v{}, runtime has no version)",
                        name_str, bv
                    );
                    true
                }
                _ => false,
            }
        } else {
            log::info!("Seeding agent '{}' from bundled", name_str);
            true
        };

        if should_copy {
            if runtime_agent.exists() {
                let tmp_name = format!(".{}.seed_tmp", name_str);
                let tmp_dir = runtime.join(&tmp_name);
                if tmp_dir.exists() {
                    let _ = std::fs::remove_dir_all(&tmp_dir);
                }
                match copy_dir_recursive(&entry.path(), &tmp_dir) {
                    Ok(()) => {
                        if let Err(e) = std::fs::remove_dir_all(&runtime_agent) {
                            log::error!("Failed to remove old agent '{}': {}", name_str, e);
                            let _ = std::fs::remove_dir_all(&tmp_dir);
                            continue;
                        }
                        if let Err(e) = std::fs::rename(&tmp_dir, &runtime_agent) {
                            log::error!("Failed to rename temp dir for agent '{}': {}", name_str, e);
                            continue;
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to copy bundled agent '{}': {}", name_str, e);
                        let _ = std::fs::remove_dir_all(&tmp_dir);
                        continue;
                    }
                }
            } else {
                if let Err(e) = copy_dir_recursive(&entry.path(), &runtime_agent) {
                    log::error!("Failed to seed agent '{}': {}", name_str, e);
                    let _ = std::fs::remove_dir_all(&runtime_agent);
                    continue;
                }
            }
        }
    }

    log::info!("Agent seeding complete (runtime dir: {:?})", runtime);
    Ok(())
}

/// Seed runtime agents directory from module agent folders.
/// Scans runtime_modules_dir() for modules containing `agent/agent.md`,
/// and copies them to runtime_agents_dir() using version-gated logic.
pub fn seed_module_agents() -> std::io::Result<()> {
    let modules_dir = runtime_modules_dir();
    let runtime = runtime_agents_dir();

    if !modules_dir.exists() {
        return Ok(());
    }

    std::fs::create_dir_all(&runtime)?;

    let entries = std::fs::read_dir(&modules_dir)?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }

        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();

        // Check for agent/ subdirectory with agent.md
        let agent_src = entry.path().join("agent");
        if !agent_src.join("agent.md").exists() {
            continue;
        }

        let runtime_agent = runtime.join(&name);

        let should_copy = if runtime_agent.exists() {
            let bundled_version = extract_version_from_agent_dir(&agent_src);
            let runtime_version = extract_version_from_agent_dir(&runtime_agent);

            match (bundled_version, runtime_version) {
                (Some(bv), Some(rv)) => {
                    if semver_is_newer(&bv, &rv) {
                        log::info!(
                            "Upgrading module agent '{}' from v{} to v{}", name_str, rv, bv
                        );
                        true
                    } else {
                        false
                    }
                }
                (Some(bv), None) => {
                    log::info!(
                        "Upgrading module agent '{}' (module has v{}, runtime has no version)",
                        name_str, bv
                    );
                    true
                }
                _ => false,
            }
        } else {
            log::info!("Seeding module agent '{}' from module dir", name_str);
            true
        };

        if should_copy {
            if runtime_agent.exists() {
                let tmp_name = format!(".{}.seed_tmp", name_str);
                let tmp_dir = runtime.join(&tmp_name);
                if tmp_dir.exists() {
                    let _ = std::fs::remove_dir_all(&tmp_dir);
                }
                match copy_dir_recursive(&agent_src, &tmp_dir) {
                    Ok(()) => {
                        if let Err(e) = std::fs::remove_dir_all(&runtime_agent) {
                            log::error!("Failed to remove old module agent '{}': {}", name_str, e);
                            let _ = std::fs::remove_dir_all(&tmp_dir);
                            continue;
                        }
                        if let Err(e) = std::fs::rename(&tmp_dir, &runtime_agent) {
                            log::error!("Failed to rename temp dir for module agent '{}': {}", name_str, e);
                            continue;
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to copy module agent '{}': {}", name_str, e);
                        let _ = std::fs::remove_dir_all(&tmp_dir);
                        continue;
                    }
                }
            } else {
                if let Err(e) = copy_dir_recursive(&agent_src, &runtime_agent) {
                    log::error!("Failed to seed module agent '{}': {}", name_str, e);
                    let _ = std::fs::remove_dir_all(&runtime_agent);
                    continue;
                }
            }
        }
    }

    log::info!("Module agent seeding complete");
    Ok(())
}

/// Get the notes directory from environment or default
pub fn notes_dir() -> String {
    resolve_backend_dir(env_vars::NOTES_DIR, defaults::NOTES_DIR)
}

/// Get the public files directory from environment or default
pub fn public_dir() -> String {
    resolve_backend_dir(env_vars::PUBLIC_DIR, defaults::PUBLIC_DIR)
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

/// Get the soul directory from environment or default
pub fn soul_dir() -> String {
    resolve_backend_dir(env_vars::SOUL_DIR, defaults::SOUL_DIR)
}

/// Get the disk quota in megabytes (0 = disabled)
pub fn disk_quota_mb() -> u64 {
    env::var(env_vars::DISK_QUOTA_MB)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(defaults::DISK_QUOTA_MB)
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
            memory_dir: resolve_backend_dir(env_vars::MEMORY_DIR, defaults::MEMORY_DIR),
            reindex_interval_secs: 300,
            enable_pre_compaction_flush: true,
            enable_cross_session_memory: true,
            cross_session_memory_limit: 5,
        }
    }
}

impl MemoryConfig {
    pub fn from_env() -> Self {
        Self {
            memory_dir: resolve_backend_dir(env_vars::MEMORY_DIR, defaults::MEMORY_DIR),
            reindex_interval_secs: env::var(env_vars::MEMORY_REINDEX_INTERVAL_SECS)
                .unwrap_or_else(|_| "300".to_string())
                .parse()
                .unwrap_or(300),
            enable_pre_compaction_flush: env::var(env_vars::MEMORY_ENABLE_PRE_COMPACTION_FLUSH)
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            enable_cross_session_memory: env::var(env_vars::MEMORY_ENABLE_CROSS_SESSION)
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            cross_session_memory_limit: env::var(env_vars::MEMORY_CROSS_SESSION_LIMIT)
                .unwrap_or_else(|_| "5".to_string())
                .parse()
                .unwrap_or(5),
        }
    }

    /// Get the path to the memory FTS database
    pub fn memory_db_path(&self) -> String {
        format!("{}/.memory.db", self.memory_dir)
    }
}

/// Get the memory configuration
pub fn memory_config() -> MemoryConfig {
    MemoryConfig::from_env()
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
            notes_dir: resolve_backend_dir(env_vars::NOTES_DIR, defaults::NOTES_DIR),
            reindex_interval_secs: 300,
        }
    }
}

impl NotesConfig {
    pub fn from_env() -> Self {
        Self {
            notes_dir: resolve_backend_dir(env_vars::NOTES_DIR, defaults::NOTES_DIR),
            reindex_interval_secs: std::env::var(env_vars::NOTES_REINDEX_INTERVAL_SECS)
                .unwrap_or_else(|_| "300".to_string())
                .parse()
                .unwrap_or(300),
        }
    }

    /// Get the path to the notes FTS database
    pub fn notes_db_path(&self) -> String {
        format!("{}/.notes.db", self.notes_dir)
    }
}

/// Get the notes configuration
pub fn notes_config() -> NotesConfig {
    NotesConfig::from_env()
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

/// Extract the version from a skill directory's frontmatter.
/// Checks {dirname}.md first (matching loader priority), then SKILL.md.
fn extract_version_from_skill_dir(dir: &Path) -> Option<String> {
    // Match loader priority: {name}.md first, then SKILL.md
    let name_md = dir.file_name()
        .map(|n| dir.join(format!("{}.md", n.to_string_lossy())));
    let skill_md = dir.join("SKILL.md");

    let md_path = if let Some(ref p) = name_md {
        if p.exists() { p.clone() } else if skill_md.exists() { skill_md } else { return None; }
    } else if skill_md.exists() {
        skill_md
    } else {
        return None;
    };

    let content = std::fs::read_to_string(&md_path).ok()?;
    // Quick parse: find "version:" in YAML frontmatter
    if !content.starts_with("---") {
        return None;
    }
    for line in content.lines().skip(1) {
        if line.trim() == "---" {
            break;
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version:") {
            let version = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !version.is_empty() {
                return Some(version);
            }
        }
    }
    None
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

/// Seed runtime skills directory from bundled skills.
/// Copies skill folders from bundled_skills_dir() to runtime_skills_dir()
/// only if the runtime copy is missing or has an older semver version.
pub fn seed_skills() -> std::io::Result<()> {
    let bundled = PathBuf::from(bundled_skills_dir());
    let runtime = PathBuf::from(runtime_skills_dir());

    if !bundled.exists() {
        log::info!("Bundled skills directory {:?} does not exist, skipping seed", bundled);
        return Ok(());
    }

    std::fs::create_dir_all(&runtime)?;

    let entries = std::fs::read_dir(&bundled)?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::warn!("Failed to read bundled skill entry: {}", e);
                continue;
            }
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();

        // Skip non-directories, inactive, managed, and _-prefixed directories
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                log::warn!("Failed to get file type for '{}': {}", name_str, e);
                continue;
            }
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        if name_str == "inactive" || name_str == "managed" || name_str.starts_with('_') {
            continue;
        }

        let runtime_skill = runtime.join(&name);

        let should_copy = if runtime_skill.exists() {
            // Check if bundled version is newer
            let bundled_version = extract_version_from_skill_dir(&entry.path());
            let runtime_version = extract_version_from_skill_dir(&runtime_skill);

            match (bundled_version, runtime_version) {
                (Some(bv), Some(rv)) => {
                    if semver_is_newer(&bv, &rv) {
                        log::info!(
                            "Upgrading skill '{}' from v{} to v{} (bundled is newer)",
                            name_str, rv, bv
                        );
                        true
                    } else {
                        false
                    }
                }
                (Some(bv), None) => {
                    // Bundled has version, runtime doesn't — treat as upgrade
                    log::info!(
                        "Upgrading skill '{}' (bundled has v{}, runtime has no version)",
                        name_str, bv
                    );
                    true
                }
                _ => false,
            }
        } else {
            log::info!("Seeding skill '{}' from bundled", name_str);
            true
        };

        if should_copy {
            // Atomic upgrade: copy to temp dir, then rename
            if runtime_skill.exists() {
                let tmp_name = format!(".{}.seed_tmp", name_str);
                let tmp_dir = runtime.join(&tmp_name);
                // Clean up any leftover temp dir from a previous failed attempt
                if tmp_dir.exists() {
                    let _ = std::fs::remove_dir_all(&tmp_dir);
                }
                match copy_dir_recursive(&entry.path(), &tmp_dir) {
                    Ok(()) => {
                        if let Err(e) = std::fs::remove_dir_all(&runtime_skill) {
                            log::error!("Failed to remove old skill '{}': {}", name_str, e);
                            let _ = std::fs::remove_dir_all(&tmp_dir);
                            continue;
                        }
                        if let Err(e) = std::fs::rename(&tmp_dir, &runtime_skill) {
                            log::error!("Failed to rename temp dir for skill '{}': {}", name_str, e);
                            continue;
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to copy bundled skill '{}': {}", name_str, e);
                        let _ = std::fs::remove_dir_all(&tmp_dir);
                        continue;
                    }
                }
            } else {
                // Fresh copy — no existing runtime dir to protect
                if let Err(e) = copy_dir_recursive(&entry.path(), &runtime_skill) {
                    log::error!("Failed to seed skill '{}': {}", name_str, e);
                    let _ = std::fs::remove_dir_all(&runtime_skill);
                    continue;
                }
            }
        }
    }

    log::info!("Skill seeding complete (runtime dir: {:?})", runtime);
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

/// Seed runtime modules directory from bundled modules.
/// Copies module folders from bundled_modules_dir() to runtime_modules_dir()
/// only if the runtime copy is missing or has an older semver version.
pub fn seed_modules() -> std::io::Result<()> {
    let bundled = bundled_modules_dir();
    let runtime = runtime_modules_dir();

    if !bundled.exists() {
        log::info!("Bundled modules directory {:?} does not exist, skipping seed", bundled);
        return Ok(());
    }

    std::fs::create_dir_all(&runtime)?;

    let entries = std::fs::read_dir(&bundled)?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log::warn!("Failed to read bundled module entry: {}", e);
                continue;
            }
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();

        // Skip non-directories, symlinks, and _-prefixed directories
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                log::warn!("Failed to get file type for '{}': {}", name_str, e);
                continue;
            }
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        if name_str.starts_with('_') {
            continue;
        }

        // Must have a module.toml to be a valid module
        if !entry.path().join("module.toml").exists() {
            continue;
        }

        let runtime_module = runtime.join(&name);

        let should_copy = if runtime_module.exists() {
            // Check if the runtime copy is manifest-only (only module.toml, no service files).
            // This happens when installed from StarkHub without a binary archive.
            // In that case, overwrite with the full bundled copy.
            let is_manifest_only = runtime_module.join("module.toml").exists()
                && !runtime_module.join("service.js").exists()
                && !runtime_module.join("service.py").exists()
                && !runtime_module.join("bin").exists();

            if is_manifest_only {
                log::info!(
                    "Replacing manifest-only module '{}' with full bundled copy",
                    name_str
                );
                true
            } else {
                let bundled_version = extract_version_from_module_toml(&entry.path());
                let runtime_version = extract_version_from_module_toml(&runtime_module);

                match (bundled_version, runtime_version) {
                    (Some(bv), Some(rv)) => {
                        if semver_is_newer(&bv, &rv) {
                            log::info!(
                                "Upgrading module '{}' from v{} to v{} (bundled is newer)",
                                name_str, rv, bv
                            );
                            true
                        } else {
                            false
                        }
                    }
                    (Some(bv), None) => {
                        log::info!(
                            "Upgrading module '{}' (bundled has v{}, runtime has no version)",
                            name_str, bv
                        );
                        true
                    }
                    _ => false,
                }
            }
        } else {
            log::info!("Seeding module '{}' from bundled", name_str);
            true
        };

        if should_copy {
            if runtime_module.exists() {
                // Atomic upgrade: copy to temp dir, then rename
                let tmp_name = format!(".{}.seed_tmp", name_str);
                let tmp_dir = runtime.join(&tmp_name);
                if tmp_dir.exists() {
                    let _ = std::fs::remove_dir_all(&tmp_dir);
                }
                match copy_dir_recursive(&entry.path(), &tmp_dir) {
                    Ok(()) => {
                        if let Err(e) = std::fs::remove_dir_all(&runtime_module) {
                            log::error!("Failed to remove old module '{}': {}", name_str, e);
                            let _ = std::fs::remove_dir_all(&tmp_dir);
                            continue;
                        }
                        if let Err(e) = std::fs::rename(&tmp_dir, &runtime_module) {
                            log::error!("Failed to rename temp dir for module '{}': {}", name_str, e);
                            continue;
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to copy bundled module '{}': {}", name_str, e);
                        let _ = std::fs::remove_dir_all(&tmp_dir);
                        continue;
                    }
                }
            } else {
                if let Err(e) = copy_dir_recursive(&entry.path(), &runtime_module) {
                    log::error!("Failed to seed module '{}': {}", name_str, e);
                    let _ = std::fs::remove_dir_all(&runtime_module);
                    continue;
                }
            }
        }
    }

    // Second pass: copy shared library directories (e.g. starkbot_sdk) that modules
    // depend on but which don't have a module.toml themselves.
    let entries2 = std::fs::read_dir(&bundled)?;
    for entry in entries2 {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !file_type.is_dir() || file_type.is_symlink() || name_str.starts_with('_') || name_str.starts_with('.') {
            continue;
        }

        // Skip actual modules (already handled above)
        if entry.path().join("module.toml").exists() {
            continue;
        }

        let runtime_lib = runtime.join(&name);
        if !runtime_lib.exists() {
            log::info!("Seeding shared library '{}' from bundled modules", name_str);
            if let Err(e) = copy_dir_recursive(&entry.path(), &runtime_lib) {
                log::error!("Failed to seed shared library '{}': {}", name_str, e);
                let _ = std::fs::remove_dir_all(&runtime_lib);
            }
        }
    }

    log::info!("Module seeding complete (runtime dir: {:?})", runtime);
    Ok(())
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

    Ok(())
}
