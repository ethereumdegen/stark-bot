use chrono::{DateTime, Duration, NaiveDate, Timelike, Utc};
use rusqlite::{Connection, Result as SqliteResult};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

use crate::models::{
    AgentSettings, ApiKey, Channel, ChatSession, IdentityLink, Memory, MemorySearchResult,
    MemoryType, MessageRole, ResetPolicy, Session, SessionMessage, SessionScope,
};

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
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
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
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

        // Agent settings table (AI provider configuration)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                endpoint TEXT NOT NULL,
                api_key TEXT NOT NULL,
                model TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

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
                expires_at TEXT
            )",
            [],
        )?;

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

        // Memories table - daily logs and long-term memories
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
                FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE SET NULL
            )",
            [],
        )?;

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

        Ok(())
    }

    // Auth Session methods (for web login sessions)
    pub fn create_session(&self) -> SqliteResult<Session> {
        let conn = self.conn.lock().unwrap();
        let token = Uuid::new_v4().to_string();
        let created_at = Utc::now();
        let expires_at = created_at + Duration::hours(24);

        conn.execute(
            "INSERT INTO auth_sessions (token, created_at, expires_at) VALUES (?1, ?2, ?3)",
            [
                &token,
                &created_at.to_rfc3339(),
                &expires_at.to_rfc3339(),
            ],
        )?;

        let id = conn.last_insert_rowid();

        Ok(Session {
            id,
            token,
            created_at,
            expires_at,
        })
    }

    pub fn validate_session(&self, token: &str) -> SqliteResult<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let mut stmt = conn.prepare(
            "SELECT id, token, created_at, expires_at FROM auth_sessions WHERE token = ?1 AND expires_at > ?2",
        )?;

        let session = stmt
            .query_row([token, &now], |row| {
                let created_at_str: String = row.get(2)?;
                let expires_at_str: String = row.get(3)?;

                Ok(Session {
                    id: row.get(0)?,
                    token: row.get(1)?,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    expires_at: DateTime::parse_from_rfc3339(&expires_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })
            .ok();

        Ok(session)
    }

    pub fn delete_session(&self, token: &str) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute("DELETE FROM auth_sessions WHERE token = ?1", [token])?;
        Ok(rows_affected > 0)
    }

    // API Key methods
    pub fn get_api_key(&self, service_name: &str) -> SqliteResult<Option<ApiKey>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, service_name, api_key, created_at, updated_at FROM external_api_keys WHERE service_name = ?1",
        )?;

        let api_key = stmt
            .query_row([service_name], |row| {
                let created_at_str: String = row.get(3)?;
                let updated_at_str: String = row.get(4)?;

                Ok(ApiKey {
                    id: row.get(0)?,
                    service_name: row.get(1)?,
                    api_key: row.get(2)?,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })
            .ok();

        Ok(api_key)
    }

    pub fn list_api_keys(&self) -> SqliteResult<Vec<ApiKey>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, service_name, api_key, created_at, updated_at FROM external_api_keys ORDER BY service_name",
        )?;

        let api_keys = stmt
            .query_map([], |row| {
                let created_at_str: String = row.get(3)?;
                let updated_at_str: String = row.get(4)?;

                Ok(ApiKey {
                    id: row.get(0)?,
                    service_name: row.get(1)?,
                    api_key: row.get(2)?,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(api_keys)
    }

    pub fn upsert_api_key(&self, service_name: &str, api_key: &str) -> SqliteResult<ApiKey> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Try to update first
        let rows_affected = conn.execute(
            "UPDATE external_api_keys SET api_key = ?1, updated_at = ?2 WHERE service_name = ?3",
            [api_key, &now, service_name],
        )?;

        if rows_affected == 0 {
            // Insert new
            conn.execute(
                "INSERT INTO external_api_keys (service_name, api_key, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                [service_name, api_key, &now, &now],
            )?;
        }

        drop(conn);

        // Return the upserted key
        self.get_api_key(service_name).map(|opt| opt.unwrap())
    }

    pub fn delete_api_key(&self, service_name: &str) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute(
            "DELETE FROM external_api_keys WHERE service_name = ?1",
            [service_name],
        )?;
        Ok(rows_affected > 0)
    }

    // Channel methods
    pub fn create_channel(
        &self,
        channel_type: &str,
        name: &str,
        bot_token: &str,
        app_token: Option<&str>,
    ) -> SqliteResult<Channel> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO external_channels (channel_type, name, enabled, bot_token, app_token, created_at, updated_at)
             VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6)",
            rusqlite::params![channel_type, name, bot_token, app_token, &now, &now],
        )?;

        let id = conn.last_insert_rowid();

        Ok(Channel {
            id,
            channel_type: channel_type.to_string(),
            name: name.to_string(),
            enabled: false,
            bot_token: bot_token.to_string(),
            app_token: app_token.map(|s| s.to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }

    pub fn get_channel(&self, id: i64) -> SqliteResult<Option<Channel>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, channel_type, name, enabled, bot_token, app_token, created_at, updated_at
             FROM external_channels WHERE id = ?1",
        )?;

        let channel = stmt
            .query_row([id], |row| {
                let created_at_str: String = row.get(6)?;
                let updated_at_str: String = row.get(7)?;

                Ok(Channel {
                    id: row.get(0)?,
                    channel_type: row.get(1)?,
                    name: row.get(2)?,
                    enabled: row.get::<_, i32>(3)? != 0,
                    bot_token: row.get(4)?,
                    app_token: row.get(5)?,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })
            .ok();

        Ok(channel)
    }

    pub fn list_channels(&self) -> SqliteResult<Vec<Channel>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, channel_type, name, enabled, bot_token, app_token, created_at, updated_at
             FROM external_channels ORDER BY channel_type, name",
        )?;

        let channels = stmt
            .query_map([], |row| {
                let created_at_str: String = row.get(6)?;
                let updated_at_str: String = row.get(7)?;

                Ok(Channel {
                    id: row.get(0)?,
                    channel_type: row.get(1)?,
                    name: row.get(2)?,
                    enabled: row.get::<_, i32>(3)? != 0,
                    bot_token: row.get(4)?,
                    app_token: row.get(5)?,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(channels)
    }

    pub fn list_enabled_channels(&self) -> SqliteResult<Vec<Channel>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, channel_type, name, enabled, bot_token, app_token, created_at, updated_at
             FROM external_channels WHERE enabled = 1 ORDER BY channel_type, name",
        )?;

        let channels = stmt
            .query_map([], |row| {
                let created_at_str: String = row.get(6)?;
                let updated_at_str: String = row.get(7)?;

                Ok(Channel {
                    id: row.get(0)?,
                    channel_type: row.get(1)?,
                    name: row.get(2)?,
                    enabled: row.get::<_, i32>(3)? != 0,
                    bot_token: row.get(4)?,
                    app_token: row.get(5)?,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(channels)
    }

    pub fn update_channel(
        &self,
        id: i64,
        name: Option<&str>,
        enabled: Option<bool>,
        bot_token: Option<&str>,
        app_token: Option<Option<&str>>,
    ) -> SqliteResult<Option<Channel>> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Build dynamic update query
        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut param_idx = 2;

        if name.is_some() {
            updates.push(format!("name = ?{}", param_idx));
            param_idx += 1;
        }
        if enabled.is_some() {
            updates.push(format!("enabled = ?{}", param_idx));
            param_idx += 1;
        }
        if bot_token.is_some() {
            updates.push(format!("bot_token = ?{}", param_idx));
            param_idx += 1;
        }
        if app_token.is_some() {
            updates.push(format!("app_token = ?{}", param_idx));
            param_idx += 1;
        }

        let sql = format!(
            "UPDATE external_channels SET {} WHERE id = ?{}",
            updates.join(", "),
            param_idx
        );

        // Build params dynamically
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        if let Some(n) = name {
            params.push(Box::new(n.to_string()));
        }
        if let Some(e) = enabled {
            params.push(Box::new(if e { 1 } else { 0 }));
        }
        if let Some(t) = bot_token {
            params.push(Box::new(t.to_string()));
        }
        if let Some(at) = app_token {
            params.push(Box::new(at.map(|s| s.to_string())));
        }
        params.push(Box::new(id));

        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        conn.execute(&sql, params_ref.as_slice())?;

        drop(conn);
        self.get_channel(id)
    }

    pub fn set_channel_enabled(&self, id: i64, enabled: bool) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let rows_affected = conn.execute(
            "UPDATE external_channels SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![if enabled { 1 } else { 0 }, &now, id],
        )?;

        Ok(rows_affected > 0)
    }

    pub fn delete_channel(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute(
            "DELETE FROM external_channels WHERE id = ?1",
            [id],
        )?;
        Ok(rows_affected > 0)
    }

    // Agent Settings methods

    /// Get the currently enabled agent settings (only one can be enabled)
    pub fn get_active_agent_settings(&self) -> SqliteResult<Option<AgentSettings>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, provider, endpoint, api_key, model, enabled, created_at, updated_at
             FROM agent_settings WHERE enabled = 1 LIMIT 1",
        )?;

        let settings = stmt
            .query_row([], |row| {
                let created_at_str: String = row.get(6)?;
                let updated_at_str: String = row.get(7)?;

                Ok(AgentSettings {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    endpoint: row.get(2)?,
                    api_key: row.get(3)?,
                    model: row.get(4)?,
                    enabled: row.get::<_, i32>(5)? != 0,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })
            .ok();

        Ok(settings)
    }

    /// Get agent settings by provider name
    pub fn get_agent_settings_by_provider(&self, provider: &str) -> SqliteResult<Option<AgentSettings>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, provider, endpoint, api_key, model, enabled, created_at, updated_at
             FROM agent_settings WHERE provider = ?1",
        )?;

        let settings = stmt
            .query_row([provider], |row| {
                let created_at_str: String = row.get(6)?;
                let updated_at_str: String = row.get(7)?;

                Ok(AgentSettings {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    endpoint: row.get(2)?,
                    api_key: row.get(3)?,
                    model: row.get(4)?,
                    enabled: row.get::<_, i32>(5)? != 0,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })
            .ok();

        Ok(settings)
    }

    /// List all agent settings
    pub fn list_agent_settings(&self) -> SqliteResult<Vec<AgentSettings>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, provider, endpoint, api_key, model, enabled, created_at, updated_at
             FROM agent_settings ORDER BY provider",
        )?;

        let settings = stmt
            .query_map([], |row| {
                let created_at_str: String = row.get(6)?;
                let updated_at_str: String = row.get(7)?;

                Ok(AgentSettings {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    endpoint: row.get(2)?,
                    api_key: row.get(3)?,
                    model: row.get(4)?,
                    enabled: row.get::<_, i32>(5)? != 0,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(settings)
    }

    /// Save agent settings (upsert by provider, and set as the only enabled one)
    pub fn save_agent_settings(
        &self,
        provider: &str,
        endpoint: &str,
        api_key: &str,
        model: &str,
    ) -> SqliteResult<AgentSettings> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // First, disable all existing settings
        conn.execute("UPDATE agent_settings SET enabled = 0, updated_at = ?1", [&now])?;

        // Check if this provider already exists
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM agent_settings WHERE provider = ?1",
                [provider],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            // Update existing
            conn.execute(
                "UPDATE agent_settings SET endpoint = ?1, api_key = ?2, model = ?3, enabled = 1, updated_at = ?4 WHERE id = ?5",
                rusqlite::params![endpoint, api_key, model, &now, id],
            )?;
        } else {
            // Insert new
            conn.execute(
                "INSERT INTO agent_settings (provider, endpoint, api_key, model, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)",
                rusqlite::params![provider, endpoint, api_key, model, &now, &now],
            )?;
        }

        drop(conn);

        // Return the saved settings
        self.get_agent_settings_by_provider(provider)
            .map(|opt| opt.unwrap())
    }

    /// Disable all agent settings (no AI provider active)
    pub fn disable_agent_settings(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute("UPDATE agent_settings SET enabled = 0, updated_at = ?1", [&now])?;
        Ok(())
    }

    // ============================================
    // Chat Session methods
    // ============================================

    /// Generate a session key from channel info
    fn generate_session_key(channel_type: &str, channel_id: i64, platform_chat_id: &str) -> String {
        format!("{}:{}:{}", channel_type, channel_id, platform_chat_id)
    }

    /// Get or create a chat session, handling reset policy
    pub fn get_or_create_chat_session(
        &self,
        channel_type: &str,
        channel_id: i64,
        platform_chat_id: &str,
        scope: SessionScope,
        agent_id: Option<&str>,
    ) -> SqliteResult<ChatSession> {
        let session_key = Self::generate_session_key(channel_type, channel_id, platform_chat_id);
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        // Try to get existing session
        if let Some(mut session) = self.get_chat_session_by_key(&session_key)? {
            // Check if session needs reset based on policy
            let should_reset = match session.reset_policy {
                ResetPolicy::Daily => {
                    let reset_hour = session.daily_reset_hour.unwrap_or(0);
                    let last_activity = session.last_activity_at;
                    let last_day = last_activity.date_naive();
                    let today = now.date_naive();

                    if today > last_day {
                        // Check if we've passed the reset hour today
                        now.hour() >= reset_hour as u32
                    } else {
                        false
                    }
                }
                ResetPolicy::Idle => {
                    if let Some(timeout) = session.idle_timeout_minutes {
                        let idle_duration = now.signed_duration_since(session.last_activity_at);
                        idle_duration.num_minutes() > timeout as i64
                    } else {
                        false
                    }
                }
                ResetPolicy::Manual | ResetPolicy::Never => false,
            };

            if should_reset {
                // Reset the session
                self.reset_chat_session(session.id)?;
                session = self.get_chat_session(session.id)?.unwrap();
            } else {
                // Update last activity
                let conn = self.conn.lock().unwrap();
                conn.execute(
                    "UPDATE chat_sessions SET last_activity_at = ?1, updated_at = ?1 WHERE id = ?2",
                    rusqlite::params![&now_str, session.id],
                )?;
                session.last_activity_at = now;
                session.updated_at = now;
            }

            return Ok(session);
        }

        // Create new session
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chat_sessions (session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour, created_at, updated_at, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, ?10, ?10, ?10)",
            rusqlite::params![
                &session_key,
                agent_id,
                scope.as_str(),
                channel_type,
                channel_id,
                platform_chat_id,
                ResetPolicy::default().as_str(),
                Option::<i32>::None,
                Some(0i32),
                &now_str,
            ],
        )?;

        let id = conn.last_insert_rowid();
        drop(conn);

        self.get_chat_session(id).map(|opt| opt.unwrap())
    }

    /// Get a chat session by ID
    pub fn get_chat_session(&self, id: i64) -> SqliteResult<Option<ChatSession>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour,
             created_at, updated_at, last_activity_at, expires_at
             FROM chat_sessions WHERE id = ?1",
        )?;

        let session = stmt
            .query_row([id], |row| Self::row_to_chat_session(row))
            .ok();

        Ok(session)
    }

    /// Get a chat session by session key
    pub fn get_chat_session_by_key(&self, session_key: &str) -> SqliteResult<Option<ChatSession>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour,
             created_at, updated_at, last_activity_at, expires_at
             FROM chat_sessions WHERE session_key = ?1 AND is_active = 1",
        )?;

        let session = stmt
            .query_row([session_key], |row| Self::row_to_chat_session(row))
            .ok();

        Ok(session)
    }

    /// Reset a chat session (mark old as inactive, create new)
    pub fn reset_chat_session(&self, id: i64) -> SqliteResult<ChatSession> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Get the old session info
        let old_session: Option<(String, Option<String>, String, String, i64, String, String, Option<i32>, Option<i32>)> = conn
            .query_row(
                "SELECT session_key, agent_id, scope, channel_type, channel_id, platform_chat_id, reset_policy, idle_timeout_minutes, daily_reset_hour
                 FROM chat_sessions WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)),
            )
            .ok();

        let Some((session_key, agent_id, scope, channel_type, channel_id, platform_chat_id, reset_policy, idle_timeout, daily_hour)) = old_session else {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        };

        // Mark old session as inactive
        conn.execute(
            "UPDATE chat_sessions SET is_active = 0, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![&now, id],
        )?;

        // Create new session with same settings
        conn.execute(
            "INSERT INTO chat_sessions (session_key, agent_id, scope, channel_type, channel_id, platform_chat_id,
             is_active, reset_policy, idle_timeout_minutes, daily_reset_hour, created_at, updated_at, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, ?10, ?10, ?10)",
            rusqlite::params![
                &session_key,
                agent_id,
                &scope,
                &channel_type,
                channel_id,
                &platform_chat_id,
                &reset_policy,
                idle_timeout,
                daily_hour,
                &now,
            ],
        )?;

        let new_id = conn.last_insert_rowid();
        drop(conn);

        self.get_chat_session(new_id).map(|opt| opt.unwrap())
    }

    /// Update session reset policy
    pub fn update_session_reset_policy(
        &self,
        id: i64,
        reset_policy: ResetPolicy,
        idle_timeout_minutes: Option<i32>,
        daily_reset_hour: Option<i32>,
    ) -> SqliteResult<Option<ChatSession>> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE chat_sessions SET reset_policy = ?1, idle_timeout_minutes = ?2, daily_reset_hour = ?3, updated_at = ?4 WHERE id = ?5",
            rusqlite::params![reset_policy.as_str(), idle_timeout_minutes, daily_reset_hour, &now, id],
        )?;

        drop(conn);
        self.get_chat_session(id)
    }

    fn row_to_chat_session(row: &rusqlite::Row) -> rusqlite::Result<ChatSession> {
        let created_at_str: String = row.get(11)?;
        let updated_at_str: String = row.get(12)?;
        let last_activity_str: String = row.get(13)?;
        let expires_at_str: Option<String> = row.get(14)?;
        let scope_str: String = row.get(3)?;
        let reset_policy_str: String = row.get(8)?;

        Ok(ChatSession {
            id: row.get(0)?,
            session_key: row.get(1)?,
            agent_id: row.get(2)?,
            scope: SessionScope::from_str(&scope_str).unwrap_or_default(),
            channel_type: row.get(4)?,
            channel_id: row.get(5)?,
            platform_chat_id: row.get(6)?,
            is_active: row.get::<_, i32>(7)? != 0,
            reset_policy: ResetPolicy::from_str(&reset_policy_str).unwrap_or_default(),
            idle_timeout_minutes: row.get(9)?,
            daily_reset_hour: row.get(10)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
            last_activity_at: DateTime::parse_from_rfc3339(&last_activity_str)
                .unwrap()
                .with_timezone(&Utc),
            expires_at: expires_at_str.map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .unwrap()
                    .with_timezone(&Utc)
            }),
        })
    }

    // ============================================
    // Session Message methods
    // ============================================

    /// Add a message to a session
    pub fn add_session_message(
        &self,
        session_id: i64,
        role: MessageRole,
        content: &str,
        user_id: Option<&str>,
        user_name: Option<&str>,
        platform_message_id: Option<&str>,
        tokens_used: Option<i32>,
    ) -> SqliteResult<SessionMessage> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        conn.execute(
            "INSERT INTO session_messages (session_id, role, content, user_id, user_name, platform_message_id, tokens_used, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                session_id,
                role.as_str(),
                content,
                user_id,
                user_name,
                platform_message_id,
                tokens_used,
                &now_str,
            ],
        )?;

        let id = conn.last_insert_rowid();

        Ok(SessionMessage {
            id,
            session_id,
            role,
            content: content.to_string(),
            user_id: user_id.map(|s| s.to_string()),
            user_name: user_name.map(|s| s.to_string()),
            platform_message_id: platform_message_id.map(|s| s.to_string()),
            tokens_used,
            created_at: now,
        })
    }

    /// Get all messages for a session
    pub fn get_session_messages(&self, session_id: i64) -> SqliteResult<Vec<SessionMessage>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, user_id, user_name, platform_message_id, tokens_used, created_at
             FROM session_messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;

        let messages = stmt
            .query_map([session_id], |row| Self::row_to_session_message(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(messages)
    }

    /// Get recent messages for a session (limited)
    pub fn get_recent_session_messages(&self, session_id: i64, limit: i32) -> SqliteResult<Vec<SessionMessage>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, user_id, user_name, platform_message_id, tokens_used, created_at
             FROM session_messages WHERE session_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;

        let mut messages: Vec<SessionMessage> = stmt
            .query_map(rusqlite::params![session_id, limit], |row| Self::row_to_session_message(row))?
            .filter_map(|r| r.ok())
            .collect();

        // Reverse to get chronological order
        messages.reverse();
        Ok(messages)
    }

    /// Count messages in a session
    pub fn count_session_messages(&self, session_id: i64) -> SqliteResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM session_messages WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
    }

    fn row_to_session_message(row: &rusqlite::Row) -> rusqlite::Result<SessionMessage> {
        let created_at_str: String = row.get(8)?;
        let role_str: String = row.get(2)?;

        Ok(SessionMessage {
            id: row.get(0)?,
            session_id: row.get(1)?,
            role: MessageRole::from_str(&role_str).unwrap_or(MessageRole::User),
            content: row.get(3)?,
            user_id: row.get(4)?,
            user_name: row.get(5)?,
            platform_message_id: row.get(6)?,
            tokens_used: row.get(7)?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
        })
    }

    // ============================================
    // Identity methods
    // ============================================

    /// Get or create an identity for a platform user
    pub fn get_or_create_identity(
        &self,
        channel_type: &str,
        platform_user_id: &str,
        platform_user_name: Option<&str>,
    ) -> SqliteResult<IdentityLink> {
        // Try to get existing
        if let Some(link) = self.get_identity_by_platform(channel_type, platform_user_id)? {
            // Update username if changed
            if platform_user_name.is_some() && link.platform_user_name.as_deref() != platform_user_name {
                let conn = self.conn.lock().unwrap();
                let now = Utc::now().to_rfc3339();
                conn.execute(
                    "UPDATE identity_links SET platform_user_name = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![platform_user_name, &now, link.id],
                )?;
            }
            return self.get_identity_by_platform(channel_type, platform_user_id).map(|opt| opt.unwrap());
        }

        // Create new identity
        let identity_id = Uuid::new_v4().to_string();
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        conn.execute(
            "INSERT INTO identity_links (identity_id, channel_type, platform_user_id, platform_user_name, is_verified, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)",
            rusqlite::params![&identity_id, channel_type, platform_user_id, platform_user_name, &now_str],
        )?;

        let id = conn.last_insert_rowid();

        Ok(IdentityLink {
            id,
            identity_id,
            channel_type: channel_type.to_string(),
            platform_user_id: platform_user_id.to_string(),
            platform_user_name: platform_user_name.map(|s| s.to_string()),
            is_verified: false,
            verified_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Link an existing identity to a new platform
    pub fn link_identity(
        &self,
        identity_id: &str,
        channel_type: &str,
        platform_user_id: &str,
        platform_user_name: Option<&str>,
    ) -> SqliteResult<IdentityLink> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        conn.execute(
            "INSERT INTO identity_links (identity_id, channel_type, platform_user_id, platform_user_name, is_verified, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)",
            rusqlite::params![identity_id, channel_type, platform_user_id, platform_user_name, &now_str],
        )?;

        let id = conn.last_insert_rowid();

        Ok(IdentityLink {
            id,
            identity_id: identity_id.to_string(),
            channel_type: channel_type.to_string(),
            platform_user_id: platform_user_id.to_string(),
            platform_user_name: platform_user_name.map(|s| s.to_string()),
            is_verified: false,
            verified_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    /// Get identity by platform credentials
    pub fn get_identity_by_platform(&self, channel_type: &str, platform_user_id: &str) -> SqliteResult<Option<IdentityLink>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, identity_id, channel_type, platform_user_id, platform_user_name, is_verified, verified_at, created_at, updated_at
             FROM identity_links WHERE channel_type = ?1 AND platform_user_id = ?2",
        )?;

        let link = stmt
            .query_row(rusqlite::params![channel_type, platform_user_id], |row| Self::row_to_identity_link(row))
            .ok();

        Ok(link)
    }

    /// Get all linked identities for an identity_id
    pub fn get_linked_identities(&self, identity_id: &str) -> SqliteResult<Vec<IdentityLink>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, identity_id, channel_type, platform_user_id, platform_user_name, is_verified, verified_at, created_at, updated_at
             FROM identity_links WHERE identity_id = ?1",
        )?;

        let links = stmt
            .query_map([identity_id], |row| Self::row_to_identity_link(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(links)
    }

    fn row_to_identity_link(row: &rusqlite::Row) -> rusqlite::Result<IdentityLink> {
        let created_at_str: String = row.get(7)?;
        let updated_at_str: String = row.get(8)?;
        let verified_at_str: Option<String> = row.get(6)?;

        Ok(IdentityLink {
            id: row.get(0)?,
            identity_id: row.get(1)?,
            channel_type: row.get(2)?,
            platform_user_id: row.get(3)?,
            platform_user_name: row.get(4)?,
            is_verified: row.get::<_, i32>(5)? != 0,
            verified_at: verified_at_str.map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .unwrap()
                    .with_timezone(&Utc)
            }),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
        })
    }

    // ============================================
    // Memory methods
    // ============================================

    /// Create a memory (daily_log or long_term)
    pub fn create_memory(
        &self,
        memory_type: MemoryType,
        content: &str,
        category: Option<&str>,
        tags: Option<&str>,
        importance: i32,
        identity_id: Option<&str>,
        session_id: Option<i64>,
        source_channel_type: Option<&str>,
        source_message_id: Option<&str>,
        log_date: Option<NaiveDate>,
        expires_at: Option<DateTime<Utc>>,
    ) -> SqliteResult<Memory> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let log_date_str = log_date.map(|d| d.to_string());
        let expires_at_str = expires_at.map(|dt| dt.to_rfc3339());

        conn.execute(
            "INSERT INTO memories (memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12)",
            rusqlite::params![
                memory_type.as_str(),
                content,
                category,
                tags,
                importance,
                identity_id,
                session_id,
                source_channel_type,
                source_message_id,
                log_date_str,
                &now_str,
                expires_at_str,
            ],
        )?;

        let id = conn.last_insert_rowid();

        Ok(Memory {
            id,
            memory_type,
            content: content.to_string(),
            category: category.map(|s| s.to_string()),
            tags: tags.map(|s| s.to_string()),
            importance,
            identity_id: identity_id.map(|s| s.to_string()),
            session_id,
            source_channel_type: source_channel_type.map(|s| s.to_string()),
            source_message_id: source_message_id.map(|s| s.to_string()),
            log_date,
            created_at: now,
            updated_at: now,
            expires_at,
        })
    }

    /// Search memories using FTS5
    pub fn search_memories(
        &self,
        query: &str,
        memory_type: Option<MemoryType>,
        identity_id: Option<&str>,
        category: Option<&str>,
        min_importance: Option<i32>,
        limit: i32,
    ) -> SqliteResult<Vec<MemorySearchResult>> {
        let conn = self.conn.lock().unwrap();

        // Build the query with filters
        let mut sql = String::from(
            "SELECT m.id, m.memory_type, m.content, m.category, m.tags, m.importance, m.identity_id,
             m.session_id, m.source_channel_type, m.source_message_id, m.log_date,
             m.created_at, m.updated_at, m.expires_at, bm25(memories_fts) as rank
             FROM memories m
             JOIN memories_fts ON m.id = memories_fts.rowid
             WHERE memories_fts MATCH ?1",
        );

        let mut conditions: Vec<String> = Vec::new();
        if memory_type.is_some() {
            conditions.push("m.memory_type = ?2".to_string());
        }
        if identity_id.is_some() {
            conditions.push(format!("m.identity_id = ?{}", if memory_type.is_some() { 3 } else { 2 }));
        }
        if category.is_some() {
            let idx = 2 + (memory_type.is_some() as usize) + (identity_id.is_some() as usize);
            conditions.push(format!("m.category = ?{}", idx));
        }
        if min_importance.is_some() {
            let idx = 2 + (memory_type.is_some() as usize) + (identity_id.is_some() as usize) + (category.is_some() as usize);
            conditions.push(format!("m.importance >= ?{}", idx));
        }

        if !conditions.is_empty() {
            sql.push_str(" AND ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(" ORDER BY rank LIMIT ?");
        let limit_idx = 2 + (memory_type.is_some() as usize) + (identity_id.is_some() as usize)
            + (category.is_some() as usize) + (min_importance.is_some() as usize);
        sql = sql.replace("LIMIT ?", &format!("LIMIT ?{}", limit_idx));

        let mut stmt = conn.prepare(&sql)?;

        // Build params dynamically
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(query.to_string())];
        if let Some(mt) = memory_type {
            params.push(Box::new(mt.as_str().to_string()));
        }
        if let Some(iid) = identity_id {
            params.push(Box::new(iid.to_string()));
        }
        if let Some(cat) = category {
            params.push(Box::new(cat.to_string()));
        }
        if let Some(mi) = min_importance {
            params.push(Box::new(mi));
        }
        params.push(Box::new(limit));

        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let results = stmt
            .query_map(params_ref.as_slice(), |row| {
                let memory = Self::row_to_memory(row)?;
                let rank: f64 = row.get(14)?;
                Ok(MemorySearchResult {
                    memory: memory.into(),
                    rank,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// Get today's daily logs
    pub fn get_todays_daily_logs(&self, identity_id: Option<&str>) -> SqliteResult<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();
        let today = Utc::now().date_naive().to_string();

        let sql = if identity_id.is_some() {
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories WHERE memory_type = 'daily_log' AND log_date = ?1 AND identity_id = ?2 ORDER BY created_at ASC"
        } else {
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories WHERE memory_type = 'daily_log' AND log_date = ?1 ORDER BY created_at ASC"
        };

        let mut stmt = conn.prepare(sql)?;

        let memories: Vec<Memory> = if let Some(iid) = identity_id {
            stmt.query_map(rusqlite::params![&today, iid], |row| Self::row_to_memory(row))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map([&today], |row| Self::row_to_memory(row))?
                .filter_map(|r| r.ok())
                .collect()
        };

        Ok(memories)
    }

    /// Get long-term memories for an identity
    pub fn get_long_term_memories(&self, identity_id: Option<&str>, min_importance: Option<i32>, limit: i32) -> SqliteResult<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();
        let min_imp = min_importance.unwrap_or(0);

        let sql = if identity_id.is_some() {
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories WHERE memory_type = 'long_term' AND identity_id = ?1 AND importance >= ?2
             ORDER BY importance DESC, created_at DESC LIMIT ?3"
        } else {
            "SELECT id, memory_type, content, category, tags, importance, identity_id, session_id,
             source_channel_type, source_message_id, log_date, created_at, updated_at, expires_at
             FROM memories WHERE memory_type = 'long_term' AND importance >= ?1
             ORDER BY importance DESC, created_at DESC LIMIT ?2"
        };

        let mut stmt = conn.prepare(sql)?;

        let memories: Vec<Memory> = if let Some(iid) = identity_id {
            stmt.query_map(rusqlite::params![iid, min_imp, limit], |row| Self::row_to_memory(row))?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![min_imp, limit], |row| Self::row_to_memory(row))?
                .filter_map(|r| r.ok())
                .collect()
        };

        Ok(memories)
    }

    /// Delete a memory
    pub fn delete_memory(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute("DELETE FROM memories WHERE id = ?1", [id])?;
        Ok(rows_affected > 0)
    }

    /// Cleanup expired memories
    pub fn cleanup_expired_memories(&self) -> SqliteResult<i64> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let rows_affected = conn.execute(
            "DELETE FROM memories WHERE expires_at IS NOT NULL AND expires_at < ?1",
            [&now],
        )?;
        Ok(rows_affected as i64)
    }

    fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
        let created_at_str: String = row.get(11)?;
        let updated_at_str: String = row.get(12)?;
        let expires_at_str: Option<String> = row.get(13)?;
        let log_date_str: Option<String> = row.get(10)?;
        let memory_type_str: String = row.get(1)?;

        Ok(Memory {
            id: row.get(0)?,
            memory_type: MemoryType::from_str(&memory_type_str).unwrap_or(MemoryType::DailyLog),
            content: row.get(2)?,
            category: row.get(3)?,
            tags: row.get(4)?,
            importance: row.get(5)?,
            identity_id: row.get(6)?,
            session_id: row.get(7)?,
            source_channel_type: row.get(8)?,
            source_message_id: row.get(9)?,
            log_date: log_date_str.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
            expires_at: expires_at_str.map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .unwrap()
                    .with_timezone(&Utc)
            }),
        })
    }
}
