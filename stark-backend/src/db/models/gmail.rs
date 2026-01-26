//! Gmail configuration database operations

use chrono::{DateTime, Utc};
use rusqlite::Result as SqliteResult;

use crate::integrations::gmail::GmailConfig;
use super::super::Database;

impl Database {
    /// Get Gmail configuration (only one config supported currently)
    pub fn get_gmail_config(&self) -> SqliteResult<Option<GmailConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, email, access_token, refresh_token, token_expires_at,
                    watch_labels, project_id, topic_name, watch_expires_at, history_id,
                    enabled, response_channel_id, auto_reply, created_at, updated_at
             FROM gmail_configs LIMIT 1"
        )?;

        let config = stmt
            .query_row([], |row| self.map_gmail_config_row(row))
            .ok();

        Ok(config)
    }

    /// Get Gmail configuration by email
    pub fn get_gmail_config_by_email(&self, email: &str) -> SqliteResult<Option<GmailConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, email, access_token, refresh_token, token_expires_at,
                    watch_labels, project_id, topic_name, watch_expires_at, history_id,
                    enabled, response_channel_id, auto_reply, created_at, updated_at
             FROM gmail_configs WHERE email = ?1"
        )?;

        let config = stmt
            .query_row([email], |row| self.map_gmail_config_row(row))
            .ok();

        Ok(config)
    }

    /// Create Gmail configuration
    pub fn create_gmail_config(
        &self,
        email: &str,
        access_token: &str,
        refresh_token: &str,
        project_id: &str,
        topic_name: &str,
        watch_labels: &str,
        response_channel_id: Option<i64>,
        auto_reply: bool,
    ) -> SqliteResult<GmailConfig> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO gmail_configs (email, access_token, refresh_token, project_id, topic_name,
                                        watch_labels, response_channel_id, auto_reply, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?9)",
            rusqlite::params![
                email, access_token, refresh_token, project_id, topic_name,
                watch_labels, response_channel_id, auto_reply as i32, &now
            ],
        )?;

        drop(conn);
        self.get_gmail_config().map(|opt| opt.unwrap())
    }

    /// Update Gmail configuration
    pub fn update_gmail_config(
        &self,
        watch_labels: Option<&str>,
        response_channel_id: Option<i64>,
        auto_reply: Option<bool>,
        enabled: Option<bool>,
    ) -> SqliteResult<GmailConfig> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Build dynamic update
        let sql = format!(
            "UPDATE gmail_configs SET updated_at = ?1{}{}{}{}",
            watch_labels.map(|_| ", watch_labels = ?2").unwrap_or(""),
            response_channel_id.map(|_| ", response_channel_id = ?3").unwrap_or(""),
            auto_reply.map(|_| ", auto_reply = ?4").unwrap_or(""),
            enabled.map(|_| ", enabled = ?5").unwrap_or(""),
        );

        conn.execute(
            &sql,
            rusqlite::params![
                &now,
                watch_labels.unwrap_or(""),
                response_channel_id.unwrap_or(0),
                auto_reply.unwrap_or(false) as i32,
                enabled.unwrap_or(true) as i32,
            ],
        )?;

        drop(conn);
        self.get_gmail_config().map(|opt| opt.unwrap())
    }

    /// Update Gmail watch info
    pub fn update_gmail_watch(
        &self,
        id: i64,
        watch_expires_at: Option<DateTime<Utc>>,
        history_id: Option<&str>,
    ) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let expires_str = watch_expires_at.map(|dt| dt.to_rfc3339());

        conn.execute(
            "UPDATE gmail_configs SET watch_expires_at = ?1, history_id = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![expires_str, history_id, &now, id],
        )?;

        Ok(())
    }

    /// Update Gmail history ID
    pub fn update_gmail_history_id(&self, id: i64, history_id: &str) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "UPDATE gmail_configs SET history_id = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![history_id, &now, id],
        )?;

        Ok(())
    }

    /// Update Gmail tokens
    pub fn update_gmail_tokens(
        &self,
        id: i64,
        access_token: &str,
        token_expires_at: Option<DateTime<Utc>>,
    ) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let expires_str = token_expires_at.map(|dt| dt.to_rfc3339());

        conn.execute(
            "UPDATE gmail_configs SET access_token = ?1, token_expires_at = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![access_token, expires_str, &now, id],
        )?;

        Ok(())
    }

    /// Delete Gmail configuration
    pub fn delete_gmail_config(&self) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let deleted = conn.execute("DELETE FROM gmail_configs", [])?;
        Ok(deleted > 0)
    }

    fn map_gmail_config_row(&self, row: &rusqlite::Row) -> rusqlite::Result<GmailConfig> {
        let token_expires_str: Option<String> = row.get(4)?;
        let watch_expires_str: Option<String> = row.get(8)?;
        let created_at_str: String = row.get(13)?;
        let updated_at_str: String = row.get(14)?;

        Ok(GmailConfig {
            id: row.get(0)?,
            email: row.get(1)?,
            access_token: row.get(2)?,
            refresh_token: row.get(3)?,
            token_expires_at: token_expires_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))
            }),
            watch_labels: row.get(5)?,
            project_id: row.get(6)?,
            topic_name: row.get(7)?,
            watch_expires_at: watch_expires_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))
            }),
            history_id: row.get(9)?,
            enabled: row.get::<_, i32>(10)? != 0,
            response_channel_id: row.get(11)?,
            auto_reply: row.get::<_, i32>(12)? != 0,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                .unwrap()
                .with_timezone(&Utc),
        })
    }
}
