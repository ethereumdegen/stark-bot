use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::types::NormalizedMessage;
use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{CronJob, HeartbeatConfig, ScheduleType};
use crate::wallet;
use chrono::{DateTime, Duration, Local, NaiveTime, Utc, Weekday, Datelike, Timelike};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::time::{interval, timeout, Duration as TokioDuration};

/// Dedicated channel ID for heartbeat concurrency guard
/// Used to prevent overlapping heartbeat cycles via ExecutionTracker
pub const HEARTBEAT_CHANNEL_ID: i64 = -999;

/// Timeout for per-agent heartbeat sessions
const AGENT_HEARTBEAT_TIMEOUT_SECS: u64 = 120;

/// Scheduler configuration
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Enable cron job processing
    pub cron_enabled: bool,
    /// Poll interval in seconds for checking due jobs
    pub poll_interval_secs: u64,
    /// Maximum concurrent job executions
    pub max_concurrent_jobs: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        SchedulerConfig {
            cron_enabled: true,
            poll_interval_secs: 10,    // Check every 10 seconds (saves ~90% scheduler CPU)
            max_concurrent_jobs: 5,
        }
    }
}

/// Default timeout for cron job execution (10 minutes)
const DEFAULT_CRON_JOB_TIMEOUT_SECS: u64 = 10 * 60;

/// Exponential backoff delays (in seconds) indexed by consecutive error count.
/// After the last entry the delay stays constant.
const ERROR_BACKOFF_SECS: &[u64] = &[
    30,       // 1st error  →  30s
    60,       // 2nd error  →   1 min
    5 * 60,   // 3rd error  →   5 min
    15 * 60,  // 4th error  →  15 min
    60 * 60,  // 5th+ error →  60 min
];

fn error_backoff_secs(error_count: i32) -> u64 {
    let idx = (error_count.max(1) - 1) as usize;
    ERROR_BACKOFF_SECS[idx.min(ERROR_BACKOFF_SECS.len() - 1)]
}

/// The scheduler service that runs cron jobs and heartbeats
pub struct Scheduler {
    db: Arc<Database>,
    dispatcher: Arc<MessageDispatcher>,
    broadcaster: Arc<EventBroadcaster>,
    execution_tracker: Arc<crate::execution::ExecutionTracker>,
    config: SchedulerConfig,
    /// Wallet provider for x402 payments in scheduled tasks (heartbeats, cron jobs)
    wallet_provider: Option<Arc<dyn wallet::WalletProvider>>,
    skill_registry: Option<Arc<crate::skills::SkillRegistry>>,
}

impl Scheduler {
    pub fn new(
        db: Arc<Database>,
        dispatcher: Arc<MessageDispatcher>,
        broadcaster: Arc<EventBroadcaster>,
        execution_tracker: Arc<crate::execution::ExecutionTracker>,
        config: SchedulerConfig,
        wallet_provider: Option<Arc<dyn wallet::WalletProvider>>,
        skill_registry: Option<Arc<crate::skills::SkillRegistry>>,
    ) -> Self {
        Scheduler {
            db,
            dispatcher,
            broadcaster,
            execution_tracker,
            config,
            wallet_provider,
            skill_registry,
        }
    }

    /// Compatibility method - db_url is no longer needed with connection pool
    #[deprecated(note = "Use new() instead - db_url is no longer needed with r2d2 connection pool")]
    pub fn new_with_db_url(
        db: Arc<Database>,
        dispatcher: Arc<MessageDispatcher>,
        broadcaster: Arc<EventBroadcaster>,
        execution_tracker: Arc<crate::execution::ExecutionTracker>,
        config: SchedulerConfig,
        _db_url: String,
    ) -> Self {
        Self::new(db, dispatcher, broadcaster, execution_tracker, config, None, None)
    }

