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
    deletable: bool,
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
        deletable: false,
    },
    IntrinsicFile {
        name: "guidelines.md",
        description: "Operational and business guidelines",
        writable: true,
        deletable: false,
    },
    IntrinsicFile {
        name: "assistant.md",
        description: "System instructions (read-only)",
        writable: false,
        deletable: false,
    },
];

#[derive(Debug, Serialize)]
struct IntrinsicFileInfo {
    name: String,
    description: String,
    writable: bool,
    deletable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_dir: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ListIntrinsicResponse {
    success: bool,
    files: Vec<IntrinsicFileInfo>,
}

/// List root intrinsic entries (files + virtual skills/ folder)
async fn list_intrinsic(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let mut files: Vec<IntrinsicFileInfo> = INTRINSIC_FILES
        .iter()
        .map(|f| IntrinsicFileInfo {
            name: f.name.to_string(),
            description: f.description.to_string(),
            writable: f.writable,
            deletable: f.deletable,
            is_dir: Some(false),
            size: intrinsic_path(f.name)
                .and_then(|p| std::fs::metadata(&p).ok())
                .map(|m| m.len()),
        })
        .collect();

    // Add virtual skills/ directory
    files.push(IntrinsicFileInfo {
        name: "skills".to_string(),
        description: "Installed skills".to_string(),
        writable: true,
        deletable: false,
        is_dir: Some(true),
        size: None,
    });

    // Add virtual modules/ directory
    files.push(IntrinsicFileInfo {
        name: "modules".to_string(),
        description: "Installed modules".to_string(),
        writable: true,
        deletable: false,
        is_dir: Some(true),
        size: None,
    });

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

/// Handle all path-based requests: GET /api/intrinsic/{path:.*}
/// Supports:
///   - /api/intrinsic/{name}             → read classic intrinsic file
///   - /api/intrinsic/skills             → list skill subfolders
///   - /api/intrinsic/skills/{skill}     → list files in skill folder
///   - /api/intrinsic/skills/{skill}/{file..} → read a file within a skill folder
async fn read_by_path(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let raw_path = path.into_inner();

    // Path traversal protection
    if raw_path.contains("..") || raw_path.contains('\0') {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "error": "Path traversal not allowed"
        }));
    }

    let segments: Vec<&str> = raw_path.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();

    if segments.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "error": "Empty path"
        }));
    }

    // Check if this is a skills/* path
    if segments[0] == "skills" {
        return handle_skills_path_read(&data, &segments[1..]).await;
    }

    // Check if this is a modules/* path
    if segments[0] == "modules" {
        return handle_modules_path_read(&data, &segments[1..]).await;
    }

    // Classic intrinsic file
    let name = &raw_path;

    let intrinsic = INTRINSIC_FILES.iter().find(|f| f.name == name.as_str());
    let intrinsic = match intrinsic {
        Some(i) => i,
        None => {
            return HttpResponse::NotFound().json(ReadIntrinsicResponse {
                success: false,
                name: name.to_string(),
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

/// Handle GET requests under skills/ path
async fn handle_skills_path_read(_data: &web::Data<AppState>, segments: &[&str]) -> HttpResponse {
    let runtime_dir = PathBuf::from(crate::config::runtime_skills_dir());

    if segments.is_empty() {
        // List skill subfolders
        let mut entries = Vec::new();
        if let Ok(dir) = std::fs::read_dir(&runtime_dir) {
            for entry in dir.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    // Skip hidden/internal dirs
                    if name.starts_with('.') || name.starts_with('_') || name == "inactive" || name == "managed" {
                        continue;
                    }
                    entries.push(IntrinsicFileInfo {
                        name,
                        description: String::new(),
                        writable: true,
                        deletable: true,
                        is_dir: Some(true),
                        size: None,
                    });
                }
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        return HttpResponse::Ok().json(ListIntrinsicResponse {
            success: true,
            files: entries,
        });
    }

    let skill_name = segments[0];
    let skill_dir = runtime_dir.join(skill_name);

    if !skill_dir.is_dir() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "success": false,
            "error": format!("Skill folder '{}' not found", skill_name)
        }));
    }

    if segments.len() == 1 {
        // List files in skill folder
        let mut entries = Vec::new();
        if let Ok(items) = list_dir_entries(&skill_dir, &skill_dir) {
            entries = items;
        }
        entries.sort_by(|a, b| {
            // Dirs first, then alphabetical
            let a_dir = a.is_dir.unwrap_or(false);
            let b_dir = b.is_dir.unwrap_or(false);
            b_dir.cmp(&a_dir).then(a.name.cmp(&b.name))
        });
        return HttpResponse::Ok().json(ListIntrinsicResponse {
            success: true,
            files: entries,
        });
    }

    // Read a specific file within the skill folder
    let relative = segments[1..].join("/");
    let file_path = skill_dir.join(&relative);

    // Check it's still within the skill dir (extra safety)
    if !file_path.starts_with(&skill_dir) {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "error": "Path traversal not allowed"
        }));
    }

    if file_path.is_dir() {
        // List directory contents
        let mut entries = Vec::new();
        if let Ok(items) = list_dir_entries(&file_path, &skill_dir) {
            entries = items;
        }
        entries.sort_by(|a, b| {
            let a_dir = a.is_dir.unwrap_or(false);
            let b_dir = b.is_dir.unwrap_or(false);
            b_dir.cmp(&a_dir).then(a.name.cmp(&b.name))
        });
        return HttpResponse::Ok().json(ListIntrinsicResponse {
            success: true,
            files: entries,
        });
    }

    if !file_path.is_file() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "success": false,
            "error": format!("File not found: {}", relative)
        }));
    }

    match fs::read(&file_path).await {
        Ok(bytes) => {
            match String::from_utf8(bytes) {
                Ok(content) => HttpResponse::Ok().json(ReadIntrinsicResponse {
                    success: true,
                    name: relative,
                    content: Some(content),
                    writable: true,
                    error: None,
                }),
                Err(_) => HttpResponse::BadRequest().json(ReadIntrinsicResponse {
                    success: false,
                    name: relative,
                    content: None,
                    writable: false,
                    error: Some("File is binary and cannot be displayed as text".to_string()),
                }),
            }
        }
        Err(e) => HttpResponse::InternalServerError().json(ReadIntrinsicResponse {
            success: false,
            name: relative,
            content: None,
            writable: true,
            error: Some(format!("Failed to read file: {}", e)),
        }),
    }
}

