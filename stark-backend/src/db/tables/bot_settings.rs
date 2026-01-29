//! Bot settings database operations

use chrono::{DateTime, Utc};
use rusqlite::Result as SqliteResult;

use crate::models::BotSettings;
use super::super::Database;

impl Database {
    /// Get bot settings (there's only one row)
    pub fn get_bot_settings(&self) -> SqliteResult<BotSettings> {
        let conn = self.conn.lock().unwrap();

        let result = conn.query_row(
            "SELECT id, bot_name, bot_email, created_at, updated_at FROM bot_settings LIMIT 1",
            [],
            |row| {
                let created_at_str: String = row.get(3)?;
                let updated_at_str: String = row.get(4)?;

                Ok(BotSettings {
                    id: row.get(0)?,
                    bot_name: row.get(1)?,
                    bot_email: row.get(2)?,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            },
        );

        match result {
            Ok(settings) => Ok(settings),
            Err(_) => Ok(BotSettings::default()),
        }
    }

    /// Update bot settings
    pub fn update_bot_settings(
        &self,
        bot_name: Option<&str>,
        bot_email: Option<&str>,
    ) -> SqliteResult<BotSettings> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Check if settings exist
        let exists: bool = conn
            .query_row("SELECT COUNT(*) FROM bot_settings", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|c| c > 0)
            .unwrap_or(false);

        if exists {
            // Update existing
            if let Some(name) = bot_name {
                conn.execute(
                    "UPDATE bot_settings SET bot_name = ?1, updated_at = ?2",
                    [name, &now],
                )?;
            }
            if let Some(email) = bot_email {
                conn.execute(
                    "UPDATE bot_settings SET bot_email = ?1, updated_at = ?2",
                    [email, &now],
                )?;
            }
        } else {
            // Insert new
            let name = bot_name.unwrap_or("StarkBot");
            let email = bot_email.unwrap_or("starkbot@users.noreply.github.com");
            conn.execute(
                "INSERT INTO bot_settings (bot_name, bot_email, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                [name, email, &now, &now],
            )?;
        }

        drop(conn);
        self.get_bot_settings()
    }
}
