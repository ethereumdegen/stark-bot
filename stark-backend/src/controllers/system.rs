//! System controller — disk usage info and cleanup endpoints.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use walkdir::WalkDir;

use crate::config;
use crate::controllers::health::VERSION;
use crate::qmd_memory::file_ops;
use crate::AppState;

/// Validate session token from request (same pattern as memory controller)
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

// ============================================================================
// Response/Request Types
// ============================================================================

#[derive(Debug, Serialize)]
struct DiskInfo {
    enabled: bool,
    used_bytes: u64,
    quota_bytes: u64,
    remaining_bytes: u64,
    percentage: u64,
    breakdown: HashMap<String, u64>,
}

#[derive(Debug, Serialize)]
struct SystemInfoResponse {
    disk: DiskInfo,
    uptime_secs: u64,
    version: String,
}

#[derive(Debug, Deserialize)]
struct CleanupMemoriesBody {
    #[serde(default = "default_older_than_days")]
    older_than_days: u32,
}

fn default_older_than_days() -> u32 {
    30
}

#[derive(Debug, Deserialize)]
struct CleanupWorkspaceBody {
    #[serde(default)]
    confirm: bool,
}

#[derive(Debug, Serialize)]
struct CleanupResponse {
    success: bool,
    deleted_count: usize,
    freed_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ============================================================================
// Helpers
// ============================================================================

/// Walk a directory and return total size in bytes.
fn dir_size(path: &str) -> u64 {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return 0;
    }
    let mut total: u64 = 0;
    for entry in WalkDir::new(p).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/system/info
async fn system_info(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let workspace_dir = config::workspace_dir();
    let memory_dir = config::memory_config().memory_dir;
    let journal_dir = config::journal_dir();
    let soul_dir = config::soul_dir();

    // Database directory
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| config::defaults::DATABASE_URL.to_string());
    let db_dir = std::path::PathBuf::from(&db_url)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| config::backend_dir().join(".db").to_string_lossy().to_string());

    // Per-directory breakdown
    let mut breakdown = HashMap::new();
    breakdown.insert("workspace".to_string(), dir_size(&workspace_dir));
    breakdown.insert("memory".to_string(), dir_size(&memory_dir));
    breakdown.insert("journal".to_string(), dir_size(&journal_dir));
    breakdown.insert("soul".to_string(), dir_size(&soul_dir));
    breakdown.insert("database".to_string(), dir_size(&db_dir));

    let total_used: u64 = breakdown.values().sum();

    let (enabled, quota_bytes, used_bytes, remaining_bytes, percentage) =
        if let Some(ref dq) = data.disk_quota {
            // Use the DiskQuotaManager's accurate figures
            let used = dq.usage_bytes();
            (
                dq.is_enabled(),
                dq.quota_bytes(),
                used,
                dq.remaining_bytes(),
                dq.usage_percentage(),
            )
        } else {
            // No disk quota configured
            (false, 0, total_used, 0, 0)
        };

    let uptime_secs = data.started_at.elapsed().as_secs();

    HttpResponse::Ok().json(SystemInfoResponse {
        disk: DiskInfo {
            enabled,
            used_bytes,
            quota_bytes,
            remaining_bytes,
            percentage,
            breakdown,
        },
        uptime_secs,
        version: VERSION.to_string(),
    })
}

/// POST /api/system/cleanup/memories
///
/// Delete daily log `.md` files older than N days.
async fn cleanup_memories(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CleanupMemoriesBody>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let memory_dir = config::memory_config().memory_dir;
    let memory_path = std::path::Path::new(&memory_dir);
    if !memory_path.exists() {
        return HttpResponse::Ok().json(CleanupResponse {
            success: true,
            deleted_count: 0,
            freed_bytes: 0,
            error: None,
        });
    }

    let cutoff = chrono::Local::now().date_naive()
        - chrono::Duration::days(body.older_than_days as i64);

    let mut deleted_count = 0usize;
    let mut freed_bytes = 0u64;

    // Walk the memory directory
    let entries: Vec<_> = WalkDir::new(memory_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();

        // Only delete daily log files (date-named .md files), never MEMORY.md
        if let Some(file_date) = file_ops::parse_date_from_filename(&name) {
            if file_date < cutoff {
                if let Ok(meta) = entry.metadata() {
                    let size = meta.len();
                    if std::fs::remove_file(entry.path()).is_ok() {
                        deleted_count += 1;
                        freed_bytes += size;
                    }
                }
            }
        }
    }

    // Refresh disk quota
    if let Some(ref dq) = data.disk_quota {
        dq.refresh();
    }

    // Reindex memory store
    if let Some(store) = data.dispatcher.memory_store() {
        let _ = store.reindex();
    }

    HttpResponse::Ok().json(CleanupResponse {
        success: true,
        deleted_count,
        freed_bytes,
        error: None,
    })
}

/// POST /api/system/cleanup/workspace
///
/// Delete all files in the workspace directory.
async fn cleanup_workspace(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CleanupWorkspaceBody>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    if !body.confirm {
        return HttpResponse::BadRequest().json(CleanupResponse {
            success: false,
            deleted_count: 0,
            freed_bytes: 0,
            error: Some("Must set confirm: true to delete workspace files".to_string()),
        });
    }

    let workspace_dir = config::workspace_dir();
    let workspace_path = std::path::Path::new(&workspace_dir);
    if !workspace_path.exists() {
        return HttpResponse::Ok().json(CleanupResponse {
            success: true,
            deleted_count: 0,
            freed_bytes: 0,
            error: None,
        });
    }

    let mut deleted_count = 0usize;
    let mut freed_bytes = 0u64;

    // Walk workspace, delete files (not directories)
    let entries: Vec<_> = WalkDir::new(workspace_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();

    for entry in entries {
        if let Ok(meta) = entry.metadata() {
            let size = meta.len();
            if std::fs::remove_file(entry.path()).is_ok() {
                deleted_count += 1;
                freed_bytes += size;
            }
        }
    }

    // Refresh disk quota
    if let Some(ref dq) = data.disk_quota {
        dq.refresh();
    }

    HttpResponse::Ok().json(CleanupResponse {
        success: true,
        deleted_count,
        freed_bytes,
        error: None,
    })
}

/// Configure system routes
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/system")
            .route("/info", web::get().to(system_info))
            .route("/cleanup/memories", web::post().to(cleanup_memories))
            .route("/cleanup/workspace", web::post().to(cleanup_workspace)),
    );
}
