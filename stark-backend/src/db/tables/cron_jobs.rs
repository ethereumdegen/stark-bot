//! Cron job and job run database operations

use chrono::Utc;
use rusqlite::{Connection, Result as SqliteResult};
use uuid::Uuid;

use crate::models::{CronJob, CronJobRun};
use super::super::Database;

impl Database {
    /// Create a new cron job
    pub fn create_cron_job(
        &self,
        name: &str,
        description: Option<&str>,
        schedule_type: &str,
        schedule_value: &str,
        timezone: Option<&str>,
        session_mode: &str,
        message: Option<&str>,
        system_event: Option<&str>,
        channel_id: Option<i64>,
        deliver_to: Option<&str>,
        deliver: bool,
        model_override: Option<&str>,
        thinking_level: Option<&str>,
        timeout_seconds: Option<i32>,
        delete_after_run: bool,
    ) -> SqliteResult<CronJob> {
        let conn = self.conn.lock().unwrap();
        let job_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO cron_jobs (
                job_id, name, description, schedule_type, schedule_value, timezone,
                session_mode, message, system_event, channel_id, deliver_to, deliver,
                model_override, thinking_level, timeout_seconds, delete_after_run,
                status, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'active', ?17, ?17)",
            rusqlite::params![
                job_id, name, description, schedule_type, schedule_value, timezone,
                session_mode, message, system_event, channel_id, deliver_to, deliver as i32,
                model_override, thinking_level, timeout_seconds, delete_after_run as i32,
                now
            ],
        )?;

