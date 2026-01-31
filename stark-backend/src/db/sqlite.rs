//! SQLite database - schema definitions and connection management
//!
//! This file contains:
//! - Database struct definition
//! - Connection management (new, init)
//! - Schema creation and migrations
//!
//! All database operations are in the models/ subdirectory.

use rusqlite::{Connection, Result as SqliteResult};
use std::path::Path;
use std::sync::Mutex;

/// Main database wrapper with connection pooling via Mutex
pub struct Database {
    pub(crate) conn: Mutex<Connection>,
}

impl Database {
    /// Create a new database connection and initialize schema
    pub fn new(database_url: &str) -> SqliteResult<Self> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = Path::new(database_url).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }

        let conn = Connection::open(database_url)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init()?;
        Ok(db)
    }

    /// Initialize all database tables and run migrations
    fn init(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();

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
                bot_token TEXT NOT NULL,
                app_token TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(channel_type, name)
            )",
            [],
        )?;

        // Agent settings table (AI endpoint configuration - simplified for x402)
        // Note: provider, api_key, model columns are deprecated (kept for migration compatibility)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                endpoint TEXT NOT NULL,
                model_archetype TEXT NOT NULL DEFAULT 'kimi',
                max_tokens INTEGER NOT NULL DEFAULT 40000,
                enabled INTEGER NOT NULL DEFAULT 0,
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

        // Migration: Add max_tokens column if it doesn't exist (for old DBs)
        let has_max_tokens: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_settings') WHERE name='max_tokens'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_max_tokens {
            conn.execute("ALTER TABLE agent_settings ADD COLUMN max_tokens INTEGER DEFAULT 40000", [])?;
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
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (channel_id) REFERENCES external_channels(id) ON DELETE CASCADE
            )",
            [],
        )?;

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

        // Agent identity (our EIP-8004 registration)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_identity (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                agent_id INTEGER NOT NULL,
                agent_registry TEXT NOT NULL,
                chain_id INTEGER NOT NULL DEFAULT 8453,
                registration_uri TEXT,
                registration_hash TEXT,
                wallet_address TEXT NOT NULL,
                owner_address TEXT,
                name TEXT,
                description TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                tx_hash TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

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
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE x402_payments SET status = ?1, tx_hash = COALESCE(?2, tx_hash) WHERE id = ?3",
            rusqlite::params![status, tx_hash, payment_id],
        )?;
        Ok(())
    }
}
