use actix_multipart::Multipart;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::skills::{DbSkillScript, Skill};
use crate::AppState;

#[derive(Serialize)]
pub struct SkillsListResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<SkillInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub version: String,
    pub source: String,
    pub enabled: bool,
    pub requires_tools: Vec<String>,
    pub requires_binaries: Vec<String>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
}

impl From<&Skill> for SkillInfo {
    fn from(skill: &Skill) -> Self {
        SkillInfo {
            name: skill.metadata.name.clone(),
            description: skill.metadata.description.clone(),
            version: skill.metadata.version.clone(),
            source: skill.source.as_str().to_string(),
            enabled: skill.enabled,
            requires_tools: skill.metadata.requires_tools.clone(),
            requires_binaries: skill.metadata.requires_binaries.clone(),
            tags: skill.metadata.tags.clone(),
            homepage: skill.metadata.homepage.clone(),
            metadata: skill.metadata.metadata.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct SkillDetailResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<SkillDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct SkillDetail {
    pub name: String,
    pub description: String,
    pub version: String,
    pub source: String,
    pub path: String,
    pub enabled: bool,
    pub requires_tools: Vec<String>,
    pub requires_binaries: Vec<String>,
    pub missing_binaries: Vec<String>,
    pub tags: Vec<String>,
    pub arguments: Vec<ArgumentInfo>,
    pub prompt_template: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scripts: Option<Vec<ScriptInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
}

#[derive(Serialize)]
pub struct ArgumentInfo {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub default: Option<String>,
}

#[derive(Serialize)]
pub struct ScriptInfo {
    pub name: String,
    pub language: String,
}

impl From<&DbSkillScript> for ScriptInfo {
    fn from(script: &DbSkillScript) -> Self {
        ScriptInfo {
            name: script.name.clone(),
            language: script.language.clone(),
        }
    }
}

impl From<&Skill> for SkillDetail {
    fn from(skill: &Skill) -> Self {
        let missing_binaries = skill.check_binaries().err().unwrap_or_default();

        let arguments: Vec<ArgumentInfo> = skill
            .metadata
            .arguments
            .iter()
            .map(|(name, arg)| ArgumentInfo {
                name: name.clone(),
                description: arg.description.clone(),
                required: arg.required,
                default: arg.default.clone(),
            })
            .collect();

        SkillDetail {
            name: skill.metadata.name.clone(),
            description: skill.metadata.description.clone(),
            version: skill.metadata.version.clone(),
            source: skill.source.as_str().to_string(),
            path: skill.path.clone(),
            enabled: skill.enabled,
            requires_tools: skill.metadata.requires_tools.clone(),
            requires_binaries: skill.metadata.requires_binaries.clone(),
            missing_binaries,
            tags: skill.metadata.tags.clone(),
            arguments,
            prompt_template: skill.prompt_template.clone(),
            scripts: None,
            homepage: skill.metadata.homepage.clone(),
            metadata: skill.metadata.metadata.clone(),
        }
    }
}

#[derive(Deserialize)]
pub struct SetEnabledRequest {
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateSkillRequest {
    pub body: String,
}

#[derive(Serialize)]
pub struct OperationResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

#[derive(Serialize)]
pub struct UploadResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<SkillInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct ScriptsListResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scripts: Option<Vec<ScriptInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/skills")
            .route("", web::get().to(list_skills))
            .route("/upload", web::post().to(upload_skill))
            .route("/reload", web::post().to(reload_skills))
            .route("/{name}", web::get().to(get_skill))
            .route("/{name}", web::put().to(update_skill))
            .route("/{name}", web::delete().to(delete_skill))
            .route("/{name}/enabled", web::put().to(set_enabled))
            .route("/{name}/scripts", web::get().to(get_skill_scripts)),
    );
}

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
            return Err(HttpResponse::Unauthorized().json(OperationResponse {
                success: false,
                message: None,
                error: Some("No authorization token provided".to_string()),
                count: None,
            }));
        }
    };

    match state.db.validate_session(&token) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HttpResponse::Unauthorized().json(OperationResponse {
            success: false,
            message: None,
            error: Some("Invalid or expired session".to_string()),
            count: None,
        })),
        Err(e) => {
            log::error!("Failed to validate session: {}", e);
            Err(HttpResponse::InternalServerError().json(OperationResponse {
                success: false,
                message: None,
                error: Some("Internal server error".to_string()),
                count: None,
            }))
        }
    }
}

