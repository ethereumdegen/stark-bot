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
    /// Script source code (included in detail views)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl From<&DbSkillScript> for ScriptInfo {
    fn from(script: &DbSkillScript) -> Self {
        ScriptInfo {
            name: script.name.clone(),
            language: script.language.clone(),
            code: Some(script.code.clone()),
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

// --- Skill Graph types ---

#[derive(Serialize)]
pub struct SkillGraphNode {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct SkillGraphEdge {
    pub source: i64,
    pub target: i64,
    pub association_type: String,
    pub strength: f64,
}

#[derive(Serialize)]
pub struct SkillGraphResponse {
    pub success: bool,
    pub nodes: Vec<SkillGraphNode>,
    pub edges: Vec<SkillGraphEdge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct SkillSearchResult {
    pub skill_id: i64,
    pub name: String,
    pub description: String,
    pub similarity: f32,
}

#[derive(Serialize)]
pub struct SkillSearchResponse {
    pub success: bool,
    pub results: Vec<SkillSearchResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct SkillEmbeddingStatsResponse {
    pub success: bool,
    pub total_skills: i64,
    pub skills_with_embeddings: i64,
    pub coverage_percent: f64,
}

#[derive(Deserialize)]
pub struct SkillSearchQuery {
    pub query: String,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct CreateAssociationRequest {
    pub source_skill_id: i64,
    pub target_skill_id: i64,
    pub association_type: Option<String>,
    pub strength: Option<f64>,
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/skills")
            .route("", web::get().to(list_skills))
            .route("/upload", web::post().to(upload_skill))
            .route("/reload", web::post().to(reload_skills))
            .route("/graph", web::get().to(get_skill_graph))
            .route("/graph/search", web::get().to(search_skills_by_embedding))
            .route("/embeddings/stats", web::get().to(get_skill_embedding_stats))
            .route("/embeddings/backfill", web::post().to(backfill_skill_embeddings))
            .route("/associations", web::post().to(create_skill_association))
            .route("/associations/rebuild", web::post().to(rebuild_skill_associations))
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
        Ok(count) => {
            // Background-backfill embeddings for any newly loaded skills
            if let Some(ref engine) = state.hybrid_search {
                let emb_gen = engine.embedding_generator().clone();
                let db = state.db.clone();
                tokio::spawn(async move {
                    if let Err(e) = crate::skills::embeddings::backfill_skill_embeddings(&db, &emb_gen).await {
                        log::warn!("[SKILL-EMB] Post-reload backfill failed: {}", e);
                    }
                });
            }

            HttpResponse::Ok().json(OperationResponse {
                success: true,
                message: Some(format!("Loaded {} skills from disk", count)),
                error: None,
                count: Some(state.skill_registry.len()),
            })
        }
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
            // Load the new skill's ABIs and presets into memory
            if let Some(skill_id) = db_skill.id {
                // Load ABIs for this skill into the in-memory index
                if let Ok(abis) = state.db.get_skill_abis(skill_id) {
                    for abi in abis {
                        crate::web3::register_abi_content(&abi.name, &abi.content);
                    }
                }
                // Load presets for this skill into the in-memory index
                crate::tools::presets::load_skill_presets_from_db(&state.db, skill_id);
            }

            // Auto-generate embedding + rebuild associations for the new skill
            if let Some(skill_id) = db_skill.id {
                if let Some(ref engine) = state.hybrid_search {
                    let emb_gen = engine.embedding_generator().clone();
                    let db = state.db.clone();
                    let skill_name = db_skill.name.clone();
                    let emb_text = crate::skills::embeddings::build_skill_embedding_text(&db_skill);
                    tokio::spawn(async move {
                        if let Ok(embedding) = emb_gen.generate(&emb_text).await {
                            let dims = embedding.len() as i32;
                            if let Err(e) = db.upsert_skill_embedding(skill_id, &embedding, "remote", dims) {
                                log::warn!("[SKILL-EMB] Failed to auto-embed skill '{}': {}", skill_name, e);
                            } else {
                                log::info!("[SKILL-EMB] Auto-embedded skill '{}'", skill_name);
                                // Rebuild associations for this skill
                                if let Err(e) = crate::skills::embeddings::rebuild_associations_for_skill(&db, skill_id, 0.30).await {
                                    log::warn!("[SKILL-ASSOC] Failed to rebuild associations for '{}': {}", skill_name, e);
                                }
                            }
                        }
                    });
                }
            }

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
        requires_api_keys: existing.metadata.requires_api_keys.clone(),
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

    // Auto-regenerate embedding + rebuild associations for the updated skill
    if let Some(ref engine) = state.hybrid_search {
        if let Ok(Some(updated)) = state.db.get_skill(&name) {
            if let Some(skill_id) = updated.id {
                let emb_gen = engine.embedding_generator().clone();
                let db = state.db.clone();
                let skill_name = name.clone();
                tokio::spawn(async move {
                    let text = crate::skills::embeddings::build_skill_embedding_text(&updated);
                    if let Ok(embedding) = emb_gen.generate(&text).await {
                        let dims = embedding.len() as i32;
                        if let Err(e) = db.upsert_skill_embedding(skill_id, &embedding, "remote", dims) {
                            log::warn!("[SKILL-EMB] Failed to re-embed skill '{}': {}", skill_name, e);
                        } else {
                            // Rebuild associations for this skill
                            if let Err(e) = crate::skills::embeddings::rebuild_associations_for_skill(&db, skill_id, 0.30).await {
                                log::warn!("[SKILL-ASSOC] Failed to rebuild associations for '{}': {}", skill_name, e);
                            }
                        }
                    }
                });
            }
        }
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

    // Get skill ID before deleting (for association cleanup)
    let skill_id = state.db.get_skill(&name).ok().flatten().and_then(|s| s.id);

    if skill_id.is_none() && !state.skill_registry.has_skill(&name) {
        return HttpResponse::NotFound().json(OperationResponse {
            success: false,
            message: None,
            error: Some(format!("Skill '{}' not found", name)),
            count: None,
        });
    }

    match state.skill_registry.delete_skill(&name) {
        Ok(true) => {
            // Clean up associations for the deleted skill
            if let Some(sid) = skill_id {
                if let Err(e) = state.db.delete_skill_associations_for(sid) {
                    log::warn!("[SKILL-ASSOC] Failed to clean up associations for deleted skill '{}': {}", name, e);
                }
            }
            HttpResponse::Ok().json(OperationResponse {
                success: true,
                message: Some(format!("Skill '{}' deleted", name)),
                error: None,
                count: None,
            })
        }
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

// --- Skill Graph Endpoints ---

async fn get_skill_graph(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let skills = match state.db.list_skills() {
        Ok(s) => s,
        Err(e) => {
            return HttpResponse::InternalServerError().json(SkillGraphResponse {
                success: false,
                nodes: vec![],
                edges: vec![],
                error: Some(format!("Failed to list skills: {}", e)),
            });
        }
    };

    let nodes: Vec<SkillGraphNode> = skills
        .iter()
        .filter_map(|s| {
            s.id.map(|id| SkillGraphNode {
                id,
                name: s.name.clone(),
                description: s.description.clone(),
                tags: s.tags.clone(),
                enabled: s.enabled,
            })
        })
        .collect();

    let edges = match state.db.list_all_skill_associations() {
        Ok(assocs) => assocs
            .into_iter()
            .map(|a| SkillGraphEdge {
                source: a.source_skill_id,
                target: a.target_skill_id,
                association_type: a.association_type,
                strength: a.strength,
            })
            .collect(),
        Err(_) => vec![],
    };

    HttpResponse::Ok().json(SkillGraphResponse {
        success: true,
        nodes,
        edges,
        error: None,
    })
}

async fn search_skills_by_embedding(
    state: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<SkillSearchQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let limit = query.limit.unwrap_or(5);

    // Try embedding search first if engine is available
    if let Some(ref engine) = state.hybrid_search {
        let emb_gen = engine.embedding_generator().clone();
        // Check if any embeddings exist
        let has_embeddings = state.db.count_skill_embeddings().unwrap_or(0) > 0;

        if has_embeddings {
            match crate::skills::embeddings::search_skills(&state.db, &emb_gen, &query.query, limit, 0.20).await {
                Ok(matches) if !matches.is_empty() => {
                    let results: Vec<SkillSearchResult> = matches
                        .into_iter()
                        .map(|(skill, sim)| SkillSearchResult {
                            skill_id: skill.id.unwrap_or(0),
                            name: skill.name,
                            description: skill.description,
                            similarity: sim,
                        })
                        .collect();
                    return HttpResponse::Ok().json(SkillSearchResponse {
                        success: true,
                        results,
                        error: None,
                    });
                }
                Ok(_) => { /* empty results — fall through to text search */ }
                Err(e) => {
                    log::warn!("[SKILL-SEARCH] Embedding search failed, falling back to text: {}", e);
                }
            }
        }
    }

    // Fallback: text-based search (works without embeddings)
    match crate::skills::embeddings::search_skills_text(&state.db, &query.query, limit) {
        Ok(matches) => {
            let results: Vec<SkillSearchResult> = matches
                .into_iter()
                .map(|(skill, sim)| SkillSearchResult {
                    skill_id: skill.id.unwrap_or(0),
                    name: skill.name,
                    description: skill.description,
                    similarity: sim,
                })
                .collect();
            HttpResponse::Ok().json(SkillSearchResponse {
                success: true,
                results,
                error: None,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(SkillSearchResponse {
            success: false,
            results: vec![],
            error: Some(e),
        }),
    }
}

async fn get_skill_embedding_stats(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let total_skills = state.db.list_enabled_skills()
        .map(|s| s.len() as i64)
        .unwrap_or(0);
    let skills_with_embeddings = state.db.count_skill_embeddings().unwrap_or(0);
    let coverage = if total_skills > 0 {
        (skills_with_embeddings as f64 / total_skills as f64) * 100.0
    } else {
        0.0
    };

    HttpResponse::Ok().json(SkillEmbeddingStatsResponse {
        success: true,
        total_skills,
        skills_with_embeddings,
        coverage_percent: coverage,
    })
}

async fn backfill_skill_embeddings(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let engine = match &state.hybrid_search {
        Some(e) => e,
        None => {
            return HttpResponse::ServiceUnavailable().json(OperationResponse {
                success: false,
                message: None,
                error: Some("Embedding engine not configured".to_string()),
                count: None,
            });
        }
    };

    let emb_gen = engine.embedding_generator().clone();

    match crate::skills::embeddings::backfill_skill_embeddings(&state.db, &emb_gen).await {
        Ok(count) => HttpResponse::Ok().json(OperationResponse {
            success: true,
            message: Some(format!("Generated {} skill embeddings", count)),
            error: None,
            count: Some(count),
        }),
        Err(e) => HttpResponse::InternalServerError().json(OperationResponse {
            success: false,
            message: None,
            error: Some(e),
            count: None,
        }),
    }
}

async fn create_skill_association(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateAssociationRequest>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let assoc_type = body.association_type.as_deref().unwrap_or("related");
    let strength = body.strength.unwrap_or(0.5);

    match state.db.create_skill_association(
        body.source_skill_id,
        body.target_skill_id,
        assoc_type,
        strength,
        None,
    ) {
        Ok(id) => HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "id": id,
        })),
        Err(e) => HttpResponse::InternalServerError().json(OperationResponse {
            success: false,
            message: None,
            error: Some(format!("Failed to create association: {}", e)),
            count: None,
        }),
    }
}

async fn rebuild_skill_associations(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    let engine = match &state.hybrid_search {
        Some(e) => e,
        None => {
            return HttpResponse::ServiceUnavailable().json(OperationResponse {
                success: false,
                message: None,
                error: Some("Embedding engine not configured".to_string()),
                count: None,
            });
        }
    };

    let emb_gen = engine.embedding_generator().clone();

    match crate::skills::embeddings::rebuild_skill_associations(&state.db, &emb_gen, 0.30).await {
        Ok(count) => HttpResponse::Ok().json(OperationResponse {
            success: true,
            message: Some(format!("Rebuilt {} skill associations", count)),
            error: None,
            count: Some(count),
        }),
        Err(e) => HttpResponse::InternalServerError().json(OperationResponse {
            success: false,
            message: None,
            error: Some(e),
            count: None,
        }),
    }
}