    /// Start the scheduler background task
    pub async fn start(self: Arc<Self>, mut shutdown_rx: oneshot::Receiver<()>) {
        log::info!(
            "Scheduler started (cron: {}, heartbeat: always, poll: {}s)",
            self.config.cron_enabled,
            self.config.poll_interval_secs
        );

        let mut poll_interval = interval(TokioDuration::from_secs(self.config.poll_interval_secs));

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    log::info!("Scheduler received shutdown signal");
                    break;
                }
                _ = poll_interval.tick() => {
                    self.tick().await;
                }
            }
        }

        log::info!("Scheduler stopped");
    }

    /// Process one tick of the scheduler
    async fn tick(&self) {
        // Process cron jobs
        if self.config.cron_enabled {
            if let Err(e) = self.process_cron_jobs().await {
                log::error!("Error processing cron jobs: {}", e);
            }
        }

        // Process kanban auto-execute tasks
        if let Err(e) = self.process_kanban_tasks().await {
            log::error!("Error processing kanban tasks: {}", e);
        }

        // Process heartbeats (always enabled - individual configs control their own enabled state)
        if let Err(e) = self.process_heartbeats().await {
            log::error!("Error processing heartbeats: {}", e);
        }

        // Run periodic cleanup tasks once per hour (at minute 0, within first poll window)
        let now = Local::now();
        if now.minute() == 0 && now.second() < self.config.poll_interval_secs as u32 {
            self.run_periodic_cleanup();
        }
    }

    /// Run periodic cleanup tasks (called approximately once per hour)
    fn run_periodic_cleanup(&self) {
        // Cleanup old Twitter processed mentions (keep last 30 days)
        match self.db.cleanup_old_processed_mentions(30) {
            Ok(count) if count > 0 => {
                log::info!("Scheduler: Cleaned up {} old Twitter processed mentions", count);
            }
            Ok(_) => {} // Nothing to clean up
            Err(e) => {
                log::error!("Scheduler: Failed to cleanup Twitter mentions: {}", e);
            }
        }

        // Cleanup old safe mode channels (keep last 60 minutes - more aggressive than FIFO logic)
        match self.db.cleanup_old_safe_mode_channels(60) {
            Ok(count) if count > 0 => {
                log::info!("Scheduler: Cleaned up {} old safe mode channels", count);
            }
            Ok(_) => {} // Nothing to clean up
            Err(e) => {
                log::error!("Scheduler: Failed to cleanup safe mode channels: {}", e);
            }
        }

        // Cleanup old telemetry spans (keep last 30 days)
        let telemetry_store = crate::telemetry::TelemetryStore::new(self.db.clone());
        telemetry_store.prune();
    }

    /// Process due cron jobs
    async fn process_cron_jobs(&self) -> Result<(), String> {
        let due_jobs = self
            .db
            .list_due_cron_jobs()
            .map_err(|e| format!("Failed to list due jobs: {}", e))?;

        for job in due_jobs {
            let scheduler = Arc::clone(&Arc::new(self.clone_inner()));
            tokio::spawn(async move {
                if let Err(e) = scheduler.execute_cron_job(&job).await {
                    log::error!("Cron job '{}' failed: {}", job.name, e);
                }
            });
        }

        Ok(())
    }

    /// Process kanban tasks that are in "ready" status (auto-execute)
    async fn process_kanban_tasks(&self) -> Result<(), String> {
        // Check if auto-execute is enabled in bot settings
        let settings = self.db.get_bot_settings()
            .map_err(|e| format!("Failed to get bot settings: {}", e))?;
        if !settings.kanban_auto_execute {
            return Ok(());
        }

        // Pick tasks one at a time in a loop (pick_next_kanban_task atomically moves to in_progress)
        loop {
            let task = self.db.pick_next_kanban_task()
                .map_err(|e| format!("Failed to pick kanban task: {}", e))?;

            let task = match task {
                Some(t) => t,
                None => break, // No more ready tasks
            };

            log::info!("Auto-executing kanban task #{}: {}", task.id, task.title);

            // Broadcast that the task was picked up
            self.broadcaster.broadcast(GatewayEvent::new(
                "kanban_item_updated",
                serde_json::json!({ "item": &task }),
            ));

            // Spawn execution in background
            let scheduler = self.clone_inner();
            let task_id = task.id;
            let task_title = task.title.clone();
            tokio::spawn(async move {
                if let Err(e) = scheduler.execute_kanban_task(&task).await {
                    log::error!("Kanban task #{} '{}' failed: {}", task_id, task_title, e);
                }
            });
        }

        Ok(())
    }

    /// Execute a single kanban task by dispatching it as a message
    async fn execute_kanban_task(&self, task: &crate::db::tables::kanban::KanbanItem) -> Result<(), String> {
        let started_at = Utc::now();

        // Build the message text from task title + description
        let message_text = if task.description.is_empty() {
            format!("[Kanban Task] {}", task.title)
        } else {
            format!("[Kanban Task] {}\n\n{}", task.title, task.description)
        };

        // Use a unique negative channel ID for kanban tasks to avoid collision
        let kanban_channel_id = -(task.id.abs() % 1_000_000 + 500_000);

        let normalized = NormalizedMessage {
            channel_id: kanban_channel_id,
            channel_type: "kanban".to_string(),
            chat_id: format!("kanban:task-{}", task.id),
            chat_name: None,
            user_id: "system".to_string(),
            user_name: "Kanban".to_string(),
            text: message_text,
            message_id: Some(format!("kanban-{}-{}", task.id, started_at.timestamp())),
            session_mode: Some("isolated".to_string()),
            selected_network: None,
            force_safe_mode: false,
        };

        // Execute with 10-minute timeout (same as cron default)
        let dispatch_result = timeout(
            TokioDuration::from_secs(DEFAULT_CRON_JOB_TIMEOUT_SECS),
            self.dispatcher.dispatch_safe(normalized),
        ).await;

        let (success, response, error_msg) = match dispatch_result {
            Ok(result) => {
                let ok = result.error.is_none();
                (ok, result.response, result.error)
            }
            Err(_) => {
                let err_msg = format!("Kanban task timed out after {}s", DEFAULT_CRON_JOB_TIMEOUT_SECS);
                log::warn!("Kanban task #{} timed out", task.id);
                (false, String::new(), Some(err_msg))
            }
        };

        // Look up the session that was created during dispatch
        let session_key = format!("kanban:{}:{}", kanban_channel_id, format!("kanban:task-{}", task.id));
        let session_id = self.db.get_chat_session_by_key(&session_key)
            .ok()
            .flatten()
            .map(|s| s.id);

        // Update the kanban item based on result
        if success {
            // Mark as complete with result and session_id
            let update = crate::db::tables::kanban::UpdateKanbanItemRequest {
                status: Some("complete".to_string()),
                result: Some(if response.len() > 2000 {
                    format!("{}...", &response[..2000])
                } else {
                    response.clone()
                }),
                session_id,
                ..Default::default()
            };
            let _ = self.db.update_kanban_item(task.id, &update);
            log::info!("Kanban task #{} completed successfully", task.id);
        } else {
            // Revert to ready so it can be retried, store error in result
            let update = crate::db::tables::kanban::UpdateKanbanItemRequest {
                status: Some("ready".to_string()),
                result: Some(format!("Error: {}", error_msg.as_deref().unwrap_or("unknown"))),
                ..Default::default()
            };
            let _ = self.db.update_kanban_item(task.id, &update);
            log::warn!("Kanban task #{} failed, reverted to ready: {:?}", task.id, error_msg);
        }

        // Broadcast update for UI refresh
        if let Ok(Some(updated_item)) = self.db.get_kanban_item(task.id) {
            self.broadcaster.broadcast(GatewayEvent::new(
                "kanban_item_updated",
                serde_json::json!({ "item": &updated_item }),
            ));
        }

        Ok(())
    }

    fn clone_inner(&self) -> Scheduler {
        Scheduler {
            db: Arc::clone(&self.db),
            dispatcher: Arc::clone(&self.dispatcher),
            broadcaster: Arc::clone(&self.broadcaster),
            execution_tracker: Arc::clone(&self.execution_tracker),
            config: self.config.clone(),
            wallet_provider: self.wallet_provider.clone(),
            skill_registry: self.skill_registry.clone(),
        }
    }

    /// Execute a single cron job
    async fn execute_cron_job(&self, job: &CronJob) -> Result<(), String> {
        let started_at = Utc::now();
        let started_at_str = started_at.to_rfc3339();

        log::info!("Executing cron job '{}' ({})", job.name, job.job_id);

        // IMPORTANT: Calculate and set next_run_at BEFORE execution to prevent race conditions
        // where the same job could be picked up twice if execution takes longer than poll interval
        let next_run = self.calculate_next_run(job);
        let next_run_str = next_run.map(|dt| dt.to_rfc3339());
        if let Err(e) = self.db.mark_cron_job_started(job.id, next_run_str.as_deref()) {
            log::error!("Failed to mark cron job as started: {}", e);
            // Continue anyway - the job should still run
        }

        // Broadcast job start event
        self.broadcaster.broadcast(GatewayEvent::custom(
            "cron_job_started",
            serde_json::json!({
                "job_id": job.job_id,
                "name": job.name,
            }),
        ));

        // Track if this is main mode for later stop event
        let is_main_mode = job.session_mode == "main";

        // Build the message to dispatch
        let message_text = job
            .message
            .clone()
            .or_else(|| job.system_event.clone())
            .unwrap_or_else(|| format!("[Cron: {}]", job.name));

        // Determine channel ID based on session_mode
        // - "main" mode: use channel 0 (web channel) to share session with web UI
        // - "isolated" mode (default): use unique negative channel ID to avoid collision
        let cron_channel_id = if is_main_mode {
            // Main mode intentionally uses web channel (0) for shared session
            job.channel_id.unwrap_or(0)
        } else {
            // Isolated mode: use explicit channel_id if set, otherwise generate unique negative ID
            job.channel_id.unwrap_or_else(|| {
                // Generate unique negative channel ID based on job_id hash
                // This avoids collision with real channel IDs (positive) and web channel (0)
                -(job.job_id.chars().fold(1i64, |acc, c| {
                    acc.wrapping_mul(31).wrapping_add(c as i64)
                }).abs() % 1_000_000 + 1) // +1 ensures we never get 0
            })
        };

        log::info!(
            "Cron job '{}' using channel_id {} (session_mode: {})",
            job.name,
            cron_channel_id,
            job.session_mode
        );

        // Broadcast cron execution started event for main mode (shows stop button in web UI)
        if is_main_mode && cron_channel_id == 0 {
            self.broadcaster.broadcast(GatewayEvent::cron_execution_started_on_channel(
                0,
                &job.job_id,
                &job.name,
                &job.session_mode,
            ));
        }

        // Create a normalized message for the dispatcher
        let normalized = NormalizedMessage {
            channel_id: cron_channel_id,
            channel_type: "cron".to_string(),
            chat_id: format!("cron:{}:{}", job.job_id, started_at.timestamp()),
            chat_name: None,
            user_id: "system".to_string(),
            user_name: format!("Cron: {}", job.name),
            text: message_text,
            message_id: Some(format!("cron-run-{}", started_at.timestamp())),
            session_mode: Some(job.session_mode.clone()),
            selected_network: None,
            force_safe_mode: false,
        };

        // Execute the job with timeout
        let timeout_secs = job.timeout_seconds
            .map(|s| s.max(10) as u64)
            .unwrap_or(DEFAULT_CRON_JOB_TIMEOUT_SECS);

        let dispatch_result = timeout(
            TokioDuration::from_secs(timeout_secs),
            self.dispatcher.dispatch_safe(normalized),
        ).await;

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds();

        // Handle timeout vs normal result
        let (success, response, error_msg) = match dispatch_result {
            Ok(result) => {
                let ok = result.error.is_none();
                (ok, result.response, result.error)
            }
            Err(_) => {
                let err_msg = format!(
                    "Job timed out after {}s", timeout_secs
                );
                log::warn!("Cron job '{}' timed out after {}s", job.name, timeout_secs);
                (false, String::new(), Some(err_msg))
            }
        };

        // Apply error backoff: on failure, push next_run_at further into the future
        // to prevent retry storms when a job keeps failing (e.g., API key expired, model down).
        // Backoff: 30s → 1min → 5min → 15min → 60min based on consecutive error count.
        let final_next_run_str = if !success {
            let new_error_count = job.error_count + 1;
            let backoff = error_backoff_secs(new_error_count);
            let backoff_time = completed_at + Duration::seconds(backoff as i64);

            // Use whichever is later: the normal next_run or the backoff time
            let final_next = match next_run {
                Some(normal_next) => {
                    if backoff_time > normal_next { Some(backoff_time) } else { Some(normal_next) }
                }
                None => Some(backoff_time), // one-shot jobs: still apply backoff
            };

            log::info!(
                "Cron job '{}' failed (error #{}) — applying {}s backoff, next run at {:?}",
                job.name, new_error_count, backoff, final_next
            );

            final_next.map(|dt| dt.to_rfc3339())
        } else {
            next_run_str.clone()
        };

        // Update job status with final result (including backoff-adjusted next_run_at)
        self.db
            .update_cron_job_run_status(
                job.id,
                &started_at_str,
                final_next_run_str.as_deref(),
                success,
                error_msg.as_deref(),
            )
            .map_err(|e| format!("Failed to update job status: {}", e))?;

        // Log the run
        let _ = self.db.log_cron_job_run(
            job.id,
            &started_at_str,
            Some(&completed_at.to_rfc3339()),
            success,
            Some(&response),
            error_msg.as_deref(),
            Some(duration_ms),
        );

        // Handle delete_after_run for one-shot jobs
        if success && job.delete_after_run {
            log::info!("Deleting one-shot cron job '{}' after successful run", job.name);
            let _ = self.db.delete_cron_job(job.id);
        }

        // Handle delivery if configured
        if job.deliver && job.channel_id.is_some() {
            self.deliver_result(job, &response).await?;
        }

        // Broadcast job completion event
        self.broadcaster.broadcast(GatewayEvent::custom(
            "cron_job_completed",
            serde_json::json!({
                "job_id": job.job_id,
                "name": job.name,
                "success": success,
                "duration_ms": duration_ms,
            }),
        ));

        // Broadcast cron execution stopped event for main mode (hides stop button in web UI)
        if is_main_mode && cron_channel_id == 0 {
            self.broadcaster.broadcast(GatewayEvent::cron_execution_stopped_on_channel(
                0,
                &job.job_id,
                if success { "completed" } else { "failed" },
            ));
        }

        log::info!(
            "Cron job '{}' completed in {}ms (success: {})",
            job.name,
            duration_ms,
            success
        );

        Ok(())
    }

    /// Calculate the next run time for a job
    fn calculate_next_run(&self, job: &CronJob) -> Option<DateTime<Utc>> {
        let now = Utc::now();

        match ScheduleType::from_str(&job.schedule_type)? {
            ScheduleType::At => {
                // One-shot jobs don't have a next run
                None
            }
            ScheduleType::Every => {
                let interval_ms: i64 = job.schedule_value.parse().ok()?;
                Some(now + Duration::milliseconds(interval_ms))
            }
            ScheduleType::Cron => {
                use cron::Schedule;
                use std::str::FromStr;

                let schedule = Schedule::from_str(&job.schedule_value).ok()?;
                schedule.upcoming(Utc).next()
            }
        }
    }

    /// Deliver job result to the configured channel
    async fn deliver_result(&self, job: &CronJob, response: &str) -> Result<(), String> {
        // For now, we just log that we would deliver
        // In a full implementation, this would send to the channel
        log::info!(
            "Would deliver cron job '{}' result to channel {} (to: {:?}): {}",
            job.name,
            job.channel_id.unwrap_or(0),
            job.deliver_to,
            if response.len() > 100 {
                format!("{}...", &response[..100])
            } else {
                response.to_string()
            }
        );

        // TODO: Implement actual channel delivery
        // This would involve looking up the channel and sending a message

        Ok(())
    }

    /// Process due heartbeats
    /// Note: Only processes the MOST RECENT heartbeat config (highest ID) to avoid duplicates
    /// IMPORTANT: Only ONE heartbeat can run at a time
    async fn process_heartbeats(&self) -> Result<(), String> {
        let due_configs = self
            .db
            .list_due_heartbeat_configs()
            .map_err(|e| format!("Failed to list due heartbeats: {}", e))?;

        // Only process the most recent heartbeat config (highest ID)
        // This prevents duplicate heartbeats if multiple configs exist
        if let Some(config) = due_configs.into_iter().max_by_key(|c| c.id) {
            // Check if within active hours
            if !self.is_within_active_hours(&config) {
                // Outside active hours - still update next_beat_at so frontend doesn't get stuck on "soon..."
                let next_beat = Utc::now() + Duration::minutes(config.interval_minutes as i64);
                let next_beat_str = next_beat.to_rfc3339();
                if let Err(e) = self.db.update_heartbeat_next_beat(config.id, &next_beat_str) {
                    log::error!("Failed to update heartbeat next_beat_at (outside active hours): {}", e);
                }
                log::debug!("[HEARTBEAT] Skipping - outside active hours, next check at {}", next_beat_str);
                return Ok(());
            }

            // Skip execution if a heartbeat is already running, but still update next_beat_at
            // so the frontend doesn't get stuck on "soon..." while waiting
            if self.execution_tracker.get_execution_id(HEARTBEAT_CHANNEL_ID).is_some() {
                // Update next_beat_at even when skipping so frontend shows correct countdown
                let next_beat = Utc::now() + Duration::minutes(config.interval_minutes as i64);
                let next_beat_str = next_beat.to_rfc3339();
                if let Err(e) = self.db.update_heartbeat_next_beat(config.id, &next_beat_str) {
                    log::error!("Failed to update heartbeat next_beat_at (already running): {}", e);
                }
                log::debug!("[HEARTBEAT] Skipping - heartbeat already running, next check at {}", next_beat_str);
                return Ok(());
            }

            let scheduler = self.clone_inner();
            tokio::spawn(async move {
                if let Err(e) = scheduler.execute_heartbeat(&config).await {
                    log::error!("Heartbeat failed: {}", e);
                }
            });
        }

        Ok(())
    }

    /// Check if current time is within active hours for a heartbeat
    fn is_within_active_hours(&self, config: &HeartbeatConfig) -> bool {
        let now = Local::now();

        // Check active days
        if let Some(ref days) = config.active_days {
            let today = now.weekday();
            let day_str = match today {
                Weekday::Mon => "mon",
                Weekday::Tue => "tue",
                Weekday::Wed => "wed",
                Weekday::Thu => "thu",
                Weekday::Fri => "fri",
                Weekday::Sat => "sat",
                Weekday::Sun => "sun",
            };

            if !days.to_lowercase().contains(day_str) {
                return false;
            }
        }

        // Check active hours
        if let (Some(start), Some(end)) = (&config.active_hours_start, &config.active_hours_end) {
            let current_time = now.time();

            let start_time = NaiveTime::parse_from_str(start, "%H:%M").unwrap_or(NaiveTime::from_hms_opt(0, 0, 0).expect("00:00:00 is valid"));
            let end_time = NaiveTime::parse_from_str(end, "%H:%M").unwrap_or(NaiveTime::from_hms_opt(23, 59, 59).expect("23:59:59 is valid"));

            // When start == end, the heartbeat is always active (24/7)
            if start_time == end_time {
                // Always running - no time restriction
            } else if start_time < end_time {
                // Normal case: start and end are on same day (e.g., 09:00-17:00)
                if current_time < start_time || current_time > end_time {
                    return false;
                }
            } else {
                // Overnight case: end is before start (e.g., 22:00-06:00)
                // Valid times are: after start OR before end
                if current_time < start_time && current_time > end_time {
                    return false;
                }
            }
        }

        true
    }

    /// Execute a heartbeat — runs all per-agent heartbeat sessions
    async fn execute_heartbeat(&self, config: &HeartbeatConfig) -> Result<(), String> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        log::info!("Executing heartbeat (config_id: {})", config.id);

        // IMPORTANT: Calculate and set next_beat_at BEFORE execution to prevent race conditions
        let next_beat = now + Duration::minutes(config.interval_minutes as i64);
        let next_beat_str = next_beat.to_rfc3339();
        if let Err(e) = self.db.update_heartbeat_next_beat(config.id, &next_beat_str) {
            log::error!("Failed to update heartbeat next_beat_at: {}", e);
        }

        // Broadcast heartbeat start event
        self.broadcaster.broadcast(GatewayEvent::custom(
            "heartbeat_started",
            serde_json::json!({
                "config_id": config.id,
                "channel_id": config.channel_id,
            }),
        ));

        // Run per-agent heartbeats (scans agent folders for heartbeat.md)
        self.run_agent_heartbeats().await;

        // Update last_beat_at (next_beat_at was already set at the start to prevent race conditions)
        if let Err(e) = self.db.update_heartbeat_last_beat_only(config.id, &now_str) {
            log::error!("Failed to update heartbeat last_beat_at: {}", e);
        }

        // Broadcast heartbeat completion event
        self.broadcaster.broadcast(GatewayEvent::custom(
            "heartbeat_completed",
            serde_json::json!({
                "config_id": config.id,
                "channel_id": config.channel_id,
            }),
        ));

        log::info!("Heartbeat completed (config_id: {})", config.id);

        Ok(())
    }

    /// Manually trigger a cron job
    pub async fn run_job_now(&self, job_id: &str) -> Result<String, String> {
        let job = self
            .db
            .get_cron_job_by_job_id(job_id)
            .map_err(|e| format!("Database error: {}", e))?
            .ok_or_else(|| format!("Job not found: {}", job_id))?;

        self.execute_cron_job(&job).await?;

        Ok(format!("Job '{}' executed successfully", job.name))
    }

    /// Scan all agent folders for heartbeat.md and spawn a session for each
    async fn run_agent_heartbeats(&self) {
        let agents_dir = crate::config::runtime_agents_dir();
        let entries = match std::fs::read_dir(&agents_dir) {
            Ok(e) => e,
            Err(e) => {
                log::warn!("[HEARTBEAT] Failed to read agents dir: {}", e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let key = entry.file_name().to_string_lossy().to_string();
            let heartbeat_path = path.join("heartbeat.md");

            if !heartbeat_path.is_file() {
                continue;
            }

            let heartbeat_text = match std::fs::read_to_string(&heartbeat_path) {
                Ok(c) => c.trim().to_string(),
                Err(e) => {
                    log::warn!(
                        "[HEARTBEAT] Failed to read {}: {}",
                        heartbeat_path.display(),
                        e
                    );
                    continue;
                }
            };

            if heartbeat_text.is_empty() {
                continue;
            }

            // Optionally prepend goals.md content if it exists
            let goals_path = path.join("goals.md");
            let prompt = match std::fs::read_to_string(&goals_path) {
                Ok(g) => {
                    let goals = g.trim();
                    if goals.is_empty() {
                        heartbeat_text
                    } else {
                        format!("## Overall Goals ##\n{}\n\n{}", goals, heartbeat_text)
                    }
                }
                Err(_) => heartbeat_text,
            };

            log::info!(
                "[HEARTBEAT] Spawning heartbeat session for agent '{}'",
                key
            );

            // Each agent heartbeat uses a deterministic negative channel ID
            // channel_type = agent key (enables hidden subtype auto-selection)
            let channel_id = -(900 + hash_to_channel_offset(&key));
            let dispatcher = Arc::clone(&self.dispatcher);
            let key_clone = key.clone();

            tokio::spawn(async move {
                let now = Utc::now();
                let normalized = NormalizedMessage {
                    channel_id,
                    channel_type: key_clone.clone(),
                    chat_id: format!("heartbeat:{}:global", key_clone),
                    chat_name: None,
                    user_id: format!("heartbeat-{}", key_clone),
                    user_name: format!("Heartbeat({})", key_clone),
                    text: prompt,
                    message_id: Some(format!(
                        "heartbeat-{}-{}",
                        key_clone,
                        now.timestamp()
                    )),
                    session_mode: Some("isolated".to_string()),
                    selected_network: None,
                    force_safe_mode: false,
                };
                let result = timeout(
                    TokioDuration::from_secs(AGENT_HEARTBEAT_TIMEOUT_SECS),
                    dispatcher.dispatch_safe(normalized),
                )
                .await;
                match result {
                    Ok(r) if r.error.is_some() => {
                        log::warn!("[HEARTBEAT:{}] Failed: {:?}", key_clone, r.error)
                    }
                    Err(_) => log::warn!(
                        "[HEARTBEAT:{}] Timed out after {}s",
                        key_clone,
                        AGENT_HEARTBEAT_TIMEOUT_SECS
                    ),
                    _ => log::info!("[HEARTBEAT:{}] Completed successfully", key_clone),
                }
            });
        }
    }

    /// Trigger a heartbeat pulse (fire and forget, like a channel message)
    ///
    /// Returns immediately after spawning the background task.
    /// Results are broadcast via WebSocket events.
    pub fn run_heartbeat_now(self: &Arc<Self>, config_id: i64) -> Result<String, String> {
        let config = self
            .db
            .get_heartbeat_config_by_id(config_id)
            .map_err(|e| format!("Database error: {}", e))?
            .ok_or_else(|| format!("Heartbeat config not found: {}", config_id))?;

        let scheduler = Arc::clone(self);

        // Spawn the heartbeat in a background task
        tokio::spawn(async move {
            log::info!("[HEARTBEAT] Starting pulse for config_id={}", config_id);

            // Broadcast start event
            scheduler.broadcaster.broadcast(GatewayEvent::custom(
                "heartbeat_pulse_started",
                serde_json::json!({ "config_id": config_id }),
            ));

            let result = scheduler.execute_heartbeat(&config).await;

            let (success, error) = match result {
                Ok(()) => {
                    log::info!("[HEARTBEAT] Pulse completed successfully");
                    (true, None)
                }
                Err(e) => {
                    log::error!("[HEARTBEAT] Pulse failed: {}", e);
                    (false, Some(e))
                }
            };

            // Always broadcast completion event so frontend knows we're done
            scheduler.broadcaster.broadcast(GatewayEvent::custom(
                "heartbeat_pulse_completed",
                serde_json::json!({
                    "config_id": config_id,
                    "success": success,
                    "error": error,
                }),
            ));
        });

        Ok("Heartbeat pulse started (running in background)".to_string())
    }
}


/// Deterministic hash of an agent key to a channel offset (0..99)
fn hash_to_channel_offset(key: &str) -> i64 {
    let mut hash: u64 = 5381;
    for b in key.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u64);
    }
    (hash % 100) as i64
}

