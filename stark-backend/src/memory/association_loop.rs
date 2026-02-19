use std::sync::Arc;

use crate::db::Database;
use super::embeddings::EmbeddingGenerator;
use super::vector_search;

/// Configuration for the background association discovery loop.
pub struct AssociationLoopConfig {
    /// Interval between loop iterations in seconds (default: 300 = 5 minutes).
    pub interval_secs: u64,
    /// Minimum cosine similarity to create an association (default: 0.65).
    pub similarity_threshold: f32,
    /// Maximum number of associations to create per memory (default: 10).
    pub max_associations_per_memory: usize,
    /// Number of recent memories to process per iteration (default: 50).
    pub batch_size: usize,
}

impl Default for AssociationLoopConfig {
    fn default() -> Self {
        Self {
            interval_secs: 300,
            similarity_threshold: 0.65,
            max_associations_per_memory: 10,
            batch_size: 50,
        }
    }
}

/// Spawn a background tokio task that periodically discovers and creates
/// associations between memories based on embedding similarity.
///
/// The loop runs indefinitely, sleeping for `config.interval_secs` between
/// iterations. Errors are logged and do not halt the loop.
pub fn spawn_association_loop(
    db: Arc<Database>,
    embedding_generator: Arc<dyn EmbeddingGenerator + Send + Sync>,
    config: AssociationLoopConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        log::info!(
            "Association loop started (interval={}s, threshold={}, max_per_memory={}, batch={})",
            config.interval_secs,
            config.similarity_threshold,
            config.max_associations_per_memory,
            config.batch_size
        );

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(config.interval_secs)).await;

            if let Err(e) = run_association_pass(&db, &embedding_generator, &config).await {
                log::error!("Association loop pass failed: {}", e);
            }
        }
    })
}

