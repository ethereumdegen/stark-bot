//! SQLite database - schema definitions and connection management

use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Result as SqliteResult;
use std::path::Path;

use super::cache::DbCache;

/// Pooled connection type alias for convenience
pub type DbConn = PooledConnection<SqliteConnectionManager>;

/// Main database wrapper with r2d2 connection pool
pub struct Database {
    pool: Pool<SqliteConnectionManager>,
    pub(crate) cache: DbCache,
}

impl Database {
    /// Create a new database connection pool and initialize schema
    pub fn new(database_url: &str) -> SqliteResult<Self> {
        Self::new_with_options(database_url, true)
    }

    /// Create a new database connection pool with optional initialization
    pub fn new_with_options(database_url: &str, init: bool) -> SqliteResult<Self> {
        if let Some(parent) = Path::new(database_url).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }

        let manager = SqliteConnectionManager::file(database_url)
            .with_init(|conn| {
                conn.execute_batch(
                    "PRAGMA busy_timeout=5000;
                     PRAGMA journal_mode=WAL;
                     PRAGMA cache_size=-64000;
                     PRAGMA mmap_size=268435456;
                     PRAGMA temp_store=memory;
                     PRAGMA synchronous=NORMAL;
                     PRAGMA foreign_keys=ON;"
                )
            });

        let pool = Pool::builder()
            .max_size(16)
            .build(manager)
            .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;

        let db = Self { pool, cache: DbCache::new() };

        if init {
            db.init()?;
        }

