use crate::channels::ChannelManager;
use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::methods;
use crate::gateway::protocol::{ChannelIdParams, RpcError, RpcRequest, RpcResponse};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::{accept_async, tungstenite::Message};

/// Authentication timeout - client must authenticate within this time
const AUTH_TIMEOUT_SECS: u64 = 30;

pub struct GatewayServer {
    db: Arc<Database>,
    channel_manager: Arc<ChannelManager>,
    broadcaster: Arc<EventBroadcaster>,
}

impl GatewayServer {
    pub fn new(
        db: Arc<Database>,
        channel_manager: Arc<ChannelManager>,
        broadcaster: Arc<EventBroadcaster>,
    ) -> Self {
        Self {
            db,
            channel_manager,
            broadcaster,
        }
    }

    pub async fn run(&self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(addr).await?;
        log::info!("Gateway WebSocket server listening on {}", addr);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    log::info!("New WebSocket connection from {}", peer_addr);
                    let db = self.db.clone();
                    let channel_manager = self.channel_manager.clone();
                    let broadcaster = self.broadcaster.clone();

                    tokio::spawn(async move {
                        if let Err(e) =
                            handle_connection(stream, db, channel_manager, broadcaster).await
                        {
                            log::error!("Connection error from {}: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    log::error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
}

/// Parameters for the auth RPC method
#[derive(Debug, Deserialize)]
struct AuthParams {
    token: String,
}

async fn handle_connection(
    stream: TcpStream,
    db: Arc<Database>,
    channel_manager: Arc<ChannelManager>,
    broadcaster: Arc<EventBroadcaster>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = accept_async(stream).await?;
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Phase 1: Authentication required before full access
    let authenticated = match tokio::time::timeout(
        Duration::from_secs(AUTH_TIMEOUT_SECS),
        wait_for_auth(&mut ws_sender, &mut ws_receiver, &db),
    )
    .await
    {
        Ok(Ok(true)) => true,
        Ok(Ok(false)) => {
            log::warn!("Gateway client failed authentication");
            return Ok(());
        }
        Ok(Err(e)) => {
            log::error!("Gateway auth error: {}", e);
            return Ok(());
        }
        Err(_) => {
            log::warn!("Gateway client auth timeout after {}s", AUTH_TIMEOUT_SECS);
            let timeout_response = RpcResponse::error(
                "".to_string(),
                RpcError::new(-32000, "Authentication timeout".to_string()),
            );
            if let Ok(json) = serde_json::to_string(&timeout_response) {
                log::debug!("[DATAGRAM] >>> TO AGENT (auth timeout):\n{}", json);
                let _ = ws_sender.send(Message::Text(json)).await;
            }
            return Ok(());
        }
    };

    if !authenticated {
        return Ok(());
    }

    log::info!("Gateway client authenticated successfully");

    // Phase 2: Full access after authentication
    // Subscribe to events
    let (client_id, mut event_rx) = broadcaster.subscribe();

    // Create a channel for sending messages to the WebSocket
    let (tx, mut rx) = mpsc::channel::<Message>(100);

    // Task to forward messages to WebSocket
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Forward RPC responses
                Some(msg) = rx.recv() => {
                    if let Message::Text(ref text) = msg {
                        log::debug!("[DATAGRAM] >>> TO AGENT (RPC response):\n{}", text);
                    }
                    if ws_sender.send(msg).await.is_err() {
                        break;
                    }
                }
                // Forward events
                Some(event) = event_rx.recv() => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        log::debug!("[DATAGRAM] >>> TO AGENT (event: {}):\n{}", event.event, json);
                        if ws_sender.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                }
                else => break,
            }
        }
    });

