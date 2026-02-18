//! Notes REST API — read-only endpoints for the web UI.
//!
//! Provides file listing, content reading, FTS5 search, note info, and tag listing.
//! All note mutations go through the `notes` tool in chat.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

use crate::config::notes_dir;
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

// --- List notes ---

#[derive(Debug, Serialize)]
struct NoteEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListNotesResponse {
    success: bool,
    path: String,
    entries: Vec<NoteEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListNotesQuery {
    path: Option<String>,
}

/// List files in the notes directory
async fn list_notes(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<ListNotesQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let notes = notes_dir();
    let notes_path = Path::new(&notes);

    let relative_path = query.path.as_deref().unwrap_or("");
    let full_path = if relative_path.is_empty() {
        notes_path.to_path_buf()
    } else {
        notes_path.join(relative_path)
    };

    if !notes_path.exists() {
        return HttpResponse::Ok().json(ListNotesResponse {
            success: true,
            path: relative_path.to_string(),
            entries: vec![],
            error: Some("Notes directory does not exist yet".to_string()),
        });
    }

    // Security: canonicalize and ensure we're within notes dir
    let canonical_notes = match notes_path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ListNotesResponse {
                success: false,
                path: relative_path.to_string(),
                entries: vec![],
                error: Some(format!("Notes not accessible: {}", e)),
            });
        }
    };

    let canonical_path = match full_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return HttpResponse::NotFound().json(ListNotesResponse {
                success: false,
                path: relative_path.to_string(),
                entries: vec![],
                error: Some("Path not found".to_string()),
            });
        }
    };

    if !canonical_path.starts_with(&canonical_notes) {
        return HttpResponse::Forbidden().json(ListNotesResponse {
            success: false,
            path: relative_path.to_string(),
            entries: vec![],
            error: Some("Access denied: path outside notes".to_string()),
        });
    }

    let mut entries = Vec::new();
    let mut read_dir = match fs::read_dir(&canonical_path).await {
        Ok(rd) => rd,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ListNotesResponse {
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

        // Skip hidden files (like .notes.db)
        if name.starts_with('.') {
            continue;
        }

        let metadata = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };

        let entry_path = entry.path();
        let rel_path = entry_path
            .strip_prefix(&canonical_notes)
            .unwrap_or(&entry_path)
            .to_string_lossy()
            .to_string();

        let modified = metadata.modified().ok().map(|t| {
            let datetime: chrono::DateTime<chrono::Utc> = t.into();
            datetime.format("%Y-%m-%d %H:%M:%S").to_string()
        });

        entries.push(NoteEntry {
            name,
            path: rel_path,
            is_dir: metadata.is_dir(),
            size: if metadata.is_dir() { 0 } else { metadata.len() },
            modified,
        });
    }

    // Sort: directories first, then by name (reverse for newest first)
    entries.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            b.is_dir.cmp(&a.is_dir)
        } else {
            b.name.cmp(&a.name)
        }
    });

    HttpResponse::Ok().json(ListNotesResponse {
        success: true,
        path: relative_path.to_string(),
        entries,
        error: None,
    })
}

// --- Read note ---

#[derive(Debug, Serialize)]
struct ReadNoteResponse {
    success: bool,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReadNoteQuery {
    path: String,
}

/// Read a note file with parsed frontmatter metadata
async fn read_note(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<ReadNoteQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let notes = notes_dir();
    let notes_path = Path::new(&notes);
    let full_path = notes_path.join(&query.path);

    if !notes_path.exists() {
        return HttpResponse::NotFound().json(ReadNoteResponse {
            success: false,
            path: query.path.clone(),
            content: None,
            size: None,
            title: None,
            tags: None,
            note_type: None,
            error: Some("Notes directory does not exist".to_string()),
        });
    }

    // Security check
    let canonical_notes = match notes_path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ReadNoteResponse {
                success: false,
                path: query.path.clone(),
                content: None,
                size: None,
                title: None,
                tags: None,
                note_type: None,
                error: Some(format!("Notes not accessible: {}", e)),
            });
        }
    };

    let canonical_path = match full_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return HttpResponse::NotFound().json(ReadNoteResponse {
                success: false,
                path: query.path.clone(),
                content: None,
                size: None,
                title: None,
                tags: None,
                note_type: None,
                error: Some("File not found".to_string()),
            });
        }
    };

    if !canonical_path.starts_with(&canonical_notes) {
        return HttpResponse::Forbidden().json(ReadNoteResponse {
            success: false,
            path: query.path.clone(),
            content: None,
            size: None,
            title: None,
            tags: None,
            note_type: None,
            error: Some("Access denied: path outside notes".to_string()),
        });
    }

    let metadata = match fs::metadata(&canonical_path).await {
        Ok(m) => m,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ReadNoteResponse {
                success: false,
                path: query.path.clone(),
                content: None,
                size: None,
                title: None,
                tags: None,
                note_type: None,
                error: Some(format!("Failed to read file metadata: {}", e)),
            });
        }
    };

    if metadata.is_dir() {
        return HttpResponse::BadRequest().json(ReadNoteResponse {
            success: false,
            path: query.path.clone(),
            content: None,
            size: None,
            title: None,
            tags: None,
            note_type: None,
            error: Some("Path is a directory, not a file".to_string()),
        });
    }

    const MAX_SIZE: u64 = 1024 * 1024;
    if metadata.len() > MAX_SIZE {
        return HttpResponse::Ok().json(ReadNoteResponse {
            success: true,
            path: query.path.clone(),
            content: None,
            size: Some(metadata.len()),
            title: None,
            tags: None,
            note_type: None,
            error: Some(format!("File too large to display ({} bytes)", metadata.len())),
        });
    }

    let raw = match fs::read(&canonical_path).await {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ReadNoteResponse {
                success: false,
                path: query.path.clone(),
                content: None,
                size: None,
                title: None,
                tags: None,
                note_type: None,
                error: Some(format!("Failed to read file: {}", e)),
            });
        }
    };

    let text = String::from_utf8_lossy(&raw).to_string();

    // Parse frontmatter for metadata
    let parsed = crate::notes::frontmatter::parse_note(&text);

    HttpResponse::Ok().json(ReadNoteResponse {
        success: true,
        path: query.path.clone(),
        content: Some(text),
        size: Some(metadata.len()),
        title: if parsed.frontmatter.title.is_empty() {
            None
        } else {
            Some(parsed.frontmatter.title)
        },
        tags: if parsed.all_tags.is_empty() {
            None
        } else {
            Some(parsed.all_tags)
        },
        note_type: Some(parsed.frontmatter.note_type),
        error: None,
    })
}

