use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;

use crate::db::tables::impulse_nodes::{CreateImpulseNodeRequest, UpdateImpulseNodeRequest};
use crate::AppState;

/// Validate session token from request
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
            return Err(HttpResponse::Unauthorized().json(serde_json::json!({
                "error": "No authorization token provided"
            })));
        }
    };

    match state.db.validate_session(&token) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Invalid or expired session"
        }))),
        Err(e) => {
            log::error!("Session validation error: {}", e);
            Err(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Internal server error"
            })))
        }
    }
}

/// Get the full impulse map graph (nodes + connections)
async fn get_graph(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    match data.db.get_impulse_graph() {
        Ok(graph) => HttpResponse::Ok().json(graph),
        Err(e) => {
            log::error!("Failed to get impulse graph: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get the full impulse map graph for guest users (no auth required, feature flag controlled)
async fn get_graph_guest(data: web::Data<AppState>) -> impl Responder {
    let guest_enabled = data.db.get_bot_settings().map(|s| s.guest_dashboard_enabled).unwrap_or(false);
    if !guest_enabled {
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "Guest dashboard is not enabled"
        }));
    }

    match data.db.get_impulse_graph() {
        Ok(graph) => HttpResponse::Ok().json(graph),
        Err(e) => {
            log::error!("Failed to get impulse graph for guest: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// List all nodes
async fn list_nodes(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    match data.db.list_impulse_nodes() {
        Ok(nodes) => HttpResponse::Ok().json(nodes),
        Err(e) => {
            log::error!("Failed to list impulse nodes: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Create a new node
async fn create_node(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateImpulseNodeRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    match data.db.create_impulse_node(&body.into_inner()) {
        Ok(node) => HttpResponse::Created().json(node),
        Err(e) => {
            log::error!("Failed to create impulse node: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get a single node
async fn get_node(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let node_id = path.into_inner();

    match data.db.get_impulse_node(node_id) {
        Ok(Some(node)) => HttpResponse::Ok().json(node),
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Node not found"
        })),
        Err(e) => {
            log::error!("Failed to get impulse node: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Update a node
async fn update_node(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<i64>,
    body: web::Json<UpdateImpulseNodeRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let node_id = path.into_inner();

    match data.db.update_impulse_node(node_id, &body.into_inner()) {
        Ok(Some(node)) => HttpResponse::Ok().json(node),
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Node not found"
        })),
        Err(e) => {
            log::error!("Failed to update impulse node: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Delete a node
async fn delete_node(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let node_id = path.into_inner();

    match data.db.delete_impulse_node(node_id) {
        Ok(true) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "message": "Node deleted"
        })),
        Ok(false) => HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Cannot delete trunk node or node not found"
        })),
        Err(e) => {
            log::error!("Failed to delete impulse node: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// List all connections
async fn list_connections(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    match data.db.list_impulse_node_connections() {
        Ok(connections) => HttpResponse::Ok().json(connections),
        Err(e) => {
            log::error!("Failed to list impulse node connections: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Create a connection
#[derive(Deserialize)]
struct CreateConnectionRequest {
    parent_id: i64,
    child_id: i64,
}

async fn create_connection(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateConnectionRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    match data.db.create_impulse_node_connection(body.parent_id, body.child_id) {
        Ok(connection) => HttpResponse::Created().json(connection),
        Err(e) => {
            log::error!("Failed to create impulse node connection: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Delete a connection
async fn delete_connection(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<(i64, i64)>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let (parent_id, child_id) = path.into_inner();

    match data.db.delete_impulse_node_connection(parent_id, child_id) {
        Ok(true) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "message": "Connection deleted"
        })),
        Ok(false) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Connection not found"
        })),
        Err(e) => {
            log::error!("Failed to delete impulse node connection: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Heartbeat session info for the sidebar
#[derive(serde::Serialize)]
struct HeartbeatSessionInfo {
    id: i64,
    impulse_node_id: Option<i64>,
    created_at: String,
    message_count: i64,
}

/// List recent heartbeat sessions with their associated impulse nodes
async fn list_heartbeat_sessions(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    match data.db.list_heartbeat_sessions(50) {
        Ok(sessions) => {
            let results: Vec<HeartbeatSessionInfo> = sessions
                .into_iter()
                .map(|(session, node_id)| {
                    let message_count = data.db.count_session_messages(session.id).unwrap_or(0);
                    HeartbeatSessionInfo {
                        id: session.id,
                        impulse_node_id: node_id,
                        created_at: session.created_at.to_rfc3339(),
                        message_count,
                    }
                })
                .collect();
            HttpResponse::Ok().json(results)
        }
        Err(e) => {
            log::error!("Failed to list heartbeat sessions: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/impulse-map")
            .route("/graph", web::get().to(get_graph))
            .route("/graph/guest", web::get().to(get_graph_guest))
            .route("/nodes", web::get().to(list_nodes))
            .route("/nodes", web::post().to(create_node))
            .route("/nodes/{id}", web::get().to(get_node))
            .route("/nodes/{id}", web::put().to(update_node))
            .route("/nodes/{id}", web::delete().to(delete_node))
            .route("/connections", web::get().to(list_connections))
            .route("/heartbeat-sessions", web::get().to(list_heartbeat_sessions))
            .route("/connections", web::post().to(create_connection))
            .route("/connections/{parent_id}/{child_id}", web::delete().to(delete_connection)),
    );
}
