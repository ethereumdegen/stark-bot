//! SQLite database - schema definitions and connection management
//!
//! This file contains:
//! - Database struct definition
//! - Connection pool management (r2d2)
//! - Schema creation and migrations
//!
//! All database operations are in the models/ subdirectory.

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
    /// Note: The `init` parameter is kept for API compatibility but the pool
    /// is always created. Set init=false to skip schema initialization.
    pub fn new_with_options(database_url: &str, init: bool) -> SqliteResult<Self> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = Path::new(database_url).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }

        // Create connection manager with SQLite pragmas
        let manager = SqliteConnectionManager::file(database_url)
            .with_init(|conn| {
                conn.execute_batch(
                    "PRAGMA journal_mode=WAL;
                     PRAGMA busy_timeout=5000;
                     PRAGMA cache_size=-64000;
                     PRAGMA mmap_size=268435456;
                     PRAGMA temp_store=memory;
                     PRAGMA synchronous=NORMAL;"
                )
            });

        // Build pool with reasonable defaults for SQLite
        // SQLite handles concurrency via WAL, so we don't need many connections
        // Each dispatch does ~20 sequential DB calls, so we need enough for concurrent dispatches
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
    /// Uses a 5-second timeout instead of panicking on pool exhaustion
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

        // Auth sessions table (renamed from sessions)
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

        // External channels table (Telegram, Slack, etc.)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS external_channels (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_type TEXT NOT NULL,
                name TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 0,
                bot_token TEXT NOT NULL DEFAULT '',
                app_token TEXT,
                safe_mode INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(channel_type, name)
            )",
            [],
        )?;

        // Migration: Add safe_mode column to external_channels if it doesn't exist
        let _ = conn.execute(
            "ALTER TABLE external_channels ADD COLUMN safe_mode INTEGER NOT NULL DEFAULT 0",
            [],
        );

        // Agent settings table (AI endpoint configuration - simplified for x402)
        // Note: provider, api_key, model columns are deprecated (kept for migration compatibility)
        // max_tokens renamed to max_response_tokens, max_context_tokens added for compaction
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                endpoint TEXT NOT NULL,
                model_archetype TEXT NOT NULL DEFAULT 'kimi',
                max_response_tokens INTEGER NOT NULL DEFAULT 40000,
                max_context_tokens INTEGER NOT NULL DEFAULT 100000,
                enabled INTEGER NOT NULL DEFAULT 0,
                secret_key TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Bot settings table (git commit author info, etc.)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS bot_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_name TEXT NOT NULL DEFAULT 'StarkBot',
                bot_email TEXT NOT NULL DEFAULT 'starkbot@users.noreply.github.com',
                web3_tx_requires_confirmation INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Migration: Add model_archetype column if it doesn't exist (for old DBs)
        let has_model_archetype: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_settings') WHERE name='model_archetype'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_model_archetype {
            conn.execute("ALTER TABLE agent_settings ADD COLUMN model_archetype TEXT DEFAULT 'kimi'", [])?;
        }

        // Migration: Rename max_tokens to max_response_tokens (for old DBs)
        let has_max_tokens: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_settings') WHERE name='max_tokens'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if has_max_tokens {
            // SQLite 3.25+ supports RENAME COLUMN
            let _ = conn.execute("ALTER TABLE agent_settings RENAME COLUMN max_tokens TO max_response_tokens", []);
        }

        // Migration: Add max_response_tokens if it doesn't exist
        let has_max_response_tokens: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_settings') WHERE name='max_response_tokens'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_max_response_tokens {
            conn.execute("ALTER TABLE agent_settings ADD COLUMN max_response_tokens INTEGER DEFAULT 40000", [])?;
        }

        // Migration: Add max_context_tokens for dynamic compaction thresholds
        let has_max_context_tokens: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_settings') WHERE name='max_context_tokens'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_max_context_tokens {
            conn.execute("ALTER TABLE agent_settings ADD COLUMN max_context_tokens INTEGER DEFAULT 100000", [])?;
        }

        // Migration: Add secret_key column if it doesn't exist (for old DBs)
        let has_secret_key: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_settings') WHERE name='secret_key'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_secret_key {
            conn.execute("ALTER TABLE agent_settings ADD COLUMN secret_key TEXT", [])?;
        }

        // Migration: Add model column for unified router dispatch
        let has_model: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_settings') WHERE name='model'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_model {
            conn.execute("ALTER TABLE agent_settings ADD COLUMN model TEXT", [])?;
        }

        // Migration: Add web3_tx_requires_confirmation column to bot_settings if it doesn't exist
        let has_web3_tx_confirmation: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='web3_tx_requires_confirmation'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_web3_tx_confirmation {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN web3_tx_requires_confirmation INTEGER NOT NULL DEFAULT 1", [])?;
        }

        // Migration: Add rpc_provider column to bot_settings if it doesn't exist
        let has_rpc_provider: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='rpc_provider'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_rpc_provider {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN rpc_provider TEXT NOT NULL DEFAULT 'defirelay'", [])?;
            conn.execute("ALTER TABLE bot_settings ADD COLUMN custom_rpc_endpoints TEXT", [])?;
        }

        // Migration: Add max_tool_iterations column to bot_settings if it doesn't exist
        let has_max_tool_iterations: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='max_tool_iterations'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_max_tool_iterations {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN max_tool_iterations INTEGER NOT NULL DEFAULT 50", [])?;
        }

        // Migration: Add rogue_mode_enabled column to bot_settings if it doesn't exist
        let has_rogue_mode: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='rogue_mode_enabled'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_rogue_mode {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN rogue_mode_enabled INTEGER NOT NULL DEFAULT 0", [])?;
        }

        // Migration: Add safe_mode_max_queries_per_10min column to bot_settings if it doesn't exist
        let has_safe_mode_max_queries: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='safe_mode_max_queries_per_10min'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_safe_mode_max_queries {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN safe_mode_max_queries_per_10min INTEGER NOT NULL DEFAULT 5", [])?;
        }

        // Migration: Add keystore_url column to bot_settings if it doesn't exist
        let has_keystore_url: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='keystore_url'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_keystore_url {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN keystore_url TEXT", [])?;
        }

        // Migration: Add enable_memory_access_for_safemode_gateway_channels column
        let has_safe_mode_memory: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='enable_memory_access_for_safemode_gateway_channels'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_safe_mode_memory {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN enable_memory_access_for_safemode_gateway_channels INTEGER NOT NULL DEFAULT 0", [])?;
        }

        // Migration: Add chat_session_memory_generation column to bot_settings if it doesn't exist
        let has_chat_session_memory_gen: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='chat_session_memory_generation'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_chat_session_memory_gen {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN chat_session_memory_generation INTEGER NOT NULL DEFAULT 1", [])?;
        }

        // Migration: Add guest_dashboard_enabled column to bot_settings if it doesn't exist
        let has_guest_dashboard: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='guest_dashboard_enabled'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_guest_dashboard {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN guest_dashboard_enabled INTEGER NOT NULL DEFAULT 0", [])?;
        }

        // Migration: Add theme_accent column to bot_settings if it doesn't exist
        let has_theme_accent: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='theme_accent'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_theme_accent {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN theme_accent TEXT", [])?;
        }

        // Migration: Add proxy_url column to bot_settings if it doesn't exist
        let has_proxy_url: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='proxy_url'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_proxy_url {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN proxy_url TEXT", [])?;
        }

        // Migration: Add kanban_auto_execute column to bot_settings if it doesn't exist
        let has_kanban_auto_execute: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('bot_settings') WHERE name='kanban_auto_execute'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_kanban_auto_execute {
            conn.execute("ALTER TABLE bot_settings ADD COLUMN kanban_auto_execute INTEGER NOT NULL DEFAULT 1", [])?;
        }

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

        // Chat sessions table - conversation context containers
        conn.execute(
            "CREATE TABLE IF NOT EXISTS chat_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_key TEXT UNIQUE NOT NULL,
                agent_id TEXT,
                scope TEXT NOT NULL DEFAULT 'dm',
                channel_type TEXT NOT NULL,
                channel_id INTEGER NOT NULL,
                platform_chat_id TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                reset_policy TEXT NOT NULL DEFAULT 'daily',
                idle_timeout_minutes INTEGER,
                daily_reset_hour INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_activity_at TEXT NOT NULL,
                expires_at TEXT,
                context_tokens INTEGER NOT NULL DEFAULT 0,
                max_context_tokens INTEGER NOT NULL DEFAULT 100000,
                compaction_id INTEGER
            )",
            [],
        )?;

        // Migration: Add context management columns if they don't exist
        let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN context_tokens INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN max_context_tokens INTEGER NOT NULL DEFAULT 100000", []);
        let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN compaction_id INTEGER", []);
        // Phase 1: Add last_flush_at for pre-compaction memory flush tracking
        let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN last_flush_at TEXT", []);
        // Task planner: Add completion_status column
        let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN completion_status TEXT NOT NULL DEFAULT 'active'", []);
        // QMD Memory: Add compaction_summary to store summary text directly
        let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN compaction_summary TEXT", []);
        // Sliding window compaction: Add generation counter and timestamp
        let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN compaction_generation INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN last_compaction_at TEXT", []);
        // Safe mode: Track if session was used in safe mode context
        let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN safe_mode INTEGER NOT NULL DEFAULT 0", []);
        // Special role: Track which special role (if any) enriched this safe-mode session
        let _ = conn.execute("ALTER TABLE chat_sessions ADD COLUMN special_role_name TEXT", []);

        // Session messages table - conversation transcripts
        conn.execute(
            "CREATE TABLE IF NOT EXISTS session_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id INTEGER NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                user_id TEXT,
                user_name TEXT,
                platform_message_id TEXT,
                tokens_used INTEGER,
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE CASCADE
            )",
            [],
        )?;

        // Telegram chat messages - passive log of ALL messages in Telegram chats
        // Independent of session system, used by telegram_read readHistory
        conn.execute(
            "CREATE TABLE IF NOT EXISTS telegram_chat_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_id INTEGER NOT NULL,
                chat_id TEXT NOT NULL,
                user_id TEXT,
                user_name TEXT,
                content TEXT NOT NULL,
                platform_message_id TEXT,
                is_bot_response INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_tcm_chat ON telegram_chat_messages(channel_id, chat_id, created_at DESC)",
            [],
        )?;

        // Identity links table - cross-channel user mapping
        conn.execute(
            "CREATE TABLE IF NOT EXISTS identity_links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                identity_id TEXT NOT NULL,
                channel_type TEXT NOT NULL,
                platform_user_id TEXT NOT NULL,
                platform_user_name TEXT,
                is_verified INTEGER NOT NULL DEFAULT 0,
                verified_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(channel_type, platform_user_id)
            )",
            [],
        )?;

        // Memories table - daily logs, long-term memories, preferences, facts, entities, tasks
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                memory_type TEXT NOT NULL,
                content TEXT NOT NULL,
                category TEXT,
                tags TEXT,
                importance INTEGER NOT NULL DEFAULT 5,
                identity_id TEXT,
                session_id INTEGER,
                source_channel_type TEXT,
                source_message_id TEXT,
                log_date TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                expires_at TEXT,
                -- Phase 2: Enhanced memory fields
                entity_type TEXT,
                entity_name TEXT,
                confidence REAL DEFAULT 1.0,
                source_type TEXT DEFAULT 'inferred',
                last_referenced_at TEXT,
                -- Phase 4: Consolidation fields
                superseded_by INTEGER,
                superseded_at TEXT,
                -- Phase 7: Temporal reasoning fields
                valid_from TEXT,
                valid_until TEXT,
                temporal_type TEXT,
                FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE SET NULL,
                FOREIGN KEY (superseded_by) REFERENCES memories(id) ON DELETE SET NULL
            )",
            [],
        )?;

        // Migration: Add new memory columns if they don't exist
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN entity_type TEXT", []);
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN entity_name TEXT", []);
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN confidence REAL DEFAULT 1.0", []);
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN source_type TEXT DEFAULT 'inferred'", []);
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN last_referenced_at TEXT", []);
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN superseded_by INTEGER", []);
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN superseded_at TEXT", []);
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN valid_from TEXT", []);
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN valid_until TEXT", []);
        let _ = conn.execute("ALTER TABLE memories ADD COLUMN temporal_type TEXT", []);

        // FTS5 virtual table for full-text search on memories
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                content,
                category,
                tags,
                content=memories,
                content_rowid=id
            )",
            [],
        )?;

        // Memory embeddings table for vector search (Phase 3)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_embeddings (
                memory_id INTEGER PRIMARY KEY,
                embedding BLOB NOT NULL,
                model TEXT NOT NULL,
                dimensions INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
            )",
            [],
        )?;

        // Create index for entity lookups (Phase 2)
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_entity ON memories(entity_type, entity_name)",
            [],
        )?;

        // Create index for temporal queries (Phase 7)
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_temporal ON memories(valid_from, valid_until)",
            [],
        )?;

        // Create index for superseded lookups (Phase 4)
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_superseded ON memories(superseded_by)",
            [],
        )?;

        // Triggers to keep FTS in sync with memories table
        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, content, category, tags)
                VALUES (new.id, new.content, new.category, new.tags);
            END",
            [],
        )?;

        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content, category, tags)
                VALUES ('delete', old.id, old.content, old.category, old.tags);
            END",
            [],
        )?;

        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content, category, tags)
                VALUES ('delete', old.id, old.content, old.category, old.tags);
                INSERT INTO memories_fts(rowid, content, category, tags)
                VALUES (new.id, new.content, new.category, new.tags);
            END",
            [],
        )?;

        // Tool configuration table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tool_configs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_id INTEGER,
                profile TEXT NOT NULL DEFAULT 'standard',
                allow_list TEXT NOT NULL DEFAULT '[]',
                deny_list TEXT NOT NULL DEFAULT '[]',
                allowed_groups TEXT NOT NULL DEFAULT '[\"web\", \"filesystem\", \"exec\"]',
                denied_groups TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(channel_id)
            )",
            [],
        )?;

        // Drop old installed_skills table if it exists (migration)
        conn.execute("DROP TABLE IF EXISTS installed_skills", [])?;

        // Skills table (database-backed skill storage)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skills (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                description TEXT NOT NULL,
                body TEXT NOT NULL,
                version TEXT NOT NULL DEFAULT '1.0.0',
                author TEXT,
                homepage TEXT,
                metadata TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                requires_tools TEXT NOT NULL DEFAULT '[]',
                requires_binaries TEXT NOT NULL DEFAULT '[]',
                arguments TEXT NOT NULL DEFAULT '{}',
                tags TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Migration: Add homepage and metadata columns if they don't exist
        let has_homepage: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('skills') WHERE name='homepage'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_homepage {
            conn.execute("ALTER TABLE skills ADD COLUMN homepage TEXT", [])?;
            conn.execute("ALTER TABLE skills ADD COLUMN metadata TEXT", [])?;
        }

        // Migration: Add subagent_type column to skills if it doesn't exist
        let _ = conn.execute("ALTER TABLE skills ADD COLUMN subagent_type TEXT", []);

        // Migration: Add requires_api_keys column to skills if it doesn't exist
        let _ = conn.execute("ALTER TABLE skills ADD COLUMN requires_api_keys TEXT NOT NULL DEFAULT '{}'", []);

        // Skill scripts table (Python/Bash scripts bundled with skills)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS skill_scripts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                skill_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                code TEXT NOT NULL,
                language TEXT NOT NULL DEFAULT 'python',
                created_at TEXT NOT NULL,
                FOREIGN KEY (skill_id) REFERENCES skills(id) ON DELETE CASCADE,
                UNIQUE(skill_id, name)
            )",
            [],
        )?;

        // Tool execution audit log
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tool_executions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_id INTEGER NOT NULL,
                session_id INTEGER,
                tool_name TEXT NOT NULL,
                parameters TEXT NOT NULL,
                success INTEGER NOT NULL,
                result TEXT,
                duration_ms INTEGER,
                executed_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE SET NULL
            )",
            [],
        )?;

        // Create index for tool executions lookup
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_tool_executions_channel ON tool_executions(channel_id, executed_at)",
            [],
        )?;

        // Cron jobs table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cron_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_id TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                description TEXT,
                schedule_type TEXT NOT NULL,
                schedule_value TEXT NOT NULL,
                timezone TEXT,
                session_mode TEXT NOT NULL DEFAULT 'isolated',
                message TEXT,
                system_event TEXT,
                channel_id INTEGER,
                deliver_to TEXT,
                deliver INTEGER NOT NULL DEFAULT 0,
                model_override TEXT,
                thinking_level TEXT,
                timeout_seconds INTEGER,
                delete_after_run INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'active',
                last_run_at TEXT,
                next_run_at TEXT,
                run_count INTEGER NOT NULL DEFAULT 0,
                error_count INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (channel_id) REFERENCES external_channels(id) ON DELETE SET NULL
            )",
            [],
        )?;

        // Cron job runs history
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cron_job_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_id INTEGER NOT NULL,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                success INTEGER NOT NULL DEFAULT 0,
                result TEXT,
                error TEXT,
                duration_ms INTEGER,
                FOREIGN KEY (job_id) REFERENCES cron_jobs(id) ON DELETE CASCADE
            )",
            [],
        )?;

        // Index for job runs lookup
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_cron_job_runs_job ON cron_job_runs(job_id, started_at DESC)",
            [],
        )?;

        // Heartbeat configuration table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS heartbeat_configs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_id INTEGER UNIQUE,
                interval_minutes INTEGER NOT NULL DEFAULT 30,
                target TEXT NOT NULL DEFAULT 'last',
                active_hours_start TEXT,
                active_hours_end TEXT,
                active_days TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_beat_at TEXT,
                next_beat_at TEXT,
                current_mind_node_id INTEGER,
                last_session_id INTEGER,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (channel_id) REFERENCES external_channels(id) ON DELETE CASCADE,
                FOREIGN KEY (current_mind_node_id) REFERENCES mind_nodes(id) ON DELETE SET NULL
            )",
            [],
        )?;

        // Migration: Add mind map columns to heartbeat_configs if they don't exist
        let _ = conn.execute(
            "ALTER TABLE heartbeat_configs ADD COLUMN current_mind_node_id INTEGER",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE heartbeat_configs ADD COLUMN last_session_id INTEGER",
            [],
        );

        // Gmail integration configuration
        conn.execute(
            "CREATE TABLE IF NOT EXISTS gmail_configs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT UNIQUE NOT NULL,
                access_token TEXT NOT NULL,
                refresh_token TEXT NOT NULL,
                token_expires_at TEXT,
                watch_labels TEXT NOT NULL DEFAULT 'INBOX',
                project_id TEXT NOT NULL,
                topic_name TEXT NOT NULL,
                watch_expires_at TEXT,
                history_id TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                response_channel_id INTEGER,
                auto_reply INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // =====================================================
        // EIP-8004 Tables (Trustless Agents)
        // =====================================================

        // x402 payment history with proof tracking
        conn.execute(
            "CREATE TABLE IF NOT EXISTS x402_payments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_id INTEGER,
                session_id INTEGER,
                execution_id TEXT,
                tool_name TEXT,
                resource TEXT,
                amount TEXT NOT NULL,
                amount_formatted TEXT,
                asset TEXT NOT NULL DEFAULT 'USDC',
                pay_to TEXT NOT NULL,
                from_address TEXT,
                tx_hash TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                block_number INTEGER,
                feedback_submitted INTEGER NOT NULL DEFAULT 0,
                feedback_id INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE SET NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_x402_payments_channel ON x402_payments(channel_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_x402_payments_tx_hash ON x402_payments(tx_hash)",
            [],
        )?;

        // Migration: Add status column to x402_payments if it doesn't exist
        let _ = conn.execute(
            "ALTER TABLE x402_payments ADD COLUMN status TEXT NOT NULL DEFAULT 'pending'",
            [],
        );

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_x402_payments_status ON x402_payments(status)",
            [],
        )?;

        // x402 payment limits — per-call maximums per token
        conn.execute(
            "CREATE TABLE IF NOT EXISTS x402_payment_limits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                asset TEXT NOT NULL UNIQUE,
                max_amount TEXT NOT NULL,
                decimals INTEGER NOT NULL DEFAULT 6,
                display_name TEXT NOT NULL,
                address TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        // Migration: Add address column to x402_payment_limits if it doesn't exist
        let _ = conn.execute(
            "ALTER TABLE x402_payment_limits ADD COLUMN address TEXT",
            [],
        );

        // Migration: drop old agent_identity table if it has the legacy wallet_address column
        {
            let has_wallet_col: bool = conn
                .prepare("PRAGMA table_info(agent_identity)")
                .and_then(|mut stmt| {
                    stmt.query_map([], |row| row.get::<_, String>(1))
                        .map(|rows| rows.filter_map(|r| r.ok()).any(|name| name == "wallet_address"))
                })
                .unwrap_or(false);
            if has_wallet_col {
                log::info!("[db] Dropping legacy agent_identity table (has wallet_address column)");
                let _ = conn.execute("DROP TABLE agent_identity", []);
            }
        }

        // Agent identity (our EIP-8004 registration — single source of truth for identity metadata)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_identity (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                agent_id INTEGER NOT NULL,
                agent_registry TEXT NOT NULL,
                chain_id INTEGER NOT NULL DEFAULT 8453,
                name TEXT,
                description TEXT,
                image TEXT,
                x402_support INTEGER NOT NULL DEFAULT 1,
                active INTEGER NOT NULL DEFAULT 1,
                services_json TEXT NOT NULL DEFAULT '[]',
                supported_trust_json TEXT NOT NULL DEFAULT '[\"reputation\",\"x402-payments\"]',
                registration_uri TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        // Migration: Add metadata columns to agent_identity if they don't exist (for old DBs)
        {
            let has_name: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('agent_identity') WHERE name='name'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map(|c| c > 0)
                .unwrap_or(false);

            if !has_name {
                log::info!("[db] Migrating agent_identity: adding metadata columns");
                let _ = conn.execute("ALTER TABLE agent_identity ADD COLUMN name TEXT", []);
                let _ = conn.execute("ALTER TABLE agent_identity ADD COLUMN description TEXT", []);
                let _ = conn.execute("ALTER TABLE agent_identity ADD COLUMN image TEXT", []);
                let _ = conn.execute("ALTER TABLE agent_identity ADD COLUMN x402_support INTEGER NOT NULL DEFAULT 1", []);
                let _ = conn.execute("ALTER TABLE agent_identity ADD COLUMN active INTEGER NOT NULL DEFAULT 1", []);
                let _ = conn.execute("ALTER TABLE agent_identity ADD COLUMN services_json TEXT NOT NULL DEFAULT '[]'", []);
                let _ = conn.execute("ALTER TABLE agent_identity ADD COLUMN supported_trust_json TEXT NOT NULL DEFAULT '[\"reputation\",\"x402-payments\"]'", []);
                let _ = conn.execute("ALTER TABLE agent_identity ADD COLUMN registration_uri TEXT", []);
                let _ = conn.execute("ALTER TABLE agent_identity ADD COLUMN updated_at TEXT NOT NULL DEFAULT (datetime('now'))", []);

                // One-time backfill from IDENTITY.json if it exists and DB row has no name yet
                Self::backfill_identity_from_file(&conn);
            }
        }

        // Reputation feedback (given and received)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS reputation_feedback (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                direction TEXT NOT NULL CHECK (direction IN ('given', 'received')),
                agent_id INTEGER NOT NULL,
                agent_registry TEXT NOT NULL,
                client_address TEXT NOT NULL,
                feedback_index INTEGER,
                value INTEGER NOT NULL,
                value_decimals INTEGER NOT NULL DEFAULT 0,
                tag1 TEXT,
                tag2 TEXT,
                endpoint TEXT,
                feedback_uri TEXT,
                feedback_hash TEXT,
                proof_of_payment_tx TEXT,
                response_uri TEXT,
                response_hash TEXT,
                is_revoked INTEGER NOT NULL DEFAULT 0,
                tx_hash TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reputation_direction ON reputation_feedback(direction)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reputation_agent ON reputation_feedback(agent_id, agent_registry)",
            [],
        )?;

        // Known agents (discovered from registry)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS known_agents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                agent_id INTEGER NOT NULL,
                agent_registry TEXT NOT NULL,
                chain_id INTEGER NOT NULL DEFAULT 8453,
                name TEXT,
                description TEXT,
                image_url TEXT,
                registration_uri TEXT,
                owner_address TEXT,
                wallet_address TEXT,
                x402_support INTEGER NOT NULL DEFAULT 0,
                services TEXT,
                supported_trust TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                reputation_score INTEGER,
                reputation_count INTEGER NOT NULL DEFAULT 0,
                total_payments TEXT,
                last_interaction_at TEXT,
                discovered_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(agent_id, agent_registry)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_known_agents_x402 ON known_agents(x402_support, is_active)",
            [],
        )?;

        // Validation records
        conn.execute(
            "CREATE TABLE IF NOT EXISTS validations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                direction TEXT NOT NULL CHECK (direction IN ('requested', 'responded')),
                request_hash TEXT NOT NULL,
                agent_id INTEGER NOT NULL,
                agent_registry TEXT,
                validator_address TEXT,
                request_uri TEXT,
                response INTEGER CHECK (response >= 0 AND response <= 100),
                response_uri TEXT,
                response_hash TEXT,
                tag TEXT,
                tx_hash TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_validations_request_hash ON validations(request_hash)",
            [],
        )?;

        // Agent contexts table - multi-agent orchestrator state persistence
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_contexts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id INTEGER NOT NULL UNIQUE,
                original_request TEXT NOT NULL,
                mode TEXT NOT NULL DEFAULT 'explore',
                subtype TEXT NOT NULL DEFAULT 'finance',
                context_sufficient INTEGER NOT NULL DEFAULT 0,
                plan_ready INTEGER NOT NULL DEFAULT 0,
                mode_iterations INTEGER NOT NULL DEFAULT 0,
                total_iterations INTEGER NOT NULL DEFAULT 0,
                exploration_notes TEXT NOT NULL DEFAULT '[]',
                findings TEXT NOT NULL DEFAULT '[]',
                plan_summary TEXT,
                scratchpad TEXT NOT NULL DEFAULT '',
                tasks_json TEXT NOT NULL DEFAULT '{\"tasks\":[]}',
                active_skill_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE CASCADE
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_agent_contexts_session ON agent_contexts(session_id)",
            [],
        )?;

        // Sub-agents table - background agent execution tracking
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sub_agents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                subagent_id TEXT UNIQUE NOT NULL,
                parent_session_id INTEGER NOT NULL,
                parent_channel_id INTEGER NOT NULL,
                session_id INTEGER,
                label TEXT NOT NULL,
                task TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                model_override TEXT,
                thinking_level TEXT,
                timeout_secs INTEGER DEFAULT 300,
                context TEXT,
                result TEXT,
                error TEXT,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                FOREIGN KEY (parent_session_id) REFERENCES chat_sessions(id),
                FOREIGN KEY (session_id) REFERENCES chat_sessions(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sub_agents_parent_session ON sub_agents(parent_session_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sub_agents_parent_channel ON sub_agents(parent_channel_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sub_agents_status ON sub_agents(status)",
            [],
        )?;

        // Migration: Add parent_subagent_id and depth columns to sub_agents
        let _ = conn.execute(
            "ALTER TABLE sub_agents ADD COLUMN parent_subagent_id TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE sub_agents ADD COLUMN depth INTEGER NOT NULL DEFAULT 0",
            [],
        );
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sub_agents_parent_subagent ON sub_agents(parent_subagent_id)",
            [],
        )?;

        // Migration: Add subtype column to agent_contexts if it doesn't exist
        let _ = conn.execute(
            "ALTER TABLE agent_contexts ADD COLUMN subtype TEXT NOT NULL DEFAULT 'finance'",
            [],
        );

        // Migration: Add active_skill_json column to agent_contexts if it doesn't exist
        let _ = conn.execute(
            "ALTER TABLE agent_contexts ADD COLUMN active_skill_json TEXT",
            [],
        );

        // Broadcasted transactions table - persistent history of all crypto tx broadcasts
        conn.execute(
            "CREATE TABLE IF NOT EXISTS broadcasted_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uuid TEXT UNIQUE NOT NULL,
                network TEXT NOT NULL,
                from_address TEXT NOT NULL,
                to_address TEXT NOT NULL,
                value TEXT NOT NULL,
                value_formatted TEXT NOT NULL,
                tx_hash TEXT,
                explorer_url TEXT,
                status TEXT NOT NULL DEFAULT 'broadcast',
                broadcast_mode TEXT NOT NULL,
                error TEXT,
                broadcast_at TEXT NOT NULL,
                confirmed_at TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_broadcasted_tx_hash ON broadcasted_transactions(tx_hash)",
            [],
        )?;

        // Channel settings table - per-channel configuration
        conn.execute(
            "CREATE TABLE IF NOT EXISTS channel_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_id INTEGER NOT NULL,
                setting_key TEXT NOT NULL,
                setting_value TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (channel_id) REFERENCES external_channels(id) ON DELETE CASCADE,
                UNIQUE(channel_id, setting_key)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_channel_settings_channel ON channel_settings(channel_id)",
            [],
        )?;

        // Mind map nodes table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mind_nodes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                body TEXT NOT NULL DEFAULT '',
                position_x REAL,
                position_y REAL,
                is_trunk INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        // Mind map node connections table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mind_node_connections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                parent_id INTEGER NOT NULL,
                child_id INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (parent_id) REFERENCES mind_nodes(id) ON DELETE CASCADE,
                FOREIGN KEY (child_id) REFERENCES mind_nodes(id) ON DELETE CASCADE,
                UNIQUE(parent_id, child_id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mind_connections_parent ON mind_node_connections(parent_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mind_connections_child ON mind_node_connections(child_id)",
            [],
        )?;

        // Kanban board items table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS kanban_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'ready',
                priority INTEGER NOT NULL DEFAULT 0,
                session_id INTEGER,
                result TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_kanban_items_status ON kanban_items(status)",
            [],
        )?;

        // NOTE: discord_user_profiles table is now owned by the discord_tipping module.
        // It gets created when the module is installed (init_tables).

        // Twitter processed mentions table - track which tweets we've already responded to
        conn.execute(
            "CREATE TABLE IF NOT EXISTS twitter_processed_mentions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tweet_id TEXT UNIQUE NOT NULL,
                channel_id INTEGER NOT NULL,
                author_id TEXT NOT NULL,
                author_username TEXT NOT NULL,
                tweet_text TEXT NOT NULL,
                processed_at TEXT NOT NULL,
                FOREIGN KEY (channel_id) REFERENCES external_channels(id) ON DELETE CASCADE
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_twitter_mentions_channel ON twitter_processed_mentions(channel_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_twitter_mentions_processed ON twitter_processed_mentions(processed_at)",
            [],
        )?;

        // Installed modules - plugin system registry
        conn.execute(
            "CREATE TABLE IF NOT EXISTS installed_modules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                module_name TEXT UNIQUE NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                version TEXT NOT NULL DEFAULT '1.0.0',
                description TEXT NOT NULL,
                has_tools INTEGER NOT NULL DEFAULT 0,
                has_dashboard INTEGER NOT NULL DEFAULT 0,
                source TEXT NOT NULL DEFAULT 'builtin',
                manifest_path TEXT,
                binary_path TEXT,
                author TEXT,
                sha256_checksum TEXT,
                installed_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        // Migration: add new columns to existing installed_modules tables
        // (safe to run repeatedly — SQLite ALTER TABLE ADD COLUMN is idempotent with IF NOT EXISTS-like behavior)
        for col in &[
            "has_tools INTEGER NOT NULL DEFAULT 0",
            "has_dashboard INTEGER NOT NULL DEFAULT 0",
            "source TEXT NOT NULL DEFAULT 'builtin'",
            "manifest_path TEXT",
            "binary_path TEXT",
            "author TEXT",
            "sha256_checksum TEXT",
        ] {
            let _ = conn.execute(
                &format!("ALTER TABLE installed_modules ADD COLUMN {}", col),
                [],
            );
        }

        // Agent subtypes - configurable agent subtypes (toolboxes)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_subtypes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key TEXT NOT NULL UNIQUE,
                label TEXT NOT NULL,
                emoji TEXT NOT NULL DEFAULT '🤖',
                description TEXT NOT NULL DEFAULT '',
                tool_groups_json TEXT NOT NULL DEFAULT '[]',
                skill_tags_json TEXT NOT NULL DEFAULT '[]',
                additional_tools_json TEXT NOT NULL DEFAULT '[]',
                prompt TEXT NOT NULL DEFAULT '',
                sort_order INTEGER NOT NULL DEFAULT 0,
                enabled INTEGER NOT NULL DEFAULT 1,
                max_iterations INTEGER NOT NULL DEFAULT 90,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        // Migration: add max_iterations column to existing agent_subtypes tables
        let _ = conn.execute(
            "ALTER TABLE agent_subtypes ADD COLUMN max_iterations INTEGER NOT NULL DEFAULT 90",
            [],
        );

        // Migration: add additional_tools_json column (explicit tool allowlist for orchestrator subtypes)
        let _ = conn.execute(
            "ALTER TABLE agent_subtypes ADD COLUMN additional_tools_json TEXT NOT NULL DEFAULT '[]'",
            [],
        );

        // Migration: add skip_task_planner column (skip TaskPlanner mode for this subtype)
        let _ = conn.execute(
            "ALTER TABLE agent_subtypes ADD COLUMN skip_task_planner INTEGER NOT NULL DEFAULT 0",
            [],
        );

        // Migration: add aliases_json column (alternative names that resolve to this subtype key)
        let _ = conn.execute(
            "ALTER TABLE agent_subtypes ADD COLUMN aliases_json TEXT NOT NULL DEFAULT '[]'",
            [],
        );

        // Keystore state - track backup/retrieval status per wallet
        conn.execute(
            "CREATE TABLE IF NOT EXISTS keystore_state (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_address TEXT UNIQUE NOT NULL,
                auto_retrieved INTEGER NOT NULL DEFAULT 0,
                last_retrieved_at TEXT,
                last_backup_at TEXT,
                last_backup_version INTEGER,
                last_backup_item_count INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        // Migration: Add auto_sync columns to keystore_state for tracking boot-time sync status
        let _ = conn.execute(
            "ALTER TABLE keystore_state ADD COLUMN auto_sync_status TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE keystore_state ADD COLUMN auto_sync_message TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE keystore_state ADD COLUMN auto_sync_at TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE keystore_state ADD COLUMN auto_sync_key_count INTEGER",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE keystore_state ADD COLUMN auto_sync_node_count INTEGER",
            [],
        );

        // =====================================================
        // Telemetry tables (agent-lightning philosophy)
        // =====================================================

        // execution_spans - structured telemetry for every tool/LLM/planning operation
        conn.execute(
            "CREATE TABLE IF NOT EXISTS execution_spans (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                span_id TEXT UNIQUE NOT NULL,
                sequence_id INTEGER NOT NULL,
                rollout_id TEXT NOT NULL,
                session_id INTEGER NOT NULL,
                attempt_idx INTEGER NOT NULL DEFAULT 0,
                parent_span_id TEXT,
                span_type TEXT NOT NULL,
                name TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                duration_ms INTEGER,
                attributes TEXT NOT NULL DEFAULT '{}',
                error TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        // Indexes for execution_spans
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_spans_rollout ON execution_spans(rollout_id)",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_spans_session ON execution_spans(session_id)",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_spans_type ON execution_spans(span_type)",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_spans_started ON execution_spans(started_at)",
            [],
        );

        // rollouts - lifecycle tracking for dispatch executions
        conn.execute(
            "CREATE TABLE IF NOT EXISTS rollouts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                rollout_id TEXT UNIQUE NOT NULL,
                session_id INTEGER NOT NULL,
                channel_id INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'queuing',
                config TEXT NOT NULL DEFAULT '{}',
                resources_id TEXT,
                created_at TEXT NOT NULL,
                completed_at TEXT,
                duration_ms INTEGER,
                result TEXT,
                error TEXT,
                metadata TEXT NOT NULL DEFAULT '{}'
            )",
            [],
        )?;

        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_rollouts_session ON rollouts(session_id)",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_rollouts_channel ON rollouts(channel_id)",
            [],
        );

        // attempts - individual retry attempts within a rollout
        conn.execute(
            "CREATE TABLE IF NOT EXISTS attempts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                rollout_id TEXT NOT NULL,
                attempt_idx INTEGER NOT NULL,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                duration_ms INTEGER,
                succeeded INTEGER NOT NULL DEFAULT 0,
                failure_reason TEXT,
                error TEXT,
                tool_calls INTEGER NOT NULL DEFAULT 0,
                llm_calls INTEGER NOT NULL DEFAULT 0,
                tokens_used INTEGER NOT NULL DEFAULT 0,
                UNIQUE(rollout_id, attempt_idx)
            )",
            [],
        )?;

        // resource_versions - versioned prompts, model configs, tool configs
        conn.execute(
            "CREATE TABLE IF NOT EXISTS resource_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                version_id TEXT UNIQUE NOT NULL,
                label TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 0,
                resources TEXT NOT NULL DEFAULT '[]',
                description TEXT,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_resource_versions_active ON resource_versions(is_active)",
            [],
        );

        // Special roles (enriched safe mode)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS special_roles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                allowed_tools TEXT NOT NULL DEFAULT '[]',
                allowed_skills TEXT NOT NULL DEFAULT '[]',
                description TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS special_role_assignments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_type TEXT NOT NULL,
                user_id TEXT NOT NULL,
                special_role_name TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (special_role_name) REFERENCES special_roles(name) ON DELETE CASCADE,
                UNIQUE(channel_type, user_id)
            );

            CREATE INDEX IF NOT EXISTS idx_sra_lookup ON special_role_assignments(channel_type, user_id);",
        )?;

        // Migration: Add label column to special_role_assignments if it doesn't exist
        let _ = conn.execute(
            "ALTER TABLE special_role_assignments ADD COLUMN label TEXT",
            [],
        );

        // =====================================================
        // Memory Associations Table (Knowledge Graph)
        // =====================================================
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_associations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_memory_id INTEGER NOT NULL,
                target_memory_id INTEGER NOT NULL,
                association_type TEXT NOT NULL DEFAULT 'related',
                strength REAL NOT NULL DEFAULT 0.5,
                metadata TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (source_memory_id) REFERENCES memories(id) ON DELETE CASCADE,
                FOREIGN KEY (target_memory_id) REFERENCES memories(id) ON DELETE CASCADE
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_assoc_source ON memory_associations(source_memory_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_assoc_target ON memory_associations(target_memory_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_assoc_type ON memory_associations(association_type)",
            [],
        )?;

        // Migration: Add updated_at column to memory_embeddings if it doesn't exist
        let _ = conn.execute(
            "ALTER TABLE memory_embeddings ADD COLUMN updated_at TEXT",
            [],
        );

        // Phase 2: Worker delegation columns
        let _ = conn.execute(
            "ALTER TABLE sub_agents ADD COLUMN mode TEXT NOT NULL DEFAULT 'standard'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE sub_agents ADD COLUMN parent_context_snapshot TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE sub_agents ADD COLUMN checkpoints TEXT",
            [],
        );
        // Coalescing and compaction settings
        let _ = conn.execute(
            "ALTER TABLE bot_settings ADD COLUMN coalescing_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE bot_settings ADD COLUMN coalescing_debounce_ms INTEGER NOT NULL DEFAULT 1500",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE bot_settings ADD COLUMN coalescing_max_wait_ms INTEGER NOT NULL DEFAULT 5000",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE bot_settings ADD COLUMN compaction_background_threshold REAL NOT NULL DEFAULT 0.80",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE bot_settings ADD COLUMN compaction_aggressive_threshold REAL NOT NULL DEFAULT 0.85",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE bot_settings ADD COLUMN compaction_emergency_threshold REAL NOT NULL DEFAULT 0.95",
            [],
        );

        Ok(())
    }

    // =====================================================
    // Agent Identity Operations
    // =====================================================

    /// One-time migration: backfill agent_identity metadata from IDENTITY.json on disk
    fn backfill_identity_from_file(conn: &DbConn) {
        let identity_path = crate::config::identity_document_path();
        let content = match std::fs::read_to_string(&identity_path) {
            Ok(c) => c,
            Err(_) => return, // No file — nothing to backfill
        };

        let reg: crate::eip8004::types::RegistrationFile = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[db] Could not parse IDENTITY.json for migration: {}", e);
                return;
            }
        };

        // Check if a DB row exists with NULL name (needs backfill)
        let needs_backfill: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_identity WHERE name IS NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !needs_backfill {
            return;
        }

        let services_json = serde_json::to_string(&reg.services).unwrap_or_else(|_| "[]".to_string());
        let supported_trust_json = serde_json::to_string(&reg.supported_trust).unwrap_or_else(|_| "[]".to_string());

        match conn.execute(
            "UPDATE agent_identity SET name = ?1, description = ?2, image = ?3, x402_support = ?4, active = ?5, services_json = ?6, supported_trust_json = ?7, updated_at = datetime('now') WHERE name IS NULL",
            rusqlite::params![
                reg.name,
                reg.description,
                reg.image,
                reg.x402_support as i32,
                reg.active as i32,
                services_json,
                supported_trust_json,
            ],
        ) {
            Ok(n) => {
                if n > 0 {
                    log::info!("[db] Backfilled agent_identity from IDENTITY.json ({} rows updated)", n);
                }
            }
            Err(e) => {
                log::warn!("[db] Failed to backfill agent_identity from IDENTITY.json: {}", e);
            }
        }
    }

    /// Get the full agent identity row (all metadata columns).
    /// Only returns identities with agent_id > 0 (properly linked on-chain).
    pub fn get_agent_identity_full(&self) -> Option<AgentIdentityRow> {
        let conn = self.conn();
        conn.query_row(
            "SELECT agent_id, agent_registry, chain_id, name, description, image, x402_support, active, services_json, supported_trust_json, registration_uri FROM agent_identity WHERE agent_id > 0 ORDER BY id DESC LIMIT 1",
            [],
            |row| {
                Ok(AgentIdentityRow {
                    agent_id: row.get(0)?,
                    agent_registry: row.get(1)?,
                    chain_id: row.get(2)?,
                    name: row.get(3)?,
                    description: row.get(4)?,
                    image: row.get(5)?,
                    x402_support: row.get::<_, i32>(6).unwrap_or(1) != 0,
                    active: row.get::<_, i32>(7).unwrap_or(1) != 0,
                    services_json: row.get::<_, String>(8).unwrap_or_else(|_| "[]".to_string()),
                    supported_trust_json: row.get::<_, String>(9).unwrap_or_else(|_| "[]".to_string()),
                    registration_uri: row.get(10)?,
                })
            },
        )
        .ok()
    }

    /// Upsert agent identity — deletes existing rows and inserts a new one with full metadata.
    /// agent_id must be > 0 (a real on-chain agent ID).
    pub fn upsert_agent_identity(
        &self,
        agent_id: i64,
        agent_registry: &str,
        chain_id: i64,
        name: Option<&str>,
        description: Option<&str>,
        image: Option<&str>,
        x402_support: bool,
        active: bool,
        services_json: &str,
        supported_trust_json: &str,
        registration_uri: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        if agent_id <= 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "agent_id must be > 0 — cannot store unlinked identity".to_string(),
            ));
        }
        let conn = self.conn();
        conn.execute("DELETE FROM agent_identity", [])?;
        conn.execute(
            "INSERT INTO agent_identity (agent_id, agent_registry, chain_id, name, description, image, x402_support, active, services_json, supported_trust_json, registration_uri)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                agent_id,
                agent_registry,
                chain_id,
                name,
                description,
                image,
                x402_support as i32,
                active as i32,
                services_json,
                supported_trust_json,
                registration_uri,
            ],
        )?;
        Ok(())
    }

    /// Update a single field on the agent identity row
    pub fn update_agent_identity_field(&self, field: &str, value: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        // Only allow known fields to prevent SQL injection
        let sql = match field {
            "name" => "UPDATE agent_identity SET name = ?1, updated_at = datetime('now')",
            "description" => "UPDATE agent_identity SET description = ?1, updated_at = datetime('now')",
            "image" => "UPDATE agent_identity SET image = ?1, updated_at = datetime('now')",
            "active" => "UPDATE agent_identity SET active = ?1, updated_at = datetime('now')",
            "x402_support" => "UPDATE agent_identity SET x402_support = ?1, updated_at = datetime('now')",
            "services_json" => "UPDATE agent_identity SET services_json = ?1, updated_at = datetime('now')",
            "supported_trust_json" => "UPDATE agent_identity SET supported_trust_json = ?1, updated_at = datetime('now')",
            "registration_uri" => "UPDATE agent_identity SET registration_uri = ?1, updated_at = datetime('now')",
            _ => return Err(rusqlite::Error::InvalidParameterName(format!("Unknown field: {}", field))),
        };
        conn.execute(sql, [value])?;
        Ok(())
    }

    /// Record an x402 payment to the database
    pub fn record_x402_payment(
        &self,
        channel_id: Option<i64>,
        tool_name: Option<&str>,
        resource: Option<&str>,
        amount: &str,
        amount_formatted: &str,
        asset: &str,
        pay_to: &str,
        tx_hash: Option<&str>,
        status: &str,
    ) -> Result<i64, rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO x402_payments (channel_id, tool_name, resource, amount, amount_formatted, asset, pay_to, tx_hash, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![channel_id, tool_name, resource, amount, amount_formatted, asset, pay_to, tx_hash, status],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update payment status and tx_hash
    pub fn update_x402_payment_status(
        &self,
        payment_id: i64,
        status: &str,
        tx_hash: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        conn.execute(
            "UPDATE x402_payments SET status = ?1, tx_hash = COALESCE(?2, tx_hash) WHERE id = ?3",
            rusqlite::params![status, tx_hash, payment_id],
        )?;
        Ok(())
    }

    // =====================================================
    // Keystore State Operations
    // =====================================================

    /// Check if auto-retrieval has been done for a wallet
    pub fn has_keystore_auto_retrieved(&self, wallet_address: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn();
        let result: Option<bool> = conn.query_row(
            "SELECT auto_retrieved FROM keystore_state WHERE wallet_address = ?1",
            [wallet_address],
            |row| row.get(0),
        ).ok();
        Ok(result.unwrap_or(false))
    }

    /// Mark that auto-retrieval has been attempted for a wallet
    pub fn mark_keystore_auto_retrieved(&self, wallet_address: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO keystore_state (wallet_address, auto_retrieved, last_retrieved_at, updated_at)
             VALUES (?1, 1, ?2, ?2)
             ON CONFLICT(wallet_address) DO UPDATE SET
                auto_retrieved = 1,
                last_retrieved_at = ?2,
                updated_at = ?2",
            rusqlite::params![wallet_address, now],
        )?;
        Ok(())
    }

    /// Record a successful backup to keystore
    pub fn record_keystore_backup(
        &self,
        wallet_address: &str,
        version: u32,
        item_count: usize,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO keystore_state (wallet_address, last_backup_at, last_backup_version, last_backup_item_count, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?2)
             ON CONFLICT(wallet_address) DO UPDATE SET
                last_backup_at = ?2,
                last_backup_version = ?3,
                last_backup_item_count = ?4,
                updated_at = ?2",
            rusqlite::params![wallet_address, now, version, item_count as i64],
        )?;
        Ok(())
    }

    /// Record a successful retrieval from keystore
    pub fn record_keystore_retrieval(&self, wallet_address: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO keystore_state (wallet_address, last_retrieved_at, updated_at)
             VALUES (?1, ?2, ?2)
             ON CONFLICT(wallet_address) DO UPDATE SET
                last_retrieved_at = ?2,
                updated_at = ?2",
            rusqlite::params![wallet_address, now],
        )?;
        Ok(())
    }

    /// Record auto-sync result (success or failure)
    pub fn record_auto_sync_result(
        &self,
        wallet_address: &str,
        status: &str,
        message: &str,
        key_count: Option<i32>,
        node_count: Option<i32>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO keystore_state (wallet_address, auto_sync_status, auto_sync_message, auto_sync_at, auto_sync_key_count, auto_sync_node_count, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?4)
             ON CONFLICT(wallet_address) DO UPDATE SET
                auto_sync_status = ?2,
                auto_sync_message = ?3,
                auto_sync_at = ?4,
                auto_sync_key_count = ?5,
                auto_sync_node_count = ?6,
                updated_at = ?4",
            rusqlite::params![wallet_address.to_lowercase(), status, message, now, key_count, node_count],
        )?;
        Ok(())
    }

    /// Get auto-sync status for a wallet
    pub fn get_auto_sync_status(&self, wallet_address: &str) -> Result<Option<AutoSyncStatus>, rusqlite::Error> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT auto_sync_status, auto_sync_message, auto_sync_at, auto_sync_key_count, auto_sync_node_count
             FROM keystore_state WHERE wallet_address = ?1",
            [wallet_address.to_lowercase()],
            |row| {
                let status: Option<String> = row.get(0)?;
                let message: Option<String> = row.get(1)?;
                let at: Option<String> = row.get(2)?;
                let key_count: Option<i32> = row.get(3)?;
                let node_count: Option<i32> = row.get(4)?;
                Ok(status.map(|s| AutoSyncStatus {
                    status: s,
                    message: message.unwrap_or_default(),
                    synced_at: at,
                    key_count,
                    node_count,
                }))
            },
        );
        match result {
            Ok(status) => Ok(status),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Full agent identity row from the DB
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentIdentityRow {
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

impl AgentIdentityRow {
    /// Convert services_json to a RegistrationFile for serialization
    pub fn to_registration_file(&self) -> crate::eip8004::types::RegistrationFile {
        let services: Vec<crate::eip8004::types::ServiceEntry> =
            serde_json::from_str(&self.services_json).unwrap_or_default();
        let supported_trust: Vec<String> =
            serde_json::from_str(&self.supported_trust_json).unwrap_or_default();

        crate::eip8004::types::RegistrationFile {
            type_url: "https://eips.ethereum.org/EIPS/eip-8004#registration-v1".to_string(),
            name: self.name.clone().unwrap_or_default(),
            description: self.description.clone().unwrap_or_default(),
            image: self.image.clone(),
            services,
            x402_support: self.x402_support,
            active: self.active,
            registrations: None,
            supported_trust,
        }
    }
}

/// Auto-sync status info
#[derive(Debug, Clone, serde::Serialize)]
pub struct AutoSyncStatus {
    pub status: String,
    pub message: String,
    pub synced_at: Option<String>,
    pub key_count: Option<i32>,
    pub node_count: Option<i32>,
}
