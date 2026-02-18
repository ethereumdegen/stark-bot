//! Gmail PubSub webhook controller
//!
//! Handles incoming Pub/Sub push notifications from Gmail and processes emails.

use actix_web::{web, HttpRequest, HttpResponse};
use base64::Engine;
use std::sync::Arc;

use crate::channels::types::{DispatchResult, NormalizedMessage};
use crate::channels::MessageDispatcher;
use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::integrations::gmail::{
    GmailClient, GmailConfig, GmailConfigResponse, GmailNotificationData,
    ParsedEmail, PubSubPushNotification, SetupGmailRequest, UpdateGmailRequest,
};
use crate::AppState;

/// Configure Gmail routes
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/gmail")
            // Webhook endpoint (no auth - uses Pub/Sub verification)
            .route("/webhook", web::post().to(handle_pubsub_push))
            // Management endpoints (require auth)
            .route("/config", web::get().to(get_config))
            .route("/config", web::post().to(setup_gmail))
            .route("/config", web::put().to(update_config))
            .route("/config", web::delete().to(delete_config))
            .route("/watch/start", web::post().to(start_watch))
            .route("/watch/stop", web::post().to(stop_watch))
            .route("/test", web::post().to(test_connection)),
    );
}

/// Handle incoming Pub/Sub push notification
async fn handle_pubsub_push(
    state: web::Data<AppState>,
    body: web::Json<PubSubPushNotification>,
) -> HttpResponse {
    log::info!("[GMAIL] Received Pub/Sub push notification");

    // Decode the message data (base64)
    let engine = base64::engine::general_purpose::STANDARD;
    let decoded = match engine.decode(&body.message.data) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::error!("[GMAIL] Failed to decode Pub/Sub message: {}", e);
            return HttpResponse::BadRequest().json(serde_json::json!({
                "success": false,
                "error": "Invalid message encoding"
            }));
        }
    };

    // Parse the notification data
    let notification: GmailNotificationData = match serde_json::from_slice(&decoded) {
        Ok(n) => n,
        Err(e) => {
            log::error!("[GMAIL] Failed to parse notification data: {}", e);
            return HttpResponse::BadRequest().json(serde_json::json!({
                "success": false,
                "error": "Invalid notification format"
            }));
        }
    };

    log::info!(
        "[GMAIL] Notification for {} with history ID {}",
        notification.email_address,
        notification.history_id
    );

    // Get Gmail config for this email
    let config = match state.db.get_gmail_config_by_email(&notification.email_address) {
        Ok(Some(c)) if c.enabled => c,
        Ok(Some(_)) => {
            log::warn!("[GMAIL] Integration disabled for {}", notification.email_address);
            return HttpResponse::Ok().json(serde_json::json!({"success": true, "skipped": true}));
        }
        Ok(None) => {
            log::warn!("[GMAIL] No config found for {}", notification.email_address);
            return HttpResponse::Ok().json(serde_json::json!({"success": true, "skipped": true}));
        }
        Err(e) => {
            log::error!("[GMAIL] Database error: {}", e);
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": "Database error"
            }));
        }
    };

    // Process the notification in the background
    let db = state.db.clone();
    let broadcaster = state.gateway.broadcaster().clone();
    let dispatcher = state.dispatcher.clone();

    tokio::spawn(async move {
        if let Err(e) = process_gmail_notification(
            &db,
            &broadcaster,
            &dispatcher,
            &config,
            &notification,
        ).await {
            log::error!("[GMAIL] Failed to process notification: {}", e);
        }
    });

    // Return 200 OK immediately (Pub/Sub expects quick response)
    HttpResponse::Ok().json(serde_json::json!({"success": true}))
}

