use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;

use crate::models::SpecialRole;
use crate::AppState;

const MAX_SPECIAL_ROLES: usize = 10;
const MAX_SPECIAL_ROLE_ASSIGNMENTS: usize = 100;

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

// --- Roles ---

async fn list_roles(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }
    match data.db.list_special_roles() {
        Ok(roles) => HttpResponse::Ok().json(roles),
        Err(e) => {
            log::error!("Failed to list special roles: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

async fn get_role(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }
    let name = path.into_inner();
    match data.db.get_special_role(&name) {
        Ok(Some(role)) => HttpResponse::Ok().json(role),
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Special role '{}' not found", name)
        })),
        Err(e) => {
            log::error!("Failed to get special role: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

#[derive(Deserialize)]
struct CreateRoleRequest {
    name: String,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    allowed_skills: Vec<String>,
    #[serde(default)]
    description: Option<String>,
}

async fn create_role(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateRoleRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    // Check limit (only for genuinely new roles, not updates)
    let name = body.name.trim().to_lowercase();
    if let Ok(None) = data.db.get_special_role(&name) {
        match data.db.count_special_roles() {
            Ok(count) if count >= MAX_SPECIAL_ROLES as i64 => {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!("Maximum of {} special roles allowed", MAX_SPECIAL_ROLES)
                }));
            }
            Err(e) => {
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": format!("Database error: {}", e)
                }));
            }
            _ => {}
        }
    }

    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Name must be non-empty and contain only alphanumeric characters and underscores"
        }));
    }

    let role = SpecialRole {
        name,
        allowed_tools: body.allowed_tools.clone(),
        allowed_skills: body.allowed_skills.clone(),
        description: body.description.clone(),
        created_at: String::new(),
        updated_at: String::new(),
    };

    match data.db.upsert_special_role(&role) {
        Ok(_) => {
            // Re-fetch to get timestamps
            match data.db.get_special_role(&role.name) {
                Ok(Some(created)) => HttpResponse::Created().json(created),
                _ => HttpResponse::Created().json(role),
            }
        }
        Err(e) => {
            log::error!("Failed to create special role: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

#[derive(Deserialize)]
struct UpdateRoleRequest {
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    allowed_skills: Option<Vec<String>>,
    #[serde(default)]
    description: Option<Option<String>>,
}

async fn update_role(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<UpdateRoleRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let name = path.into_inner();
    let existing = match data.db.get_special_role(&name) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({
                "error": format!("Special role '{}' not found", name)
            }));
        }
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }));
        }
    };

    let updated = SpecialRole {
        name: existing.name,
        allowed_tools: body.allowed_tools.clone().unwrap_or(existing.allowed_tools),
        allowed_skills: body.allowed_skills.clone().unwrap_or(existing.allowed_skills),
        description: body.description.clone().unwrap_or(existing.description),
        created_at: existing.created_at,
        updated_at: existing.updated_at,
    };

    match data.db.upsert_special_role(&updated) {
        Ok(_) => {
            match data.db.get_special_role(&updated.name) {
                Ok(Some(refreshed)) => HttpResponse::Ok().json(refreshed),
                _ => HttpResponse::Ok().json(updated),
            }
        }
        Err(e) => {
            log::error!("Failed to update special role: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

async fn delete_role(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let name = path.into_inner();
    match data.db.delete_special_role(&name) {
        Ok(true) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "message": format!("Special role '{}' deleted", name)
        })),
        Ok(false) => HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Special role '{}' not found", name)
        })),
        Err(e) => {
            log::error!("Failed to delete special role: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

// --- Assignments ---

#[derive(Deserialize)]
struct AssignmentQuery {
    #[serde(default)]
    role_name: Option<String>,
}

async fn list_assignments(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<AssignmentQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }
    match data.db.list_special_role_assignments(query.role_name.as_deref()) {
        Ok(assignments) => HttpResponse::Ok().json(assignments),
        Err(e) => {
            log::error!("Failed to list special role assignments: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

#[derive(Deserialize)]
struct CreateAssignmentRequest {
    channel_type: String,
    user_id: String,
    special_role_name: String,
}

async fn create_assignment(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateAssignmentRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    // Check assignment limit
    match data.db.count_special_role_assignments() {
        Ok(count) if count >= MAX_SPECIAL_ROLE_ASSIGNMENTS as i64 => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Maximum of {} special role assignments allowed", MAX_SPECIAL_ROLE_ASSIGNMENTS)
            }));
        }
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }));
        }
        _ => {}
    }

    // Verify role exists
    match data.db.get_special_role(&body.special_role_name) {
        Ok(None) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Special role '{}' does not exist", body.special_role_name)
            }));
        }
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }));
        }
        Ok(Some(_)) => {}
    }

    match data.db.create_special_role_assignment(&body.channel_type, &body.user_id, &body.special_role_name) {
        Ok(assignment) => HttpResponse::Created().json(assignment),
        Err(e) => {
            log::error!("Failed to create special role assignment: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

async fn delete_assignment(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let id = path.into_inner();
    match data.db.delete_special_role_assignment(id) {
        Ok(true) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "message": format!("Assignment #{} deleted", id)
        })),
        Ok(false) => HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Assignment #{} not found", id)
        })),
        Err(e) => {
            log::error!("Failed to delete special role assignment: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

// --- Grants lookup (for debugging) ---

#[derive(Deserialize)]
struct GrantsQuery {
    channel_type: String,
    user_id: String,
}

async fn get_grants(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<GrantsQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }
    match data.db.get_special_role_grants(&query.channel_type, &query.user_id) {
        Ok(grants) => HttpResponse::Ok().json(grants),
        Err(e) => {
            log::error!("Failed to get special role grants: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/special-roles")
            .route("", web::get().to(list_roles))
            .route("", web::post().to(create_role))
            .route("/assignments", web::get().to(list_assignments))
            .route("/assignments", web::post().to(create_assignment))
            .route("/assignments/{id}", web::delete().to(delete_assignment))
            .route("/grants", web::get().to(get_grants))
            .route("/{name}", web::get().to(get_role))
            .route("/{name}", web::put().to(update_role))
            .route("/{name}", web::delete().to(delete_role)),
    );
}