// --- Search notes ---

#[derive(Debug, Serialize)]
struct SearchNotesResponse {
    success: bool,
    query: String,
    results: Vec<SearchResultItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct SearchResultItem {
    file_path: String,
    title: String,
    tags: String,
    snippet: String,
}

#[derive(Debug, Deserialize)]
struct SearchNotesQuery {
    q: String,
    limit: Option<i32>,
}

/// Full-text search across notes
async fn search_notes(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<SearchNotesQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let notes_store = match data.dispatcher.notes_store() {
        Some(store) => store,
        None => {
            return HttpResponse::ServiceUnavailable().json(SearchNotesResponse {
                success: false,
                query: query.q.clone(),
                results: vec![],
                error: Some("Notes store not initialized".to_string()),
            });
        }
    };

    let limit = query.limit.unwrap_or(20).min(50).max(1);

    match notes_store.search(&query.q, limit) {
        Ok(results) => {
            let items: Vec<SearchResultItem> = results
                .into_iter()
                .map(|r| SearchResultItem {
                    file_path: r.file_path,
                    title: r.title,
                    tags: r.tags,
                    snippet: r.snippet,
                })
                .collect();

            HttpResponse::Ok().json(SearchNotesResponse {
                success: true,
                query: query.q.clone(),
                results: items,
                error: None,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(SearchNotesResponse {
            success: false,
            query: query.q.clone(),
            results: vec![],
            error: Some(format!("Search failed: {}", e)),
        }),
    }
}

// --- Notes info ---

#[derive(Debug, Serialize)]
struct NotesInfoResponse {
    success: bool,
    notes_path: String,
    exists: bool,
    file_count: usize,
}

async fn notes_info(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let notes = notes_dir();
    let exists = Path::new(&notes).exists();

    let file_count = if let Some(store) = data.dispatcher.notes_store() {
        store.list_files().map(|f| f.len()).unwrap_or(0)
    } else {
        0
    };

    HttpResponse::Ok().json(NotesInfoResponse {
        success: true,
        notes_path: notes,
        exists,
        file_count,
    })
}

// --- Tags ---

#[derive(Debug, Serialize)]
struct TagsResponse {
    success: bool,
    tags: Vec<TagItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct TagItem {
    tag: String,
    count: usize,
}

async fn list_tags(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let notes_store = match data.dispatcher.notes_store() {
        Some(store) => store,
        None => {
            return HttpResponse::ServiceUnavailable().json(TagsResponse {
                success: false,
                tags: vec![],
                error: Some("Notes store not initialized".to_string()),
            });
        }
    };

    match notes_store.list_tags() {
        Ok(tags) => {
            let items: Vec<TagItem> = tags
                .into_iter()
                .map(|(tag, count)| TagItem { tag, count })
                .collect();

            HttpResponse::Ok().json(TagsResponse {
                success: true,
                tags: items,
                error: None,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(TagsResponse {
            success: false,
            tags: vec![],
            error: Some(format!("Failed to list tags: {}", e)),
        }),
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/notes")
            .route("", web::get().to(list_notes))
            .route("/read", web::get().to(read_note))
            .route("/search", web::get().to(search_notes))
            .route("/info", web::get().to(notes_info))
            .route("/tags", web::get().to(list_tags)),
    );
}
