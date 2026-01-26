//! Auth session database operations

use chrono::{Duration, Utc};
use rusqlite::Result as SqliteResult;
use uuid::Uuid;

use crate::models::Session;
use super::super::Database;

impl Database {
    /// Create a new auth session for web login
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

    /// Validate a session token and extend its expiry if valid
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
                    created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    expires_at: chrono::DateTime::parse_from_rfc3339(&expires_at_str)
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

    /// Delete a session (logout)
    pub fn delete_session(&self, token: &str) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute("DELETE FROM auth_sessions WHERE token = ?1", [token])?;
        Ok(rows_affected > 0)
    }
}
