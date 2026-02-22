use actix_web::{web, HttpResponse};
use serde::Serialize;
use std::path::PathBuf;

use crate::config::public_dir;

/// Allowed image extensions for public serving
const ALLOWED_EXTENSIONS: &[&str] = &["png", "svg", "jpg", "jpeg", "gif", "webp"];

/// Get MIME type for an image extension
fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Check if a filename has an allowed image extension
fn is_allowed_image(filename: &str) -> bool {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    ALLOWED_EXTENSIONS.contains(&ext.as_str())
}

#[derive(Serialize)]
struct PublicFileEntry {
    name: String,
    size: u64,
    url: String,
}

/// List available public image files
async fn list_public_files() -> HttpResponse {
    let dir = PathBuf::from(public_dir());

    if !dir.exists() {
        let empty: Vec<PublicFileEntry> = Vec::new();
        return HttpResponse::Ok().json(serde_json::json!({
            "files": empty
        }));
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip dotfiles, directories, and non-image files
            if name.starts_with('.') || !is_allowed_image(&name) {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !metadata.is_file() {
                continue;
            }
            files.push(PublicFileEntry {
                url: format!("/public/{}", name),
                name,
                size: metadata.len(),
            });
        }
    }

    files.sort_by(|a, b| a.name.cmp(&b.name));

    HttpResponse::Ok().json(serde_json::json!({ "files": files }))
}

/// Serve a public image file
async fn serve_public_file(path: web::Path<String>) -> HttpResponse {
    let filename = path.into_inner();

    // Reject non-image files
    if !is_allowed_image(&filename) {
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "Only image files (png, svg, jpg, jpeg, gif, webp) are served from /public/"
        }));
    }

    // Reject path traversal attempts and hidden files
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') || filename.starts_with('.') {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Invalid filename"
        }));
    }

    let dir = PathBuf::from(public_dir());
    let file_path = dir.join(&filename);

    // Canonicalize and verify within public dir
    let canonical_dir = match dir.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return HttpResponse::NotFound().json(serde_json::json!({
                "error": "Public directory not found"
            }));
        }
    };

    let canonical_file = match file_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return HttpResponse::NotFound().json(serde_json::json!({
                "error": "File not found"
            }));
        }
    };

    if !canonical_file.starts_with(&canonical_dir) {
        return HttpResponse::Forbidden().json(serde_json::json!({
            "error": "Access denied"
        }));
    }

    // Read and serve the file
    match tokio::fs::read(&canonical_file).await {
        Ok(contents) => {
            let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
            HttpResponse::Ok()
                .content_type(mime_for_ext(&ext))
                .append_header(("Cache-Control", "public, max-age=300"))
                .body(contents)
        }
        Err(_) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "File not found"
        })),
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/public")
            .route("", web::get().to(list_public_files))
            .route("/", web::get().to(list_public_files))
            .route("/{filename}", web::get().to(serve_public_file)),
    );
}
