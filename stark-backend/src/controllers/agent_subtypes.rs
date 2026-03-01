use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;

use crate::agents::loader;
use crate::ai::multi_agent::types::{self, AgentSubtypeConfig};
use crate::AppState;

const MAX_SUBTYPES: usize = 10;

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

/// List all agent subtypes (from in-memory registry, backed by disk).
async fn list_subtypes(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    HttpResponse::Ok().json(types::all_subtype_configs_unfiltered())
}

/// Get a single agent subtype by key.
async fn get_subtype(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let key = path.into_inner();
    match types::get_subtype_config(&key) {
        Some(subtype) => HttpResponse::Ok().json(subtype),
        None => HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Agent subtype '{}' not found", key)
        })),
    }
}

#[derive(Deserialize)]
struct CreateSubtypeRequest {
    key: String,
    #[serde(default)]
    version: Option<String>,
    label: String,
    emoji: String,
    description: String,
    tool_groups: Vec<String>,
    skill_tags: Vec<String>,
    #[serde(default)]
    additional_tools: Vec<String>,
    prompt: String,
    #[serde(default)]
    sort_order: i32,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    max_iterations: Option<u32>,
    #[serde(default)]
    skip_task_planner: bool,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    preferred_ai_model: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Create a new agent subtype (writes to disk).
async fn create_subtype(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateSubtypeRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    // Check limit
    let current_count = types::all_subtype_configs_unfiltered().len();
    if current_count >= MAX_SUBTYPES {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("Maximum of {} agent subtypes allowed", MAX_SUBTYPES)
        }));
    }

    // Validate key format
    let key = body.key.to_lowercase();
    if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Key must be non-empty and contain only alphanumeric characters and underscores"
        }));
    }

    let config = AgentSubtypeConfig {
        key,
        version: body.version.clone().unwrap_or_default(),
        label: body.label.clone(),
        emoji: body.emoji.clone(),
        description: body.description.clone(),
        tool_groups: body.tool_groups.clone(),
        skill_tags: body.skill_tags.clone(),
        additional_tools: body.additional_tools.clone(),
        prompt: body.prompt.clone(),
        sort_order: body.sort_order,
        enabled: body.enabled,
        max_iterations: body.max_iterations.unwrap_or(90),
        skip_task_planner: body.skip_task_planner,
        aliases: body.aliases.clone(),
        hidden: body.hidden,
        preferred_ai_model: body.preferred_ai_model.as_ref().filter(|s| !s.is_empty()).cloned(),
        hooks: Vec::new(),
    };

    let agents_dir = crate::config::runtime_agents_dir();
    match loader::write_agent_folder(&agents_dir, &config) {
        Ok(_) => {
            loader::reload_registry_from_disk();
            HttpResponse::Created().json(config)
        }
        Err(e) => {
            log::error!("Failed to create agent subtype: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to write agent: {}", e)
            }))
        }
    }
}

#[derive(Deserialize)]
struct UpdateSubtypeRequest {
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    emoji: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tool_groups: Option<Vec<String>>,
    #[serde(default)]
    skill_tags: Option<Vec<String>>,
    #[serde(default)]
    additional_tools: Option<Vec<String>>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    sort_order: Option<i32>,
    #[serde(default)]
    max_iterations: Option<u32>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    skip_task_planner: Option<bool>,
    #[serde(default)]
    aliases: Option<Vec<String>>,
    #[serde(default)]
    hidden: Option<bool>,
    #[serde(default)]
    preferred_ai_model: Option<String>,
}

