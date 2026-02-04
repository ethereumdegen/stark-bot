use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::types::NormalizedMessage;
use crate::db::Database;
use crate::execution::ExecutionTracker;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{CronJob, HeartbeatConfig, JobStatus, ScheduleType};
use crate::tools::ToolRegistry;
use chrono::{DateTime, Duration, Local, NaiveTime, Utc, Weekday, Datelike, Timelike};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::time::{interval, timeout, Duration as TokioDuration};

/// Constants for heartbeat identity - ensures only ONE identity ever exists
pub const HEARTBEAT_CHANNEL_TYPE: &str = "heartbeat";
pub const HEARTBEAT_USER_ID: &str = "heartbeat-system";
pub const HEARTBEAT_USER_NAME: &str = "Heartbeat";
/// Fixed chat_id ensures we reuse the same session (no timestamp suffix)
pub const HEARTBEAT_CHAT_ID: &str = "heartbeat:global";
/// Dedicated channel ID for heartbeat - NEVER use channel 0 (web UI)
/// Using -999 to avoid collision with real channels and cron job negative IDs
pub const HEARTBEAT_CHANNEL_ID: i64 = -999;

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
            poll_interval_secs: 1,     // Check every second
            max_concurrent_jobs: 5,
        }
    }
}

/// Maximum time for a heartbeat execution before timeout (60 seconds)
const HEARTBEAT_TIMEOUT_SECS: u64 = 60;

/// The scheduler service that runs cron jobs and heartbeats
pub struct Scheduler {
    db: Arc<Database>,
    dispatcher: Arc<MessageDispatcher>,
    broadcaster: Arc<EventBroadcaster>,
    config: SchedulerConfig,
}

impl Scheduler {
    pub fn new(
        db: Arc<Database>,
        dispatcher: Arc<MessageDispatcher>,
        broadcaster: Arc<EventBroadcaster>,
        config: SchedulerConfig,
    ) -> Self {
        Scheduler {
            db,
            dispatcher,
            broadcaster,
            config,
        }
    }

