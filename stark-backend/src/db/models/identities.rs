//! Identity link database operations

use chrono::{DateTime, Utc};
use rusqlite::Result as SqliteResult;
use uuid::Uuid;

use crate::models::IdentityLink;
use super::super::Database;

impl Database {
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

    /// List all identity links (unique identities)
    pub fn list_identities(&self) -> SqliteResult<Vec<IdentityLink>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, identity_id, channel_type, platform_user_id, platform_user_name, is_verified, verified_at, created_at, updated_at
             FROM identity_links ORDER BY updated_at DESC LIMIT 100",
        )?;

        let links = stmt
            .query_map([], |row| Self::row_to_identity_link(row))?
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
}