/// Update an existing agent subtype (reads from registry, writes to disk).
async fn update_subtype(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<UpdateSubtypeRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let key = path.into_inner();

    // Get existing from registry
    let existing = match types::get_subtype_config(&key) {
        Some(s) => s,
        None => {
            return HttpResponse::NotFound().json(serde_json::json!({
                "error": format!("Agent subtype '{}' not found", key)
            }));
        }
    };

    // Merge updates
    let updated = AgentSubtypeConfig {
        key: existing.key,
        version: existing.version,
        label: body.label.clone().unwrap_or(existing.label),
        emoji: body.emoji.clone().unwrap_or(existing.emoji),
        description: body.description.clone().unwrap_or(existing.description),
        tool_groups: body.tool_groups.clone().unwrap_or(existing.tool_groups),
        skill_tags: body.skill_tags.clone().unwrap_or(existing.skill_tags),
        additional_tools: body.additional_tools.clone().unwrap_or(existing.additional_tools),
        prompt: body.prompt.clone().unwrap_or(existing.prompt),
        sort_order: body.sort_order.unwrap_or(existing.sort_order),
        enabled: body.enabled.unwrap_or(existing.enabled),
        max_iterations: body.max_iterations.unwrap_or(existing.max_iterations),
        skip_task_planner: body.skip_task_planner.unwrap_or(existing.skip_task_planner),
        aliases: body.aliases.clone().unwrap_or(existing.aliases),
        hidden: body.hidden.unwrap_or(existing.hidden),
        preferred_ai_model: match &body.preferred_ai_model {
            Some(s) if s.is_empty() => None,           // explicit clear
            Some(s) => Some(s.clone()),                 // set new value
            None => existing.preferred_ai_model,        // field omitted, preserve
        },
        hooks: Vec::new(),
    };

    let agents_dir = crate::config::runtime_agents_dir();
    match loader::write_agent_folder(&agents_dir, &updated) {
        Ok(_) => {
            loader::reload_registry_from_disk();
            HttpResponse::Ok().json(updated)
        }
        Err(e) => {
            log::error!("Failed to update agent subtype: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to write agent: {}", e)
            }))
        }
    }
}

/// Delete an agent subtype (removes folder from disk).
async fn delete_subtype(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let key = path.into_inner();

    // Check it exists
    if types::get_subtype_config(&key).is_none() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Agent subtype '{}' not found", key)
        }));
    }

    let agents_dir = crate::config::runtime_agents_dir();
    match loader::delete_agent_folder(&agents_dir, &key) {
        Ok(_) => {
            loader::reload_registry_from_disk();
            HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "message": format!("Agent subtype '{}' deleted", key)
            }))
        }
        Err(e) => {
            log::error!("Failed to delete agent subtype: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to delete agent: {}", e)
            }))
        }
    }
}

/// Reset agent subtypes to defaults — re-seed from bundled config/agents/ and reload.
async fn reset_defaults(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let agents_dir = crate::config::runtime_agents_dir();

    // Delete all existing agent folders
    if agents_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&agents_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with('.') {
                        let _ = std::fs::remove_dir_all(entry.path());
                    }
                }
            }
        }
    }

    loader::reload_registry_from_disk();

    let count = types::all_subtype_configs_unfiltered().len();
    HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": format!("Reset to {} default agent subtypes", count),
        "count": count
    }))
}

/// Export all agent subtypes as a ZIP containing all agent folders.
async fn export_subtypes(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    use std::io::{Cursor, Write};

    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let agents_dir = crate::config::runtime_agents_dir();
    let subtypes = types::all_subtype_configs_unfiltered();

    let buf = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let options = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for config in &subtypes {
        let folder = agents_dir.join(&config.key);
        if !folder.is_dir() {
            continue;
        }
        if let Err(e) = add_agent_folder_to_zip(&mut zip, &folder, &config.key, options) {
            log::warn!("[AGENTS] Failed to add '{}' to export ZIP: {}", config.key, e);
        }
    }

    let buf = match zip.finish() {
        Ok(b) => b.into_inner(),
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to create ZIP: {}", e)
            }));
        }
    };

    HttpResponse::Ok()
        .content_type("application/zip")
        .insert_header(("Content-Disposition", "attachment; filename=\"agent_subtypes.zip\""))
        .body(buf)
}

/// Export a single agent subtype as a ZIP of its agent folder.
async fn export_single_subtype(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    use std::io::{Cursor, Write};

    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let key = path.into_inner();
    if types::get_subtype_config(&key).is_none() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Agent subtype '{}' not found", key)
        }));
    }

    let agents_dir = crate::config::runtime_agents_dir();
    let folder = agents_dir.join(&key);
    if !folder.is_dir() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Agent folder '{}' not found on disk", key)
        }));
    }

    let buf = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let options = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    if let Err(e) = add_agent_folder_to_zip(&mut zip, &folder, &key, options) {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": format!("Failed to create ZIP: {}", e)
        }));
    }

    let buf = match zip.finish() {
        Ok(b) => b.into_inner(),
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to create ZIP: {}", e)
            }));
        }
    };

    HttpResponse::Ok()
        .content_type("application/zip")
        .insert_header(("Content-Disposition", format!("attachment; filename=\"{}.zip\"", key)))
        .body(buf)
}

