use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::models::{
    GetOrCreateIdentityRequest, IdentityResponse, LinkIdentityRequest, LinkedAccountInfo,
};
use crate::AppState;

/// Get or create an identity for a platform user
async fn get_or_create_identity(
    data: web::Data<AppState>,
    body: web::Json<GetOrCreateIdentityRequest>,
) -> impl Responder {
    match data.db.get_or_create_identity(
        &body.channel_type,
        &body.platform_user_id,
        body.platform_user_name.as_deref(),
    ) {
        Ok(link) => {
            // Get all linked accounts for this identity
            let linked_accounts = match data.db.get_linked_identities(&link.identity_id) {
                Ok(links) => links.iter().map(LinkedAccountInfo::from).collect(),
                Err(_) => vec![LinkedAccountInfo::from(&link)],
            };

            HttpResponse::Ok().json(IdentityResponse {
                identity_id: link.identity_id,
                linked_accounts,
                created_at: link.created_at,
            })
        }
        Err(e) => {
            log::error!("Failed to get or create identity: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get identity by platform credentials
#[derive(Deserialize)]
struct GetIdentityQuery {
    channel_type: String,
    platform_user_id: String,
}

async fn get_identity(
    data: web::Data<AppState>,
    query: web::Query<GetIdentityQuery>,
) -> impl Responder {
    match data
        .db
        .get_identity_by_platform(&query.channel_type, &query.platform_user_id)
    {
        Ok(Some(link)) => {
            // Get all linked accounts for this identity
            let linked_accounts = match data.db.get_linked_identities(&link.identity_id) {
                Ok(links) => links.iter().map(LinkedAccountInfo::from).collect(),
                Err(_) => vec![LinkedAccountInfo::from(&link)],
            };

            HttpResponse::Ok().json(IdentityResponse {
                identity_id: link.identity_id,
                linked_accounts,
                created_at: link.created_at,
            })
        }
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Identity not found"
        })),
        Err(e) => {
            log::error!("Failed to get identity: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Link an existing identity to another platform
async fn link_identity(
    data: web::Data<AppState>,
    body: web::Json<LinkIdentityRequest>,
) -> impl Responder {
    // First check if this platform/user already has an identity
    if let Ok(Some(_)) = data
        .db
        .get_identity_by_platform(&body.channel_type, &body.platform_user_id)
    {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": "This platform user is already linked to an identity"
        }));
    }

    match data.db.link_identity(
        &body.identity_id,
        &body.channel_type,
        &body.platform_user_id,
        body.platform_user_name.as_deref(),
    ) {
        Ok(link) => {
            // Get all linked accounts for this identity
            let linked_accounts = match data.db.get_linked_identities(&link.identity_id) {
                Ok(links) => links.iter().map(LinkedAccountInfo::from).collect(),
                Err(_) => vec![LinkedAccountInfo::from(&link)],
            };

            HttpResponse::Created().json(IdentityResponse {
                identity_id: link.identity_id,
                linked_accounts,
                created_at: link.created_at,
            })
        }
        Err(e) => {
            log::error!("Failed to link identity: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

/// Get all linked identities for a given identity_id
async fn get_linked_identities(
    data: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    let identity_id = path.into_inner();

    match data.db.get_linked_identities(&identity_id) {
        Ok(links) if !links.is_empty() => {
            let linked_accounts: Vec<LinkedAccountInfo> =
                links.iter().map(LinkedAccountInfo::from).collect();
            let created_at = links.first().map(|l| l.created_at).unwrap();

            HttpResponse::Ok().json(IdentityResponse {
                identity_id,
                linked_accounts,
                created_at,
            })
        }
        Ok(_) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Identity not found"
        })),
        Err(e) => {
            log::error!("Failed to get linked identities: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Database error: {}", e)
            }))
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/identity")
            .route("", web::post().to(get_or_create_identity))
            .route("", web::get().to(get_identity))
            .route("/link", web::post().to(link_identity))
            .route("/{identity_id}", web::get().to(get_linked_identities)),
    );
}
