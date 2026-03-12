//! Actix-Web WebSocket handler for event broadcasting and tx queue management.

use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::{RpcError, RpcRequest, RpcResponse};
use crate::tx_queue::TxQueueManager;
use crate::wallet::WalletProvider;
use actix_web::{web, HttpRequest, HttpResponse};
use actix_ws::AggregatedMessage;
use futures_util::StreamExt;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const AUTH_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Deserialize)]
struct AuthParams {
    token: String,
}

#[derive(Debug, Deserialize)]
struct TxQueueParams {
    uuid: String,
}

pub async fn ws_handler(
    req: HttpRequest,
    stream: web::Payload,
    db: web::Data<Arc<Database>>,
    broadcaster: web::Data<Arc<EventBroadcaster>>,
    tx_queue: web::Data<Arc<TxQueueManager>>,
    wallet_provider: web::Data<Option<Arc<dyn WalletProvider>>>,
) -> Result<HttpResponse, actix_web::Error> {
    let (response, session, msg_stream) = actix_ws::handle(&req, stream)?;

    let db = db.get_ref().clone();
    let broadcaster = broadcaster.get_ref().clone();
    let tx_queue = tx_queue.get_ref().clone();
    let wallet_provider = wallet_provider.get_ref().clone();

    actix_web::rt::spawn(handle_ws_connection(
        session, msg_stream, db, broadcaster, tx_queue, wallet_provider,
    ));

    Ok(response)
}

async fn handle_ws_connection(
    mut session: actix_ws::Session,
    msg_stream: actix_ws::MessageStream,
    db: Arc<Database>,
    broadcaster: Arc<EventBroadcaster>,
    tx_queue: Arc<TxQueueManager>,
    wallet_provider: Option<Arc<dyn WalletProvider>>,
) {
    log::info!("New WebSocket connection");

    let mut msg_stream = msg_stream
        .aggregate_continuations()
        .max_continuation_size(64 * 1024);

    // Phase 1: Authentication
    let authenticated = match tokio::time::timeout(
        Duration::from_secs(AUTH_TIMEOUT_SECS),
        wait_for_auth(&mut session, &mut msg_stream, &db),
    ).await {
        Ok(Ok(true)) => true,
        Ok(Ok(false)) => {
            log::warn!("WebSocket client failed authentication");
            let _ = session.close(None).await;
            return;
        }
        Ok(Err(e)) => {
            log::error!("WebSocket auth error: {}", e);
            let _ = session.close(None).await;
            return;
        }
        Err(_) => {
            log::warn!("WebSocket auth timeout after {}s", AUTH_TIMEOUT_SECS);
            let response = RpcResponse::error("".to_string(), RpcError::new(-32000, "Authentication timeout".to_string()));
            if let Ok(json) = serde_json::to_string(&response) {
                let _ = session.text(json).await;
            }
            let _ = session.close(None).await;
            return;
        }
    };

    if !authenticated {
        let _ = session.close(None).await;
        return;
    }

    log::info!("WebSocket client authenticated");

    // Subscribe to events
    let (client_id, mut event_rx) = broadcaster.subscribe();

    // Replay recent events
    let recent_events = broadcaster.get_recent_events();
    if !recent_events.is_empty() {
        for event in recent_events {
            if let Ok(json) = serde_json::to_string(&event) {
                if session.text(json).await.is_err() {
                    broadcaster.unsubscribe(&client_id);
                    let _ = session.close(None).await;
                    return;
                }
            }
        }
    }

    let (tx, mut rx) = mpsc::channel::<String>(100);
    let mut send_session = session.clone();
    let client_id_clone = client_id.clone();

    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(msg) = rx.recv() => {
                    if send_session.text(msg).await.is_err() { break; }
                }
                Some(event) = event_rx.recv() => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        if send_session.text(json).await.is_err() { break; }
                    }
                }
                else => break,
            }
        }
        log::debug!("WebSocket send task ended for client {}", client_id_clone);
    });

    // Process incoming messages
    while let Some(msg_result) = msg_stream.next().await {
        match msg_result {
            Ok(AggregatedMessage::Text(text)) => {
                let response = process_request(&text, &tx_queue, &broadcaster, &wallet_provider).await;
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = tx.send(json).await;
                }
            }
            Ok(AggregatedMessage::Ping(data)) => {
                if session.pong(&data).await.is_err() { break; }
            }
            Ok(AggregatedMessage::Close(_)) => break,
            Err(e) => {
                log::error!("WebSocket error: {:?}", e);
                break;
            }
            _ => {}
        }
    }

    broadcaster.unsubscribe(&client_id);
    send_task.abort();
    let _ = session.close(None).await;
    log::info!("WebSocket client {} disconnected", client_id);
}