/// Recursively add all files in an agent folder to a ZIP archive.
fn add_agent_folder_to_zip<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    folder: &std::path::Path,
    prefix: &str,
    options: zip::write::FileOptions,
) -> Result<(), String> {
    let entries = std::fs::read_dir(folder)
        .map_err(|e| format!("Failed to read directory {}: {}", folder.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            // Recurse into subdirectories (e.g. hooks/)
            let sub_prefix = format!("{}/{}", prefix, name);
            add_agent_folder_to_zip(zip, &path, &sub_prefix, options)?;
        } else if path.is_file() {
            let rel_path = format!("{}/{}", prefix, name);
            let content = std::fs::read(&path)
                .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
            zip.start_file(&rel_path, options)
                .map_err(|e| format!("Failed to start ZIP entry '{}': {}", rel_path, e))?;
            std::io::Write::write_all(zip, &content)
                .map_err(|e| format!("Failed to write ZIP entry '{}': {}", rel_path, e))?;
        }
    }

    Ok(())
}

#[derive(Deserialize)]
struct ImportRequest {
    ron: String,
    /// If true, delete all existing subtypes before importing
    #[serde(default)]
    replace: bool,
}

/// Import agent subtypes from RON (writes to disk).
async fn import_subtypes(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<ImportRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    // Parse RON
    let configs: Vec<AgentSubtypeConfig> = match ron::from_str(&body.ron) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Invalid RON: {}", e)
            }));
        }
    };

    if configs.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "No subtypes found in import data"
        }));
    }

    let agents_dir = crate::config::runtime_agents_dir();

    // Check limit
    if !body.replace {
        let existing_count = types::all_subtype_configs_unfiltered().len();
        if existing_count + configs.len() > MAX_SUBTYPES {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!(
                    "Import would exceed maximum of {} subtypes ({} existing + {} imported)",
                    MAX_SUBTYPES, existing_count, configs.len()
                )
            }));
        }
    }

    // If replace mode, delete all existing first
    if body.replace {
        if agents_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&agents_dir) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if !name.starts_with('.') {
                            let _ = std::fs::remove_dir_all(entry.path());
                        }
                    }
                }
            }
        }
    }

    // Write imported subtypes to disk
    let mut imported = 0;
    let mut errors = Vec::new();
    for config in &configs {
        if config.key.is_empty() || !config.key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            errors.push(format!("Invalid key: '{}'", config.key));
            continue;
        }
        match loader::write_agent_folder(&agents_dir, config) {
            Ok(_) => imported += 1,
            Err(e) => errors.push(format!("Failed to import '{}': {}", config.key, e)),
        }
    }

    loader::reload_registry_from_disk();

    let mut response = serde_json::json!({
        "success": true,
        "imported": imported,
        "total": configs.len(),
        "message": format!("Imported {} of {} subtypes", imported, configs.len())
    });

    if !errors.is_empty() {
        response["errors"] = serde_json::json!(errors);
    }

    HttpResponse::Ok().json(response)
}

// --- StarkHub featured/install ---

/// GET /api/agent-subtypes/featured_remote — get featured agent subtypes from StarkHub
async fn featured_remote(
    _data: web::Data<AppState>,
    _req: HttpRequest,
) -> impl Responder {
    let client = crate::integrations::starkhub_client::StarkHubClient::new();
    let featured = match client.list_agent_subtypes().await {
        Ok(f) => f,
        Err(e) => {
            log::error!("[AGENTS] Failed to fetch agent subtypes from StarkHub: {}", e);
            return HttpResponse::BadGateway().json(serde_json::json!({
                "error": format!("Failed to fetch from StarkHub: {}", e)
            }));
        }
    };

    // Filter out already-installed agent subtypes
    let existing = types::all_subtype_configs_unfiltered();
    let existing_keys: std::collections::HashSet<String> =
        existing.iter().map(|c| c.key.clone()).collect();

    let filtered: Vec<_> = featured
        .into_iter()
        .filter(|a| !existing_keys.contains(&a.key))
        .collect();

    HttpResponse::Ok().json(filtered)
}