        let id = conn.last_insert_rowid();
        self.get_cron_job_by_id_internal(&conn, id)
    }

    fn get_cron_job_by_id_internal(&self, conn: &Connection, id: i64) -> SqliteResult<CronJob> {
        conn.query_row(
            "SELECT id, job_id, name, description, schedule_type, schedule_value, timezone,
                    session_mode, message, system_event, channel_id, deliver_to, deliver,
                    model_override, thinking_level, timeout_seconds, delete_after_run,
                    status, last_run_at, next_run_at, run_count, error_count, last_error,
                    created_at, updated_at
             FROM cron_jobs WHERE id = ?1",
            [id],
            |row| self.map_cron_job_row(row),
        )
    }

    fn map_cron_job_row(&self, row: &rusqlite::Row) -> SqliteResult<CronJob> {
        Ok(CronJob {
            id: row.get(0)?,
            job_id: row.get(1)?,
            name: row.get(2)?,
            description: row.get(3)?,
            schedule_type: row.get(4)?,
            schedule_value: row.get(5)?,
            timezone: row.get(6)?,
            session_mode: row.get(7)?,
            message: row.get(8)?,
            system_event: row.get(9)?,
            channel_id: row.get(10)?,
            deliver_to: row.get(11)?,
            deliver: row.get::<_, i32>(12)? != 0,
            model_override: row.get(13)?,
            thinking_level: row.get(14)?,
            timeout_seconds: row.get(15)?,
            delete_after_run: row.get::<_, i32>(16)? != 0,
            status: row.get(17)?,
            last_run_at: row.get(18)?,
            next_run_at: row.get(19)?,
            run_count: row.get(20)?,
            error_count: row.get(21)?,
            last_error: row.get(22)?,
            created_at: row.get(23)?,
            updated_at: row.get(24)?,
        })
    }

    /// Get a cron job by ID
    pub fn get_cron_job(&self, id: i64) -> SqliteResult<Option<CronJob>> {
        let conn = self.conn.lock().unwrap();
        match self.get_cron_job_by_id_internal(&conn, id) {
            Ok(job) => Ok(Some(job)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get a cron job by job_id (UUID string)
    pub fn get_cron_job_by_job_id(&self, job_id: &str) -> SqliteResult<Option<CronJob>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT id, job_id, name, description, schedule_type, schedule_value, timezone,
                    session_mode, message, system_event, channel_id, deliver_to, deliver,
                    model_override, thinking_level, timeout_seconds, delete_after_run,
                    status, last_run_at, next_run_at, run_count, error_count, last_error,
                    created_at, updated_at
             FROM cron_jobs WHERE job_id = ?1",
            [job_id],
            |row| self.map_cron_job_row(row),
        ) {
            Ok(job) => Ok(Some(job)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// List all cron jobs
    pub fn list_cron_jobs(&self) -> SqliteResult<Vec<CronJob>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, job_id, name, description, schedule_type, schedule_value, timezone,
                    session_mode, message, system_event, channel_id, deliver_to, deliver,
                    model_override, thinking_level, timeout_seconds, delete_after_run,
                    status, last_run_at, next_run_at, run_count, error_count, last_error,
                    created_at, updated_at
             FROM cron_jobs ORDER BY created_at DESC"
        )?;

        let jobs: Vec<CronJob> = stmt
            .query_map([], |row| self.map_cron_job_row(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(jobs)
    }

    /// List active cron jobs that are due to run
    pub fn list_due_cron_jobs(&self) -> SqliteResult<Vec<CronJob>> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let mut stmt = conn.prepare(
            "SELECT id, job_id, name, description, schedule_type, schedule_value, timezone,
                    session_mode, message, system_event, channel_id, deliver_to, deliver,
                    model_override, thinking_level, timeout_seconds, delete_after_run,
                    status, last_run_at, next_run_at, run_count, error_count, last_error,
                    created_at, updated_at
             FROM cron_jobs
             WHERE status = 'active' AND (next_run_at IS NULL OR next_run_at <= ?1)
             ORDER BY next_run_at ASC"
        )?;

        let jobs: Vec<CronJob> = stmt
            .query_map([&now], |row| self.map_cron_job_row(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(jobs)
    }

    /// Update a cron job
    #[allow(clippy::too_many_arguments)]
    pub fn update_cron_job(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        schedule_type: Option<&str>,
        schedule_value: Option<&str>,
        timezone: Option<&str>,
        session_mode: Option<&str>,
        message: Option<&str>,
        system_event: Option<&str>,
        channel_id: Option<i64>,
        deliver_to: Option<&str>,
        deliver: Option<bool>,
        model_override: Option<&str>,
        thinking_level: Option<&str>,
        timeout_seconds: Option<i32>,
        delete_after_run: Option<bool>,
        status: Option<&str>,
    ) -> SqliteResult<CronJob> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Build dynamic update query
        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut param_index = 2;

        if name.is_some() { updates.push(format!("name = ?{}", param_index)); param_index += 1; }
        if description.is_some() { updates.push(format!("description = ?{}", param_index)); param_index += 1; }
        if schedule_type.is_some() { updates.push(format!("schedule_type = ?{}", param_index)); param_index += 1; }
        if schedule_value.is_some() { updates.push(format!("schedule_value = ?{}", param_index)); param_index += 1; }
        if timezone.is_some() { updates.push(format!("timezone = ?{}", param_index)); param_index += 1; }
        if session_mode.is_some() { updates.push(format!("session_mode = ?{}", param_index)); param_index += 1; }
        if message.is_some() { updates.push(format!("message = ?{}", param_index)); param_index += 1; }
        if system_event.is_some() { updates.push(format!("system_event = ?{}", param_index)); param_index += 1; }
        if channel_id.is_some() { updates.push(format!("channel_id = ?{}", param_index)); param_index += 1; }
        if deliver_to.is_some() { updates.push(format!("deliver_to = ?{}", param_index)); param_index += 1; }
        if deliver.is_some() { updates.push(format!("deliver = ?{}", param_index)); param_index += 1; }
        if model_override.is_some() { updates.push(format!("model_override = ?{}", param_index)); param_index += 1; }
        if thinking_level.is_some() { updates.push(format!("thinking_level = ?{}", param_index)); param_index += 1; }
        if timeout_seconds.is_some() { updates.push(format!("timeout_seconds = ?{}", param_index)); param_index += 1; }
        if delete_after_run.is_some() { updates.push(format!("delete_after_run = ?{}", param_index)); param_index += 1; }
        if status.is_some() { updates.push(format!("status = ?{}", param_index)); param_index += 1; }

        let query = format!(
            "UPDATE cron_jobs SET {} WHERE id = ?{}",
            updates.join(", "),
            param_index
        );

        // Build params dynamically
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];
        if let Some(v) = name { params.push(Box::new(v.to_string())); }
        if let Some(v) = description { params.push(Box::new(v.to_string())); }
        if let Some(v) = schedule_type { params.push(Box::new(v.to_string())); }
        if let Some(v) = schedule_value { params.push(Box::new(v.to_string())); }
        if let Some(v) = timezone { params.push(Box::new(v.to_string())); }
        if let Some(v) = session_mode { params.push(Box::new(v.to_string())); }
        if let Some(v) = message { params.push(Box::new(v.to_string())); }
        if let Some(v) = system_event { params.push(Box::new(v.to_string())); }
        if let Some(v) = channel_id { params.push(Box::new(v)); }
        if let Some(v) = deliver_to { params.push(Box::new(v.to_string())); }
        if let Some(v) = deliver { params.push(Box::new(v as i32)); }
        if let Some(v) = model_override { params.push(Box::new(v.to_string())); }
        if let Some(v) = thinking_level { params.push(Box::new(v.to_string())); }
        if let Some(v) = timeout_seconds { params.push(Box::new(v)); }
        if let Some(v) = delete_after_run { params.push(Box::new(v as i32)); }
        if let Some(v) = status { params.push(Box::new(v.to_string())); }
        params.push(Box::new(id));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        conn.execute(&query, params_refs.as_slice())?;

        self.get_cron_job_by_id_internal(&conn, id)
    }

    /// Update cron job run status
    pub fn update_cron_job_run_status(
        &self,
        id: i64,
        last_run_at: &str,
        next_run_at: Option<&str>,
        success: bool,
        error: Option<&str>,
    ) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        if success {
            conn.execute(
                "UPDATE cron_jobs SET
                    last_run_at = ?1, next_run_at = ?2, run_count = run_count + 1,
                    last_error = NULL, updated_at = ?3
                 WHERE id = ?4",
                rusqlite::params![last_run_at, next_run_at, now, id],
            )?;
        } else {
            conn.execute(
                "UPDATE cron_jobs SET
                    last_run_at = ?1, next_run_at = ?2, error_count = error_count + 1,
                    last_error = ?3, updated_at = ?4
                 WHERE id = ?5",
                rusqlite::params![last_run_at, next_run_at, error, now, id],
            )?;
        }

        Ok(())
    }

    /// Delete a cron job
    pub fn delete_cron_job(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute("DELETE FROM cron_jobs WHERE id = ?1", [id])?;
        Ok(rows_affected > 0)
    }

    /// Log a cron job run
    pub fn log_cron_job_run(
        &self,
        job_id: i64,
        started_at: &str,
        completed_at: Option<&str>,
        success: bool,
        result: Option<&str>,
        error: Option<&str>,
        duration_ms: Option<i64>,
    ) -> SqliteResult<CronJobRun> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT INTO cron_job_runs (job_id, started_at, completed_at, success, result, error, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![job_id, started_at, completed_at, success as i32, result, error, duration_ms],
        )?;

        let id = conn.last_insert_rowid();

        Ok(CronJobRun {
            id,
            job_id,
            started_at: started_at.to_string(),
            completed_at: completed_at.map(|s| s.to_string()),
            success,
            result: result.map(|s| s.to_string()),
            error: error.map(|s| s.to_string()),
            duration_ms,
        })
    }

    /// Get recent runs for a cron job
    pub fn get_cron_job_runs(&self, job_id: i64, limit: i32) -> SqliteResult<Vec<CronJobRun>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, job_id, started_at, completed_at, success, result, error, duration_ms
             FROM cron_job_runs WHERE job_id = ?1 ORDER BY started_at DESC LIMIT ?2"
        )?;

        let runs: Vec<CronJobRun> = stmt
            .query_map([job_id, limit as i64], |row| {
                Ok(CronJobRun {
                    id: row.get(0)?,
                    job_id: row.get(1)?,
                    started_at: row.get(2)?,
                    completed_at: row.get(3)?,
                    success: row.get::<_, i32>(4)? != 0,
                    result: row.get(5)?,
                    error: row.get(6)?,
                    duration_ms: row.get(7)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(runs)
    }
}