/// Process Gmail notification - fetch new emails and dispatch to agent
async fn process_gmail_notification(
    db: &Arc<Database>,
    broadcaster: &Arc<EventBroadcaster>,
    dispatcher: &Arc<MessageDispatcher>,
    config: &GmailConfig,
    notification: &GmailNotificationData,
) -> Result<(), String> {
    // Create Gmail client
    let client = GmailClient::new(
        config.access_token.clone(),
        config.refresh_token.clone(),
    );

    // Get history since last processed
    let start_history_id = config.history_id.as_deref()
        .unwrap_or(&notification.history_id);

    let labels: Vec<&str> = config.watch_labels.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let history = client.get_history(
        "me",
        start_history_id,
        if labels.is_empty() { None } else { Some(&labels) },
    ).await?;

    // Extract new message IDs
    let mut new_message_ids = Vec::new();
    if let Some(history_records) = history.history {
        for record in history_records {
            if let Some(messages_added) = record.messages_added {
                for msg in messages_added {
                    new_message_ids.push(msg.message.id);
                }
            }
        }
    }

    log::info!("[GMAIL] Found {} new messages", new_message_ids.len());

    // Process each new message
    for message_id in new_message_ids {
        match client.get_message("me", &message_id, Some("full")).await {
            Ok(message) => {
                let parsed = message.parse();
                log::info!(
                    "[GMAIL] Processing email from {} - Subject: {}",
                    parsed.from,
                    parsed.subject
                );

                // Dispatch to agent
                if let Err(e) = dispatch_email(
                    db,
                    broadcaster,
                    dispatcher,
                    config,
                    &parsed,
                    &client,
                ).await {
                    log::error!("[GMAIL] Failed to dispatch email {}: {}", message_id, e);
                }
            }
            Err(e) => {
                log::error!("[GMAIL] Failed to fetch message {}: {}", message_id, e);
            }
        }
    }

    // Update history ID
    if let Some(new_history_id) = history.history_id {
        if let Err(e) = db.update_gmail_history_id(config.id, &new_history_id) {
            log::error!("[GMAIL] Failed to update history ID: {}", e);
        }
    }

    Ok(())
}

/// Dispatch email to the agent for processing
async fn dispatch_email(
    db: &Arc<Database>,
    broadcaster: &Arc<EventBroadcaster>,
    dispatcher: &Arc<MessageDispatcher>,
    config: &GmailConfig,
    email: &ParsedEmail,
    client: &GmailClient,
) -> Result<DispatchResult, String> {
    // Build message content
    let message_content = format!(
        "New email received:\n\
        From: {}\n\
        Subject: {}\n\
        ---\n\
        {}",
        email.from,
        email.subject,
        if email.body.len() > 4000 {
            format!("{}...\n\n[Body truncated]", &email.body[..4000])
        } else {
            email.body.clone()
        }
    );

    // Create normalized message
    let normalized = NormalizedMessage {
        channel_id: config.response_channel_id.unwrap_or(0),
        channel_type: "gmail".to_string(),
        chat_id: email.thread_id.clone(),
        chat_name: None,
        user_id: email.from.clone(),
        user_name: extract_name_from_email(&email.from),
        text: message_content,
        message_id: Some(email.message_id.clone()),
        session_mode: None,
        selected_network: None,
        force_safe_mode: false,
        preferred_subtype: None,
    };

    // Broadcast event
    broadcaster.broadcast(GatewayEvent::custom(
        "gmail_received",
        serde_json::json!({
            "from": email.from,
            "subject": email.subject,
            "thread_id": email.thread_id,
        }),
    ));

    // Dispatch to agent
    let result = dispatcher.dispatch_safe(normalized).await;

    // If auto-reply is enabled and we got a successful response, send reply
    if config.auto_reply && result.error.is_none() && !result.response.is_empty() {
        let response_text = &result.response;
        log::info!("[GMAIL] Sending auto-reply to {}", email.from);

        // Extract reply-to address
        let reply_to = &email.from;
        let message_id_header = email.message_id.clone();

        match client.send_reply(
            "me",
            &email.thread_id,
            reply_to,
            &email.subject,
            response_text,
            Some(&format!("<{}>", message_id_header)),
        ).await {
            Ok(_) => {
                log::info!("[GMAIL] Auto-reply sent successfully");
                broadcaster.broadcast(GatewayEvent::custom(
                    "gmail_reply_sent",
                    serde_json::json!({
                        "to": reply_to,
                        "subject": email.subject,
                    }),
                ));
            }
            Err(e) => {
                log::error!("[GMAIL] Failed to send auto-reply: {}", e);
            }
        }
    }

    Ok(result)
}

