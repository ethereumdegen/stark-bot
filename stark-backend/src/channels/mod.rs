pub mod discord;
pub mod dispatcher;
pub mod safe_mode_rate_limiter;
pub mod session_writer;
pub mod slack;
pub mod telegram;
pub mod twitter;
pub mod types;
pub mod coalescing;
pub mod util;

pub use dispatcher::MessageDispatcher;
pub use safe_mode_rate_limiter::{SafeModeChannelRateLimiter, SafeModeQueryResult};
pub use types::{ChannelHandle, ChannelType, NormalizedMessage};

use crate::db::Database;
use crate::execution::ExecutionTracker;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::Channel;
use crate::tools::ToolRegistry;
use crate::tx_queue::TxQueueManager;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::oneshot;

/// Manages all running channel listeners
pub struct ChannelManager {
    db: Arc<Database>,
    broadcaster: Arc<EventBroadcaster>,
    running_channels: Arc<DashMap<i64, ChannelHandle>>,
    tool_registry: Option<Arc<ToolRegistry>>,
    execution_tracker: Arc<ExecutionTracker>,
    /// Wallet provider for x402 payments and transaction signing
    /// Either EnvWalletProvider (Standard mode) or FlashWalletProvider (Flash mode)
    wallet_provider: Option<Arc<dyn crate::wallet::WalletProvider>>,
    tx_queue: Option<Arc<TxQueueManager>>,
    skill_registry: Option<Arc<crate::skills::SkillRegistry>>,
}

impl ChannelManager {
    pub fn new(db: Arc<Database>, broadcaster: Arc<EventBroadcaster>) -> Self {
        let execution_tracker = Arc::new(ExecutionTracker::new(broadcaster.clone()));
        Self {
            db,
            broadcaster,
            running_channels: Arc::new(DashMap::new()),
            tool_registry: None,
            execution_tracker,
            wallet_provider: None,
            tx_queue: None,
            skill_registry: None,
        }
    }

    pub fn new_with_tools(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
    ) -> Self {
        Self::new_with_tools_and_wallet(db, broadcaster, tool_registry, None)
    }

    /// Create a ChannelManager with tools and wallet provider
    /// The wallet_provider encapsulates both Standard mode (EnvWalletProvider)
    /// and Flash mode (FlashWalletProvider)
    pub fn new_with_tools_and_wallet(
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
        tool_registry: Arc<ToolRegistry>,
        wallet_provider: Option<Arc<dyn crate::wallet::WalletProvider>>,
    ) -> Self {
        let execution_tracker = Arc::new(ExecutionTracker::new(broadcaster.clone()));
        Self {
            db,
            broadcaster,
            running_channels: Arc::new(DashMap::new()),
            tool_registry: Some(tool_registry),
            execution_tracker,
            wallet_provider,
            tx_queue: None,
            skill_registry: None,
        }
    }

    /// Set the transaction queue manager for web3 transactions
    pub fn with_tx_queue(mut self, tx_queue: Arc<TxQueueManager>) -> Self {
        self.tx_queue = Some(tx_queue);
        self
    }

    /// Set the skill registry for skill-based tools (use_skill, manage_skills)
    pub fn with_skill_registry(mut self, skill_registry: Arc<crate::skills::SkillRegistry>) -> Self {
        self.skill_registry = Some(skill_registry);
        self
    }

    /// Check if a channel is currently running
    pub fn is_running(&self, channel_id: i64) -> bool {
        self.running_channels.contains_key(&channel_id)
    }

    /// Get list of running channel IDs
    pub fn running_channel_ids(&self) -> Vec<i64> {
        self.running_channels.iter().map(|e| *e.key()).collect()
    }

