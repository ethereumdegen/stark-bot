use crate::gateway::protocol::GatewayEvent;
use dashmap::DashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Broadcasts events to all connected WebSocket clients
pub struct EventBroadcaster {
    clients: DashMap<String, mpsc::Sender<GatewayEvent>>,
}

impl EventBroadcaster {
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
        }
    }

    /// Subscribe a new client and return (client_id, receiver)
    pub fn subscribe(&self) -> (String, mpsc::Receiver<GatewayEvent>) {
        let client_id = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel(100);
        self.clients.insert(client_id.clone(), tx);
        log::debug!("Client {} subscribed to events", client_id);
        (client_id, rx)
    }

    /// Unsubscribe a client
    pub fn unsubscribe(&self, client_id: &str) {
        self.clients.remove(client_id);
        log::debug!("Client {} unsubscribed from events", client_id);
    }

    /// Broadcast an event to all connected clients
    pub fn broadcast(&self, event: GatewayEvent) {
        let event_name = event.event.clone();
        let mut failed_clients = Vec::new();

        // Log the full event payload for debugging
        if let Ok(json) = serde_json::to_string_pretty(&event) {
            log::debug!(
                "[DATAGRAM] BROADCAST event '{}' to {} clients:\n{}",
                event_name,
                self.clients.len(),
                json
            );
        }

        for entry in self.clients.iter() {
            let client_id = entry.key().clone();
            let sender = entry.value();

            if sender.try_send(event.clone()).is_err() {
                // Client channel full or closed
                failed_clients.push(client_id);
            }
        }

        // Clean up failed clients
        for client_id in failed_clients {
            self.clients.remove(&client_id);
            log::debug!("Removed disconnected client {}", client_id);
        }
    }

    /// Get the number of connected clients
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }
}

impl Default for EventBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}
