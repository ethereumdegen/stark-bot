//! HTTP API endpoints for the module/plugin system
//!
//! Modules are standalone microservices. This controller manages their
//! install/uninstall/enable/disable state in the bot's database and
//! hot-registers/unregisters their tools at runtime.

use actix_multipart::Multipart;
use actix_web::{web, HttpRequest, HttpResponse};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use crate::AppState;

use super::validate_session;

/// Kill the service process listening on a given port (if any).
fn kill_service_on_port(port: u16) {
    let output = std::process::Command::new("lsof")
        .args(["-ti", &format!("tcp:{}", port)])
        .output();
    if let Ok(out) = output {
        let pids = String::from_utf8_lossy(&out.stdout);
        let my_pid = std::process::id().to_string();
        for pid_str in pids.split_whitespace() {
            let pid = pid_str.trim();
            if !pid.is_empty() && pid != my_pid {
                log::info!("[MODULE] Killing service process PID {} on port {}", pid, port);
                let _ = std::process::Command::new("kill").arg(pid).output();
            }
        }
    }
}

/// Start a module's service if not already running.
/// Checks the module manifest for a `command` field first, falling back to binary discovery.
fn start_module_service(module_name: &str, port: u16, db: &crate::db::Database) {
    // Already running?
    if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
        log::info!("[MODULE] {} already running on port {} — skipping start", module_name, port);
        return;
    }

    // Check if the module has a command in its manifest
    let registry = crate::modules::ModuleRegistry::new();
    if let Some(module) = registry.get(module_name) {
        if let Some(command) = module.manifest_command() {
            let module_dir = match module.module_dir() {
                Some(dir) => dir.clone(),
                None => {
                    log::warn!("[MODULE] {} has command but no module_dir — cannot start", module_name);
                    return;
                }
            };
            let mut cmd = std::process::Command::new("sh");
            cmd.arg("-c").arg(&command);
            cmd.current_dir(&module_dir);
            cmd.stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit());
            cmd.env("MODULE_PORT", port.to_string());

            // Pass API keys from database
            if let Ok(Some(key)) = db.get_api_key("ALCHEMY_API_KEY") {
                cmd.env("ALCHEMY_API_KEY", &key.api_key);
            } else if let Ok(val) = std::env::var("ALCHEMY_API_KEY") {
                if !val.is_empty() {
                    cmd.env("ALCHEMY_API_KEY", &val);
                }
            }

            match cmd.spawn() {
                Ok(_) => log::info!("[MODULE] Started {} via `{}` (port {})", module_name, command, port),
                Err(e) => log::error!("[MODULE] Failed to start {} via `{}`: {}", module_name, command, e),
            }
            return;
        }
    }

    // Fallback: binary discovery
    let self_exe = std::env::current_exe().unwrap_or_default();
    let exe_dir = self_exe.parent().unwrap_or(std::path::Path::new("."));

    let binary_name = module_name.replace('_', "-") + "-service";
    let exe_path = exe_dir.join(&binary_name);
    if !exe_path.exists() {
        log::warn!("[MODULE] Service binary not found: {} — cannot start", exe_path.display());
        return;
    }

    let mut cmd = std::process::Command::new(&exe_path);
    cmd.stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    // Pass API keys from database
    if let Ok(Some(key)) = db.get_api_key("ALCHEMY_API_KEY") {
        cmd.env("ALCHEMY_API_KEY", &key.api_key);
    } else if let Ok(val) = std::env::var("ALCHEMY_API_KEY") {
        if !val.is_empty() {
            cmd.env("ALCHEMY_API_KEY", &val);
        }
    }

    match cmd.spawn() {
        Ok(_) => log::info!("[MODULE] Started {} (port {})", binary_name, port),
        Err(e) => log::error!("[MODULE] Failed to start {}: {}", binary_name, e),
    }
}

#[derive(Serialize)]
struct ModuleInfo {
    name: String,
    description: String,
    version: String,
    installed: bool,
    enabled: bool,
    has_tools: bool,
    has_dashboard: bool,
    has_skill: bool,
    has_ext_endpoints: bool,
    ext_endpoint_count: usize,
    service_url: String,
    service_port: u16,
    installed_at: Option<String>,
}

#[derive(Deserialize)]
struct ModuleActionRequest {
    action: String, // "install", "uninstall", "enable", "disable"
}