/// Handle GET requests under modules/ path
async fn handle_modules_path_read(_data: &web::Data<AppState>, segments: &[&str]) -> HttpResponse {
    let runtime_dir = crate::config::runtime_modules_dir();

    if segments.is_empty() {
        // List module subfolders
        let mut entries = Vec::new();
        if let Ok(dir) = std::fs::read_dir(&runtime_dir) {
            for entry in dir.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    // Skip hidden/internal dirs
                    if name.starts_with('.') || name.starts_with('_') {
                        continue;
                    }
                    entries.push(IntrinsicFileInfo {
                        name,
                        description: String::new(),
                        writable: true,
                        deletable: true,
                        is_dir: Some(true),
                        size: None,
                    });
                }
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        return HttpResponse::Ok().json(ListIntrinsicResponse {
            success: true,
            files: entries,
        });
    }

    let module_name = segments[0];
    let module_dir = runtime_dir.join(module_name);

    if !module_dir.is_dir() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "success": false,
            "error": format!("Module folder '{}' not found", module_name)
        }));
    }

    if segments.len() == 1 {
        // List files in module folder
        let mut entries = Vec::new();
        if let Ok(items) = list_dir_entries(&module_dir, &module_dir) {
            entries = items;
        }
        entries.sort_by(|a, b| {
            let a_dir = a.is_dir.unwrap_or(false);
            let b_dir = b.is_dir.unwrap_or(false);
            b_dir.cmp(&a_dir).then(a.name.cmp(&b.name))
        });
        return HttpResponse::Ok().json(ListIntrinsicResponse {
            success: true,
            files: entries,
        });
    }

    // Read a specific file within the module folder
    let relative = segments[1..].join("/");
    let file_path = module_dir.join(&relative);

    // Check it's still within the module dir (extra safety)
    if !file_path.starts_with(&module_dir) {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "success": false,
            "error": "Path traversal not allowed"
        }));
    }

    if file_path.is_dir() {
        // List directory contents
        let mut entries = Vec::new();
        if let Ok(items) = list_dir_entries(&file_path, &module_dir) {
            entries = items;
        }
        entries.sort_by(|a, b| {
            let a_dir = a.is_dir.unwrap_or(false);
            let b_dir = b.is_dir.unwrap_or(false);
            b_dir.cmp(&a_dir).then(a.name.cmp(&b.name))
        });
        return HttpResponse::Ok().json(ListIntrinsicResponse {
            success: true,
            files: entries,
        });
    }

    if !file_path.is_file() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "success": false,
            "error": format!("File not found: {}", relative)
        }));
    }

    match fs::read(&file_path).await {
        Ok(bytes) => {
            match String::from_utf8(bytes) {
                Ok(content) => HttpResponse::Ok().json(ReadIntrinsicResponse {
                    success: true,
                    name: relative,
                    content: Some(content),
                    writable: true,
                    error: None,
                }),
                Err(_) => HttpResponse::BadRequest().json(ReadIntrinsicResponse {
                    success: false,
                    name: relative,
                    content: None,
                    writable: false,
                    error: Some("File is binary and cannot be displayed as text".to_string()),
                }),
            }
        }
        Err(e) => HttpResponse::InternalServerError().json(ReadIntrinsicResponse {
            success: false,
            name: relative,
            content: None,
            writable: true,
            error: Some(format!("Failed to read file: {}", e)),
        }),
    }
}

