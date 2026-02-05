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
    pub const JOURNAL_DIR: &str = "STARK_JOURNAL_DIR";
    pub const SOUL_DIR: &str = "STARK_SOUL_DIR";
    // QMD Memory configuration (simplified file-based memory system)
    pub const MEMORY_DIR: &str = "STARK_MEMORY_DIR";
    pub const MEMORY_REINDEX_INTERVAL_SECS: &str = "STARK_MEMORY_REINDEX_INTERVAL_SECS";
    // Legacy: still used by context manager
    pub const MEMORY_ENABLE_PRE_COMPACTION_FLUSH: &str = "STARK_MEMORY_ENABLE_PRE_COMPACTION_FLUSH";
    pub const MEMORY_ENABLE_CROSS_SESSION: &str = "STARK_MEMORY_ENABLE_CROSS_SESSION";
    pub const MEMORY_CROSS_SESSION_LIMIT: &str = "STARK_MEMORY_CROSS_SESSION_LIMIT";
    // Guest dashboard feature flag
    pub const GUEST_DASHBOARD: &str = "GUEST_DASHBOARD";
}

/// Default values
pub mod defaults {
    pub const PORT: u16 = 8080;
    pub const DATABASE_URL: &str = "./.db/stark.db";
    pub const WORKSPACE_DIR: &str = "./workspace";
    pub const SKILLS_DIR: &str = "./skills";
    pub const JOURNAL_DIR: &str = "./journal";
    pub const SOUL_DIR: &str = "./soul";
}

/// Get the workspace directory from environment or default
pub fn workspace_dir() -> String {
    env::var(env_vars::WORKSPACE_DIR).unwrap_or_else(|_| defaults::WORKSPACE_DIR.to_string())
}

/// Get the skills directory from environment or default
pub fn skills_dir() -> String {
    env::var(env_vars::SKILLS_DIR).unwrap_or_else(|_| defaults::SKILLS_DIR.to_string())
}

/// Get the journal directory from environment or default
pub fn journal_dir() -> String {
    env::var(env_vars::JOURNAL_DIR).unwrap_or_else(|_| defaults::JOURNAL_DIR.to_string())
}

/// Get the soul directory from environment or default
/// Checks both ./soul and ../soul if env var not set (handles running from stark-backend/)
pub fn soul_dir() -> String {
    if let Ok(dir) = env::var(env_vars::SOUL_DIR) {
        return dir;
    }
    // Check if soul dir exists at default location or parent directory
    // This handles running from repo root vs stark-backend directory
    if Path::new("./soul").exists() {
        "./soul".to_string()
    } else if Path::new("../soul").exists() {
        "../soul".to_string()
    } else {
        defaults::SOUL_DIR.to_string()
    }
}

/// Get the burner wallet private key from environment (for tools)
pub fn burner_wallet_private_key() -> Option<String> {
    env::var(env_vars::BURNER_WALLET_PRIVATE_KEY).ok()
}

/// Check if guest dashboard is enabled via environment variable
pub fn guest_dashboard_enabled() -> bool {
    env::var(env_vars::GUEST_DASHBOARD)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
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
            memory_dir: "./memory".to_string(),
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
            memory_dir: env::var(env_vars::MEMORY_DIR)
                .unwrap_or_else(|_| "./memory".to_string()),
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

/// Get the path to SOUL.md in the soul directory
pub fn soul_document_path() -> PathBuf {
    PathBuf::from(soul_dir()).join("SOUL.md")
}

/// Get the path to GUIDELINES.md in the soul directory
pub fn guidelines_document_path() -> PathBuf {
    PathBuf::from(soul_dir()).join("GUIDELINES.md")
}

/// Find the original SOUL.md in the repo root
fn find_original_soul() -> Option<PathBuf> {
    let candidates = [".", "..", "../..", "../../.."];
    for candidate in candidates {
        let path = PathBuf::from(candidate).join("SOUL.md");
        if path.exists() {
            return path.canonicalize().ok();
        }
    }
    None
}

/// Find the original GUIDELINES.md in the repo root
fn find_original_guidelines() -> Option<PathBuf> {
    let candidates = [".", "..", "../..", "../../.."];
    for candidate in candidates {
        let path = PathBuf::from(candidate).join("GUIDELINES.md");
        if path.exists() {
            return path.canonicalize().ok();
        }
    }
    None
}

/// Initialize the workspace, journal, and soul directories
/// This should be called at startup before any agent processing begins
/// SOUL.md is copied fresh on every startup from the original to the soul directory
/// This protects the original from agent modifications while allowing the user
/// to edit the original (via web UI) with changes propagating on restart
pub fn initialize_workspace() -> std::io::Result<()> {
    let workspace = workspace_dir();
    let workspace_path = Path::new(&workspace);

    // Create workspace directory if it doesn't exist
    std::fs::create_dir_all(workspace_path)?;

    // Create journal directory if it doesn't exist
    let journal = journal_dir();
    let journal_path = Path::new(&journal);
    std::fs::create_dir_all(journal_path)?;

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
