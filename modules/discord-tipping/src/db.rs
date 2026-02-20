//! SQLite database operations for discord user profiles.

use discord_tipping_types::*;
use rusqlite::Result as SqliteResult;
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<rusqlite::Connection>,
}

impl Db {
    pub fn open(path: &str) -> SqliteResult<Self> {
        let conn = if path == ":memory:" {
            rusqlite::Connection::open_in_memory()?
        } else {
            rusqlite::Connection::open(path)?
        };
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.create_tables()?;
        Ok(db)
    }

    fn create_tables(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS discord_user_profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                discord_user_id TEXT NOT NULL UNIQUE,
                discord_username TEXT,
                public_address TEXT,
                registration_status TEXT NOT NULL DEFAULT 'unregistered',
                registered_at TEXT,
                last_interaction_at TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_discord_profiles_address
             ON discord_user_profiles(public_address)",
            [],
        )?;
        Ok(())
    }

    pub fn get_or_create_profile(
        &self,
        discord_user_id: &str,
        username: &str,
    ) -> Result<DiscordUserProfile, String> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT OR IGNORE INTO discord_user_profiles (discord_user_id, discord_username)
             VALUES (?1, ?2)",
            rusqlite::params![discord_user_id, username],
        )
        .map_err(|e| format!("Failed to insert profile: {}", e))?;

        conn.execute(
            "UPDATE discord_user_profiles
             SET last_interaction_at = datetime('now'),
                 discord_username = ?2,
                 updated_at = datetime('now')
             WHERE discord_user_id = ?1",
            rusqlite::params![discord_user_id, username],
        )
        .map_err(|e| format!("Failed to update profile: {}", e))?;

        get_profile_impl(&conn, discord_user_id)
    }

    pub fn get_profile(&self, discord_user_id: &str) -> Result<Option<DiscordUserProfile>, String> {
        let conn = self.conn.lock().unwrap();
        match get_profile_impl(&conn, discord_user_id) {
            Ok(p) => Ok(Some(p)),
            Err(e) if e.contains("not found") => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn get_profile_by_address(
        &self,
        address: &str,
    ) -> Result<Option<DiscordUserProfile>, String> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, discord_user_id, discord_username, public_address,
                    registration_status, registered_at, last_interaction_at,
                    created_at, updated_at
             FROM discord_user_profiles
             WHERE LOWER(public_address) = LOWER(?1)",
            rusqlite::params![address],
            |row| row_to_profile(row),
        );
        match result {
            Ok(profile) => Ok(Some(profile)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Database error: {}", e)),
        }
    }

    pub fn register_address(
        &self,
        discord_user_id: &str,
        address: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE discord_user_profiles
             SET public_address = ?1,
                 registration_status = 'registered',
                 registered_at = datetime('now'),
                 updated_at = datetime('now')
             WHERE discord_user_id = ?2",
            rusqlite::params![address, discord_user_id],
        )
        .map_err(|e| format!("Failed to register address: {}", e))?;
        Ok(())
    }

    pub fn unregister_address(&self, discord_user_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE discord_user_profiles
             SET public_address = NULL,
                 registration_status = 'unregistered',
                 registered_at = NULL,
                 updated_at = datetime('now')
             WHERE discord_user_id = ?1",
            rusqlite::params![discord_user_id],
        )
        .map_err(|e| format!("Failed to unregister address: {}", e))?;
        Ok(())
    }

    pub fn list_all_profiles(&self) -> Result<Vec<DiscordUserProfile>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, discord_user_id, discord_username, public_address,
                        registration_status, registered_at, last_interaction_at,
                        created_at, updated_at
                 FROM discord_user_profiles
                 ORDER BY updated_at DESC",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let profiles = stmt
            .query_map([], |row| row_to_profile(row))
            .map_err(|e| format!("Failed to query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(profiles)
    }

    pub fn list_registered_profiles(&self) -> Result<Vec<DiscordUserProfile>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, discord_user_id, discord_username, public_address,
                        registration_status, registered_at, last_interaction_at,
                        created_at, updated_at
                 FROM discord_user_profiles
                 WHERE registration_status = 'registered' AND public_address IS NOT NULL",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let profiles = stmt
            .query_map([], |row| row_to_profile(row))
            .map_err(|e| format!("Failed to query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(profiles)
    }

    pub fn get_stats(&self) -> Result<ProfileStats, String> {
        let conn = self.conn.lock().unwrap();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM discord_user_profiles", [], |r| r.get(0))
            .unwrap_or(0);
        let registered: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM discord_user_profiles WHERE registration_status = 'registered'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Ok(ProfileStats {
            total_profiles: total,
            registered_count: registered,
            unregistered_count: total - registered,
        })
    }

    pub fn clear_and_restore(&self, entries: &[BackupEntry]) -> Result<usize, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM discord_user_profiles", [])
            .map_err(|e| format!("Failed to clear: {}", e))?;

        let mut count = 0;
        for entry in entries {
            conn.execute(
                "INSERT OR IGNORE INTO discord_user_profiles (discord_user_id, discord_username)
                 VALUES (?1, ?2)",
                rusqlite::params![entry.discord_user_id, entry.discord_username],
            )
            .map_err(|e| format!("Failed to insert: {}", e))?;

            conn.execute(
                "UPDATE discord_user_profiles
                 SET public_address = ?1,
                     registration_status = 'registered',
                     registered_at = COALESCE(?2, datetime('now')),
                     updated_at = datetime('now')
                 WHERE discord_user_id = ?3",
                rusqlite::params![entry.public_address, entry.registered_at, entry.discord_user_id],
            )
            .map_err(|e| format!("Failed to register: {}", e))?;
            count += 1;
        }
        Ok(count)
    }
}

fn get_profile_impl(
    conn: &rusqlite::Connection,
    discord_user_id: &str,
) -> Result<DiscordUserProfile, String> {
    conn.query_row(
        "SELECT id, discord_user_id, discord_username, public_address,
                registration_status, registered_at, last_interaction_at,
                created_at, updated_at
         FROM discord_user_profiles
         WHERE discord_user_id = ?1",
        rusqlite::params![discord_user_id],
        |row| row_to_profile(row),
    )
    .map_err(|e| format!("Profile not found: {}", e))
}

fn row_to_profile(row: &rusqlite::Row) -> rusqlite::Result<DiscordUserProfile> {
    Ok(DiscordUserProfile {
        id: row.get(0)?,
        discord_user_id: row.get(1)?,
        discord_username: row.get(2)?,
        public_address: row.get(3)?,
        registration_status: row.get(4)?,
        registered_at: row.get(5)?,
        last_interaction_at: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}