async fn list_skills(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let skills: Vec<SkillInfo> = state
        .skill_registry
        .list()
        .iter()
        .map(|s| s.into())
        .collect();

    HttpResponse::Ok().json(skills)
}

async fn get_skill(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let name = path.into_inner();

    match state.skill_registry.get(&name) {
        Some(skill) => {
            let mut detail: SkillDetail = (&skill).into();

            // Get associated scripts
            let scripts = state.skill_registry.get_skill_scripts(&name);
            if !scripts.is_empty() {
                detail.scripts = Some(scripts.iter().map(|s| s.into()).collect());
            }

            HttpResponse::Ok().json(SkillDetailResponse {
                success: true,
                skill: Some(detail),
                error: None,
            })
        }
        None => HttpResponse::NotFound().json(SkillDetailResponse {
            success: false,
            skill: None,
            error: Some(format!("Skill '{}' not found", name)),
        }),
    }
}

async fn set_enabled(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<SetEnabledRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let name = path.into_inner();

    if !state.skill_registry.has_skill(&name) {
        return HttpResponse::NotFound().json(OperationResponse {
            success: false,
            message: None,
            error: Some(format!("Skill '{}' not found", name)),
            count: None,
        });
    }

    // Update in registry (which updates the database)
    state.skill_registry.set_enabled(&name, body.enabled);

    let status = if body.enabled { "enabled" } else { "disabled" };
    HttpResponse::Ok().json(OperationResponse {
        success: true,
        message: Some(format!("Skill '{}' {}", name, status)),
        error: None,
        count: None,
    })
}

async fn reload_skills(state: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    match state.skill_registry.reload().await {
        Ok(count) => HttpResponse::Ok().json(OperationResponse {
            success: true,
            message: Some(format!("Loaded {} skills from disk", count)),
            error: None,
            count: Some(state.skill_registry.len()),
        }),
        Err(e) => {
            log::error!("Failed to reload skills: {}", e);
            HttpResponse::InternalServerError().json(OperationResponse {
                success: false,
                message: None,
                error: Some(format!("Failed to reload skills: {}", e)),
                count: None,
            })
        }
    }
}

async fn upload_skill(
    state: web::Data<AppState>,
    req: HttpRequest,
    mut payload: Multipart,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    // Read the uploaded file and capture filename
    let mut file_data: Vec<u8> = Vec::new();
    let mut filename: Option<String> = None;

    while let Some(item) = payload.next().await {
        match item {
            Ok(mut field) => {
                // Capture filename from content disposition
                if filename.is_none() {
                    filename = field.content_disposition()
                        .get_filename()
                        .map(|s| s.to_string());
                }

                while let Some(chunk) = field.next().await {
                    match chunk {
                        Ok(data) => file_data.extend_from_slice(&data),
                        Err(e) => {
                            return HttpResponse::BadRequest().json(UploadResponse {
                                success: false,
                                skill: None,
                                error: Some(format!("Failed to read upload data: {}", e)),
                            });
                        }
                    }
                }
            }
            Err(e) => {
                return HttpResponse::BadRequest().json(UploadResponse {
                    success: false,
                    skill: None,
                    error: Some(format!("Failed to process upload: {}", e)),
                });
            }
        }
    }

    if file_data.is_empty() {
        return HttpResponse::BadRequest().json(UploadResponse {
            success: false,
            skill: None,
            error: Some("No file uploaded".to_string()),
        });
    }

    // Reject uploads larger than 10MB (ZIP bomb protection)
    if file_data.len() > crate::disk_quota::MAX_SKILL_ZIP_BYTES {
        return HttpResponse::BadRequest().json(UploadResponse {
            success: false,
            skill: None,
            error: Some(format!(
                "Upload rejected: file size ({} bytes) exceeds the 10MB limit for skill uploads.",
                file_data.len()
            )),
        });
    }

    // Determine file type from filename or content
    let is_markdown = filename
        .as_ref()
        .map(|f| f.to_lowercase().ends_with(".md"))
        .unwrap_or(false);

    // Parse and create the skill based on file type
    let result = if is_markdown {
        // Parse as markdown file
        match String::from_utf8(file_data) {
            Ok(content) => state.skill_registry.create_skill_from_markdown(&content),
            Err(e) => Err(format!("Invalid UTF-8 in markdown file: {}", e)),
        }
    } else {
        // Parse as ZIP file
        state.skill_registry.create_skill_from_zip(&file_data)
    };

    match result {
        Ok(db_skill) => {
            let skill = db_skill.into_skill();
            HttpResponse::Ok().json(UploadResponse {
                success: true,
                skill: Some((&skill).into()),
                error: None,
            })
        }
        Err(e) => {
            log::error!("Failed to create skill: {}", e);
            HttpResponse::BadRequest().json(UploadResponse {
                success: false,
                skill: None,
                error: Some(e),
            })
        }
    }
}

