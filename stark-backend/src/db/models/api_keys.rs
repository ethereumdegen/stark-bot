//! API key database operations

use chrono::{DateTime, Utc};
use rusqlite::Result as SqliteResult;

use crate::models::ApiKey;
use super::super::Database;

impl Database {
    /// Get an API key by service name
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

    /// List all API keys
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

    /// Insert or update an API key
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

    /// Delete an API key by service name
    pub fn delete_api_key(&self, service_name: &str) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute(
            "DELETE FROM external_api_keys WHERE service_name = ?1",
            [service_name],
        )?;
        Ok(rows_affected > 0)
    }
}
