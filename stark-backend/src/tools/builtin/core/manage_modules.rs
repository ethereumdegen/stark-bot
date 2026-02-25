//! Module management tool — install, uninstall, enable, disable, list, search StarkHub, install remote, import ZIP, export
//!
//! Modules are standalone microservices. This tool manages the bot's
//! record of which modules are installed/enabled and hot-registers their tools.
//! It also integrates with StarkHub (hub.starkbot.ai) for discovering and
//! downloading remote modules. Supports importing modules from ZIP files and
//! exporting module manifests for publishing.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct ManageModulesTool {
    definition: ToolDefinition,
}

impl ManageModulesTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "action".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Action: 'list' available modules, 'install' a builtin module, 'uninstall', 'enable', 'disable', 'status', 'search_hub' to search StarkHub, 'install_remote' to download from StarkHub, 'update' to check for updates, 'import_zip' to install from a ZIP file, or 'export' to get module manifest for publishing".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "list".to_string(),
                    "install".to_string(),
                    "uninstall".to_string(),
                    "enable".to_string(),
                    "disable".to_string(),
                    "status".to_string(),
                    "search_hub".to_string(),
                    "install_remote".to_string(),
                    "update".to_string(),
                    "import_zip".to_string(),
                    "export".to_string(),
                ]),
            },
        );

        properties.insert(
            "name".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Module name. For remote modules use '@username/slug' format (e.g. '@ethereumdegen/wallet-monitor'). For local modules, just the name.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "query".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Search query for 'search_hub' action".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "File path for 'import_zip' action (path to the ZIP file on disk)".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ManageModulesTool {
            definition: ToolDefinition {
                name: "manage_modules".to_string(),
                description: "Manage StarkBot plugin modules. List, install/uninstall local modules, search StarkHub for remote modules, download and install modules from StarkHub, import modules from ZIP files, or export module manifests for publishing.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["action".to_string()],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct ModuleParams {
    action: String,
    name: Option<String>,
    query: Option<String>,
    path: Option<String>,
}

/// Parse "@username/slug" into (username, slug).
fn parse_remote_name(name: &str) -> Option<(&str, &str)> {
    let name = name.strip_prefix('@').unwrap_or(name);
    let parts: Vec<&str> = name.splitn(2, '/').collect();
    if parts.len() == 2 {
        Some((parts[0], parts[1]))
    } else {
        None
    }
}

#[async_trait]
impl Tool for ManageModulesTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ModuleParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let db = match context.database.as_ref() {
            Some(db) => db,
            None => return ToolResult::error("Database not available"),
        };

        match params.action.as_str() {
            "list" => {
                let registry = crate::modules::ModuleRegistry::new();
                let installed = db.list_installed_modules().unwrap_or_default();

                let mut output = String::from("**Available Modules**\n\n");

                for module in registry.available_modules() {
                    let installed_entry = installed.iter().find(|m| m.module_name == module.name());
                    let status = match installed_entry {
                        Some(e) if e.enabled => "installed & enabled",
                        Some(_) => "installed (disabled)",
                        None => "not installed",
                    };

                    let source = installed_entry
                        .map(|e| e.source.as_str())
                        .unwrap_or("builtin");

                    output.push_str(&format!(
                        "**{}** v{} — {}\n  Status: {} | Source: {} | Service: {} | Tools: {} | Dashboard: {}\n\n",
                        module.name(),
                        module.version(),
                        module.description(),
                        status,
                        source,
                        module.service_url(),
                        if module.has_tools() { "yes" } else { "no" },
                        if module.has_dashboard() { "yes" } else { "no" },
                    ));
                }

                ToolResult::success(output)
            }

            "install" => {
                let name = match params.name.as_deref() {
                    Some(n) => n,
                    None => return ToolResult::error("'name' is required for 'install' action"),
                };

                if db.is_module_installed(name).unwrap_or(false) {
                    return ToolResult::error(format!("Module '{}' is already installed. Use 'enable' to re-enable it.", name));
                }

                let registry = crate::modules::ModuleRegistry::new();
                let module = match registry.get(name) {
                    Some(m) => m,
                    None => return ToolResult::error(format!("Unknown module: '{}'. Use action='list' to see available modules, or 'search_hub' to find remote modules.", name)),
                };

                match db.install_module(
                    name,
                    module.description(),
                    module.version(),
                    module.has_tools(),
                    module.has_dashboard(),
                ) {
                    Ok(_entry) => {
                        let mut result_parts = vec![
                            format!("Module '{}' installed successfully!", name),
                            format!("Service URL: {}", module.service_url()),
                        ];

                        if module.has_tools() {
                            result_parts.push("Tools registered (available after restart or on next session).".to_string());
                        }
                        if module.has_dashboard() {
                            result_parts.push(format!("Dashboard: {}/", module.service_url()));
                        }

                        // Install skill if module provides one
                        if let Some(skill_md) = module.skill_content() {
                            if let Some(skill_registry) = context.skill_registry.as_ref() {
                                match skill_registry.create_skill_from_markdown(skill_md) {
                                    Ok(_) => result_parts.push("Skill installed.".to_string()),
                                    Err(e) => result_parts.push(format!("Warning: Failed to install skill: {}", e)),
                                }
                            }
                        }

                        ToolResult::success(result_parts.join("\n"))
                    }
                    Err(e) => ToolResult::error(format!("Failed to install module: {}", e)),
                }
            }

            "uninstall" => {
                let name = match params.name.as_deref() {
                    Some(n) => n,
                    None => return ToolResult::error("'name' is required for 'uninstall' action"),
                };
                match db.uninstall_module(name) {
                    Ok(true) => ToolResult::success(format!(
                        "Module '{}' uninstalled. The service continues running independently.",
                        name
                    )),
                    Ok(false) => ToolResult::error(format!("Module '{}' is not installed", name)),
                    Err(e) => ToolResult::error(format!("Failed to uninstall: {}", e)),
                }
            }

            "enable" => {
                let name = match params.name.as_deref() {
                    Some(n) => n,
                    None => return ToolResult::error("'name' is required for 'enable' action"),
                };
                match db.set_module_enabled(name, true) {
                    Ok(true) => ToolResult::success(format!(
                        "Module '{}' enabled. Tools are now active.",
                        name
                    )),
                    Ok(false) => ToolResult::error(format!("Module '{}' is not installed", name)),
                    Err(e) => ToolResult::error(format!("Failed to enable: {}", e)),
                }
            }

            "disable" => {
                let name = match params.name.as_deref() {
                    Some(n) => n,
                    None => return ToolResult::error("'name' is required for 'disable' action"),
                };
                match db.set_module_enabled(name, false) {
                    Ok(true) => ToolResult::success(format!(
                        "Module '{}' disabled. Tools hidden. Service continues running.",
                        name
                    )),
                    Ok(false) => ToolResult::error(format!("Module '{}' is not installed", name)),
                    Err(e) => ToolResult::error(format!("Failed to disable: {}", e)),
                }
            }

            "status" => {
                let name = match params.name.as_deref() {
                    Some(n) => n,
                    None => return ToolResult::error("'name' is required for 'status' action"),
                };

                let registry = crate::modules::ModuleRegistry::new();
                let module = match registry.get(name) {
                    Some(m) => m,
                    None => return ToolResult::error(format!("Unknown module: '{}'", name)),
                };

                match db.get_installed_module(name) {
                    Ok(Some(m)) => {
                        ToolResult::success(json!({
                            "module": m.module_name,
                            "version": m.version,
                            "enabled": m.enabled,
                            "description": m.description,
                            "has_tools": m.has_tools,
                            "has_dashboard": m.has_dashboard,
                            "source": m.source,
                            "author": m.author,
                            "service_url": module.service_url(),
                            "installed_at": m.installed_at.to_rfc3339(),
                        }).to_string())
                    }
                    Ok(None) => ToolResult::error(format!("Module '{}' is not installed", name)),
                    Err(e) => ToolResult::error(format!("Failed to get status: {}", e)),
                }
            }

            "search_hub" => {
                let query = match params.query.as_deref().or(params.name.as_deref()) {
                    Some(q) => q,
                    None => return ToolResult::error("'query' or 'name' is required for 'search_hub' action"),
                };

                let client = crate::integrations::starkhub_client::StarkHubClient::new();
                match client.search_modules(query).await {
                    Ok(modules) => {
                        if modules.is_empty() {
                            return ToolResult::success(format!("No modules found on StarkHub for '{}'", query));
                        }

                        let mut output = format!("**StarkHub Search Results for '{}'**\n\n", query);
                        for m in &modules {
                            let author = m.author_username.as_deref().unwrap_or(&m.author_address[..10]);
                            output.push_str(&format!(
                                "**{}** v{} by @{} — {}\n  Tools: [{}] | {} installs\n  Install: manage_modules(action=\"install_remote\", name=\"@{}/{}\")\n\n",
                                m.name, m.version, author, m.description,
                                m.tools_provided.join(", "),
                                m.install_count,
                                author, m.slug,
                            ));
                        }
                        ToolResult::success(output)
                    }
                    Err(e) => ToolResult::error(format!("StarkHub search failed: {}", e)),
                }
            }

            "install_remote" => {
                let name = match params.name.as_deref() {
                    Some(n) => n,
                    None => return ToolResult::error("'name' is required for 'install_remote'. Use '@username/slug' format."),
                };

                let (username, slug) = match parse_remote_name(name) {
                    Some(parts) => parts,
                    None => return ToolResult::error("Invalid name format. Use '@username/slug' (e.g. '@ethereumdegen/wallet-monitor')"),
                };

                // Check if already installed
                if db.is_module_installed(slug).unwrap_or(false) {
                    return ToolResult::error(format!("Module '{}' is already installed. Use 'update' to check for newer versions.", slug));
                }

                let client = crate::integrations::starkhub_client::StarkHubClient::new();

                // Get module info from StarkHub
                let module_info = match client.get_module(username, slug).await {
                    Ok(m) => m,
                    Err(e) => return ToolResult::error(e),
                };

                let platform = crate::integrations::starkhub_client::current_platform();

                // Get download info
                let download_info = match client.get_download_info(username, slug, platform).await {
                    Ok(d) => d,
                    Err(e) => return ToolResult::error(format!(
                        "No binary available for platform '{}': {}. Available platforms: {}",
                        platform, e,
                        module_info.platforms.iter().map(|p| p.platform.as_str()).collect::<Vec<_>>().join(", ")
                    )),
                };

                // Download binary archive
                let archive_bytes = match client.download_binary(&download_info.download_url).await {
                    Ok(bytes) => bytes,
                    Err(e) => return ToolResult::error(e),
                };

                // Verify SHA-256 checksum
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&archive_bytes);
                let computed_hash = format!("{:x}", hasher.finalize());

                if computed_hash != download_info.sha256_checksum {
                    return ToolResult::error(format!(
                        "Checksum mismatch! Expected {}, got {}. Download may be corrupted.",
                        download_info.sha256_checksum, computed_hash
                    ));
                }

                // Extract to ~/.starkbot/modules/<slug>/
                let modules_dir = std::env::var("STARKBOT_MODULES_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| {
                        std::env::var("HOME")
                            .map(std::path::PathBuf::from)
                            .unwrap_or_else(|_| std::path::PathBuf::from("."))
                            .join(".starkbot")
                            .join("modules")
                    });

                let module_dir = modules_dir.join(slug);
                if let Err(e) = std::fs::create_dir_all(&module_dir) {
                    return ToolResult::error(format!("Failed to create module directory: {}", e));
                }

                // Extract tar.gz archive
                use std::io::Read;
                let decoder = flate2::read::GzDecoder::new(&archive_bytes[..]);
                let mut archive = tar::Archive::new(decoder);
                if let Err(e) = archive.unpack(&module_dir) {
                    return ToolResult::error(format!("Failed to extract module archive: {}", e));
                }

                // Make service binary executable
                let manifest_path = module_dir.join("module.toml");
                let binary_path = module_dir.join("bin").join(format!("{}-service", slug));

                #[cfg(unix)]
                if binary_path.exists() {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &binary_path,
                        std::fs::Permissions::from_mode(0o755),
                    );
                }

                // Register in database
                let author_str = module_info.author.username
                    .as_deref()
                    .map(|u| format!("@{}", u))
                    .unwrap_or_else(|| module_info.author.wallet_address.clone());

                match db.install_module_full(
                    slug,
                    &module_info.description,
                    &module_info.version,
                    !module_info.tools_provided.is_empty(),
                    false, // has_dashboard from manifest
                    "starkhub",
                    Some(&manifest_path.to_string_lossy()),
                    Some(&binary_path.to_string_lossy()),
                    Some(&author_str),
                    Some(&computed_hash),
                ) {
                    Ok(_) => {
                        let mut result = vec![
                            format!("Module '@{}/{}' installed from StarkHub!", username, slug),
                            format!("Version: {}", module_info.version),
                            format!("Location: {}", module_dir.display()),
                        ];
                        if !module_info.tools_provided.is_empty() {
                            result.push(format!("Tools: {}", module_info.tools_provided.join(", ")));
                        }
                        result.push("Restart StarkBot to activate the module and its tools.".to_string());
                        ToolResult::success(result.join("\n"))
                    }
                    Err(e) => ToolResult::error(format!("Failed to register module: {}", e)),
                }
            }

            "update" => {
                let name = match params.name.as_deref() {
                    Some(n) => n,
                    None => return ToolResult::error("'name' is required for 'update' action"),
                };

                let installed = match db.get_installed_module(name) {
                    Ok(Some(m)) => m,
                    Ok(None) => return ToolResult::error(format!("Module '{}' is not installed", name)),
                    Err(e) => return ToolResult::error(format!("Failed to check module: {}", e)),
                };

                if installed.source != "starkhub" {
                    return ToolResult::error(format!(
                        "Module '{}' is a built-in module (source: {}). Only StarkHub modules can be updated.",
                        name, installed.source
                    ));
                }

                let author = installed.author.as_deref().unwrap_or("unknown");
                let username = author.strip_prefix('@').unwrap_or(author);

                let client = crate::integrations::starkhub_client::StarkHubClient::new();
                match client.get_module(username, name).await {
                    Ok(remote) => {
                        if remote.version == installed.version {
                            ToolResult::success(format!(
                                "Module '{}' is up to date (v{}).",
                                name, installed.version
                            ))
                        } else {
                            ToolResult::success(format!(
                                "Update available for '{}': v{} -> v{}\nRun: manage_modules(action=\"install_remote\", name=\"@{}/{}\")",
                                name, installed.version, remote.version, username, name
                            ))
                        }
                    }
                    Err(e) => ToolResult::error(format!("Failed to check for updates: {}", e)),
                }
            }

            "import_zip" => {
                let zip_path = match params.path.as_deref() {
                    Some(p) => p,
                    None => return ToolResult::error("'path' is required for 'import_zip' action (path to the ZIP file on disk)"),
                };

                // Read ZIP file from disk
                let zip_data = match std::fs::read(zip_path) {
                    Ok(data) => data,
                    Err(e) => return ToolResult::error(format!("Failed to read ZIP file '{}': {}", zip_path, e)),
                };

                // ZIP bomb protection
                if zip_data.len() > crate::disk_quota::MAX_SKILL_ZIP_BYTES {
                    return ToolResult::error(format!(
                        "ZIP file too large ({} bytes). Maximum allowed: 10MB.",
                        zip_data.len()
                    ));
                }

                // Parse the ZIP
                let parsed = match crate::modules::zip_parser::parse_module_zip(&zip_data) {
                    Ok(p) => p,
                    Err(e) => return ToolResult::error(format!("Failed to parse module ZIP: {}", e)),
                };

                let module_name = parsed.module_name.clone();

                // Check if already installed
                if db.is_module_installed(&module_name).unwrap_or(false) {
                    return ToolResult::error(format!(
                        "Module '{}' is already installed. Uninstall it first, then re-import.",
                        module_name
                    ));
                }

                // Extract to runtime modules directory
                let modules_dir = crate::config::runtime_modules_dir();
                let module_dir = match crate::modules::zip_parser::extract_module_to_dir(&parsed, &modules_dir) {
                    Ok(dir) => dir,
                    Err(e) => return ToolResult::error(format!("Failed to extract module: {}", e)),
                };

                // Read manifest info for DB registration
                let manifest = &parsed.manifest;
                let has_tools = !manifest.tools.is_empty();
                let has_dashboard = manifest.service.has_dashboard;
                let author = manifest.module.author.as_deref();
                let manifest_path = module_dir.join("module.toml");

                // Register in database
                match db.install_module_full(
                    &module_name,
                    &manifest.module.description,
                    &manifest.module.version,
                    has_tools,
                    has_dashboard,
                    "zip_import",
                    Some(&manifest_path.to_string_lossy()),
                    None, // no binary path
                    author,
                    None, // no checksum
                ) {
                    Ok(_) => {
                        // Install bundled skill if present (prefer skill_dir, fall back to content_file)
                        if let Some(ref skill_cfg) = manifest.skill {
                            if let Some(skill_registry) = context.skill_registry.as_ref() {
                                if let Some(ref dir) = skill_cfg.skill_dir {
                                    let skill_dir = module_dir.join(dir);
                                    if skill_dir.is_dir() {
                                        match skill_registry.create_skill_from_module_dir(&skill_dir).await {
                                            Ok(_) => log::info!("[MODULE] Installed skill from module '{}' (skill dir)", module_name),
                                            Err(e) => log::warn!("[MODULE] Failed to install skill dir from module '{}': {}", module_name, e),
                                        }
                                    }
                                } else if let Some(ref content_file) = skill_cfg.content_file {
                                    let skill_path = module_dir.join(content_file);
                                    if let Ok(skill_content) = std::fs::read_to_string(&skill_path) {
                                        match skill_registry.create_skill_from_markdown(&skill_content) {
                                            Ok(_) => log::info!("[MODULE] Installed skill from module '{}'", module_name),
                                            Err(e) => log::warn!("[MODULE] Failed to install skill from module '{}': {}", module_name, e),
                                        }
                                    }
                                }
                            }
                        }

                        let mut result = vec![
                            format!("Module '{}' imported from ZIP successfully!", module_name),
                            format!("Version: {}", manifest.module.version),
                            format!("Location: {}", module_dir.display()),
                        ];
                        if has_tools {
                            let tool_names: Vec<_> = manifest.tools.iter().map(|t| t.name.as_str()).collect();
                            result.push(format!("Tools: {}", tool_names.join(", ")));
                        }
                        result.push("Restart StarkBot to activate the module and its tools.".to_string());
                        ToolResult::success(result.join("\n"))
                    }
                    Err(e) => ToolResult::error(format!("Failed to register module: {}", e)),
                }
            }

            "export" => {
                let name = match params.name.as_deref() {
                    Some(n) => n,
                    None => return ToolResult::error("'name' is required for 'export' action"),
                };

                let modules_dir = crate::config::runtime_modules_dir();
                let manifest_path = modules_dir.join(name).join("module.toml");

                if !manifest_path.exists() {
                    return ToolResult::error(format!(
                        "Module '{}' not found at {}. Only modules in the runtime modules directory can be exported.",
                        name, manifest_path.display()
                    ));
                }

                match std::fs::read_to_string(&manifest_path) {
                    Ok(content) => {
                        ToolResult::success(format!(
                            "**Module manifest for '{}':**\n\n```toml\n{}\n```",
                            name, content
                        ))
                    }
                    Err(e) => ToolResult::error(format!("Failed to read module.toml: {}", e)),
                }
            }

            _ => ToolResult::error(format!(
                "Unknown action: '{}'. Use 'list', 'install', 'uninstall', 'enable', 'disable', 'status', 'search_hub', 'install_remote', 'update', 'import_zip', or 'export'.",
                params.action
            )),
        }
    }

    fn safety_level(&self) -> crate::tools::types::ToolSafetyLevel {
        crate::tools::types::ToolSafetyLevel::Standard
    }
}