async fn update_skill(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<UpdateSkillRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let name = path.into_inner();

    // Get the existing skill
    let existing = match state.skill_registry.get(&name) {
        Some(skill) => skill,
        None => {
            return HttpResponse::NotFound().json(SkillDetailResponse {
                success: false,
                skill: None,
                error: Some(format!("Skill '{}' not found", name)),
            });
        }
    };

    // Build an updated DbSkill with the new body
    let now = chrono::Utc::now().to_rfc3339();
    let db_skill = crate::skills::DbSkill {
        id: None,
        name: existing.metadata.name.clone(),
        description: existing.metadata.description.clone(),
        body: body.body.clone(),
        version: existing.metadata.version.clone(),
        author: existing.metadata.author.clone(),
        homepage: existing.metadata.homepage.clone(),
        metadata: existing.metadata.metadata.clone(),
        enabled: existing.enabled,
        requires_tools: existing.metadata.requires_tools.clone(),
        requires_binaries: existing.metadata.requires_binaries.clone(),
        arguments: existing.metadata.arguments.clone(),
        tags: existing.metadata.tags.clone(),
        subagent_type: existing.metadata.subagent_type.clone(),
        created_at: now.clone(),
        updated_at: now,
    };

    // Force-update in database (bypass version check)
    if let Err(e) = state.db.create_skill_force(&db_skill) {
        log::error!("Failed to update skill '{}': {}", name, e);
        return HttpResponse::InternalServerError().json(SkillDetailResponse {
            success: false,
            skill: None,
            error: Some(format!("Failed to update skill: {}", e)),
        });
    }

    // Re-fetch the updated skill
    match state.skill_registry.get(&name) {
        Some(skill) => {
            let mut detail: SkillDetail = (&skill).into();
            let scripts = state.skill_registry.get_skill_scripts(&name);
            if !scripts.is_empty() {
                detail.scripts = Some(scripts.iter().map(|s| s.into()).collect());
            }
            HttpResponse::Ok().json(SkillDetailResponse {
                success: true,
                skill: Some(detail),
                error: None,
            })
        }
        None => HttpResponse::InternalServerError().json(SkillDetailResponse {
            success: false,
            skill: None,
            error: Some("Skill not found after update".to_string()),
        }),
    }
}

async fn delete_skill(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let name = path.into_inner();

    if !state.skill_registry.has_skill(&name) {
        return HttpResponse::NotFound().json(OperationResponse {
            success: false,
            message: None,
            error: Some(format!("Skill '{}' not found", name)),
            count: None,
        });
    }

    match state.skill_registry.delete_skill(&name) {
        Ok(true) => HttpResponse::Ok().json(OperationResponse {
            success: true,
            message: Some(format!("Skill '{}' deleted", name)),
            error: None,
            count: None,
        }),
        Ok(false) => HttpResponse::NotFound().json(OperationResponse {
            success: false,
            message: None,
            error: Some(format!("Skill '{}' not found", name)),
            count: None,
        }),
        Err(e) => {
            log::error!("Failed to delete skill: {}", e);
            HttpResponse::InternalServerError().json(OperationResponse {
                success: false,
                message: None,
                error: Some(format!("Failed to delete skill: {}", e)),
                count: None,
            })
        }
    }
}

async fn get_skill_scripts(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let name = path.into_inner();

    if !state.skill_registry.has_skill(&name) {
        return HttpResponse::NotFound().json(ScriptsListResponse {
            success: false,
            scripts: None,
            error: Some(format!("Skill '{}' not found", name)),
        });
    }

    let scripts = state.skill_registry.get_skill_scripts(&name);
    let script_infos: Vec<ScriptInfo> = scripts.iter().map(|s| s.into()).collect();

    HttpResponse::Ok().json(ScriptsListResponse {
        success: true,
        scripts: Some(script_infos),
        error: None,
    })
}