/// Execute a single association discovery pass.
pub async fn run_association_pass(
    db: &Database,
    embedding_generator: &Arc<dyn EmbeddingGenerator + Send + Sync>,
    config: &AssociationLoopConfig,
) -> Result<(), String> {
    // 1. Find recent memories that have fewer than max_associations_per_memory associations
    let memories_to_process = {
        let conn = db.conn();

        let mut stmt = conn
            .prepare(
                "SELECT m.id, m.content
                 FROM memories m
                 LEFT JOIN (
                     SELECT memory_id, COUNT(*) AS cnt FROM (
                         SELECT source_memory_id AS memory_id FROM memory_associations
                         UNION ALL
                         SELECT target_memory_id AS memory_id FROM memory_associations
                     ) GROUP BY memory_id
                 ) a ON a.memory_id = m.id
                 WHERE COALESCE(a.cnt, 0) < ?1
                 ORDER BY m.created_at DESC
                 LIMIT ?2",
            )
            .map_err(|e| format!("Failed to prepare association loop query: {}", e))?;

        let results: Vec<(i64, String)> = stmt
            .query_map(
                rusqlite::params![config.max_associations_per_memory as i32, config.batch_size as i32],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|e| format!("Failed to query memories for association loop: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        results
    };

    if memories_to_process.is_empty() {
        log::info!("Association loop: no memories to process");
        return Ok(());
    }

    log::info!(
        "Association loop: processing {} memories",
        memories_to_process.len()
    );

    // 2. Load all existing embeddings
    let all_embeddings = load_all_embeddings(db)?;

    let mut total_created: usize = 0;

    for (memory_id, content) in &memories_to_process {
        // 3. Ensure this memory has an embedding
        let embedding = match find_embedding(&all_embeddings, *memory_id) {
            Some(emb) => emb.clone(),
            None => {
                // Generate embedding if missing
                match embedding_generator.generate(content).await {
                    Ok(emb) => {
                        // Store the embedding
                        if let Err(e) = store_embedding(db, *memory_id, &emb) {
                            log::warn!(
                                "Failed to store embedding for memory {}: {}",
                                memory_id,
                                e
                            );
                            continue;
                        }
                        // Rate limit API calls
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        emb
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to generate embedding for memory {}: {}",
                            memory_id,
                            e
                        );
                        continue;
                    }
                }
            }
        };

        // 4. Find similar memories via vector search
        let similar = vector_search::find_similar(
            &embedding,
            &all_embeddings,
            config.max_associations_per_memory,
            config.similarity_threshold,
        );

        // 5. Count existing associations for this memory
        let existing_count = count_associations(db, *memory_id)?;
        let slots_available = config
            .max_associations_per_memory
            .saturating_sub(existing_count);

        if slots_available == 0 {
            continue;
        }

        // 6. Create associations for the most similar memories
        let mut created_in_batch = 0;

        for result in similar.iter().take(slots_available) {
            // Skip self-associations
            if result.memory_id == *memory_id {
                continue;
            }

            // Check if association already exists
            if association_exists(db, *memory_id, result.memory_id)? {
                continue;
            }

            // Create the association
            if let Err(e) = create_association(
                db,
                *memory_id,
                result.memory_id,
                "related",
                result.similarity,
            ) {
                log::warn!(
                    "Failed to create association {} -> {}: {}",
                    memory_id,
                    result.memory_id,
                    e
                );
                continue;
            }

            created_in_batch += 1;
            total_created += 1;
        }

        if created_in_batch > 0 {
            log::info!(
                "Created {} associations for memory {}",
                created_in_batch,
                memory_id
            );
        }
    }

    log::info!(
        "Association loop pass complete: created {} new associations",
        total_created
    );

    Ok(())
}

/// Load all memory embeddings from the database.
fn load_all_embeddings(db: &Database) -> Result<Vec<(i64, Vec<f32>)>, String> {
    db.list_memory_embeddings()
        .map_err(|e| format!("Failed to load memory embeddings: {}", e))
}

/// Find an embedding for a specific memory ID in the preloaded list.
fn find_embedding(embeddings: &[(i64, Vec<f32>)], memory_id: i64) -> Option<&Vec<f32>> {
    embeddings
        .iter()
        .find(|(id, _)| *id == memory_id)
        .map(|(_, emb)| emb)
}

/// Store an embedding in the database.
fn store_embedding(db: &Database, memory_id: i64, embedding: &[f32]) -> Result<(), String> {
    let dims = embedding.len() as i32;
    db.upsert_memory_embedding(memory_id, embedding, "association_loop", dims)
        .map_err(|e| format!("Failed to insert embedding: {}", e))
}

/// Count existing associations for a memory (both directions).
fn count_associations(db: &Database, memory_id: i64) -> Result<usize, String> {
    let conn = db.conn();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_associations
             WHERE source_memory_id = ?1 OR target_memory_id = ?1",
            rusqlite::params![memory_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count associations: {}", e))?;

    Ok(count as usize)
}

/// Check if an association already exists between two memories (in either direction).
fn association_exists(
    db: &Database,
    source_id: i64,
    target_id: i64,
) -> Result<bool, String> {
    let conn = db.conn();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_associations
             WHERE (source_memory_id = ?1 AND target_memory_id = ?2)
                OR (source_memory_id = ?2 AND target_memory_id = ?1)",
            rusqlite::params![source_id, target_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to check association existence: {}", e))?;

    Ok(count > 0)
}

/// Create a new association between two memories.
fn create_association(
    db: &Database,
    source_id: i64,
    target_id: i64,
    association_type: &str,
    strength: f32,
) -> Result<(), String> {
    let conn = db.conn();

    conn.execute(
        "INSERT INTO memory_associations (source_memory_id, target_memory_id, association_type, strength, created_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
        rusqlite::params![source_id, target_id, association_type, strength],
    )
    .map_err(|e| format!("Failed to insert association: {}", e))?;

    Ok(())
}
