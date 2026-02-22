use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;
use walkdir::WalkDir;

use crate::config::{public_dir, workspace_dir};
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

#[derive(Debug, Serialize)]
struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListFilesResponse {
    success: bool,
    path: String,
    entries: Vec<FileEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListFilesQuery {
    path: Option<String>,
    include_dir_sizes: Option<bool>,
}

/// List files in the workspace directory
async fn list_files(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<ListFilesQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let workspace = workspace_dir();
    let workspace_path = Path::new(&workspace);

    // Resolve the requested path — "public/" prefix routes to the public dir
    let relative_path = query.path.as_deref().unwrap_or("");
    let (full_path, base_path) = if relative_path == "public" || relative_path.starts_with("public/") {
        let public = public_dir();
        let public_path = Path::new(&public).to_path_buf();
        let sub = relative_path.strip_prefix("public/").unwrap_or("");
        if sub.is_empty() {
            (public_path.clone(), public_path)
        } else {
            (public_path.join(sub), public_path)
        }
    } else {
        let fp = if relative_path.is_empty() {
            workspace_path.to_path_buf()
        } else {
            workspace_path.join(relative_path)
        };
        (fp, workspace_path.to_path_buf())
    };

    // Security check: canonicalize and ensure we're within the base directory
    let canonical_workspace = match base_path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ListFilesResponse {
                success: false,
                path: relative_path.to_string(),
                entries: vec![],
                error: Some(format!("Directory not accessible: {}", e)),
            });
        }
    };

    let canonical_path = match full_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // Path doesn't exist - check if workspace exists first
            if !workspace_path.exists() {
                return HttpResponse::Ok().json(ListFilesResponse {
                    success: true,
                    path: relative_path.to_string(),
                    entries: vec![],
                    error: Some("Workspace directory does not exist yet".to_string()),
                });
            }
            return HttpResponse::NotFound().json(ListFilesResponse {
                success: false,
                path: relative_path.to_string(),
                entries: vec![],
                error: Some("Path not found".to_string()),
            });
        }
    };

    // Ensure path is within workspace
    if !canonical_path.starts_with(&canonical_workspace) {
        return HttpResponse::Forbidden().json(ListFilesResponse {
            success: false,
            path: relative_path.to_string(),
            entries: vec![],
            error: Some("Access denied: path outside workspace".to_string()),
        });
    }

    let include_dir_sizes = query.include_dir_sizes.unwrap_or(false);

    // Read directory contents
    let mut entries = Vec::new();
    let mut read_dir = match fs::read_dir(&canonical_path).await {
        Ok(rd) => rd,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ListFilesResponse {
                success: false,
                path: relative_path.to_string(),
                entries: vec![],
                error: Some(format!("Failed to read directory: {}", e)),
            });
        }
    };

    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = match entry.file_name().to_str() {
            Some(n) => n.to_string(),
            None => continue,
        };

        let metadata = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };

        let entry_path = entry.path();
        let rel_path = if relative_path == "public" || relative_path.starts_with("public/") {
            // Files inside public/ — prefix with "public/"
            let inner = entry_path
                .strip_prefix(&canonical_workspace)
                .unwrap_or(&entry_path)
                .to_string_lossy()
                .to_string();
            format!("public/{}", inner)
        } else {
            entry_path
                .strip_prefix(&canonical_workspace)
                .unwrap_or(&entry_path)
                .to_string_lossy()
                .to_string()
        };

        let modified = metadata.modified().ok().map(|t| {
            let datetime: chrono::DateTime<chrono::Utc> = t.into();
            datetime.format("%Y-%m-%d %H:%M:%S").to_string()
        });

        let size = if metadata.is_dir() {
            if include_dir_sizes {
                compute_dir_size(&entry_path)
            } else {
                0
            }
        } else {
            metadata.len()
        };

        entries.push(FileEntry {
            name,
            path: rel_path,
            is_dir: metadata.is_dir(),
            size,
            modified,
        });
    }

    // At the workspace root, inject a virtual "public" directory entry
    if relative_path.is_empty() {
        let pub_dir = public_dir();
        let pub_path = Path::new(&pub_dir);
        if pub_path.exists() {
            entries.push(FileEntry {
                name: "public".to_string(),
                path: "public".to_string(),
                is_dir: true,
                size: 0,
                modified: None,
            });
        }
    }

    // Sort: directories first, then by name
    entries.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            b.is_dir.cmp(&a.is_dir)
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });

    HttpResponse::Ok().json(ListFilesResponse {
        success: true,
        path: relative_path.to_string(),
        entries,
        error: None,
    })
}