/// List entries in a directory, returning IntrinsicFileInfo items
fn list_dir_entries(dir: &std::path::Path, _base: &std::path::Path) -> Result<Vec<IntrinsicFileInfo>, std::io::Error> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.path().is_dir();
        let size = if !is_dir {
            entry.metadata().ok().map(|m| m.len())
        } else {
            None
        };
        entries.push(IntrinsicFileInfo {
            name,
            description: String::new(),
            writable: true,
            deletable: true,
            is_dir: Some(is_dir),
            size,
        });
    }
    Ok(entries)
}

/// Handle PUT requests: /api/intrinsic/{path:.*}
async fn write_by_path(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<WriteIntrinsicRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let raw_path = path.into_inner();

    if raw_path.contains("..") || raw_path.contains('\0') {
        return HttpResponse::BadRequest().json(WriteIntrinsicResponse {
            success: false,
            error: Some("Path traversal not allowed".to_string()),
        });
    }

    let segments: Vec<&str> = raw_path.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();

    // Check if this is a skills/* path
    if segments.len() >= 3 && segments[0] == "skills" {
        let runtime_dir = PathBuf::from(crate::config::runtime_skills_dir());
        let skill_name = segments[1];
        let relative = segments[2..].join("/");
        let file_path = runtime_dir.join(skill_name).join(&relative);

        // Safety check
        if !file_path.starts_with(runtime_dir.join(skill_name)) {
            return HttpResponse::BadRequest().json(WriteIntrinsicResponse {
                success: false,
                error: Some("Path traversal not allowed".to_string()),
            });
        }

        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                    success: false,
                    error: Some(format!("Failed to create directory: {}", e)),
                });
            }
        }

        if let Err(e) = fs::write(&file_path, &body.content).await {
            log::error!("Failed to write skill file {}/{}: {}", skill_name, relative, e);
            return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                success: false,
                error: Some(format!("Failed to write file: {}", e)),
            });
        }

        // Only sync to DB when the main skill definition file is changed
        let file_name = segments.last().unwrap_or(&"");
        let is_skill_md = *file_name == "SKILL.md"
            || *file_name == format!("{}.md", skill_name);
        if is_skill_md {
            let skill_dir_path = runtime_dir.join(skill_name);
            sync_single_skill_to_db(&data, &skill_dir_path, skill_name).await;
        }

        log::info!("Updated skill file: {}/{}", skill_name, relative);
        return HttpResponse::Ok().json(WriteIntrinsicResponse {
            success: true,
            error: None,
        });
    }

    // Check if this is a modules/* path
    if segments.len() >= 3 && segments[0] == "modules" {
        let runtime_dir = crate::config::runtime_modules_dir();
        let module_name = segments[1];
        let relative = segments[2..].join("/");
        let file_path = runtime_dir.join(module_name).join(&relative);

        // Safety check
        if !file_path.starts_with(runtime_dir.join(module_name)) {
            return HttpResponse::BadRequest().json(WriteIntrinsicResponse {
                success: false,
                error: Some("Path traversal not allowed".to_string()),
            });
        }

        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                    success: false,
                    error: Some(format!("Failed to create directory: {}", e)),
                });
            }
        }

        if let Err(e) = fs::write(&file_path, &body.content).await {
            log::error!("Failed to write module file {}/{}: {}", module_name, relative, e);
            return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                success: false,
                error: Some(format!("Failed to write file: {}", e)),
            });
        }

        log::info!("Updated module file: {}/{}", module_name, relative);
        return HttpResponse::Ok().json(WriteIntrinsicResponse {
            success: true,
            error: None,
        });
    }

    // Classic intrinsic file
    let name = &raw_path;
    let intrinsic = match INTRINSIC_FILES.iter().find(|f| f.name == name.as_str()) {
        Some(i) => i,
        None => {
            return HttpResponse::NotFound().json(WriteIntrinsicResponse {
                success: false,
                error: Some("Intrinsic file not found".to_string()),
            });
        }
    };

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

