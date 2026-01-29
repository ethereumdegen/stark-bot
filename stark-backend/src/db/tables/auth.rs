//! Auth sessions and auth challenges database operations

use chrono::{DateTime, Duration, Utc};
use rusqlite::Result as SqliteResult;

use crate::models::Session;
use super::super::Database;

impl Database {
    // ============================================
    // Auth Session methods (for web login sessions)
    // ============================================

    pub fn create_session(&self) -> SqliteResult<Session> {
        self.create_session_for_address(None)
    }

    pub fn create_session_for_address(&self, public_address: Option<&str>) -> SqliteResult<Session> {
        let conn = self.conn.lock().unwrap();
        let token = Self::generate_session_token();
        let created_at = Utc::now();
        let expires_at = created_at + Duration::hours(24);

        conn.execute(
            "INSERT INTO auth_sessions (token, public_address, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                &token,
                public_address,
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

    fn generate_session_token() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (0..32)
            .map(|_| format!("{:x}", rng.r#gen::<u8>() % 16))
            .collect()
    }

    pub fn validate_session(&self, token: &str) -> SqliteResult<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        let mut stmt = conn.prepare(
            "SELECT id, token, created_at, expires_at FROM auth_sessions WHERE token = ?1 AND expires_at > ?2",
        )?;

        let session = stmt
            .query_row([token, &now_str], |row| {
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

        // Extend session expiry on successful validation (keep active sessions alive)
        if session.is_some() {
            let new_expires = (now + Duration::hours(24)).to_rfc3339();
            let _ = conn.execute(
                "UPDATE auth_sessions SET expires_at = ?1 WHERE token = ?2",
                [&new_expires, token],
            );
        }

        Ok(session)
    }

    pub fn delete_session(&self, token: &str) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute("DELETE FROM auth_sessions WHERE token = ?1", [token])?;
        Ok(rows_affected > 0)
    }

    // ============================================
    // Auth Challenge methods (for SIWE)
    // ============================================

    pub fn create_or_update_challenge(&self, public_address: &str, challenge: &str) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Upsert: insert or replace existing challenge for this address
        conn.execute(
            "INSERT INTO auth_challenges (public_address, challenge, created_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(public_address) DO UPDATE SET challenge = ?2, created_at = ?3",
            [public_address, challenge, &now],
        )?;

        Ok(())
    }

    pub fn get_challenge(&self, public_address: &str) -> SqliteResult<Option<String>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT challenge FROM auth_challenges WHERE public_address = ?1",
        )?;

        let challenge = stmt
            .query_row([public_address], |row| row.get(0))
            .ok();

        Ok(challenge)
    }

    pub fn validate_challenge(&self, public_address: &str, challenge: &str) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM auth_challenges WHERE public_address = ?1 AND challenge = ?2",
            [public_address, challenge],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    pub fn delete_challenge(&self, public_address: &str) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute(
            "DELETE FROM auth_challenges WHERE public_address = ?1",
            [public_address],
        )?;
        Ok(rows_affected > 0)
    }
}