#[derive(Debug, Serialize)]
struct ReadFileResponse {
    success: bool,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_binary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReadFileQuery {
    path: String,
}

/// Read a file from the workspace
async fn read_file(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<ReadFileQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let workspace = workspace_dir();
    let workspace_path = Path::new(&workspace);

    // Resolve base dir — "public/" prefix routes to the public dir
    let (full_path, base_path) = if query.path.starts_with("public/") {
        let public = public_dir();
        let public_path = Path::new(&public).to_path_buf();
        let sub = query.path.strip_prefix("public/").unwrap_or("");
        (public_path.join(sub), public_path)
    } else {
        (workspace_path.join(&query.path), workspace_path.to_path_buf())
    };

    // Security check
    let canonical_workspace = match base_path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ReadFileResponse {
                success: false,
                path: query.path.clone(),
                content: None,
                size: None,
                is_binary: None,
                error: Some(format!("Directory not accessible: {}", e)),
            });
        }
    };

    let canonical_path = match full_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return HttpResponse::NotFound().json(ReadFileResponse {
                success: false,
                path: query.path.clone(),
                content: None,
                size: None,
                is_binary: None,
                error: Some("File not found".to_string()),
            });
        }
    };

    if !canonical_path.starts_with(&canonical_workspace) {
        return HttpResponse::Forbidden().json(ReadFileResponse {
            success: false,
            path: query.path.clone(),
            content: None,
            size: None,
            is_binary: None,
            error: Some("Access denied: path outside allowed directory".to_string()),
        });
    }

    // Check if it's a file
    let metadata = match fs::metadata(&canonical_path).await {
        Ok(m) => m,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ReadFileResponse {
                success: false,
                path: query.path.clone(),
                content: None,
                size: None,
                is_binary: None,
                error: Some(format!("Failed to read file metadata: {}", e)),
            });
        }
    };

    if metadata.is_dir() {
        return HttpResponse::BadRequest().json(ReadFileResponse {
            success: false,
            path: query.path.clone(),
            content: None,
            size: None,
            is_binary: None,
            error: Some("Path is a directory, not a file".to_string()),
        });
    }

    // Read file content (limit to 1MB for safety)
    const MAX_SIZE: u64 = 1024 * 1024;
    if metadata.len() > MAX_SIZE {
        return HttpResponse::Ok().json(ReadFileResponse {
            success: true,
            path: query.path.clone(),
            content: None,
            size: Some(metadata.len()),
            is_binary: Some(true),
            error: Some(format!("File too large to display ({} bytes)", metadata.len())),
        });
    }

    let content = match fs::read(&canonical_path).await {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ReadFileResponse {
                success: false,
                path: query.path.clone(),
                content: None,
                size: None,
                is_binary: None,
                error: Some(format!("Failed to read file: {}", e)),
            });
        }
    };

    // Check if binary
    let is_binary = content.iter().take(8000).any(|&b| b == 0);
    if is_binary {
        return HttpResponse::Ok().json(ReadFileResponse {
            success: true,
            path: query.path.clone(),
            content: None,
            size: Some(metadata.len()),
            is_binary: Some(true),
            error: Some("Binary file cannot be displayed".to_string()),
        });
    }

    let text = String::from_utf8_lossy(&content).to_string();

    HttpResponse::Ok().json(ReadFileResponse {
        success: true,
        path: query.path.clone(),
        content: Some(text),
        size: Some(metadata.len()),
        is_binary: Some(false),
        error: None,
    })
}

#[derive(Debug, Serialize)]
struct WorkspaceInfoResponse {
    success: bool,
    workspace_path: String,
    exists: bool,
}

/// Get workspace info
async fn workspace_info(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let workspace = workspace_dir();
    let exists = Path::new(&workspace).exists();

    HttpResponse::Ok().json(WorkspaceInfoResponse {
        success: true,
        workspace_path: workspace,
        exists,
    })
}

/// Compute total size of a directory by walking all files.
fn compute_dir_size(path: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

#[derive(Debug, Deserialize)]
struct DeleteFileRequest {
    path: String,
}

#[derive(Debug, Serialize)]
struct DeleteFileResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    freed_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Delete a file or directory from the workspace
async fn delete_file(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<DeleteFileRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let workspace = workspace_dir();
    let workspace_path = Path::new(&workspace);
    let full_path = workspace_path.join(&body.path);

    // Security check: canonicalize and ensure within workspace
    let canonical_workspace = match workspace_path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::InternalServerError().json(DeleteFileResponse {
                success: false,
                deleted_count: None,
                freed_bytes: None,
                error: Some(format!("Workspace not accessible: {}", e)),
            });
        }
    };

    let canonical_path = match full_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return HttpResponse::NotFound().json(DeleteFileResponse {
                success: false,
                deleted_count: None,
                freed_bytes: None,
                error: Some("Path not found".to_string()),
            });
        }
    };

    // Must be inside workspace and not the workspace root itself
    if !canonical_path.starts_with(&canonical_workspace) || canonical_path == canonical_workspace {
        return HttpResponse::Forbidden().json(DeleteFileResponse {
            success: false,
            deleted_count: None,
            freed_bytes: None,
            error: Some("Access denied: path outside workspace or is workspace root".to_string()),
        });
    }

    // Compute size before deletion
    let freed_bytes = if canonical_path.is_dir() {
        compute_dir_size(&canonical_path)
    } else {
        std::fs::metadata(&canonical_path).map(|m| m.len()).unwrap_or(0)
    };

    // Count entries
    let deleted_count = if canonical_path.is_dir() {
        WalkDir::new(&canonical_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .count() as u64
    } else {
        1
    };

    // Delete
    let result = if canonical_path.is_dir() {
        std::fs::remove_dir_all(&canonical_path)
    } else {
        std::fs::remove_file(&canonical_path)
    };

    match result {
        Ok(_) => {
            // Refresh disk quota if available
            if let Some(dq) = &data.disk_quota {
                dq.refresh();
            }

            HttpResponse::Ok().json(DeleteFileResponse {
                success: true,
                deleted_count: Some(deleted_count),
                freed_bytes: Some(freed_bytes),
                error: None,
            })
        }
        Err(e) => {
            HttpResponse::InternalServerError().json(DeleteFileResponse {
                success: false,
                deleted_count: None,
                freed_bytes: None,
                error: Some(format!("Failed to delete: {}", e)),
            })
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/files")
            .route("", web::get().to(list_files))
            .route("/read", web::get().to(read_file))
            .route("/delete", web::delete().to(delete_file))
            .route("/workspace", web::get().to(workspace_info)),
    );
}