    // Process incoming messages
    while let Some(msg_result) = ws_receiver.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                log::debug!("[DATAGRAM] <<< FROM AGENT (RPC request):\n{}", text);
                let response = process_request(&text, &db, &channel_manager, &broadcaster).await;
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = tx.send(Message::Text(json)).await;
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = tx.send(Message::Pong(data)).await;
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Err(e) => {
                log::error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    // Cleanup
    broadcaster.unsubscribe(&client_id);
    send_task.abort();

    Ok(())
}

/// Wait for authentication from the client
/// Client must send: {"jsonrpc":"2.0","id":"1","method":"auth","params":{"token":"..."}}
async fn wait_for_auth<S, R>(
    ws_sender: &mut S,
    ws_receiver: &mut R,
    db: &Arc<Database>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>>
where
    S: SinkExt<Message> + Unpin,
    R: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    while let Some(msg_result) = ws_receiver.next().await {
        match msg_result {
            Ok(Message::Text(text)) => {
                log::debug!("[DATAGRAM] <<< FROM AGENT (auth phase):\n{}", text);
                // Try to parse as RPC request
                let request: RpcRequest = match serde_json::from_str(&text) {
                    Ok(req) => req,
                    Err(_) => {
                        let response = RpcResponse::error("".to_string(), RpcError::parse_error());
                        if let Ok(json) = serde_json::to_string(&response) {
                            log::debug!("[DATAGRAM] >>> TO AGENT (parse error):\n{}", json);
                            let _ = ws_sender.send(Message::Text(json)).await;
                        }
                        continue;
                    }
                };

                // Only allow "auth" and "ping" methods before authentication
                match request.method.as_str() {
                    "auth" => {
                        let params: AuthParams = match serde_json::from_value(request.params.clone()) {
                            Ok(p) => p,
                            Err(e) => {
                                let response = RpcResponse::error(
                                    request.id.clone(),
                                    RpcError::invalid_params(format!("Missing or invalid token: {}", e)),
                                );
                                if let Ok(json) = serde_json::to_string(&response) {
                                    log::debug!("[DATAGRAM] >>> TO AGENT (invalid token params):\n{}", json);
                                    let _ = ws_sender.send(Message::Text(json)).await;
                                }
                                continue;
                            }
                        };

                        // Validate token against database
                        match db.validate_session(&params.token) {
                            Ok(Some(_session)) => {
                                // Valid session found
                                let response = RpcResponse::success(
                                    request.id,
                                    serde_json::json!({"authenticated": true}),
                                );
                                if let Ok(json) = serde_json::to_string(&response) {
                                    log::debug!("[DATAGRAM] >>> TO AGENT (auth success):\n{}", json);
                                    let _ = ws_sender.send(Message::Text(json)).await;
                                }
                                return Ok(true);
                            }
                            Ok(None) => {
                                // No valid session found (invalid or expired)
                                let response = RpcResponse::error(
                                    request.id,
                                    RpcError::new(-32001, "Invalid or expired token".to_string()),
                                );
                                if let Ok(json) = serde_json::to_string(&response) {
                                    log::debug!("[DATAGRAM] >>> TO AGENT (auth failed - invalid/expired):\n{}", json);
                                    let _ = ws_sender.send(Message::Text(json)).await;
                                }
                                return Ok(false);
                            }
                            Err(e) => {
                                log::error!("Database error validating token: {}", e);
                                let response = RpcResponse::error(
                                    request.id,
                                    RpcError::internal_error(format!("Database error: {}", e)),
                                );
                                if let Ok(json) = serde_json::to_string(&response) {
                                    log::debug!("[DATAGRAM] >>> TO AGENT (auth db error):\n{}", json);
                                    let _ = ws_sender.send(Message::Text(json)).await;
                                }
                                return Ok(false);
                            }
                        }
                    }
                    "ping" => {
                        // Allow ping before auth
                        let response = RpcResponse::success(request.id, serde_json::json!("pong"));
                        if let Ok(json) = serde_json::to_string(&response) {
                            log::debug!("[DATAGRAM] >>> TO AGENT (ping response):\n{}", json);
                            let _ = ws_sender.send(Message::Text(json)).await;
                        }
                    }
                    _ => {
                        // Reject other methods until authenticated
                        let response = RpcResponse::error(
                            request.id,
                            RpcError::new(-32002, "Authentication required. Call 'auth' method first.".to_string()),
                        );
                        if let Ok(json) = serde_json::to_string(&response) {
                            log::debug!("[DATAGRAM] >>> TO AGENT (auth required):\n{}", json);
                            let _ = ws_sender.send(Message::Text(json)).await;
                        }
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = ws_sender.send(Message::Pong(data)).await;
            }
            Ok(Message::Close(_)) => {
                return Ok(false);
            }
            Err(e) => {
                log::error!("WebSocket error during auth: {}", e);
                return Err(Box::new(e));
            }
            _ => {}
        }
    }

    Ok(false)
}

async fn process_request(
    text: &str,
    db: &Arc<Database>,
    channel_manager: &Arc<ChannelManager>,
    broadcaster: &Arc<EventBroadcaster>,
) -> RpcResponse {
    // Parse the request
    let request: RpcRequest = match serde_json::from_str(text) {
        Ok(req) => req,
        Err(_) => {
            return RpcResponse::error("".to_string(), RpcError::parse_error());
        }
    };

    let id = request.id.clone();

    // Dispatch to handler
    let result = dispatch_method(&request, db, channel_manager, broadcaster).await;

    match result {
        Ok(value) => RpcResponse::success(id, value),
        Err(error) => RpcResponse::error(id, error),
    }
}

async fn dispatch_method(
    request: &RpcRequest,
    db: &Arc<Database>,
    channel_manager: &Arc<ChannelManager>,
    broadcaster: &Arc<EventBroadcaster>,
) -> Result<serde_json::Value, RpcError> {
    match request.method.as_str() {
        "ping" => methods::handle_ping().await,
        "status" => methods::handle_status(broadcaster.clone()).await,
        "channels.status" => methods::handle_channels_status(db.clone(), channel_manager.clone()).await,
        "channels.start" => {
            let params: ChannelIdParams = serde_json::from_value(request.params.clone())
                .map_err(|e| RpcError::invalid_params(format!("Invalid params: {}", e)))?;
            methods::handle_channels_start(params, db.clone(), channel_manager.clone()).await
        }
        "channels.stop" => {
            let params: ChannelIdParams = serde_json::from_value(request.params.clone())
                .map_err(|e| RpcError::invalid_params(format!("Invalid params: {}", e)))?;
            methods::handle_channels_stop(params, channel_manager.clone(), db.clone()).await
        }
        "channels.restart" => {
            let params: ChannelIdParams = serde_json::from_value(request.params.clone())
                .map_err(|e| RpcError::invalid_params(format!("Invalid params: {}", e)))?;
            methods::handle_channels_restart(params, db.clone(), channel_manager.clone()).await
        }
        _ => Err(RpcError::method_not_found()),
    }
}
