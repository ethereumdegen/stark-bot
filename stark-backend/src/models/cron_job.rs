use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Schedule type for cron jobs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleType {
    /// One-shot at a specific time
    At,
    /// Fixed interval in milliseconds
    Every,
    /// Standard 5-field cron expression
    Cron,
}

impl ScheduleType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScheduleType::At => "at",
            ScheduleType::Every => "every",
            ScheduleType::Cron => "cron",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "at" => Some(ScheduleType::At),
            "every" => Some(ScheduleType::Every),
            "cron" => Some(ScheduleType::Cron),
            _ => None,
        }
    }
}

/// Session mode for cron job execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionMode {
    /// Run in the main session with context
    Main,
    /// Run in an isolated session
    Isolated,
}

impl SessionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionMode::Main => "main",
            SessionMode::Isolated => "isolated",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "main" => Some(SessionMode::Main),
            "isolated" => Some(SessionMode::Isolated),
            _ => None,
        }
    }
}

/// Status of a cron job
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Active,
    Paused,
    Completed,
    Failed,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Active => "active",
            JobStatus::Paused => "paused",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "active" => Some(JobStatus::Active),
            "paused" => Some(JobStatus::Paused),
            "completed" => Some(JobStatus::Completed),
            "failed" => Some(JobStatus::Failed),
            _ => None,
        }
    }
}

/// A scheduled cron job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: i64,
    pub job_id: String,
    pub name: String,
    pub description: Option<String>,
    pub schedule_type: String,
    /// For "at": ISO 8601 timestamp, "every": milliseconds, "cron": cron expression
    pub schedule_value: String,
    /// IANA timezone for cron expressions
    pub timezone: Option<String>,
    pub session_mode: String,
    /// The message/prompt to execute
    pub message: Option<String>,
    /// System event for main session mode
    pub system_event: Option<String>,
    /// Channel to deliver results to
    pub channel_id: Option<i64>,
    /// Specific recipient (e.g., user ID, phone number)
    pub deliver_to: Option<String>,
    /// Whether to auto-deliver results
    pub deliver: bool,
    /// Model override (e.g., "claude-opus-4")
    pub model_override: Option<String>,
    /// Thinking level override
    pub thinking_level: Option<String>,
    /// Timeout in seconds
    pub timeout_seconds: Option<i32>,
    /// Delete after successful run (for one-shot jobs)
    pub delete_after_run: bool,
    pub status: String,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub run_count: i32,
    pub error_count: i32,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to create a new cron job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCronJobRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub schedule_type: String,
    pub schedule_value: String,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default = "default_session_mode")]
    pub session_mode: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub system_event: Option<String>,
    #[serde(default)]
    pub channel_id: Option<i64>,
    #[serde(default)]
    pub deliver_to: Option<String>,
    #[serde(default)]
    pub deliver: bool,
    #[serde(default)]
    pub model_override: Option<String>,
    #[serde(default)]
    pub thinking_level: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<i32>,
    #[serde(default)]
    pub delete_after_run: bool,
}

fn default_session_mode() -> String {
    "isolated".to_string()
}

/// Request to update a cron job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCronJobRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub schedule_type: Option<String>,
    #[serde(default)]
    pub schedule_value: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub session_mode: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub system_event: Option<String>,
    #[serde(default)]
    pub channel_id: Option<i64>,
    #[serde(default)]
    pub deliver_to: Option<String>,
    #[serde(default)]
    pub deliver: Option<bool>,
    #[serde(default)]
    pub model_override: Option<String>,
    #[serde(default)]
    pub thinking_level: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<i32>,
    #[serde(default)]
    pub delete_after_run: Option<bool>,
    #[serde(default)]
    pub status: Option<String>,
}

/// Response for cron job operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobResponse {
    pub success: bool,
    pub job: Option<CronJob>,
    pub jobs: Option<Vec<CronJob>>,
    pub error: Option<String>,
}

/// A record of a cron job execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobRun {
    pub id: i64,
    pub job_id: i64,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub success: bool,
    pub result: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<i64>,
}

/// Heartbeat configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    pub id: i64,
    pub channel_id: Option<i64>,
    /// Interval in minutes (default: 30)
    pub interval_minutes: i32,
    /// Target session: "last" or specific session key
    pub target: String,
    /// Active hours start (HH:MM format)
    pub active_hours_start: Option<String>,
    /// Active hours end (HH:MM format)
    pub active_hours_end: Option<String>,
    /// Days of week (comma-separated: mon,tue,wed,thu,fri,sat,sun)
    pub active_days: Option<String>,
    pub enabled: bool,
    pub last_beat_at: Option<String>,
    pub next_beat_at: Option<String>,
    /// Current position in impulse map (for meandering)
    pub current_impulse_node_id: Option<i64>,
    /// Last heartbeat session ID (for context continuity)
    pub last_session_id: Option<i64>,
    /// Whether to run impulse evolver after each heartbeat
    pub impulse_evolver: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to update heartbeat configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateHeartbeatConfigRequest {
    #[serde(default)]
    pub interval_minutes: Option<i32>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub active_hours_start: Option<String>,
    #[serde(default)]
    pub active_hours_end: Option<String>,
    #[serde(default)]
    pub active_days: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub impulse_evolver: Option<bool>,
}

/// Response for heartbeat config operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfigResponse {
    pub success: bool,
    pub config: Option<HeartbeatConfig>,
    pub error: Option<String>,
}

impl CronJob {
    /// Calculate the next run time based on schedule
    pub fn calculate_next_run(&self) -> Option<DateTime<Utc>> {
        let now = Utc::now();

        match ScheduleType::from_str(&self.schedule_type)? {
            ScheduleType::At => {
                // One-shot: parse the ISO 8601 timestamp
                DateTime::parse_from_rfc3339(&self.schedule_value)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
                    .filter(|dt| *dt > now)
            }
            ScheduleType::Every => {
                // Interval: add milliseconds to last run or now
                let interval_ms: i64 = self.schedule_value.parse().ok()?;
                let base = self
                    .last_run_at
                    .as_ref()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or(now);
                Some(base + chrono::Duration::milliseconds(interval_ms))
            }
            ScheduleType::Cron => {
                // Parse cron expression and find next occurrence
                use cron::Schedule;
                use std::str::FromStr;

                let schedule = Schedule::from_str(&self.schedule_value).ok()?;
                schedule.upcoming(Utc).next()
            }
        }
    }

    /// Check if the job is due to run
    pub fn is_due(&self) -> bool {
        if self.status != JobStatus::Active.as_str() {
            return false;
        }

        if let Some(next_run) = self.next_run_at.as_ref() {
            if let Ok(next) = DateTime::parse_from_rfc3339(next_run) {
                return Utc::now() >= next.with_timezone(&Utc);
            }
        }

        false
    }
}