// --- StarkHub publish/install ---

#[derive(Deserialize)]
struct InstallFromHubRequest {
    username: String,
    slug: String,
}

/// POST /api/agent-subtypes/publish/{key} — publish an agent subtype to StarkHub
async fn publish_to_hub(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let key = path.into_inner();

    // Read the auth token from the request
    let auth_token = match req
        .headers()
        .get("X-StarkHub-Token")
        .and_then(|h| h.to_str().ok())
    {
        Some(t) => t.to_string(),
        None => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "X-StarkHub-Token header required for publishing"
            }));
        }
    };

    let agents_dir = crate::config::runtime_agents_dir();
    let agent_folder = agents_dir.join(&key);

    if !agent_folder.is_dir() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Agent subtype '{}' not found on disk", key)
        }));
    }

    // Read agent.md
    let agent_md_path = agent_folder.join("agent.md");
    let raw_agent_md = match std::fs::read_to_string(&agent_md_path) {
        Ok(content) => content,
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to read agent.md: {}", e)
            }));
        }
    };

    let client = crate::integrations::starkhub_client::StarkHubClient::new();

    // Publish agent.md as the main record
    let result = match client.publish_agent_subtype(&raw_agent_md, &auth_token).await {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::BadGateway().json(serde_json::json!({
                "error": format!("Failed to publish to StarkHub: {}", e)
            }));
        }
    };

    let username = result["username"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let slug = result["slug"].as_str().unwrap_or(&key).to_string();

    // Upload additional files (everything except agent.md)
    let mut uploaded_files = Vec::new();
    let mut skipped_files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&agent_folder) {
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name == "agent.md" || !entry.path().is_file() {
                continue;
            }
            match std::fs::read_to_string(entry.path()) {
                Ok(content) => {
                    match client
                        .upload_agent_subtype_file(&username, &slug, &file_name, &content, &auth_token)
                        .await
                    {
                        Ok(_) => uploaded_files.push(file_name),
                        Err(e) => {
                            log::warn!("[AGENTS] Failed to upload file '{}': {}", file_name, e);
                            skipped_files.push(file_name);
                        }
                    }
                }
                Err(_) => {
                    log::warn!("[AGENTS] Skipping binary file '{}' (not UTF-8)", file_name);
                    skipped_files.push(file_name);
                }
            }
        }
    }

    let mut resp = serde_json::json!({
        "success": true,
        "slug": slug,
        "username": username,
        "uploaded_files": uploaded_files,
        "message": result.get("message").and_then(|m| m.as_str()).unwrap_or("Published"),
    });
    if !skipped_files.is_empty() {
        resp["skipped_files"] = serde_json::json!(skipped_files);
    }
    HttpResponse::Ok().json(resp)
}