    /// Start a channel listener
    pub async fn start_channel(&self, mut channel: Channel) -> Result<(), String> {
        let channel_id = channel.id;
        let channel_type = channel.channel_type.clone();
        let channel_name = channel.name.clone();

        // Check if already running
        if self.is_running(channel_id) {
            return Err(format!("Channel {} is already running", channel_id));
        }

        // Load bot_token from channel settings (preferred), fall back to DB column value.
        // This ensures settings always take precedence over the legacy column.
        {
            let setting_key = match channel_type.as_str() {
                "discord" => "discord_bot_token",
                "telegram" => "telegram_bot_token",
                "slack" => "slack_bot_token",
                _ => "", // Twitter and ExternalChannel don't use bot_token
            };
            if !setting_key.is_empty() {
                if let Ok(Some(token)) = self.db.get_channel_setting(channel_id, setting_key) {
                    if !token.is_empty() {
                        channel.bot_token = token;
                    }
                }
            }
        }

        // Load app_token from channel settings (preferred), fall back to DB column value.
        if channel_type == "slack" {
            if let Ok(Some(token)) = self.db.get_channel_setting(channel_id, "slack_app_token") {
                if !token.is_empty() {
                    channel.app_token = Some(token);
                }
            }
        }

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Create dispatcher with or without tools (and wallet provider for x402 payment support)
        let dispatcher = if let Some(ref tool_registry) = self.tool_registry {
            let mut disp = MessageDispatcher::new_with_wallet_and_skills(
                self.db.clone(),
                self.broadcaster.clone(),
                tool_registry.clone(),
                self.execution_tracker.clone(),
                self.wallet_provider.clone(),
                self.skill_registry.clone(),
            );
            // Add tx_queue if available (needed for web3 transactions)
            if let Some(ref tx_queue) = self.tx_queue {
                disp = disp.with_tx_queue(tx_queue.clone());
            }
            Arc::new(disp)
        } else {
            Arc::new(MessageDispatcher::new_without_tools(
                self.db.clone(),
                self.broadcaster.clone(),
            ))
        };

        // Store handle
        let handle = ChannelHandle::new(
            channel_id,
            channel_type.clone(),
            channel_name.clone(),
            shutdown_tx,
        );
        self.running_channels.insert(channel_id, handle);

        // Start the appropriate listener
        let broadcaster = self.broadcaster.clone();
        let running_channels = self.running_channels.clone();

        // Parse channel type
        let channel_type_enum = match types::ChannelType::from_str(&channel_type) {
            Some(ct) => ct,
            None => {
                self.running_channels.remove(&channel_id);
                return Err(format!("Unknown channel type: {}", channel_type));
            }
        };

        match channel_type_enum {
            types::ChannelType::Telegram => {
                let db = self.db.clone();
                tokio::spawn(async move {
                    let result = telegram::start_telegram_listener(
                        channel,
                        dispatcher,
                        broadcaster.clone(),
                        db,
                        shutdown_rx,
                    )
                    .await;

                    if let Err(e) = result {
                        log::error!("Telegram listener error: {}", e);
                        broadcaster.broadcast(GatewayEvent::channel_error(channel_id, &e));
                    }

                    // Remove from running channels
                    running_channels.remove(&channel_id);
                });
            }
            types::ChannelType::Slack => {
                let db = self.db.clone();
                let safe_mode_rate_limiter = SafeModeChannelRateLimiter::new(db.clone());
                tokio::spawn(async move {
                    let result = slack::start_slack_listener(
                        channel,
                        dispatcher,
                        broadcaster.clone(),
                        db,
                        safe_mode_rate_limiter,
                        shutdown_rx,
                    )
                    .await;

                    if let Err(e) = result {
                        log::error!("Slack listener error: {}", e);
                        broadcaster.broadcast(GatewayEvent::channel_error(channel_id, &e));
                    }

                    // Remove from running channels
                    running_channels.remove(&channel_id);
                });
            }
            types::ChannelType::Discord => {
                let db = self.db.clone();
                let safe_mode_rate_limiter = SafeModeChannelRateLimiter::new(db.clone());
                tokio::spawn(async move {
                    let result = discord::start_discord_listener(
                        channel,
                        dispatcher,
                        broadcaster.clone(),
                        db,
                        safe_mode_rate_limiter,
                        shutdown_rx,
                    )
                    .await;

                    if let Err(e) = result {
                        log::error!("Discord listener error: {}", e);
                        broadcaster.broadcast(GatewayEvent::channel_error(channel_id, &e));
                    }

                    // Remove from running channels
                    running_channels.remove(&channel_id);
                });
            }
            types::ChannelType::Twitter => {
                let db = self.db.clone();
                tokio::spawn(async move {
                    let result = twitter::start_twitter_listener(
                        channel,
                        dispatcher,
                        broadcaster.clone(),
                        db,
                        shutdown_rx,
                    )
                    .await;

                    if let Err(e) = result {
                        log::error!("Twitter listener error: {}", e);
                        broadcaster.broadcast(GatewayEvent::channel_error(channel_id, &e));
                    }

                    // Remove from running channels
                    running_channels.remove(&channel_id);
                });
            }
            types::ChannelType::ExternalChannel => {
                // No listener needed — HTTP request/response model.
                // Channel being in running_channels is sufficient.
                log::info!("External channel '{}' started (no listener)", channel_name);
            }
        }

        log::info!(
            "Started {} channel listener: {} (id={})",
            channel_type,
            channel_name,
            channel_id
        );

        Ok(())
    }

    /// Stop a channel listener
    pub async fn stop_channel(&self, channel_id: i64) -> Result<(), String> {
        match self.running_channels.remove(&channel_id) {
            Some((_, handle)) => {
                log::info!(
                    "Stopping {} channel: {} (id={})",
                    handle.channel_type,
                    handle.name,
                    channel_id
                );

                // Send shutdown signal
                let _ = handle.shutdown_tx.send(());

                Ok(())
            }
            None => Err(format!("Channel {} is not running", channel_id)),
        }
    }

    /// Stop all running channels
    pub async fn stop_all(&self) {
        let ids: Vec<i64> = self.running_channels.iter().map(|e| *e.key()).collect();
        for id in ids {
            let _ = self.stop_channel(id).await;
        }
    }
}