/// Handle DELETE requests: /api/intrinsic/{path:.*}
async fn delete_by_path(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let raw_path = path.into_inner();

    if raw_path.contains("..") || raw_path.contains('\0') {
        return HttpResponse::BadRequest().json(WriteIntrinsicResponse {
            success: false,
            error: Some("Path traversal not allowed".to_string()),
        });
    }

    let segments: Vec<&str> = raw_path.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();

    // Check if this is a skills/* path
    if segments.len() >= 2 && segments[0] == "skills" {
        let runtime_dir = PathBuf::from(crate::config::runtime_skills_dir());
        let skill_name = segments[1];

        if segments.len() == 2 {
            // Delete entire skill folder + DB entry
            let skill_dir = runtime_dir.join(skill_name);
            if skill_dir.is_dir() {
                if let Err(e) = std::fs::remove_dir_all(&skill_dir) {
                    return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                        success: false,
                        error: Some(format!("Failed to delete skill folder: {}", e)),
                    });
                }
                // Delete from DB only (folder already removed above)
                if let Err(e) = data.db.delete_skill(skill_name) {
                    log::warn!("Failed to delete skill '{}' from DB: {}", skill_name, e);
                }
                log::info!("Deleted skill folder: {}", skill_name);
            }
            return HttpResponse::Ok().json(WriteIntrinsicResponse {
                success: true,
                error: None,
            });
        }

        // Delete a specific file within a skill folder
        let relative = segments[2..].join("/");
        let file_path = runtime_dir.join(skill_name).join(&relative);
        if !file_path.starts_with(runtime_dir.join(skill_name)) {
            return HttpResponse::BadRequest().json(WriteIntrinsicResponse {
                success: false,
                error: Some("Path traversal not allowed".to_string()),
            });
        }

        if file_path.is_file() {
            if let Err(e) = fs::remove_file(&file_path).await {
                return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                    success: false,
                    error: Some(format!("Failed to delete file: {}", e)),
                });
            }
        } else if file_path.is_dir() {
            if let Err(e) = std::fs::remove_dir_all(&file_path) {
                return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                    success: false,
                    error: Some(format!("Failed to delete directory: {}", e)),
                });
            }
        }

        return HttpResponse::Ok().json(WriteIntrinsicResponse {
            success: true,
            error: None,
        });
    }

    // Check if this is a modules/* path
    if segments.len() >= 2 && segments[0] == "modules" {
        let runtime_dir = crate::config::runtime_modules_dir();
        let module_name = segments[1];

        if segments.len() == 2 {
            // Delete entire module folder
            let module_dir = runtime_dir.join(module_name);
            if module_dir.is_dir() {
                if let Err(e) = std::fs::remove_dir_all(&module_dir) {
                    return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                        success: false,
                        error: Some(format!("Failed to delete module folder: {}", e)),
                    });
                }
                log::info!("Deleted module folder: {}", module_name);
            }
            return HttpResponse::Ok().json(WriteIntrinsicResponse {
                success: true,
                error: None,
            });
        }

        // Delete a specific file within a module folder
        let relative = segments[2..].join("/");
        let file_path = runtime_dir.join(module_name).join(&relative);
        if !file_path.starts_with(runtime_dir.join(module_name)) {
            return HttpResponse::BadRequest().json(WriteIntrinsicResponse {
                success: false,
                error: Some("Path traversal not allowed".to_string()),
            });
        }

        if file_path.is_file() {
            if let Err(e) = fs::remove_file(&file_path).await {
                return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                    success: false,
                    error: Some(format!("Failed to delete file: {}", e)),
                });
            }
        } else if file_path.is_dir() {
            if let Err(e) = std::fs::remove_dir_all(&file_path) {
                return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
                    success: false,
                    error: Some(format!("Failed to delete directory: {}", e)),
                });
            }
        }

        return HttpResponse::Ok().json(WriteIntrinsicResponse {
            success: true,
            error: None,
        });
    }

    // Classic intrinsic file
    let name = &raw_path;
    let intrinsic = match INTRINSIC_FILES.iter().find(|f| f.name == name.as_str()) {
        Some(i) => i,
        None => {
            return HttpResponse::NotFound().json(WriteIntrinsicResponse {
                success: false,
                error: Some("Intrinsic file not found".to_string()),
            });
        }
    };

    if !intrinsic.deletable {
        return HttpResponse::Forbidden().json(WriteIntrinsicResponse {
            success: false,
            error: Some(format!("'{}' cannot be deleted", name)),
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

    if !full_path.exists() {
        return HttpResponse::Ok().json(WriteIntrinsicResponse {
            success: true,
            error: None,
        });
    }

    if let Err(e) = fs::remove_file(&full_path).await {
        log::error!("Failed to delete intrinsic file {}: {}", name, e);
        return HttpResponse::InternalServerError().json(WriteIntrinsicResponse {
            success: false,
            error: Some(format!("Failed to delete file: {}", e)),
        });
    }

    log::info!("Deleted intrinsic file: {}", name);

    HttpResponse::Ok().json(WriteIntrinsicResponse {
        success: true,
        error: None,
    })
}

/// Re-sync a single skill from its disk folder to the DB
async fn sync_single_skill_to_db(data: &web::Data<AppState>, skill_dir: &std::path::Path, skill_name: &str) {
    use crate::skills::loader::load_skill_from_file_with_dir;
    use crate::skills::SkillSource;

    // Find the .md file (match loader priority: {name}.md first)
    let named_md = skill_dir.join(format!("{}.md", skill_name));
    let skill_md = skill_dir.join("SKILL.md");
    let md_path = if named_md.exists() {
        named_md
    } else if skill_md.exists() {
        skill_md
    } else {
        return;
    };

    match load_skill_from_file_with_dir(&md_path, SkillSource::Managed, Some(skill_dir.to_path_buf())).await {
        Ok(skill) => {
            // Re-import just this single skill via force-update (avoid full reload)
            let content = match std::fs::read_to_string(&md_path) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("Failed to read skill file for re-sync {}: {}", skill_name, e);
                    return;
                }
            };
            if let Err(e) = data.skill_registry.create_skill_from_markdown_force(&content) {
                log::warn!("Failed to re-sync skill '{}' to DB: {}", skill_name, e);
            } else {
                log::info!("Re-synced skill '{}' to DB after edit", skill.metadata.name);
            }
        }
        Err(e) => {
            log::warn!("Failed to parse updated skill {}: {}", skill_name, e);
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/intrinsic")
            .route("", web::get().to(list_intrinsic))
            .route("/{path:.*}", web::get().to(read_by_path))
            .route("/{path:.*}", web::put().to(write_by_path))
            .route("/{path:.*}", web::delete().to(delete_by_path)),
    );
}