/// POST /api/agent-subtypes/install — install an agent subtype from StarkHub
async fn install_from_hub(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<InstallFromHubRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let auth_token = req
        .headers()
        .get("X-StarkHub-Token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_string();

    let client = crate::integrations::starkhub_client::StarkHubClient::new();

    // Try ZIP bundle download first (faster — single request for all files)
    if let Ok(Some(zip_bytes)) = client
        .download_bundle("agent-subtypes", &body.username, &body.slug, &auth_token)
        .await
    {
        match extract_agent_zip(&zip_bytes) {
            Ok(files) => {
                // Find agent.md in the extracted files
                let agent_md = files.iter().find(|(name, _)| name == "agent.md");
                if let Some((_, md_content)) = agent_md {
                    let raw_agent_md = String::from_utf8_lossy(md_content).to_string();

                    match loader::parse_agent_file(&raw_agent_md) {
                        Ok(config) => {
                            let agents_dir = crate::config::runtime_agents_dir();
                            let agent_folder = agents_dir.join(&config.key);

                            if let Err(e) = std::fs::create_dir_all(&agent_folder) {
                                return HttpResponse::InternalServerError().json(serde_json::json!({
                                    "error": format!("Failed to create agent folder: {}", e)
                                }));
                            }

                            // Write all extracted files
                            let mut downloaded_files = Vec::new();
                            for (name, content) in &files {
                                let file_path = agent_folder.join(name);
                                if let Some(parent) = file_path.parent() {
                                    let _ = std::fs::create_dir_all(parent);
                                }
                                if let Err(e) = std::fs::write(&file_path, content) {
                                    log::warn!("Failed to write file '{}': {}", name, e);
                                } else {
                                    downloaded_files.push(name.clone());
                                }
                            }

                            loader::reload_registry_from_disk();

                            return HttpResponse::Ok().json(serde_json::json!({
                                "success": true,
                                "key": config.key,
                                "label": config.label,
                                "files": downloaded_files,
                                "message": format!("Installed agent subtype '{}' from @{}/{}", config.key, body.username, body.slug),
                            }));
                        }
                        Err(e) => {
                            log::warn!("[AGENTS] ZIP bundle agent.md parse failed, falling back: {}", e);
                        }
                    }
                } else {
                    log::warn!("[AGENTS] ZIP bundle missing agent.md, falling back to individual downloads");
                }
            }
            Err(e) => {
                log::warn!("[AGENTS] ZIP bundle extract failed, falling back: {}", e);
            }
        }
    }

    // Fallback: individual file downloads (legacy items without bundles)
    let raw_agent_md = match client
        .download_agent_subtype(&body.username, &body.slug, &auth_token)
        .await
    {
        Ok(md) => md,
        Err(e) => {
            return HttpResponse::BadGateway().json(serde_json::json!({
                "error": format!("Failed to download from StarkHub: {}", e)
            }));
        }
    };

    let config = match loader::parse_agent_file(&raw_agent_md) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Failed to parse downloaded agent.md: {}", e)
            }));
        }
    };

    let agents_dir = crate::config::runtime_agents_dir();
    let agent_folder = agents_dir.join(&config.key);

    if let Err(e) = std::fs::create_dir_all(&agent_folder) {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": format!("Failed to create agent folder: {}", e)
        }));
    }

    if let Err(e) = std::fs::write(agent_folder.join("agent.md"), &raw_agent_md) {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": format!("Failed to write agent.md: {}", e)
        }));
    }

    let mut downloaded_files = vec!["agent.md".to_string()];
    if let Ok(files) = client
        .list_agent_subtype_files(&body.username, &body.slug)
        .await
    {
        for file_summary in &files {
            if let Ok(file_detail) = client
                .get_agent_subtype_file(&body.username, &body.slug, &file_summary.file_name)
                .await
            {
                let file_path = agent_folder.join(&file_detail.file_name);
                if let Err(e) = std::fs::write(&file_path, &file_detail.content) {
                    log::warn!("Failed to write file '{}': {}", file_detail.file_name, e);
                } else {
                    downloaded_files.push(file_detail.file_name);
                }
            }
        }
    }

    loader::reload_registry_from_disk();

    HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "key": config.key,
        "label": config.label,
        "files": downloaded_files,
        "message": format!("Installed agent subtype '{}' from @{}/{}", config.key, body.username, body.slug),
    }))
}

/// Extract files from an agent subtype ZIP bundle.
/// Returns a vec of (relative_path, content_bytes).
fn extract_agent_zip(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use std::io::{Cursor, Read};
    use zip::ZipArchive;

    let cursor = Cursor::new(data);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("Failed to read ZIP: {}", e))?;

    let mut files = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read ZIP entry: {}", e))?;

        let name = file.name().to_string();
        if name.ends_with('/') {
            continue;
        }

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| format!("Failed to read '{}': {}", name, e))?;

        files.push((name, buf));
    }

    Ok(files)
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/agent-subtypes")
            .route("", web::get().to(list_subtypes))
            .route("/reset-defaults", web::post().to(reset_defaults))
            .route("/export", web::get().to(export_subtypes))
            .route("/import", web::post().to(import_subtypes))
            .route("/featured_remote", web::get().to(featured_remote))
            .route("/install", web::post().to(install_from_hub))
            .route("/publish/{key}", web::post().to(publish_to_hub))
            .route("/{key}/export", web::get().to(export_single_subtype))
            .route("/{key}", web::get().to(get_subtype))
            .route("", web::post().to(create_subtype))
            .route("/{key}", web::put().to(update_subtype))
            .route("/{key}", web::delete().to(delete_subtype)),
    );
}