/// Extract display name from email address
fn extract_name_from_email(email: &str) -> String {
    // Format: "Name <email@example.com>" or just "email@example.com"
    if let Some(start) = email.find('<') {
        email[..start].trim().trim_matches('"').to_string()
    } else {
        email.split('@').next().unwrap_or("Unknown").to_string()
    }
}

// === Management Endpoints ===

fn validate_session_from_request(
    state: &web::Data<AppState>,
    req: &HttpRequest,
) -> Result<(), HttpResponse> {
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return Err(HttpResponse::Unauthorized().json(GmailConfigResponse {
                success: false,
                config: None,
                error: Some("No authorization token provided".to_string()),
            }));
        }
    };

    match state.db.validate_session(&token) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HttpResponse::Unauthorized().json(GmailConfigResponse {
            success: false,
            config: None,
            error: Some("Invalid or expired session".to_string()),
        })),
        Err(e) => {
            log::error!("Failed to validate session: {}", e);
            Err(HttpResponse::InternalServerError().json(GmailConfigResponse {
                success: false,
                config: None,
                error: Some("Internal server error".to_string()),
            }))
        }
    }
}

/// Get Gmail configuration
async fn get_config(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    match state.db.get_gmail_config() {
        Ok(Some(config)) => HttpResponse::Ok().json(GmailConfigResponse {
            success: true,
            config: Some(config.into()),
            error: None,
        }),
        Ok(None) => HttpResponse::Ok().json(GmailConfigResponse {
            success: true,
            config: None,
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(GmailConfigResponse {
            success: false,
            config: None,
            error: Some(format!("Database error: {}", e)),
        }),
    }
}

/// Set up Gmail integration
async fn setup_gmail(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<SetupGmailRequest>,
) -> HttpResponse {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    // Validate the tokens by getting user profile
    let client = GmailClient::new(body.access_token.clone(), body.refresh_token.clone());

    match client.get_profile("me").await {
        Ok(profile) => {
            if profile.email_address != body.email {
                return HttpResponse::BadRequest().json(GmailConfigResponse {
                    success: false,
                    config: None,
                    error: Some(format!(
                        "Token email ({}) doesn't match provided email ({})",
                        profile.email_address, body.email
                    )),
                });
            }
        }
        Err(e) => {
            return HttpResponse::BadRequest().json(GmailConfigResponse {
                success: false,
                config: None,
                error: Some(format!("Invalid OAuth tokens: {}", e)),
            });
        }
    }

    // Store configuration
    match state.db.create_gmail_config(
        &body.email,
        &body.access_token,
        &body.refresh_token,
        &body.project_id,
        &body.topic_name,
        body.watch_labels.as_deref().unwrap_or("INBOX"),
        body.response_channel_id,
        body.auto_reply.unwrap_or(false),
    ) {
        Ok(config) => HttpResponse::Created().json(GmailConfigResponse {
            success: true,
            config: Some(config.into()),
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(GmailConfigResponse {
            success: false,
            config: None,
            error: Some(format!("Failed to save config: {}", e)),
        }),
    }
}

/// Update Gmail configuration
async fn update_config(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<UpdateGmailRequest>,
) -> HttpResponse {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    match state.db.update_gmail_config(
        body.watch_labels.as_deref(),
        body.response_channel_id,
        body.auto_reply,
        body.enabled,
    ) {
        Ok(config) => HttpResponse::Ok().json(GmailConfigResponse {
            success: true,
            config: Some(config.into()),
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(GmailConfigResponse {
            success: false,
            config: None,
            error: Some(format!("Failed to update config: {}", e)),
        }),
    }
}

/// Delete Gmail configuration
async fn delete_config(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    match state.db.delete_gmail_config() {
        Ok(true) => HttpResponse::Ok().json(GmailConfigResponse {
            success: true,
            config: None,
            error: None,
        }),
        Ok(false) => HttpResponse::NotFound().json(GmailConfigResponse {
            success: false,
            config: None,
            error: Some("No Gmail configuration found".to_string()),
        }),
        Err(e) => HttpResponse::InternalServerError().json(GmailConfigResponse {
            success: false,
            config: None,
            error: Some(format!("Failed to delete config: {}", e)),
        }),
    }
}

/// Start Gmail watch
async fn start_watch(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let config = match state.db.get_gmail_config() {
        Ok(Some(c)) => c,
        Ok(None) => {
            return HttpResponse::NotFound().json(GmailConfigResponse {
                success: false,
                config: None,
                error: Some("No Gmail configuration found. Set up Gmail first.".to_string()),
            });
        }
        Err(e) => {
            return HttpResponse::InternalServerError().json(GmailConfigResponse {
                success: false,
                config: None,
                error: Some(format!("Database error: {}", e)),
            });
        }
    };

    let client = GmailClient::new(config.access_token.clone(), config.refresh_token.clone());

    // Build topic name
    let topic_name = format!("projects/{}/topics/{}", config.project_id, config.topic_name);

    let labels: Vec<&str> = config.watch_labels.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    match client.setup_watch("me", &topic_name, &labels).await {
        Ok(watch_response) => {
            // Update config with watch expiration and history ID
            let expiration = chrono::DateTime::parse_from_rfc3339(&watch_response.expiration)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc));

            if let Err(e) = state.db.update_gmail_watch(
                config.id,
                expiration,
                Some(&watch_response.history_id),
            ) {
                log::error!("[GMAIL] Failed to update watch info: {}", e);
            }

            HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "history_id": watch_response.history_id,
                "expiration": watch_response.expiration
            }))
        }
        Err(e) => HttpResponse::InternalServerError().json(GmailConfigResponse {
            success: false,
            config: None,
            error: Some(format!("Failed to start watch: {}", e)),
        }),
    }
}

/// Stop Gmail watch
async fn stop_watch(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let config = match state.db.get_gmail_config() {
        Ok(Some(c)) => c,
        Ok(None) => {
            return HttpResponse::NotFound().json(GmailConfigResponse {
                success: false,
                config: None,
                error: Some("No Gmail configuration found".to_string()),
            });
        }
        Err(e) => {
            return HttpResponse::InternalServerError().json(GmailConfigResponse {
                success: false,
                config: None,
                error: Some(format!("Database error: {}", e)),
            });
        }
    };

    let client = GmailClient::new(config.access_token.clone(), config.refresh_token.clone());

    match client.stop_watch("me").await {
        Ok(()) => {
            // Clear watch expiration
            if let Err(e) = state.db.update_gmail_watch(config.id, None, None) {
                log::error!("[GMAIL] Failed to clear watch info: {}", e);
            }

            HttpResponse::Ok().json(serde_json::json!({"success": true}))
        }
        Err(e) => HttpResponse::InternalServerError().json(GmailConfigResponse {
            success: false,
            config: None,
            error: Some(format!("Failed to stop watch: {}", e)),
        }),
    }
}

/// Test Gmail connection
async fn test_connection(state: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let config = match state.db.get_gmail_config() {
        Ok(Some(c)) => c,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({
                "success": false,
                "error": "No Gmail configuration found"
            }));
        }
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "success": false,
                "error": format!("Database error: {}", e)
            }));
        }
    };

    let client = GmailClient::new(config.access_token.clone(), config.refresh_token.clone());

    match client.get_profile("me").await {
        Ok(profile) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "email": profile.email_address,
            "messages_total": profile.messages_total,
            "threads_total": profile.threads_total
        })),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "success": false,
            "error": format!("Connection failed: {}", e)
        })),
    }
}
