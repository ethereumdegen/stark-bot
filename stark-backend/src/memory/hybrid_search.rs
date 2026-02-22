use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::db::Database;
use super::embeddings::EmbeddingGenerator;
use super::vector_search;

/// Hint returned at write time suggesting possible duplicates or related content.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConsolidationHint {
    pub memory_id: i64,
    /// First 200 chars of the existing memory's content
    pub content_preview: String,
    /// Cosine similarity score (0.0–1.0)
    pub similarity: f64,
    /// Human-readable suggestion
    pub suggestion: String,
}

/// Result from the hybrid search engine, combining FTS, vector, and graph signals.
#[derive(Debug, Clone)]
pub struct HybridSearchResult {
    pub memory_id: i64,
    pub content: String,
    pub memory_type: String,
    pub importance: i32,
    pub rrf_score: f64,
    pub fts_rank: Option<f64>,
    pub vector_similarity: Option<f32>,
    pub association_count: Option<i32>,
}

/// Hybrid search engine that combines full-text search, vector similarity,
/// and graph-based association signals using Reciprocal Rank Fusion (RRF).
#[derive(Clone)]
pub struct HybridSearchEngine {
    db: Arc<Database>,
    embedding_generator: Arc<dyn EmbeddingGenerator + Send + Sync>,
    backfill_running: Arc<AtomicBool>,
}