/// Activate a module at runtime: register its tools.
async fn activate_module(data: &web::Data<AppState>, module_name: &str) {
    let registry = crate::modules::ModuleRegistry::new();
    let module = match registry.get(module_name) {
        Some(m) => m,
        None => {
            log::warn!("[MODULE] activate_module: unknown module '{}'", module_name);
            return;
        }
    };

    if module.has_tools() {
        for tool in module.create_tools() {
            log::info!("[MODULE] Hot-registered tool: {} (from {})", tool.name(), module_name);
            data.tool_registry.register(tool);
        }
    }
}

/// Deactivate a module at runtime: unregister its tools.
async fn deactivate_module(data: &web::Data<AppState>, module_name: &str) {
    let registry = crate::modules::ModuleRegistry::new();
    let module = match registry.get(module_name) {
        Some(m) => m,
        None => {
            log::warn!("[MODULE] deactivate_module: unknown module '{}'", module_name);
            return;
        }
    };

    if module.has_tools() {
        for tool in module.create_tools() {
            let name = tool.name();
            if data.tool_registry.unregister(&name) {
                log::info!("[MODULE] Unregistered tool: {} (from {})", name, module_name);
            }
        }
    }
}

/// GET /api/modules — list all available modules with install status
async fn list_modules(data: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = validate_session(&data, &req) {
        return resp;
    }

    let registry = crate::modules::ModuleRegistry::new();
    let installed = data.db.list_installed_modules().unwrap_or_default();

    let mut modules = Vec::new();
    for module in registry.available_modules() {
        let installed_entry = installed.iter().find(|m| m.module_name == module.name());

        let ext_endpoints = module.ext_endpoint_list();
        modules.push(ModuleInfo {
            name: module.name().to_string(),
            description: module.description().to_string(),
            version: module.version().to_string(),
            installed: installed_entry.is_some(),
            enabled: installed_entry.map(|e| e.enabled).unwrap_or(false),
            has_tools: module.has_tools(),
            has_dashboard: module.has_dashboard(),
            has_skill: module.has_skill(),
            has_ext_endpoints: !ext_endpoints.is_empty(),
            ext_endpoint_count: ext_endpoints.len(),
            service_url: module.service_url(),
            service_port: module.default_port(),
            installed_at: installed_entry.map(|e| e.installed_at.to_rfc3339()),
        });
    }

    HttpResponse::Ok().json(modules)
}

