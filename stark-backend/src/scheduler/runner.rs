use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::types::NormalizedMessage;
use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::{CronJob, HeartbeatConfig, JobStatus, ScheduleType};
use chrono::{DateTime, Duration, Local, NaiveTime, Utc, Weekday, Datelike};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::time::{interval, Duration as TokioDuration};

/// Constants for heartbeat identity - ensures only ONE identity ever exists
pub const HEARTBEAT_CHANNEL_TYPE: &str = "heartbeat";
pub const HEARTBEAT_USER_ID: &str = "heartbeat-system";
pub const HEARTBEAT_USER_NAME: &str = "Heartbeat";
/// Fixed chat_id ensures we reuse the same session (no timestamp suffix)
pub const HEARTBEAT_CHAT_ID: &str = "heartbeat:global";

/// Scheduler configuration
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Enable cron job processing
    pub cron_enabled: bool,
    /// Enable heartbeat processing
    pub heartbeat_enabled: bool,
    /// Poll interval in seconds for checking due jobs
    pub poll_interval_secs: u64,
    /// Maximum concurrent job executions
    pub max_concurrent_jobs: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        SchedulerConfig {
            cron_enabled: true,
            heartbeat_enabled: false,  // Disabled - too noisy
            poll_interval_secs: 60,    // Check once per minute instead of 10 seconds
            max_concurrent_jobs: 5,
        }
    }
}

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

    /// Start the scheduler background task
    pub async fn start(self: Arc<Self>, mut shutdown_rx: oneshot::Receiver<()>) {
        log::info!(
            "Scheduler started (cron: {}, heartbeat: {}, poll: {}s)",
            self.config.cron_enabled,
            self.config.heartbeat_enabled,
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

        // Process heartbeats
        if self.config.heartbeat_enabled {
            if let Err(e) = self.process_heartbeats().await {
                log::error!("Error processing heartbeats: {}", e);
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
    async fn process_heartbeats(&self) -> Result<(), String> {
        let due_configs = self
            .db
            .list_due_heartbeat_configs()
            .map_err(|e| format!("Failed to list due heartbeats: {}", e))?;

        for config in due_configs {
            // Check if within active hours
            if !self.is_within_active_hours(&config) {
                continue;
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

            if current_time < start_time || current_time > end_time {
                return false;
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

        // Use fixed constants to ensure only ONE heartbeat identity/session ever exists
        let normalized = NormalizedMessage {
            channel_id: config.channel_id.unwrap_or(0),
            channel_type: HEARTBEAT_CHANNEL_TYPE.to_string(),
            chat_id: HEARTBEAT_CHAT_ID.to_string(),
            user_id: HEARTBEAT_USER_ID.to_string(),
            user_name: HEARTBEAT_USER_NAME.to_string(),
            text: message_text,
            message_id: Some(format!("heartbeat-{}", now.timestamp())),
            session_mode: None, // Use normal session mode to reuse session
            selected_network: None,
        };

        // Execute the heartbeat
        let result = self.dispatcher.dispatch(normalized).await;

        // === GET SESSION ID ===
        // Query the session using the fixed heartbeat session key
        let session_key = format!("{}:{}:{}", HEARTBEAT_CHANNEL_TYPE, config.channel_id.unwrap_or(0), HEARTBEAT_CHAT_ID);
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

        // Update last_beat_at
        self.db
            .update_heartbeat_last_beat(config.id, &now_str, &next_beat_str)
            .map_err(|e| format!("Failed to update heartbeat status: {}", e))?;

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

    /// Manually trigger a heartbeat (force pulse)
    pub async fn run_heartbeat_now(&self, config_id: i64) -> Result<String, String> {
        let config = self
            .db
            .get_heartbeat_config_by_id(config_id)
            .map_err(|e| format!("Database error: {}", e))?
            .ok_or_else(|| format!("Heartbeat config not found: {}", config_id))?;

        self.execute_heartbeat(&config).await?;

        Ok("Heartbeat pulse executed successfully".to_string())
    }
}