impl HybridSearchEngine {
    /// Create a new hybrid search engine.
    pub fn new(
        db: Arc<Database>,
        embedding_generator: Arc<dyn EmbeddingGenerator + Send + Sync>,
    ) -> Self {
        Self {
            db,
            embedding_generator,
            backfill_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get a reference to the embedding generator.
    pub fn embedding_generator(&self) -> &Arc<dyn EmbeddingGenerator + Send + Sync> {
        &self.embedding_generator
    }

    /// Run a full hybrid search combining FTS5, vector similarity, and graph associations.
    /// If `agent_subtype` is provided, same-subtype memories get a 1.25x score boost.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
        agent_subtype: Option<&str>,
    ) -> Result<Vec<HybridSearchResult>, String> {
        // 1. FTS5 search
        let fts_results = self.fts_search(query)?;
        log::debug!("[HYBRID_SEARCH] FTS returned {} results for query: {:?}", fts_results.len(), query);

        // 2. Vector search
        let vector_results = self.vector_search(query).await;
        log::debug!("[HYBRID_SEARCH] Vector returned {} results", vector_results.len());

        // 3. Graph search: expand from FTS/vector hits to find their neighbors
        let seed_ids: Vec<i64> = fts_results.iter().map(|(id, _)| *id)
            .chain(vector_results.iter().map(|(id, _)| *id))
            .collect();
        let graph_results = self.graph_expand(&seed_ids)?;
        log::debug!("[HYBRID_SEARCH] Graph expanded to {} neighbors from {} seeds", graph_results.len(), seed_ids.len());

        // 4. RRF merge
        let mut merged = self.rrf_merge(&fts_results, &vector_results, &graph_results, limit * 2);

        // 5. Apply subtype boost: 1.25x for same-subtype memories
        if let Some(subtype) = agent_subtype {
            if !subtype.is_empty() {
                self.apply_subtype_boost(&mut merged, subtype);
                // Re-sort after boost
                merged.sort_by(|a, b| b.rrf_score.partial_cmp(&a.rrf_score).unwrap_or(std::cmp::Ordering::Equal));
            }
        }

        merged.truncate(limit);
        Ok(merged)
    }

    /// Run FTS-only search, skipping vector and graph signals.
    pub fn fts_only(
        &self,
        query: &str,
        limit: usize,
        agent_subtype: Option<&str>,
    ) -> Result<Vec<HybridSearchResult>, String> {
        let fts_results = self.fts_search(query)?;

        let empty_vec: Vec<(i64, f32)> = Vec::new();
        let empty_graph: Vec<(i64, i32)> = Vec::new();
        let mut merged = self.rrf_merge(&fts_results, &empty_vec, &empty_graph, limit * 2);

        // Apply subtype boost
        if let Some(subtype) = agent_subtype {
            if !subtype.is_empty() {
                self.apply_subtype_boost(&mut merged, subtype);
                merged.sort_by(|a, b| b.rrf_score.partial_cmp(&a.rrf_score).unwrap_or(std::cmp::Ordering::Equal));
            }
        }

        merged.truncate(limit);
        Ok(merged)
    }

    /// Escape a query string for safe use with FTS5 MATCH by quoting each token.
    /// This prevents FTS5 operators (AND, OR, NOT, NEAR, *, etc.) from being
    /// interpreted as syntax.
    fn sanitize_fts5_query(query: &str) -> String {
        query
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| {
                // Double-quote each token, escaping any internal double-quotes
                let escaped = token.replace('"', "\"\"");
                format!("\"{}\"", escaped)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Perform FTS5 full-text search against the memories_fts table.
    /// Returns (memory_id, rank) pairs.
    fn fts_search(&self, query: &str) -> Result<Vec<(i64, f64)>, String> {
        let sanitized = Self::sanitize_fts5_query(query);
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.db.conn();

        let mut stmt = conn
            .prepare(
                "SELECT m.id, fts.rank
                 FROM memories_fts fts
                 JOIN memories m ON m.id = fts.rowid
                 WHERE memories_fts MATCH ?1
                 ORDER BY fts.rank
                 LIMIT 100",
            )
            .map_err(|e| format!("Failed to prepare FTS query: {}", e))?;

        let results = stmt
            .query_map(rusqlite::params![sanitized], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| format!("Failed to execute FTS query: {}", e))?
            .filter_map(|r| r.ok())
            .collect::<Vec<(i64, f64)>>();

        Ok(results)
    }

    /// Perform vector similarity search by generating an embedding for the query
    /// and comparing against all stored memory embeddings.
    /// Returns (memory_id, similarity) pairs.
    async fn vector_search(&self, query: &str) -> Vec<(i64, f32)> {
        let query_embedding = match self.embedding_generator.generate(query).await {
            Ok(emb) => emb,
            Err(e) => {
                log::warn!("Failed to generate query embedding for vector search: {}", e);
                return Vec::new();
            }
        };

        // Load all stored embeddings
        let candidates = match self.load_embeddings() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Failed to load memory embeddings: {}", e);
                return Vec::new();
            }
        };

        let results = vector_search::find_similar(&query_embedding, &candidates, 100, 0.0);

        results
            .into_iter()
            .map(|r| (r.memory_id, r.similarity))
            .collect()
    }

    /// Load all memory embeddings from the database.
    fn load_embeddings(&self) -> Result<Vec<(i64, Vec<f32>)>, String> {
        self.db
            .list_memory_embeddings()
            .map_err(|e| format!("Failed to load memory embeddings: {}", e))
    }

    /// Expand from seed memory IDs to their graph neighbors.
    /// Returns neighbor (memory_id, total_strength) pairs ranked by connection
    /// strength to the seed set. This makes the graph signal query-aware rather
    /// than just returning the globally most-connected memories.
    fn graph_expand(&self, seed_ids: &[i64]) -> Result<Vec<(i64, i32)>, String> {
        if seed_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Deduplicate and cap seed set to avoid huge queries
        let mut unique_seeds: Vec<i64> = seed_ids.to_vec();
        unique_seeds.sort_unstable();
        unique_seeds.dedup();
        unique_seeds.truncate(50);

        let conn = self.db.conn();
        let n = unique_seeds.len();

        // Build 4 sets of numbered placeholders for the IN clauses
        let make_placeholders = |offset: usize| -> String {
            (1..=n).map(|i| format!("?{}", offset + i)).collect::<Vec<_>>().join(", ")
        };
        let p1 = make_placeholders(0);
        let p2 = make_placeholders(n);
        let p3 = make_placeholders(2 * n);
        let p4 = make_placeholders(3 * n);

        let query = format!(
            "SELECT neighbor_id, SUM(strength_int) as total_strength FROM (
                 SELECT target_memory_id AS neighbor_id,
                        CAST(strength * 100 AS INTEGER) AS strength_int
                 FROM memory_associations
                 WHERE source_memory_id IN ({p1})
                   AND target_memory_id NOT IN ({p2})
                 UNION ALL
                 SELECT source_memory_id AS neighbor_id,
                        CAST(strength * 100 AS INTEGER) AS strength_int
                 FROM memory_associations
                 WHERE target_memory_id IN ({p3})
                   AND source_memory_id NOT IN ({p4})
             )
             GROUP BY neighbor_id
             ORDER BY total_strength DESC
             LIMIT 100"
        );

        // Bind seed_ids four times (once per IN clause)
        let all_params: Vec<i64> = std::iter::repeat(unique_seeds.iter().copied())
            .take(4)
            .flatten()
            .collect();

        let mut stmt = conn.prepare(&query)
            .map_err(|e| format!("Failed to prepare graph expand query: {}", e))?;

        let results = stmt
            .query_map(rusqlite::params_from_iter(all_params.iter()), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i32>(1)?))
            })
            .map_err(|e| format!("Failed to execute graph expand query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// Find consolidation hints for new content before it is stored.
    ///
    /// Generates an embedding for `content`, runs vector similarity search,
    /// and returns hints for memories above the 0.70 similarity threshold.
    pub async fn find_consolidation_hints(
        &self,
        content: &str,
        limit: usize,
    ) -> Vec<ConsolidationHint> {
        let query_embedding = match self.embedding_generator.generate(content).await {
            Ok(emb) => emb,
            Err(e) => {
                log::warn!("Failed to generate embedding for consolidation hints: {}", e);
                return Vec::new();
            }
        };

        let candidates = match self.load_embeddings() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Failed to load embeddings for consolidation hints: {}", e);
                return Vec::new();
            }
        };

        let results = vector_search::find_similar(&query_embedding, &candidates, limit, 0.70);

        let conn = self.db.conn();
        let mut hints = Vec::new();

        for hit in results {
            let similarity = hit.similarity as f64;
            let suggestion = if similarity >= 0.85 {
                "possible duplicate — consider merging".to_string()
            } else {
                "related content exists — review before saving".to_string()
            };

            // Fetch the content preview
            let preview = conn
                .query_row(
                    "SELECT content FROM memories WHERE id = ?1",
                    rusqlite::params![hit.memory_id],
                    |row| row.get::<_, String>(0),
                )
                .ok()
                .map(|c| {
                    if c.chars().count() > 200 {
                        let truncated: String = c.chars().take(200).collect();
                        format!("{}...", truncated)
                    } else {
                        c
                    }
                })
                .unwrap_or_default();

            hints.push(ConsolidationHint {
                memory_id: hit.memory_id,
                content_preview: preview,
                similarity,
                suggestion,
            });
        }

        hints
    }

    /// Check if a backfill is currently running.
    pub fn is_backfill_running(&self) -> bool {
        self.backfill_running.load(Ordering::Relaxed)
    }

    /// Backfill embeddings for memories that don't have them yet.
    /// Returns the count of embeddings generated.
    /// Only one backfill can run at a time — concurrent calls return an error.
    pub async fn backfill_embeddings(&self) -> Result<usize, String> {
        // Prevent concurrent backfill runs
        if self.backfill_running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err("Backfill is already running".to_string());
        }

        let result = self.backfill_embeddings_inner().await;
        self.backfill_running.store(false, Ordering::SeqCst);
        result
    }

    async fn backfill_embeddings_inner(&self) -> Result<usize, String> {
        let memories: Vec<(i64, String)> = {
            let conn = self.db.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT m.id, m.content
                     FROM memories m
                     LEFT JOIN memory_embeddings e ON e.memory_id = m.id
                     WHERE e.memory_id IS NULL
                     ORDER BY m.id",
                )
                .map_err(|e| format!("Failed to prepare backfill query: {}", e))?;

            stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| format!("Failed to query memories for backfill: {}", e))?
            .filter_map(|r| r.ok())
            .collect()
        };

        let total = memories.len();
        let mut generated = 0;

        for (memory_id, content) in &memories {
            match self.embedding_generator.generate(content).await {
                Ok(embedding) => {
                    let dims = embedding.len() as i32;
                    self.db
                        .upsert_memory_embedding(*memory_id, &embedding, "backfill", dims)
                        .map_err(|e| format!("Failed to store embedding for memory {}: {}", memory_id, e))?;

                    generated += 1;
                    if generated % 10 == 0 {
                        log::info!("[BACKFILL] Progress: {}/{}", generated, total);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to generate embedding for memory {}: {}", memory_id, e);
                }
            }
            // Rate limit: avoid hitting OpenAI API limits during bulk operations
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        log::info!("[BACKFILL] Complete: generated {} embeddings out of {} memories", generated, total);
        Ok(generated)
    }

    /// Apply a multiplicative score boost to memories whose agent_subtype matches.
    /// This is a soft preference — cross-subtype memories still appear, just ranked lower.
    fn apply_subtype_boost(&self, results: &mut [HybridSearchResult], subtype: &str) {
        let conn = self.db.conn();
        for result in results.iter_mut() {
            let mem_subtype: Option<String> = conn
                .query_row(
                    "SELECT agent_subtype FROM memories WHERE id = ?1",
                    rusqlite::params![result.memory_id],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            if let Some(ref ms) = mem_subtype {
                if ms.eq_ignore_ascii_case(subtype) {
                    result.rrf_score *= 1.25;
                }
            }
        }
    }

    /// Merge results from multiple search signals using Reciprocal Rank Fusion.
    ///
    /// For each result list, assigns rank positions and calculates:
    ///   score = sum(1.0 / (60.0 + rank))
    /// across all lists where the memory appears.
    fn rrf_merge(
        &self,
        fts_results: &[(i64, f64)],
        vector_results: &[(i64, f32)],
        graph_results: &[(i64, i32)],
        limit: usize,
    ) -> Vec<HybridSearchResult> {
        let mut scores: HashMap<i64, f64> = HashMap::new();
        let mut fts_ranks: HashMap<i64, f64> = HashMap::new();
        let mut vector_sims: HashMap<i64, f32> = HashMap::new();
        let mut assoc_counts: HashMap<i64, i32> = HashMap::new();

        // FTS signal
        for (rank, (memory_id, fts_rank)) in fts_results.iter().enumerate() {
            let rrf = 1.0 / (60.0 + rank as f64);
            *scores.entry(*memory_id).or_insert(0.0) += rrf;
            fts_ranks.insert(*memory_id, *fts_rank);
        }

        // Vector signal
        for (rank, (memory_id, similarity)) in vector_results.iter().enumerate() {
            let rrf = 1.0 / (60.0 + rank as f64);
            *scores.entry(*memory_id).or_insert(0.0) += rrf;
            vector_sims.insert(*memory_id, *similarity);
        }

        // Graph signal
        for (rank, (memory_id, count)) in graph_results.iter().enumerate() {
            let rrf = 1.0 / (60.0 + rank as f64);
            *scores.entry(*memory_id).or_insert(0.0) += rrf;
            assoc_counts.insert(*memory_id, *count);
        }

        // Sort by RRF score descending
        let mut sorted: Vec<(i64, f64)> = scores.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(limit);

        // Fetch memory details for the top results
        let conn = self.db.conn();
        let mut results = Vec::new();

        for (memory_id, rrf_score) in sorted {
            let row = conn.query_row(
                "SELECT content, memory_type, importance FROM memories WHERE id = ?1",
                rusqlite::params![memory_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, f64>(2)?.round() as i32,
                    ))
                },
            );

            match row {
                Ok((content, memory_type, importance)) => {
                    results.push(HybridSearchResult {
                        memory_id,
                        content,
                        memory_type,
                        importance,
                        rrf_score,
                        fts_rank: fts_ranks.get(&memory_id).copied(),
                        vector_similarity: vector_sims.get(&memory_id).copied(),
                        association_count: assoc_counts.get(&memory_id).copied(),
                    });
                }
                Err(e) => {
                    log::warn!(
                        "Failed to fetch memory {} during RRF merge: {}",
                        memory_id,
                        e
                    );
                }
            }
        }

        results
    }
}