/// POST /api/modules/{name} — install, uninstall, enable, or disable a module
async fn module_action(
    data: web::Data<AppState>,
    req: HttpRequest,
    name: web::Path<String>,
    body: web::Json<ModuleActionRequest>,
) -> HttpResponse {
    if let Err(resp) = validate_session(&data, &req) {
        return resp;
    }

    let name = name.into_inner();
    let action = &body.action;

    match action.as_str() {
        "install" => {
            if data.db.is_module_installed(&name).unwrap_or(false) {
                return HttpResponse::Conflict().json(serde_json::json!({
                    "error": format!("Module '{}' is already installed", name)
                }));
            }

            let registry = crate::modules::ModuleRegistry::new();
            let module = match registry.get(&name) {
                Some(m) => m,
                None => return HttpResponse::NotFound().json(serde_json::json!({
                    "error": format!("Unknown module: '{}'", name)
                })),
            };

            match data.db.install_module(
                &name,
                module.description(),
                module.version(),
                module.has_tools(),
                module.has_dashboard(),
            ) {
                Ok(_) => {
                    // Install skill if provided
                    if let Some(skill_md) = module.skill_content() {
                        let _ = data.skill_registry.create_skill_from_markdown(skill_md);
                    }

                    // Hot-activate: register tools immediately
                    activate_module(&data, &name).await;

                    HttpResponse::Ok().json(serde_json::json!({
                        "status": "installed",
                        "message": format!("Module '{}' installed and activated.", name),
                        "service_url": module.service_url(),
                    }))
                }
                Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": format!("Install failed: {}", e)
                })),
            }
        }

        "uninstall" => {
            deactivate_module(&data, &name).await;

            // Delete module skill
            {
                let registry = crate::modules::ModuleRegistry::new();
                if let Some(module) = registry.get(&name) {
                    if let Some(skill_md) = module.skill_content() {
                        if let Ok((metadata, _)) = crate::skills::zip_parser::parse_skill_md(skill_md) {
                            let _ = data.skill_registry.delete_skill(&metadata.name);
                        }
                    }
                }
            }

            match data.db.uninstall_module(&name) {
                Ok(true) => HttpResponse::Ok().json(serde_json::json!({
                    "status": "uninstalled",
                    "message": format!("Module '{}' deactivated and uninstalled.", name)
                })),
                Ok(false) => HttpResponse::NotFound().json(serde_json::json!({
                    "error": format!("Module '{}' is not installed", name)
                })),
                Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": format!("Uninstall failed: {}", e)
                })),
            }
        }

        "enable" => {
            let registry = crate::modules::ModuleRegistry::new();
            let module = match registry.get(&name) {
                Some(m) => m,
                None => return HttpResponse::NotFound().json(serde_json::json!({
                    "error": format!("Unknown module: '{}'", name)
                })),
            };

            // Auto-install if not already installed
            let already_installed = data.db.is_module_installed(&name).unwrap_or(false);
            if !already_installed {
                if let Err(e) = data.db.install_module(
                    &name,
                    module.description(),
                    module.version(),
                    module.has_tools(),
                    module.has_dashboard(),
                ) {
                    return HttpResponse::InternalServerError().json(serde_json::json!({
                        "error": format!("Install failed: {}", e)
                    }));
                }
            }

            // Ensure the module's skill is created and enabled
            if let Some(skill_md) = module.skill_content() {
                // Create if it doesn't exist yet (idempotent — create_skill skips duplicates)
                let _ = data.skill_registry.create_skill_from_markdown(skill_md);
                // Always mark it enabled in case it was previously disabled
                if let Ok((metadata, _)) = crate::skills::zip_parser::parse_skill_md(skill_md) {
                    data.skill_registry.set_enabled(&metadata.name, true);
                }
            }

            match data.db.set_module_enabled(&name, true) {
                Ok(true) => {
                    activate_module(&data, &name).await;
                    // Start the module's service process if not already running
                    start_module_service(&name, module.default_port(), &data.db);
                    HttpResponse::Ok().json(serde_json::json!({
                        "status": "enabled",
                        "message": format!("Module '{}' enabled.", name)
                    }))
                }
                Ok(false) => HttpResponse::NotFound().json(serde_json::json!({
                    "error": format!("Module '{}' not found", name)
                })),
                Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": format!("Enable failed: {}", e)
                })),
            }
        }

        "disable" => {
            deactivate_module(&data, &name).await;

            // Disable module skill and kill the service process
            {
                let registry = crate::modules::ModuleRegistry::new();
                if let Some(module) = registry.get(&name) {
                    if let Some(skill_md) = module.skill_content() {
                        if let Ok((metadata, _)) = crate::skills::zip_parser::parse_skill_md(skill_md) {
                            data.skill_registry.set_enabled(&metadata.name, false);
                        }
                    }
                    // Kill the module's service process
                    kill_service_on_port(module.default_port());
                }
            }

            match data.db.set_module_enabled(&name, false) {
                Ok(true) => HttpResponse::Ok().json(serde_json::json!({
                    "status": "disabled",
                    "message": format!("Module '{}' deactivated, disabled, and service stopped.", name)
                })),
                Ok(false) => HttpResponse::NotFound().json(serde_json::json!({
                    "error": format!("Module '{}' is not installed", name)
                })),
                Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": format!("Disable failed: {}", e)
                })),
            }
        }

        "restart" => {
            let registry = crate::modules::ModuleRegistry::new();
            match registry.get(&name) {
                Some(module) => {
                    let port = module.default_port();
                    kill_service_on_port(port);
                    // Brief pause to let the port free up
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    start_module_service(&name, port, &data.db);
                    HttpResponse::Ok().json(serde_json::json!({
                        "status": "restarted",
                        "message": format!("Module '{}' service restarted on port {}.", name, port)
                    }))
                }
                None => HttpResponse::NotFound().json(serde_json::json!({
                    "error": format!("Unknown module: '{}'", name)
                })),
            }
        }

        _ => HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("Unknown action: '{}'. Use 'install', 'uninstall', 'enable', 'disable', or 'restart'.", action)
        })),
    }
}

/// GET /api/modules/{name}/dashboard — get module-specific dashboard data
async fn module_dashboard(
    data: web::Data<AppState>,
    req: HttpRequest,
    name: web::Path<String>,
) -> HttpResponse {
    if let Err(resp) = validate_session(&data, &req) {
        return resp;
    }

    let name = name.into_inner();

    // Check if module is installed and enabled
    let installed = data.db.list_installed_modules().unwrap_or_default();
    let module_entry = installed.iter().find(|m| m.module_name == name);
    match module_entry {
        None => return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Module '{}' is not installed", name)
        })),
        Some(entry) if !entry.enabled => return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("Module '{}' is disabled", name)
        })),
        _ => {}
    }

    let registry = crate::modules::ModuleRegistry::new();
    let module = match registry.get(&name) {
        Some(m) => m,
        None => return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Unknown module: '{}'", name)
        })),
    };

    if !module.has_dashboard() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Module '{}' does not have a dashboard", name)
        }));
    }

    match module.dashboard_data(&data.db).await {
        Some(data) => HttpResponse::Ok().json(data),
        None => HttpResponse::Ok().json(serde_json::json!({})),
    }
}

