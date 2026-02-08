use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

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

/// Intrinsic file definition
#[derive(Clone)]
struct IntrinsicFile {
    name: &'static str,
    description: &'static str,
    writable: bool,
}

/// Resolve the absolute path for an intrinsic file by name
fn intrinsic_path(name: &str) -> Option<PathBuf> {
    match name {
        "soul.md" => Some(crate::config::soul_document_path()),
        "guidelines.md" => Some(crate::config::guidelines_document_path()),
        "assistant.md" => Some(crate::config::backend_dir().join("src/ai/multi_agent/prompts/assistant.md")),
        _ => None,
    }
}

/// List of intrinsic files
const INTRINSIC_FILES: &[IntrinsicFile] = &[
    IntrinsicFile {
        name: "soul.md",
        description: "Agent personality and identity",
        writable: true,
    },
    IntrinsicFile {
        name: "guidelines.md",
        description: "Operational and business guidelines",
        writable: true,
    },
    IntrinsicFile {
        name: "assistant.md",
        description: "System instructions (read-only)",
        writable: false,
    },
];

#[derive(Debug, Serialize)]
struct IntrinsicFileInfo {
    name: String,
    description: String,
    writable: bool,
}

#[derive(Debug, Serialize)]
struct ListIntrinsicResponse {
    success: bool,
    files: Vec<IntrinsicFileInfo>,
}

/// List all intrinsic files
async fn list_intrinsic(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let files: Vec<IntrinsicFileInfo> = INTRINSIC_FILES
        .iter()
        .map(|f| IntrinsicFileInfo {
            name: f.name.to_string(),
            description: f.description.to_string(),
            writable: f.writable,
        })
        .collect();

    HttpResponse::Ok().json(ListIntrinsicResponse {
        success: true,
        files,
    })
}

#[derive(Debug, Serialize)]
struct ReadIntrinsicResponse {
    success: bool,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    writable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Read an intrinsic file by name
async fn read_intrinsic(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let name = path.into_inner();

    // Find the intrinsic file
    let intrinsic = INTRINSIC_FILES.iter().find(|f| f.name == name);
    let intrinsic = match intrinsic {
        Some(i) => i,
        None => {
            return HttpResponse::NotFound().json(ReadIntrinsicResponse {
                success: false,
                name,
                content: None,
                writable: false,
                error: Some("Intrinsic file not found".to_string()),
            });
        }
    };

    let full_path = match intrinsic_path(intrinsic.name) {
        Some(p) => p,
        None => {
            return HttpResponse::InternalServerError().json(ReadIntrinsicResponse {
                success: false,
                name: intrinsic.name.to_string(),
                content: None,
                writable: intrinsic.writable,
                error: Some("Could not resolve file path".to_string()),
            });
        }
    };

    // Read the file
    let content = match fs::read_to_string(&full_path).await {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to read intrinsic file {}: {}", intrinsic.name, e);
            return HttpResponse::InternalServerError().json(ReadIntrinsicResponse {
                success: false,
                name: intrinsic.name.to_string(),
                content: None,
                writable: intrinsic.writable,
                error: Some(format!("Failed to read file: {}", e)),
            });
        }
    };

    HttpResponse::Ok().json(ReadIntrinsicResponse {
        success: true,
        name: intrinsic.name.to_string(),
        content: Some(content),
        writable: intrinsic.writable,
        error: None,
    })
}

#[derive(Debug, Deserialize)]
struct WriteIntrinsicRequest {
    content: String,
}

#[derive(Debug, Serialize)]
struct WriteIntrinsicResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Write to an intrinsic file (only writable ones)
async fn write_intrinsic(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<WriteIntrinsicRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let name = path.into_inner();

    // Find the intrinsic file
    let intrinsic = INTRINSIC_FILES.iter().find(|f| f.name == name);
    let intrinsic = match intrinsic {
        Some(i) => i,
        None => {
            return HttpResponse::NotFound().json(WriteIntrinsicResponse {
                success: false,
                error: Some("Intrinsic file not found".to_string()),
            });
        }
    };

    // Check if writable
    if !intrinsic.writable {
        return HttpResponse::Forbidden().json(WriteIntrinsicResponse {
            success: false,
            error: Some("This file is read-only".to_string()),
        });
    }

    let full_path = match intrinsic_path(intrinsic.name) {
        Some(p) => p,
        None => {
            return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                success: false,
                error: Some("Could not resolve file path".to_string()),
            });
        }
    };

    // Write the file
    if let Err(e) = fs::write(&full_path, &body.content).await {
        log::error!("Failed to write intrinsic file {}: {}", intrinsic.name, e);
        return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
            success: false,
            error: Some(format!("Failed to write file: {}", e)),
        });
    }

    log::info!("Updated intrinsic file: {}", intrinsic.name);

    HttpResponse::Ok().json(WriteIntrinsicResponse {
        success: true,
        error: None,
    })
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/intrinsic")
            .route("", web::get().to(list_intrinsic))
            .route("/{name}", web::get().to(read_intrinsic))
            .route("/{name}", web::put().to(write_intrinsic)),
    );
}
