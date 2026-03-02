pub mod actix_ws;
pub mod events;
pub mod methods;
pub mod protocol;

pub use events::EventBroadcaster;

use crate::channels::ChannelManager;
use crate::db::Database;
use crate::tools::ToolRegistry;
use crate::tx_queue::TxQueueManager;
use crate::wallet::WalletProvider;
use std::sync::Arc;

/// Main Gateway struct that owns all channel connections and exposes WebSocket RPC
pub struct Gateway {
    db: Arc<Database>,
    channel_manager: Arc<ChannelManager>,
    broadcaster: Arc<EventBroadcaster>,
}

impl Gateway {
    pub fn new(db: Arc<Database>) -> Self {
        let broadcaster = Arc::new(EventBroadcaster::new());
        let channel_manager = Arc::new(ChannelManager::new(db.clone(), broadcaster.clone()));

        Self {
            db,
            channel_manager,
            broadcaster,
        }
    }

    /// Create a new Gateway with tool registry support
    pub fn new_with_tools(db: Arc<Database>, tool_registry: Arc<ToolRegistry>) -> Self {
        Self::new_with_tools_wallet_and_tx_queue(db, tool_registry, None, None, None)
    }

    /// Create a new Gateway with tool registry and wallet provider for x402 payments
    pub fn new_with_tools_and_wallet(
        db: Arc<Database>,
        tool_registry: Arc<ToolRegistry>,
        wallet_provider: Option<Arc<dyn WalletProvider>>,
    ) -> Self {
        Self::new_with_tools_wallet_and_tx_queue(db, tool_registry, wallet_provider, None, None)
    }

    /// Create a fully-configured Gateway with all optional subsystems
    pub fn new_with_tools_wallet_and_tx_queue(
        db: Arc<Database>,
        tool_registry: Arc<ToolRegistry>,
        wallet_provider: Option<Arc<dyn WalletProvider>>,
        tx_queue: Option<Arc<TxQueueManager>>,
        skill_registry: Option<Arc<crate::skills::SkillRegistry>>,
    ) -> Self {
        Self::new_full(db, tool_registry, wallet_provider, tx_queue, skill_registry, None)
    }

    /// Create a fully-configured Gateway with all optional subsystems including credits session
    pub fn new_full(
        db: Arc<Database>,
        tool_registry: Arc<ToolRegistry>,
        wallet_provider: Option<Arc<dyn WalletProvider>>,
        tx_queue: Option<Arc<TxQueueManager>>,
        skill_registry: Option<Arc<crate::skills::SkillRegistry>>,
        credits_session: Option<Arc<crate::credits_session::CreditsSessionClient>>,
    ) -> Self {
        let broadcaster = Arc::new(EventBroadcaster::new());
        let mut channel_manager = ChannelManager::new_with_tools_and_wallet(
            db.clone(),
            broadcaster.clone(),
            tool_registry,
            wallet_provider,
        );
        if let Some(tq) = tx_queue {
            channel_manager = channel_manager.with_tx_queue(tq);
        }
        if let Some(sr) = skill_registry {
            channel_manager = channel_manager.with_skill_registry(sr);
        }
        if let Some(cs) = credits_session {
            channel_manager = channel_manager.with_credits_session(cs);
        }
        let channel_manager = Arc::new(channel_manager);

        Self {
            db,
            channel_manager,
            broadcaster,
        }
    }

    /// Start all channels that have auto_start_on_boot setting enabled.
    /// Queries ALL channels (not just enabled ones) so that channels which were
    /// stopped before a reboot still auto-start if the setting is true.
    pub async fn start_enabled_channels(&self) {
        match self.db.list_channels() {
            Ok(channels) => {
                for channel in channels {
                    let id = channel.id;
                    let name = channel.name.clone();
                    let channel_type = channel.channel_type.clone();

                    // Check if channel has auto_start_on_boot setting enabled
                    let should_auto_start = self.db
                        .get_channel_setting(id, "auto_start_on_boot")
                        .ok()
                        .flatten()
                        .map(|v| v == "true")
                        .unwrap_or(false);

                    if !should_auto_start {
                        log::debug!(
                            "Skipping {} channel {} (auto_start_on_boot not enabled)",
                            channel_type,
                            name
                        );
                        continue;
                    }

                    match self.channel_manager.start_channel(channel).await {
                        Ok(()) => {
                            // Mark channel as enabled in DB so it shows as running
                            let _ = self.db.set_channel_enabled(id, true);
                            log::info!("Auto-started {} channel: {}", channel_type, name);
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to auto-start {} channel {}: {}",
                                channel_type,
                                name,
                                e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to load channels for auto-start: {}", e);
            }
        }
    }

    /// Get the event broadcaster for emitting events
    pub fn broadcaster(&self) -> Arc<EventBroadcaster> {
        self.broadcaster.clone()
    }

    /// Get the channel manager
    pub fn channel_manager(&self) -> Arc<ChannelManager> {
        self.channel_manager.clone()
    }
}