/// GET /api/modules/{name}/status — proxy health check to the module's service
async fn module_status(
    data: web::Data<AppState>,
    req: HttpRequest,
    name: web::Path<String>,
) -> HttpResponse {
    if let Err(resp) = validate_session(&data, &req) {
        return resp;
    }

    let name = name.into_inner();

    let registry = crate::modules::ModuleRegistry::new();
    let module = match registry.get(&name) {
        Some(m) => m,
        None => return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Unknown module: '{}'", name)
        })),
    };

    let url = format!("{}/rpc/status", module.service_url());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap_or_default();

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let body = resp.text().await.unwrap_or_default();
            HttpResponse::Ok()
                .content_type("application/json")
                .body(body)
        }
        Ok(_) => HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "status": "unhealthy",
            "error": "Service returned non-200 response"
        })),
        Err(_) => HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "status": "offline",
            "error": "Service unreachable"
        })),
    }
}

/// POST /api/modules/reload — full resync of all module tools
async fn reload_modules(data: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    if let Err(resp) = validate_session(&data, &req) {
        return resp;
    }

    let module_registry = crate::modules::ModuleRegistry::new();
    let mut activated = Vec::new();
    let mut deactivated = Vec::new();

    // 1. Unregister all module tools
    for module in module_registry.available_modules() {
        if module.has_tools() {
            for tool in module.create_tools() {
                data.tool_registry.unregister(&tool.name());
            }
        }
    }

    // 2. Read DB for installed + enabled modules, activate tools and sync skills
    let installed = data.db.list_installed_modules().unwrap_or_default();
    for entry in &installed {
        if let Some(module) = module_registry.get(&entry.module_name) {
            if entry.enabled {
                // Re-register tools
                if module.has_tools() {
                    for tool in module.create_tools() {
                        log::info!("[MODULE] Reload: registered tool '{}' (from {})", tool.name(), entry.module_name);
                        data.tool_registry.register(tool);
                    }
                }
                // Ensure skill is created and enabled
                if let Some(skill_md) = module.skill_content() {
                    let _ = data.skill_registry.create_skill_from_markdown(skill_md);
                    if let Ok((metadata, _)) = crate::skills::zip_parser::parse_skill_md(skill_md) {
                        data.skill_registry.set_enabled(&metadata.name, true);
                    }
                }
                activated.push(entry.module_name.clone());
            } else {
                // Ensure skill is disabled for disabled modules
                if let Some(skill_md) = module.skill_content() {
                    if let Ok((metadata, _)) = crate::skills::zip_parser::parse_skill_md(skill_md) {
                        data.skill_registry.set_enabled(&metadata.name, false);
                    }
                }
                deactivated.push(entry.module_name.clone());
            }
        }
    }

    log::info!("[MODULE] Reload complete: {} activated, {} inactive", activated.len(), deactivated.len());

    HttpResponse::Ok().json(serde_json::json!({
        "status": "reloaded",
        "activated": activated,
        "deactivated": deactivated,
        "message": format!("Reloaded {} module(s).", activated.len())
    }))
}

/// GET /api/modules/{name}/proxy/{path:.*} — reverse-proxy to the module's internal service.
/// This allows the frontend iframe to reach module dashboards without exposing their ports.
async fn module_proxy(
    data: web::Data<AppState>,
    path: web::Path<(String, String)>,
    req: HttpRequest,
) -> HttpResponse {
    if let Err(resp) = validate_session(&data, &req) {
        return resp;
    }

    let (name, sub_path) = path.into_inner();

    let registry = crate::modules::ModuleRegistry::new();
    let module = match registry.get(&name) {
        Some(m) => m,
        None => return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Unknown module: '{}'", name)
        })),
    };

    if !module.has_dashboard() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("Module '{}' does not have a dashboard", name)
        }));
    }

    let target_url = if sub_path.is_empty() {
        format!("{}/", module.service_url())
    } else {
        format!("{}/{}", module.service_url(), sub_path)
    };

    // Forward query string if present
    let target_url = if let Some(qs) = req.uri().query() {
        format!("{}?{}", target_url, qs)
    } else {
        target_url
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    match client.get(&target_url).send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            let body = resp.bytes().await.unwrap_or_default();

            HttpResponse::build(actix_web::http::StatusCode::from_u16(status).unwrap_or(actix_web::http::StatusCode::BAD_GATEWAY))
                .content_type(content_type)
                .body(body)
        }
        Err(e) => HttpResponse::BadGateway().json(serde_json::json!({
            "error": format!("Could not reach module service: {}", e)
        })),
    }
}