    /// Compatibility method - db_url is no longer needed with connection pool
    #[deprecated(note = "Use new() instead - db_url is no longer needed with r2d2 connection pool")]
    pub fn new_with_db_url(
        db: Arc<Database>,
        dispatcher: Arc<MessageDispatcher>,
        broadcaster: Arc<EventBroadcaster>,
        config: SchedulerConfig,
        _db_url: String,
    ) -> Self {
        Self::new(db, dispatcher, broadcaster, config)
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

        // Process heartbeats (always enabled - individual configs control their own enabled state)
        if let Err(e) = self.process_heartbeats().await {
            log::error!("Error processing heartbeats: {}", e);
        }

        // Run periodic cleanup tasks once per hour (at minute 0, second 0-1)
        let now = Local::now();
        if now.minute() == 0 && now.second() <= 1 {
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

    fn clone_inner(&self) -> Scheduler {
        Scheduler {
            db: Arc::clone(&self.db),
            dispatcher: Arc::clone(&self.dispatcher),
            broadcaster: Arc::clone(&self.broadcaster),
            config: self.config.clone(),
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
            chat_id: format!("cron:{}", job.job_id),
            user_id: "system".to_string(),
            user_name: format!("Cron: {}", job.name),
            text: message_text,
            message_id: Some(format!("cron-run-{}", started_at.timestamp())),
            session_mode: Some(job.session_mode.clone()),
            selected_network: None,
            force_safe_mode: false,
        };

        // Execute the job
        let result = self.dispatcher.dispatch(normalized).await;

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds();

        // Note: next_run_at was already set at the start to prevent race conditions
        // Update job status with final result
        let success = result.error.is_none();
        self.db
            .update_cron_job_run_status(
                job.id,
                &started_at_str,
                next_run_str.as_deref(),
                success,
                result.error.as_deref(),
            )
            .map_err(|e| format!("Failed to update job status: {}", e))?;

        // Log the run
        let _ = self.db.log_cron_job_run(
            job.id,
            &started_at_str,
            Some(&completed_at.to_rfc3339()),
            success,
            Some(&result.response),
            result.error.as_deref(),
            Some(duration_ms),
        );

        // Handle delete_after_run for one-shot jobs
        if success && job.delete_after_run {
            log::info!("Deleting one-shot cron job '{}' after successful run", job.name);
            let _ = self.db.delete_cron_job(job.id);
        }

        // Handle delivery if configured
        if job.deliver && job.channel_id.is_some() {
            self.deliver_result(job, &result.response).await?;
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

            let start_time = NaiveTime::parse_from_str(start, "%H:%M").unwrap_or(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
            let end_time = NaiveTime::parse_from_str(end, "%H:%M").unwrap_or(NaiveTime::from_hms_opt(23, 59, 59).unwrap());

            // Handle overnight schedules (e.g., 22:00-06:00)
            if start_time <= end_time {
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

    /// Execute a heartbeat check - now with mind map meandering
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

        // === MIND MAP MEANDERING ===
        // Get the next node to visit (starts at trunk, then meanders)
        let next_node = self.db.get_next_heartbeat_node(config.current_mind_node_id)
            .map_err(|e| format!("Failed to get next heartbeat node: {}", e))?;

        let node_depth = self.db.get_mind_node_depth(next_node.id).unwrap_or(0);

        log::info!(
            "Heartbeat visiting mind node {} (depth: {}, is_trunk: {}, body_len: {})",
            next_node.id, node_depth, next_node.is_trunk, next_node.body.len()
        );

        // Broadcast heartbeat start event with node info
        self.broadcaster.broadcast(GatewayEvent::custom(
            "heartbeat_started",
            serde_json::json!({
                "config_id": config.id,
                "channel_id": config.channel_id,
                "mind_node_id": next_node.id,
                "mind_node_depth": node_depth,
                "is_trunk": next_node.is_trunk,
            }),
        ));

        // === BUILD HEARTBEAT MESSAGE ===
        let node_content = if next_node.body.is_empty() {
            if next_node.is_trunk {
                "This is the trunk node (root of your mind map). It's currently empty.".to_string()
            } else {
                "This node is currently empty.".to_string()
            }
        } else {
            next_node.body.clone()
        };

        let message_text = format!(
            "[HEARTBEAT - Mind Map Reflection]\n\
            Current Position: Node #{} (depth: {}{})\n\
            Node Content: {}\n\n\
            Instructions:\n\
            - Reflect on this node's content in the context of your mind map\n\
            - Consider connections to other thoughts and ideas\n\
            - If the node is empty, consider what thoughts belong here\n\
            - You may update this node's content or create new connected nodes\n\
            - Review any pending tasks or items that relate to this area\n\
            - Respond with HEARTBEAT_OK if no action needed",
            next_node.id,
            node_depth,
            if next_node.is_trunk { ", trunk" } else { "" },
            node_content
        );

        // Use fixed constants for heartbeat identity, but isolated session mode
        // to prevent session state corruption from breaking other functionality
        // IMPORTANT: Always use HEARTBEAT_CHANNEL_ID (-999) to avoid polluting web UI (channel 0)
        let normalized = NormalizedMessage {
            channel_id: HEARTBEAT_CHANNEL_ID,
            channel_type: HEARTBEAT_CHANNEL_TYPE.to_string(),
            chat_id: HEARTBEAT_CHAT_ID.to_string(),
            user_id: HEARTBEAT_USER_ID.to_string(),
            user_name: HEARTBEAT_USER_NAME.to_string(),
            text: message_text,
            message_id: Some(format!("heartbeat-{}", now.timestamp())),
            session_mode: Some("isolated".to_string()), // Isolated to prevent state corruption
            selected_network: None,
            force_safe_mode: false,
        };

        // Execute the heartbeat
        let result = self.dispatcher.dispatch(normalized).await;

        // === GET SESSION ID ===
        // Query the session using the fixed heartbeat session key
        let session_key = format!("{}:{}:{}", HEARTBEAT_CHANNEL_TYPE, HEARTBEAT_CHANNEL_ID, HEARTBEAT_CHAT_ID);
        let new_session_id = self.db.get_chat_session_by_key(&session_key)
            .ok()
            .flatten()
            .map(|s| s.id);

        // Update heartbeat config with new mind position and session ID
        if let Err(e) = self.db.update_heartbeat_mind_position(
            config.id,
            Some(next_node.id),
            new_session_id,
        ) {
            log::error!("Failed to update heartbeat mind position: {}", e);
        }

        // Update last_beat_at only (next_beat_at was already set at the start to prevent race conditions)
        if let Err(e) = self.db.update_heartbeat_last_beat_only(config.id, &now_str) {
            log::error!("Failed to update heartbeat last_beat_at: {}", e);
        }

        // Check for HEARTBEAT_OK suppression
        if result.response.contains("HEARTBEAT_OK") {
            log::debug!("Heartbeat response contains HEARTBEAT_OK, suppressing output");
        }

        // Broadcast heartbeat completion event
        self.broadcaster.broadcast(GatewayEvent::custom(
            "heartbeat_completed",
            serde_json::json!({
                "config_id": config.id,
                "channel_id": config.channel_id,
                "mind_node_id": next_node.id,
                "success": result.error.is_none(),
            }),
        ));

        log::info!(
            "Heartbeat completed (config_id: {}, visited node: {})",
            config.id, next_node.id
        );

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

        // Clone what we need for the background task
        let db = Arc::clone(&self.db);
        let broadcaster = Arc::clone(&self.broadcaster);

        // Spawn the heartbeat in a background task
        tokio::spawn(async move {
            log::info!("[HEARTBEAT] Starting pulse for config_id={}", config_id);

            // Broadcast start event
            broadcaster.broadcast(GatewayEvent::custom(
                "heartbeat_pulse_started",
                serde_json::json!({ "config_id": config_id }),
            ));

            // Create dispatcher for this task (uses shared db pool)
            let tracker = Arc::new(ExecutionTracker::new(broadcaster.clone()));
            let tool_registry = Arc::new(ToolRegistry::new());
            let burner_wallet = std::env::var("BURNER_WALLET_BOT_PRIVATE_KEY").ok();
            let dispatcher = Arc::new(MessageDispatcher::new_with_wallet(
                db.clone(),
                broadcaster.clone(),
                tool_registry,
                tracker,
                burner_wallet,
            ));

            // Execute with timeout
            let result = timeout(
                TokioDuration::from_secs(HEARTBEAT_TIMEOUT_SECS),
                execute_heartbeat_isolated(&db, &dispatcher, &broadcaster, &config)
            ).await;

            let (success, error) = match result {
                Ok(Ok(())) => {
                    log::info!("[HEARTBEAT] Pulse completed successfully");
                    (true, None)
                }
                Ok(Err(e)) => {
                    log::error!("[HEARTBEAT] Pulse failed: {}", e);
                    (false, Some(e))
                }
                Err(_) => {
                    let msg = format!("Heartbeat timed out after {} seconds", HEARTBEAT_TIMEOUT_SECS);
                    log::error!("[HEARTBEAT] {}", msg);
                    (false, Some(msg))
                }
            };

            // Always broadcast completion event so frontend knows we're done
            broadcaster.broadcast(GatewayEvent::custom(
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

/// Execute heartbeat with isolated DB and dispatcher (doesn't block main server)
/// Updates position and creates session IMMEDIATELY, then defers AI call to background
async fn execute_heartbeat_isolated(
    db: &Arc<Database>,
    dispatcher: &Arc<MessageDispatcher>,
    broadcaster: &Arc<EventBroadcaster>,
    config: &HeartbeatConfig,
) -> Result<(), String> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    log::info!("[HEARTBEAT-ISOLATED] Executing heartbeat (config_id: {})", config.id);
    log::info!("[HEARTBEAT-ISOLATED] current_mind_node_id: {:?}", config.current_mind_node_id);

    // Calculate and set next_beat_at BEFORE execution
    let next_beat = now + Duration::minutes(config.interval_minutes as i64);
    let next_beat_str = next_beat.to_rfc3339();
    log::info!("[HEARTBEAT-ISOLATED] Updating next_beat_at...");
    if let Err(e) = db.update_heartbeat_next_beat(config.id, &next_beat_str) {
        log::error!("[HEARTBEAT-ISOLATED] Failed to update next_beat_at: {}", e);
    }

    // Get the next node to visit
    log::info!("[HEARTBEAT-ISOLATED] Getting next heartbeat node...");
    let next_node = match db.get_next_heartbeat_node(config.current_mind_node_id) {
        Ok(node) => {
            log::info!("[HEARTBEAT-ISOLATED] Got next node: id={}", node.id);
            node
        }
        Err(e) => {
            log::error!("[HEARTBEAT-ISOLATED] Failed to get next node: {}", e);
            return Err(format!("Failed to get next heartbeat node: {}", e));
        }
    };

    // Calculate depth using iterative BFS (safe from cycles)
    let node_depth = db.get_mind_node_depth(next_node.id).unwrap_or(0);

    log::info!(
        "[HEARTBEAT-ISOLATED] Visiting mind node {} (is_trunk: {})",
        next_node.id, next_node.is_trunk
    );

    // === IMMEDIATE UPDATES (before AI call) ===

    // Update heartbeat config with new position immediately
    if let Err(e) = db.update_heartbeat_mind_position(config.id, Some(next_node.id), None) {
        log::error!("[HEARTBEAT-ISOLATED] Failed to update mind position: {}", e);
    }

    // Update last_beat_at only (next_beat_at was already set above)
    if let Err(e) = db.update_heartbeat_last_beat_only(config.id, &now_str) {
        log::error!("[HEARTBEAT-ISOLATED] Failed to update last_beat_at: {}", e);
    }

    // Broadcast heartbeat start event with node info (UI can animate now)
    broadcaster.broadcast(GatewayEvent::custom(
        "heartbeat_started",
        serde_json::json!({
            "config_id": config.id,
            "channel_id": config.channel_id,
            "mind_node_id": next_node.id,
            "mind_node_depth": node_depth,
            "is_trunk": next_node.is_trunk,
        }),
    ));

    // Build heartbeat message
    let node_content = if next_node.body.is_empty() {
        if next_node.is_trunk {
            "This is the trunk node (root of your mind map). It's currently empty.".to_string()
        } else {
            "This node is currently empty.".to_string()
        }
    } else {
        next_node.body.clone()
    };

    let message_text = format!(
        "[HEARTBEAT - Mind Map Reflection]\n\
        Current Position: Node #{} (depth: {}{})\n\
        Node Content: {}\n\n\
        Instructions:\n\
        - Reflect on this node's content in the context of your mind map\n\
        - Consider connections to other thoughts and ideas\n\
        - If the node is empty, consider what thoughts belong here\n\
        - You may update this node's content or create new connected nodes\n\
        - Review any pending tasks or items that relate to this area\n\
        - Respond with HEARTBEAT_OK if no action needed",
        next_node.id,
        node_depth,
        if next_node.is_trunk { ", trunk" } else { "" },
        node_content
    );

    // IMPORTANT: Always use HEARTBEAT_CHANNEL_ID (-999) to avoid polluting web UI (channel 0)
    let normalized = NormalizedMessage {
        channel_id: HEARTBEAT_CHANNEL_ID,
        channel_type: HEARTBEAT_CHANNEL_TYPE.to_string(),
        chat_id: HEARTBEAT_CHAT_ID.to_string(),
        user_id: HEARTBEAT_USER_ID.to_string(),
        user_name: HEARTBEAT_USER_NAME.to_string(),
        text: message_text,
        message_id: Some(format!("heartbeat-{}", now.timestamp())),
        session_mode: Some("isolated".to_string()),
        selected_network: None,
        force_safe_mode: false,
    };

    // === DEFERRED AI CALL (fire and forget) ===
    let dispatcher = Arc::clone(dispatcher);
    let broadcaster = Arc::clone(broadcaster);
    let config_id = config.id;
    let config_channel_id = config.channel_id; // Keep for broadcast events only
    let node_id = next_node.id;
    let db = Arc::clone(db);

    tokio::spawn(async move {
        log::info!("[HEARTBEAT-AI] Starting dispatch task for node {}", node_id);
        log::info!("[HEARTBEAT-AI] channel_type={}, channel_id={}, chat_id={}",
            HEARTBEAT_CHANNEL_TYPE, HEARTBEAT_CHANNEL_ID, HEARTBEAT_CHAT_ID);

        let result = dispatcher.dispatch(normalized).await;

        log::info!("[HEARTBEAT-AI] Dispatch returned. Response len: {}, Error: {:?}",
            result.response.len(), result.error);

        if let Some(ref err) = result.error {
            log::error!("[HEARTBEAT-AI] Dispatch failed: {}", err);
        } else {
            log::info!("[HEARTBEAT-AI] Dispatch completed successfully");
        }

        // Update session ID after dispatch (session created during dispatch)
        let session_key = format!("{}:{}:{}", HEARTBEAT_CHANNEL_TYPE, HEARTBEAT_CHANNEL_ID, HEARTBEAT_CHAT_ID);
        log::info!("[HEARTBEAT-AI] Looking for session with key: {}", session_key);

        match db.get_chat_session_by_key(&session_key) {
            Ok(Some(session)) => {
                log::info!("[HEARTBEAT-AI] Found session id={}, updating heartbeat config", session.id);
                let _ = db.update_heartbeat_mind_position(config_id, Some(node_id), Some(session.id));
            }
            Ok(None) => {
                log::warn!("[HEARTBEAT-AI] No session found with key: {}", session_key);
            }
            Err(e) => {
                log::error!("[HEARTBEAT-AI] Error looking up session: {}", e);
            }
        }

        // Broadcast completion
        broadcaster.broadcast(GatewayEvent::custom(
            "heartbeat_completed",
            serde_json::json!({
                "config_id": config_id,
                "channel_id": config_channel_id,
                "mind_node_id": node_id,
                "success": result.error.is_none(),
                "error": result.error,
            }),
        ));
    });

    log::info!("[HEARTBEAT-ISOLATED] Position updated, AI call deferred (node: {})", next_node.id);

    Ok(())
}
