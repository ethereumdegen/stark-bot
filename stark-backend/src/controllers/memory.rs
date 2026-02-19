//! Memory controller - REST API for the unified DB-backed memory system
//!
//! Provides endpoints for browsing, searching, and viewing memories.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

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

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Serialize)]
struct MemoryEntry {
    /// Synthetic path for backward compatibility (e.g., "2024-01-15.md" or "MEMORY.md")
    path: String,
    /// Memory type: "daily_log" or "long_term"
    memory_type: String,
    /// Parsed date if this is a daily log
    date: Option<String>,
    /// Identity ID
    identity_id: Option<String>,
    /// Number of entries for this date/type
    entry_count: i64,
}

#[derive(Debug, Serialize)]
struct ListFilesResponse {
    success: bool,
    files: Vec<MemoryEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct MemoryItem {
    id: i64,
    content: String,
    memory_type: String,
    importance: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    log_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_type: Option<String>,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct ReadMemoriesResponse {
    success: bool,
    memory_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity_id: Option<String>,
    memories: Vec<MemoryItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct SearchResult {
    memory_id: i64,
    content: String,
    memory_type: String,
    importance: i64,
    score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    log_date: Option<String>,
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    success: bool,
    query: String,
    results: Vec<SearchResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct MemoryStatsResponse {
    success: bool,
    total_memories: i64,
    daily_log_count: i64,
    long_term_count: i64,
    identity_count: i64,
    identities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    earliest_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct AppendResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct MemoryInfoResponse {
    success: bool,
    backend: String,
    total_memories: i64,
}

// ============================================================================
// Request Types
// ============================================================================

#[derive(Debug, Deserialize)]
struct SearchQuery {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: i32,
    identity_id: Option<String>,
}

fn default_search_limit() -> i32 {
    20
}

#[derive(Debug, Deserialize)]
struct DailyLogQuery {
    date: Option<String>,
    identity_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LongTermQuery {
    identity_id: Option<String>,
    #[serde(default = "default_long_term_limit")]
    limit: Option<i32>,
}

fn default_long_term_limit() -> Option<i32> {
    None
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    identity_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AppendBody {
    content: String,
    identity_id: Option<String>,
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/memory/files - List memory dates and types
async fn list_files(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<ListQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let identity_id = query.identity_id.as_deref();
    let mut files: Vec<MemoryEntry> = Vec::new();

    // Add long_term entry with count
    let long_term_count = match data.db.get_long_term_memories(identity_id, 1000) {
        Ok(mems) => mems.len() as i64,
        Err(_) => 0,
    };
    if long_term_count > 0 {
        files.push(MemoryEntry {
            path: "MEMORY.md".to_string(),
            memory_type: "long_term".to_string(),
            date: None,
            identity_id: identity_id.map(|s| s.to_string()),
            entry_count: long_term_count,
        });
    }

    // Add daily_log entries for each date
    if let Ok(dates) = data.db.list_memory_dates(identity_id) {
        for date in dates {
            let count = match data.db.get_daily_log_memories(&date, identity_id, 1000) {
                Ok(mems) => mems.len() as i64,
                Err(_) => 0,
            };
            files.push(MemoryEntry {
                path: format!("{}.md", date),
                memory_type: "daily_log".to_string(),
                date: Some(date),
                identity_id: identity_id.map(|s| s.to_string()),
                entry_count: count,
            });
        }
    }

    HttpResponse::Ok().json(ListFilesResponse {
        success: true,
        files,
        error: None,
    })
}

/// GET /api/memory/search - Search memories with BM25 full-text search
async fn search(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<SearchQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let limit = query.limit.clamp(1, 100);

    match data.db.search_memories_fts(&query.query, query.identity_id.as_deref(), limit) {
        Ok(results) => {
            let results: Vec<SearchResult> = results
                .into_iter()
                .map(|(mem, rank)| SearchResult {
                    memory_id: mem.id,
                    content: if mem.content.chars().count() > 300 {
                        let truncated: String = mem.content.chars().take(300).collect();
                        format!("{}...", truncated)
                    } else {
                        mem.content
                    },
                    memory_type: mem.memory_type,
                    importance: mem.importance,
                    score: -rank, // Negate BM25 (returns negative)
                    identity_id: mem.identity_id,
                    log_date: mem.log_date,
                })
                .collect();

            HttpResponse::Ok().json(SearchResponse {
                success: true,
                query: query.query.clone(),
                results,
                error: None,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(SearchResponse {
            success: false,
            query: query.query.clone(),
            results: vec![],
            error: Some(format!("Search failed: {}", e)),
        }),
    }
}

/// Helper to convert MemoryRows to MemoryItems
fn rows_to_items(rows: Vec<crate::db::tables::memories::MemoryRow>) -> Vec<MemoryItem> {
    rows.into_iter()
        .map(|m| MemoryItem {
            id: m.id,
            content: m.content,
            memory_type: m.memory_type,
            importance: m.importance,
            identity_id: m.identity_id,
            log_date: m.log_date,
            source_type: m.source_type,
            created_at: m.created_at,
        })
        .collect()
}

/// GET /api/memory/daily - Get today's or a specific date's daily log
async fn get_daily_log(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<DailyLogQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let identity_id = query.identity_id.as_deref();

    let (date_str, memories) = if let Some(date_str) = &query.date {
        // Validate date format
        if chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").is_err() {
            return HttpResponse::BadRequest().json(ReadMemoriesResponse {
                success: false,
                memory_type: "daily_log".to_string(),
                date: None,
                identity_id: None,
                memories: vec![],
                error: Some("Invalid date format. Use YYYY-MM-DD".to_string()),
            });
        }
        match data.db.get_daily_log_memories(date_str, identity_id, 200) {
            Ok(mems) => (date_str.clone(), mems),
            Err(e) => {
                return HttpResponse::InternalServerError().json(ReadMemoriesResponse {
                    success: false,
                    memory_type: "daily_log".to_string(),
                    date: Some(date_str.clone()),
                    identity_id: identity_id.map(|s| s.to_string()),
                    memories: vec![],
                    error: Some(format!("Failed to read daily log: {}", e)),
                });
            }
        }
    } else {
        // Get today's log
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        match data.db.get_today_daily_log(identity_id, 200) {
            Ok(mems) => (today, mems),
            Err(e) => {
                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                return HttpResponse::Ok().json(ReadMemoriesResponse {
                    success: true,
                    memory_type: "daily_log".to_string(),
                    date: Some(today),
                    identity_id: identity_id.map(|s| s.to_string()),
                    memories: vec![],
                    error: Some(format!("No entries yet: {}", e)),
                });
            }
        }
    };

    HttpResponse::Ok().json(ReadMemoriesResponse {
        success: true,
        memory_type: "daily_log".to_string(),
        date: Some(date_str),
        identity_id: identity_id.map(|s| s.to_string()),
        memories: rows_to_items(memories),
        error: None,
    })
}

/// GET /api/memory/long-term - Get long-term memories
async fn get_long_term(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<LongTermQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let identity_id = query.identity_id.as_deref();
    let limit = query.limit.unwrap_or(100);

    match data.db.get_long_term_memories(identity_id, limit) {
        Ok(mems) => HttpResponse::Ok().json(ReadMemoriesResponse {
            success: true,
            memory_type: "long_term".to_string(),
            date: None,
            identity_id: identity_id.map(|s| s.to_string()),
            memories: rows_to_items(mems),
            error: None,
        }),
        Err(e) => HttpResponse::Ok().json(ReadMemoriesResponse {
            success: true,
            memory_type: "long_term".to_string(),
            date: None,
            identity_id: identity_id.map(|s| s.to_string()),
            memories: vec![],
            error: Some(format!("No long-term memories yet: {}", e)),
        }),
    }
}

/// POST /api/memory/daily - Append to today's daily log
async fn append_daily_log(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<AppendBody>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let identity_id = body.identity_id.as_deref();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    match data.db.insert_memory(
        "daily_log",
        &body.content,
        None, None, 5, identity_id, None, None, None,
        Some("api"), Some(&today),
    ) {
        Ok(_id) => HttpResponse::Ok().json(AppendResponse {
            success: true,
            message: Some("Added to daily log".to_string()),
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(AppendResponse {
            success: false,
            message: None,
            error: Some(format!("Failed to append: {}", e)),
        }),
    }
}

/// POST /api/memory/long-term - Append to long-term memory
async fn append_long_term(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<AppendBody>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let identity_id = body.identity_id.as_deref();

    match data.db.insert_memory(
        "long_term",
        &body.content,
        None, None, 5, identity_id, None, None, None,
        Some("api"), None,
    ) {
        Ok(_id) => HttpResponse::Ok().json(AppendResponse {
            success: true,
            message: Some("Added to long-term memory".to_string()),
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(AppendResponse {
            success: false,
            message: None,
            error: Some(format!("Failed to append: {}", e)),
        }),
    }
}

/// GET /api/memory/stats - Get memory statistics
async fn get_stats(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    match data.db.get_memory_stats() {
        Ok(stats) => HttpResponse::Ok().json(MemoryStatsResponse {
            success: true,
            total_memories: stats.total_memories,
            daily_log_count: stats.daily_log_count,
            long_term_count: stats.long_term_count,
            identity_count: stats.identity_count,
            identities: stats.identities,
            earliest_date: stats.earliest_date,
            latest_date: stats.latest_date,
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(MemoryStatsResponse {
            success: false,
            total_memories: 0,
            daily_log_count: 0,
            long_term_count: 0,
            identity_count: 0,
            identities: vec![],
            earliest_date: None,
            latest_date: None,
            error: Some(format!("Failed to get stats: {}", e)),
        }),
    }
}

/// POST /api/memory/reindex - No-op (FTS triggers handle sync automatically)
async fn reindex(_data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    // FTS is auto-synced via triggers on the memories table, so reindex is a no-op
    HttpResponse::Ok().json(AppendResponse {
        success: true,
        message: Some("FTS index is auto-synced via triggers. No reindex needed.".to_string()),
        error: None,
    })
}

/// GET /api/memory/info - Get memory system info
async fn memory_info(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let total = data.db.count_memories().unwrap_or(0);

    HttpResponse::Ok().json(MemoryInfoResponse {
        success: true,
        backend: "sqlite".to_string(),
        total_memories: total,
    })
}

// ============================================================================
// Graph & Association Types
// ============================================================================

#[derive(Debug, Serialize)]
struct GraphNode {
    id: i64,
    content: String,
    memory_type: String,
    importance: i32,
}

#[derive(Debug, Serialize)]
struct GraphEdge {
    source: i64,
    target: i64,
    association_type: String,
    strength: f64,
}

#[derive(Debug, Serialize)]
struct GraphResponse {
    success: bool,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct AssociationResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    associations: Option<Vec<AssociationItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<AssociationItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct AssociationItem {
    id: i64,
    source_memory_id: i64,
    target_memory_id: i64,
    association_type: String,
    strength: f64,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct CreateAssociationBody {
    source_memory_id: i64,
    target_memory_id: i64,
    #[serde(default = "default_association_type")]
    association_type: String,
    #[serde(default = "default_strength")]
    strength: f64,
}

fn default_association_type() -> String {
    "related".to_string()
}

fn default_strength() -> f64 {
    0.5
}

#[derive(Debug, Deserialize)]
struct AssociationQuery {
    memory_id: i64,
}

#[derive(Debug, Deserialize)]
struct DeleteAssociationPath {
    id: i64,
}

#[derive(Debug, Serialize)]
struct HybridSearchResponse {
    success: bool,
    query: String,
    mode: String,
    results: Vec<HybridSearchItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct HybridSearchItem {
    memory_id: i64,
    content: String,
    memory_type: String,
    importance: i32,
    rrf_score: f64,
    fts_rank: Option<f64>,
    vector_similarity: Option<f32>,
    association_count: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct HybridSearchQuery {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: i32,
}

#[derive(Debug, Serialize)]
struct EmbeddingStatsResponse {
    success: bool,
    total_memories: i64,
    memories_with_embeddings: i64,
    memories_without_embeddings: i64,
    coverage_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct BackfillResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ============================================================================
// Graph & Association Handlers
// ============================================================================

/// GET /api/memory/graph - Get memory graph data (nodes + edges)
async fn get_graph(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let conn = data.db.conn();

    // Get all memories as nodes
    let nodes: Vec<GraphNode> = {
        let mut stmt = match conn.prepare(
            "SELECT id, content, memory_type, importance FROM memories ORDER BY id",
        ) {
            Ok(s) => s,
            Err(e) => {
                return HttpResponse::InternalServerError().json(GraphResponse {
                    success: false,
                    nodes: vec![],
                    edges: vec![],
                    error: Some(format!("Failed to query memories: {}", e)),
                });
            }
        };

        match stmt.query_map([], |row| {
            Ok(GraphNode {
                id: row.get(0)?,
                content: {
                    let c: String = row.get(1)?;
                    if c.chars().count() > 200 {
                        let truncated: String = c.chars().take(200).collect();
                        format!("{}...", truncated)
                    } else {
                        c
                    }
                },
                memory_type: row.get(2)?,
                importance: row.get::<_, f64>(3)?.round() as i32,
            })
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                return HttpResponse::InternalServerError().json(GraphResponse {
                    success: false,
                    nodes: vec![],
                    edges: vec![],
                    error: Some(format!("Failed to query memories: {}", e)),
                });
            }
        }
    };

    // Get all associations as edges
    let edges: Vec<GraphEdge> = {
        let mut stmt = match conn.prepare(
            "SELECT source_memory_id, target_memory_id, association_type, strength FROM memory_associations",
        ) {
            Ok(s) => s,
            Err(e) => {
                return HttpResponse::InternalServerError().json(GraphResponse {
                    success: false,
                    nodes: vec![],
                    edges: vec![],
                    error: Some(format!("Failed to query associations: {}", e)),
                });
            }
        };

        match stmt.query_map([], |row| {
            Ok(GraphEdge {
                source: row.get(0)?,
                target: row.get(1)?,
                association_type: row.get(2)?,
                strength: row.get(3)?,
            })
        }) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                return HttpResponse::InternalServerError().json(GraphResponse {
                    success: false,
                    nodes: vec![],
                    edges: vec![],
                    error: Some(format!("Failed to query associations: {}", e)),
                });
            }
        }
    };

    HttpResponse::Ok().json(GraphResponse {
        success: true,
        nodes,
        edges,
        error: None,
    })
}

/// POST /api/memory/associations - Create a new association
async fn create_association(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateAssociationBody>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let strength = body.strength.clamp(0.0, 1.0);

    match data.db.create_memory_association(
        body.source_memory_id,
        body.target_memory_id,
        &body.association_type,
        strength,
        None,
    ) {
        Ok(id) => HttpResponse::Ok().json(AssociationResponse {
            success: true,
            associations: None,
            created: Some(AssociationItem {
                id,
                source_memory_id: body.source_memory_id,
                target_memory_id: body.target_memory_id,
                association_type: body.association_type.clone(),
                strength,
                created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            }),
            deleted: None,
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(AssociationResponse {
            success: false,
            associations: None,
            created: None,
            deleted: None,
            error: Some(format!("Failed to create association: {}", e)),
        }),
    }
}

/// GET /api/memory/associations - List associations for a memory
async fn list_associations(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<AssociationQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    match data.db.get_memory_associations(query.memory_id) {
        Ok(associations) => {
            let items: Vec<AssociationItem> = associations
                .into_iter()
                .map(|a| AssociationItem {
                    id: a.id,
                    source_memory_id: a.source_memory_id,
                    target_memory_id: a.target_memory_id,
                    association_type: a.association_type,
                    strength: a.strength as f64,
                    created_at: a.created_at,
                })
                .collect();

            HttpResponse::Ok().json(AssociationResponse {
                success: true,
                associations: Some(items),
                created: None,
                deleted: None,
                error: None,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(AssociationResponse {
            success: false,
            associations: None,
            created: None,
            deleted: None,
            error: Some(format!("Failed to list associations: {}", e)),
        }),
    }
}

/// DELETE /api/memory/associations/{id} - Delete an association
async fn delete_association(
    data: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<i64>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let association_id = path.into_inner();

    match data.db.delete_memory_association(association_id) {
        Ok(deleted) => HttpResponse::Ok().json(AssociationResponse {
            success: true,
            associations: None,
            created: None,
            deleted: Some(deleted),
            error: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(AssociationResponse {
            success: false,
            associations: None,
            created: None,
            deleted: None,
            error: Some(format!("Failed to delete association: {}", e)),
        }),
    }
}

/// GET /api/memory/hybrid-search - Combined FTS + vector + graph search
async fn hybrid_search(
    data: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<HybridSearchQuery>,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let engine = match &data.hybrid_search {
        Some(engine) => engine,
        None => {
            return HttpResponse::ServiceUnavailable().json(HybridSearchResponse {
                success: false,
                query: query.query.clone(),
                mode: "hybrid".to_string(),
                results: vec![],
                error: Some("Hybrid search engine not initialized".to_string()),
            });
        }
    };

    let limit = query.limit.clamp(1, 50) as usize;

    match engine.search(&query.query, limit).await {
        Ok(results) => {
            let items: Vec<HybridSearchItem> = results
                .into_iter()
                .map(|r| HybridSearchItem {
                    memory_id: r.memory_id,
                    content: r.content,
                    memory_type: r.memory_type,
                    importance: r.importance,
                    rrf_score: r.rrf_score,
                    fts_rank: r.fts_rank,
                    vector_similarity: r.vector_similarity,
                    association_count: r.association_count,
                })
                .collect();

            HttpResponse::Ok().json(HybridSearchResponse {
                success: true,
                query: query.query.clone(),
                mode: "hybrid".to_string(),
                results: items,
                error: None,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(HybridSearchResponse {
            success: false,
            query: query.query.clone(),
            mode: "hybrid".to_string(),
            results: vec![],
            error: Some(format!("Hybrid search failed: {}", e)),
        }),
    }
}

/// GET /api/memory/embeddings/stats - Get embedding statistics
async fn embedding_stats(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let conn = data.db.conn();

    let total_memories: i64 = conn
        .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
        .unwrap_or(0);

    let memories_with_embeddings: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_embeddings", [], |row| row.get(0))
        .unwrap_or(0);

    let memories_without = total_memories - memories_with_embeddings;
    let coverage = if total_memories > 0 {
        (memories_with_embeddings as f64 / total_memories as f64) * 100.0
    } else {
        0.0
    };

    HttpResponse::Ok().json(EmbeddingStatsResponse {
        success: true,
        total_memories,
        memories_with_embeddings,
        memories_without_embeddings: memories_without,
        coverage_percent: coverage,
        error: None,
    })
}

/// POST /api/memory/embeddings/backfill - Trigger embedding backfill
async fn backfill_embeddings(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let engine = match &data.hybrid_search {
        Some(engine) => engine,
        None => {
            return HttpResponse::ServiceUnavailable().json(BackfillResponse {
                success: false,
                message: None,
                error: Some("Hybrid search engine not initialized. Embedding backfill requires an embedding provider.".to_string()),
            });
        }
    };

    // Check if a backfill is already running
    if engine.is_backfill_running() {
        return HttpResponse::Conflict().json(BackfillResponse {
            success: false,
            message: None,
            error: Some("A backfill is already running. Please wait for it to complete.".to_string()),
        });
    }

    // Run backfill in background
    let engine = engine.clone();
    tokio::spawn(async move {
        match engine.backfill_embeddings().await {
            Ok(count) => log::info!("[EMBEDDINGS] Backfill complete: {} embeddings generated", count),
            Err(e) => log::error!("[EMBEDDINGS] Backfill failed: {}", e),
        }
    });

    HttpResponse::Ok().json(BackfillResponse {
        success: true,
        message: Some("Embedding backfill started in background".to_string()),
        error: None,
    })
}

/// POST /api/memory/associations/rebuild - Trigger association discovery pass
async fn rebuild_associations(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&data, &req) {
        return resp;
    }

    let engine = match &data.hybrid_search {
        Some(engine) => engine,
        None => {
            return HttpResponse::ServiceUnavailable().json(BackfillResponse {
                success: false,
                message: None,
                error: Some("Hybrid search engine not initialized. Association rebuild requires an embedding provider.".to_string()),
            });
        }
    };

    let db = data.db.clone();
    let embedding_generator = engine.embedding_generator().clone();
    let config = crate::memory::association_loop::AssociationLoopConfig::default();

    tokio::spawn(async move {
        match crate::memory::association_loop::run_association_pass(&db, &embedding_generator, &config).await {
            Ok(()) => log::info!("[ASSOCIATIONS] Rebuild pass complete"),
            Err(e) => log::error!("[ASSOCIATIONS] Rebuild pass failed: {}", e),
        }
    });

    HttpResponse::Ok().json(BackfillResponse {
        success: true,
        message: Some("Association rebuild started in background".to_string()),
        error: None,
    })
}

/// Configure memory routes
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/memory")
            .route("/files", web::get().to(list_files))
            .route("/search", web::get().to(search))
            .route("/daily", web::get().to(get_daily_log))
            .route("/daily", web::post().to(append_daily_log))
            .route("/long-term", web::get().to(get_long_term))
            .route("/long-term", web::post().to(append_long_term))
            .route("/stats", web::get().to(get_stats))
            .route("/reindex", web::post().to(reindex))
            .route("/info", web::get().to(memory_info))
            // Phase 1: Memory System Overhaul endpoints
            .route("/graph", web::get().to(get_graph))
            .route("/associations", web::post().to(create_association))
            .route("/associations", web::get().to(list_associations))
            .route("/associations/{id}", web::delete().to(delete_association))
            .route("/hybrid-search", web::get().to(hybrid_search))
            .route("/embeddings/stats", web::get().to(embedding_stats))
            .route("/embeddings/backfill", web::post().to(backfill_embeddings))
            .route("/associations/rebuild", web::post().to(rebuild_associations)),
    );
}