/// POST /api/modules/upload — import a module from a ZIP file upload
async fn upload_module(
    data: web::Data<AppState>,
    req: HttpRequest,
    mut payload: Multipart,
) -> HttpResponse {
    if let Err(resp) = validate_session(&data, &req) {
        return resp;
    }

    // Read the uploaded file
    let mut file_data: Vec<u8> = Vec::new();

    while let Some(item) = payload.next().await {
        match item {
            Ok(mut field) => {
                while let Some(chunk) = field.next().await {
                    match chunk {
                        Ok(bytes) => file_data.extend_from_slice(&bytes),
                        Err(e) => {
                            return HttpResponse::BadRequest().json(serde_json::json!({
                                "error": format!("Failed to read upload data: {}", e)
                            }));
                        }
                    }
                }
            }
            Err(e) => {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!("Failed to process upload: {}", e)
                }));
            }
        }
    }

    if file_data.is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "No file uploaded"
        }));
    }

    // ZIP bomb protection
    if file_data.len() > crate::disk_quota::MAX_SKILL_ZIP_BYTES {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!(
                "Upload rejected: file size ({} bytes) exceeds the 10MB limit.",
                file_data.len()
            )
        }));
    }

    // Parse the module ZIP
    let parsed = match crate::modules::zip_parser::parse_module_zip(&file_data) {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Failed to parse module ZIP: {}", e)
            }));
        }
    };

    let module_name = parsed.module_name.clone();

    // Check if already installed
    if data.db.is_module_installed(&module_name).unwrap_or(false) {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": format!("Module '{}' is already installed. Uninstall it first.", module_name)
        }));
    }

    // Extract to runtime modules directory
    let modules_dir = crate::config::runtime_modules_dir();
    let module_dir = match crate::modules::zip_parser::extract_module_to_dir(&parsed, &modules_dir) {
        Ok(dir) => dir,
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to extract module: {}", e)
            }));
        }
    };

    let manifest = &parsed.manifest;
    let has_tools = !manifest.tools.is_empty();
    let has_dashboard = manifest.service.has_dashboard;
    let author = manifest.module.author.as_deref();
    let manifest_path = module_dir.join("module.toml");

    // Register in database
    match data.db.install_module_full(
        &module_name,
        &manifest.module.description,
        &manifest.module.version,
        has_tools,
        has_dashboard,
        "zip_import",
        Some(&manifest_path.to_string_lossy()),
        None,
        author,
        None,
    ) {
        Ok(_) => {
            // Hot-activate: register tools immediately
            activate_module(&data, &module_name).await;

            // Install bundled skill if present
            if let Some(ref skill_cfg) = manifest.skill {
                let skill_path = module_dir.join(&skill_cfg.content_file);
                if let Ok(skill_content) = std::fs::read_to_string(&skill_path) {
                    let _ = data.skill_registry.create_skill_from_markdown(&skill_content);
                }
            }

            HttpResponse::Ok().json(serde_json::json!({
                "status": "imported",
                "module": module_name,
                "version": manifest.module.version,
                "description": manifest.module.description,
                "has_tools": has_tools,
                "has_dashboard": has_dashboard,
                "location": module_dir.display().to_string(),
                "message": format!("Module '{}' imported and activated.", module_name)
            }))
        }
        Err(e) => {
            // Clean up extracted files on DB failure
            let _ = std::fs::remove_dir_all(&module_dir);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to register module: {}", e)
            }))
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/modules")
            .route("", web::get().to(list_modules))
            .route("/upload", web::post().to(upload_module))
            .route("/reload", web::post().to(reload_modules))
            .route("/{name}/dashboard", web::get().to(module_dashboard))
            .route("/{name}/status", web::get().to(module_status))
            .route("/{name}/proxy/{path:.*}", web::get().to(module_proxy))
            .route("/{name}", web::post().to(module_action)),
    );
}