        Ok(db)
    }

    /// Get a connection from the pool
    #[inline]
    pub fn conn(&self) -> DbConn {
        self.pool.get_timeout(std::time::Duration::from_secs(5))
            .expect("Failed to get database connection from pool (timeout after 5s)")
    }

    /// Initialize all database tables and run migrations
    fn init(&self) -> SqliteResult<()> {
        let conn = self.conn();

        // Migrate: rename sessions -> auth_sessions if the old table exists
        let old_table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sessions'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if old_table_exists {
            conn.execute("ALTER TABLE sessions RENAME TO auth_sessions", [])?;
        }

        // Auth sessions table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS auth_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                token TEXT UNIQUE NOT NULL,
                public_address TEXT,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
            )",
            [],
        )?;

        // Auth challenges table for SIWE
        conn.execute(
            "CREATE TABLE IF NOT EXISTS auth_challenges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                public_address TEXT UNIQUE NOT NULL,
                challenge TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        // External API keys table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS external_api_keys (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                service_name TEXT UNIQUE NOT NULL,
                api_key TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Bot settings table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS bot_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_name TEXT NOT NULL DEFAULT 'StarkBot',
                bot_email TEXT NOT NULL DEFAULT 'starkbot@users.noreply.github.com',
                web3_tx_requires_confirmation INTEGER NOT NULL DEFAULT 0,
                rpc_provider TEXT NOT NULL DEFAULT 'defirelay',
                custom_rpc_endpoints TEXT,
                rogue_mode_enabled INTEGER NOT NULL DEFAULT 0,
                keystore_url TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Bot settings migrations for older DBs
        let _ = conn.execute("ALTER TABLE bot_settings ADD COLUMN web3_tx_requires_confirmation INTEGER NOT NULL DEFAULT 1", []);
        let _ = conn.execute("ALTER TABLE bot_settings ADD COLUMN rpc_provider TEXT NOT NULL DEFAULT 'defirelay'", []);
        let _ = conn.execute("ALTER TABLE bot_settings ADD COLUMN custom_rpc_endpoints TEXT", []);
        let _ = conn.execute("ALTER TABLE bot_settings ADD COLUMN rogue_mode_enabled INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE bot_settings ADD COLUMN keystore_url TEXT", []);

        // Initialize bot_settings with defaults if empty
        let bot_settings_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bot_settings", [], |row| row.get(0))
            .unwrap_or(0);

        if bot_settings_count == 0 {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO bot_settings (bot_name, bot_email, created_at, updated_at) VALUES ('StarkBot', 'starkbot@users.noreply.github.com', ?1, ?2)",
                [&now, &now],
            )?;
        }

        // Broadcasted transactions table (crypto tx history)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS broadcasted_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tx_hash TEXT NOT NULL,
                from_address TEXT NOT NULL,
                to_address TEXT NOT NULL,
                value TEXT NOT NULL DEFAULT '0',
                network TEXT NOT NULL DEFAULT 'base',
                status TEXT NOT NULL DEFAULT 'pending',
                block_number INTEGER,
                gas_used TEXT,
                description TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Broadcasted transactions migrations
        let _ = conn.execute("ALTER TABLE broadcasted_transactions ADD COLUMN network TEXT NOT NULL DEFAULT 'base'", []);
        let _ = conn.execute("ALTER TABLE broadcasted_transactions ADD COLUMN block_number INTEGER", []);
        let _ = conn.execute("ALTER TABLE broadcasted_transactions ADD COLUMN gas_used TEXT", []);
        let _ = conn.execute("ALTER TABLE broadcasted_transactions ADD COLUMN description TEXT", []);

        // x402 payment limits table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS x402_payment_limits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                asset TEXT UNIQUE NOT NULL,
                max_amount TEXT NOT NULL,
                decimals INTEGER NOT NULL DEFAULT 6,
                display_name TEXT NOT NULL,
                address TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Agent identity table (for SIWA)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_identity (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                agent_id INTEGER,
                agent_registry TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        // Starflask agents table (capability → agent mapping)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS starflask_agents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                capability TEXT UNIQUE NOT NULL,
                agent_id TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                pack_hashes TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL DEFAULT 'provisioned',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        // Starflask command log table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS starflask_command_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                capability TEXT NOT NULL,
                session_id TEXT,
                message TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                result TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        // Keystore auto-sync tracking
        conn.execute(
            "CREATE TABLE IF NOT EXISTS keystore_state (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_address TEXT UNIQUE NOT NULL,
                auto_retrieved INTEGER NOT NULL DEFAULT 0,
                last_sync_status TEXT,
                last_sync_message TEXT,
                last_sync_keys_restored INTEGER,
                last_sync_nodes_restored INTEGER,
                last_sync_at TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        Ok(())
    }

    /// Check if keystore auto-retrieval has been done for this wallet
    pub fn has_keystore_auto_retrieved(&self, wallet_address: &str) -> Result<bool, String> {
        let conn = self.conn();
        match conn.query_row(
            "SELECT auto_retrieved FROM keystore_state WHERE wallet_address = ?1",
            [&wallet_address.to_lowercase()],
            |row| row.get::<_, bool>(0),
        ) {
            Ok(val) => Ok(val),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Mark keystore as auto-retrieved for this wallet
    pub fn mark_keystore_auto_retrieved(&self, wallet_address: &str) -> Result<(), String> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO keystore_state (wallet_address, auto_retrieved, updated_at)
             VALUES (?1, 1, datetime('now'))
             ON CONFLICT(wallet_address) DO UPDATE SET auto_retrieved = 1, updated_at = datetime('now')",
            [&wallet_address.to_lowercase()],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Record auto-sync result
    pub fn record_auto_sync_result(
        &self,
        wallet_address: &str,
        status: &str,
        message: &str,
        keys_restored: Option<i32>,
        nodes_restored: Option<i32>,
    ) -> Result<(), String> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO keystore_state (wallet_address, last_sync_status, last_sync_message,
             last_sync_keys_restored, last_sync_nodes_restored, last_sync_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'))
             ON CONFLICT(wallet_address) DO UPDATE SET
             last_sync_status = ?2, last_sync_message = ?3,
             last_sync_keys_restored = ?4, last_sync_nodes_restored = ?5,
             last_sync_at = datetime('now'), updated_at = datetime('now')",
            rusqlite::params![
                &wallet_address.to_lowercase(),
                status,
                message,
                keys_restored,
                nodes_restored,
            ],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }
}
