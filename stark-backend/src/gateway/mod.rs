pub mod actix_ws;
pub mod events;
pub mod methods;
pub mod protocol;

pub use events::EventBroadcaster;

use crate::channels::ChannelManager;
use crate::db::Database;
use crate::tools::ToolRegistry;
use crate::tx_queue::TxQueueManager;
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
        Self::new_with_tools_and_wallet(db, tool_registry, None)
    }

    /// Create a new Gateway with tool registry and wallet support for x402 payments
    pub fn new_with_tools_and_wallet(
        db: Arc<Database>,
        tool_registry: Arc<ToolRegistry>,
        burner_wallet_private_key: Option<String>,
    ) -> Self {
        Self::new_with_tools_wallet_and_tx_queue(db, tool_registry, burner_wallet_private_key, None)
    }

    /// Create a new Gateway with tool registry, wallet, and transaction queue support
    pub fn new_with_tools_wallet_and_tx_queue(
        db: Arc<Database>,
        tool_registry: Arc<ToolRegistry>,
        burner_wallet_private_key: Option<String>,
        tx_queue: Option<Arc<TxQueueManager>>,
    ) -> Self {
        let broadcaster = Arc::new(EventBroadcaster::new());
        let mut channel_manager = ChannelManager::new_with_tools_and_wallet(
            db.clone(),
            broadcaster.clone(),
            tool_registry,
            burner_wallet_private_key,
        );
        // Add tx_queue if provided (needed for web3 transactions in channels)
        if let Some(tq) = tx_queue {
            channel_manager = channel_manager.with_tx_queue(tq);
        }
        let channel_manager = Arc::new(channel_manager);

        Self {
            db,
            channel_manager,
            broadcaster,
        }
    }

    /// Start all channels that have auto_start_on_boot setting enabled
    pub async fn start_enabled_channels(&self) {
        match self.db.list_enabled_channels() {
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
                log::error!("Failed to load enabled channels: {}", e);
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