async fn wait_for_auth(
    session: &mut actix_ws::Session,
    msg_stream: &mut (impl StreamExt<Item = Result<AggregatedMessage, actix_ws::ProtocolError>> + Unpin),
    db: &Arc<Database>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    while let Some(msg_result) = msg_stream.next().await {
        match msg_result {
            Ok(AggregatedMessage::Text(text)) => {
                let request: RpcRequest = match serde_json::from_str(&text) {
                    Ok(req) => req,
                    Err(_) => {
                        let response = RpcResponse::error("".to_string(), RpcError::parse_error());
                        if let Ok(json) = serde_json::to_string(&response) {
                            let _ = session.text(json).await;
                        }
                        continue;
                    }
                };

                match request.method.as_str() {
                    "auth" => {
                        let params: AuthParams = match serde_json::from_value(request.params.clone()) {
                            Ok(p) => p,
                            Err(e) => {
                                let response = RpcResponse::error(request.id, RpcError::invalid_params(format!("Invalid token: {}", e)));
                                if let Ok(json) = serde_json::to_string(&response) {
                                    let _ = session.text(json).await;
                                }
                                continue;
                            }
                        };

                        match db.validate_session(&params.token) {
                            Ok(Some(_)) => {
                                let response = RpcResponse::success(request.id, serde_json::json!({"authenticated": true}));
                                if let Ok(json) = serde_json::to_string(&response) {
                                    let _ = session.text(json).await;
                                }
                                return Ok(true);
                            }
                            Ok(None) => {
                                let response = RpcResponse::error(request.id, RpcError::new(-32001, "Invalid or expired token".to_string()));
                                if let Ok(json) = serde_json::to_string(&response) {
                                    let _ = session.text(json).await;
                                }
                                return Ok(false);
                            }
                            Err(e) => {
                                let response = RpcResponse::error(request.id, RpcError::internal_error(format!("Database error: {}", e)));
                                if let Ok(json) = serde_json::to_string(&response) {
                                    let _ = session.text(json).await;
                                }
                                return Ok(false);
                            }
                        }
                    }
                    "ping" => {
                        let response = RpcResponse::success(request.id, serde_json::json!("pong"));
                        if let Ok(json) = serde_json::to_string(&response) {
                            let _ = session.text(json).await;
                        }
                    }
                    _ => {
                        let response = RpcResponse::error(request.id, RpcError::new(-32002, "Authentication required".to_string()));
                        if let Ok(json) = serde_json::to_string(&response) {
                            let _ = session.text(json).await;
                        }
                    }
                }
            }
            Ok(AggregatedMessage::Ping(data)) => { let _ = session.pong(&data).await; }
            Ok(AggregatedMessage::Close(_)) => return Ok(false),
            Err(e) => return Err(format!("WebSocket error: {:?}", e).into()),
            _ => {}
        }
    }
    Ok(false)
}

async fn process_request(
    text: &str,
    tx_queue: &Arc<TxQueueManager>,
    broadcaster: &Arc<EventBroadcaster>,
    wallet_provider: &Option<Arc<dyn WalletProvider>>,
) -> RpcResponse {
    let request: RpcRequest = match serde_json::from_str(text) {
        Ok(req) => req,
        Err(_) => return RpcResponse::error("".to_string(), RpcError::parse_error()),
    };

    let id = request.id.clone();

    match request.method.as_str() {
        "ping" => RpcResponse::success(id, serde_json::json!("pong")),
        "status" => RpcResponse::success(id, serde_json::json!({
            "connected_clients": broadcaster.client_count(),
        })),
        "tx_queue.confirm" => {
            let params: TxQueueParams = match serde_json::from_value(request.params.clone()) {
                Ok(p) => p,
                Err(e) => return RpcResponse::error(id, RpcError::invalid_params(format!("Invalid params: {}", e))),
            };
            match handle_tx_confirm(&params.uuid, tx_queue, broadcaster, wallet_provider).await {
                Ok(val) => RpcResponse::success(id, val),
                Err(e) => RpcResponse::error(id, e),
            }
        }
        "tx_queue.deny" => {
            let params: TxQueueParams = match serde_json::from_value(request.params.clone()) {
                Ok(p) => p,
                Err(e) => return RpcResponse::error(id, RpcError::invalid_params(format!("Invalid params: {}", e))),
            };
            match handle_tx_deny(&params.uuid, tx_queue, broadcaster).await {
                Ok(val) => RpcResponse::success(id, val),
                Err(e) => RpcResponse::error(id, e),
            }
        }
        _ => RpcResponse::error(id, RpcError::method_not_found()),
    }
}

async fn handle_tx_confirm(
    uuid: &str,
    tx_queue: &Arc<TxQueueManager>,
    broadcaster: &Arc<EventBroadcaster>,
    wallet_provider: &Option<Arc<dyn WalletProvider>>,
) -> Result<serde_json::Value, RpcError> {
    let wp = wallet_provider.as_ref()
        .ok_or_else(|| RpcError::new(-32000, "No wallet configured".to_string()))?;

    let entry = tx_queue.get(uuid)
        .ok_or_else(|| RpcError::new(-32000, format!("Transaction {} not found in queue", uuid)))?;

    let rpc = crate::x402::X402EvmRpc::new_with_wallet_provider(
        wp.clone(), &entry.network, None, false,
    ).map_err(|e| RpcError::internal_error(e))?;

    let signed_bytes = hex::decode(entry.signed_tx_hex.trim_start_matches("0x"))
        .map_err(|e| RpcError::internal_error(format!("Invalid signed tx hex: {}", e)))?;
    let _tx_hash = rpc.send_raw_transaction(&signed_bytes).await
        .map_err(|e| RpcError::internal_error(e))?;

    tx_queue.remove(uuid);

    let tx_hash_str = format!("{:?}", _tx_hash);
    broadcaster.broadcast(crate::gateway::protocol::GatewayEvent::new(
        "tx_queue.confirmed",
        serde_json::json!({ "uuid": uuid, "tx_hash": tx_hash_str }),
    ));

    Ok(serde_json::json!({ "uuid": uuid, "tx_hash": tx_hash_str }))
}

async fn handle_tx_deny(
    uuid: &str,
    tx_queue: &Arc<TxQueueManager>,
    broadcaster: &Arc<EventBroadcaster>,
) -> Result<serde_json::Value, RpcError> {
    tx_queue.remove(uuid);

    broadcaster.broadcast(crate::gateway::protocol::GatewayEvent::new(
        "tx_queue.denied",
        serde_json::json!({ "uuid": uuid }),
    ));

    Ok(serde_json::json!({ "uuid": uuid, "status": "denied" }))
}
