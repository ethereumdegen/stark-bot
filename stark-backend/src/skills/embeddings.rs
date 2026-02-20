//! Skill embedding and association logic
//! Provides semantic skill discovery via vector embeddings

use crate::db::Database;
use crate::memory::EmbeddingGenerator;
use crate::memory::vector_search;
use crate::skills::types::DbSkill;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Tag category constants — mirrors frontend groupings so skills in the same
/// domain (e.g. all DeFi skills) get a similarity boost when building edges.
const FINANCE_TAGS: &[&str] = &[
    "crypto", "defi", "finance", "trading", "swap", "transfer", "wallet",
    "yield", "lending", "bridge", "payments", "token", "price", "nft",
];
const CODE_TAGS: &[&str] = &[
    "development", "git", "code", "debugging", "testing", "deployment",
    "ci-cd", "devops", "infrastructure",
];
const SOCIAL_TAGS: &[&str] = &[
    "social", "messaging", "twitter", "discord", "telegram",
    "communication", "social-media",
];
const SECRETARY_TAGS: &[&str] = &[
    "secretary", "productivity", "notes", "scheduling", "cron", "automation",
];

/// Determine the high-level category for a skill based on its tags.
fn tag_category(tags: &[String]) -> Option<&'static str> {
    let lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    if lower.iter().any(|t| FINANCE_TAGS.contains(&t.as_str())) {
        return Some("finance");
    }
    if lower.iter().any(|t| CODE_TAGS.contains(&t.as_str())) {
        return Some("code");
    }
    if lower.iter().any(|t| SOCIAL_TAGS.contains(&t.as_str())) {
        return Some("social");
    }
    if lower.iter().any(|t| SECRETARY_TAGS.contains(&t.as_str())) {
        return Some("secretary");
    }
    None
}

/// Compute an effective similarity score that boosts skills sharing a tag
/// category or exact tags, so domain-related skills connect even when their
/// embedding descriptions are worded differently.
fn effective_similarity(raw: f32, skill_a: Option<&DbSkill>, skill_b: Option<&DbSkill>) -> f32 {
    let (Some(a), Some(b)) = (skill_a, skill_b) else {
        return raw;
    };

    // Exact shared tags → strong boost
    let shared_exact = a.tags.iter().any(|t| b.tags.contains(t));
    if shared_exact {
        return raw + 0.15;
    }

    // Same tag category → moderate boost
    if let (Some(cat_a), Some(cat_b)) = (tag_category(&a.tags), tag_category(&b.tags)) {
        if cat_a == cat_b {
            return raw + 0.10;
        }
    }

    raw
}

/// Build embedding text for a skill (concise representation for vector search)
pub fn build_skill_embedding_text(skill: &DbSkill) -> String {
    let tags = if skill.tags.is_empty() {
        String::new()
    } else {
        format!(". Tags: {}", skill.tags.join(", "))
    };
    format!("{}: {}{}", skill.name, skill.description, tags)
}

/// Backfill embeddings for all enabled skills that don't have one yet.
/// Returns the number of embeddings generated.
pub async fn backfill_skill_embeddings(
    db: &Arc<Database>,
    embedding_gen: &Arc<dyn EmbeddingGenerator + Send + Sync>,
) -> Result<usize, String> {
    let missing_ids = db.list_skills_without_embeddings(100)
        .map_err(|e| format!("Failed to list skills without embeddings: {}", e))?;

    if missing_ids.is_empty() {
        return Ok(0);
    }

    let mut count = 0;
    let inter_request_delay = Duration::from_millis(200);

    for skill_id in &missing_ids {
        let skill = match db.get_skill_by_id(*skill_id) {
            Ok(Some(s)) => s,
            _ => continue,
        };

        let text = build_skill_embedding_text(&skill);

        // Retry up to 3 times with backoff for rate-limited requests
        let mut result = None;
        for attempt in 0..3 {
            match embedding_gen.generate(&text).await {
                Ok(embedding) => {
                    result = Some(embedding);
                    break;
                }
                Err(e) if e.contains("429") && attempt < 2 => {
                    let backoff = Duration::from_secs((attempt + 1) as u64 * 2);
                    log::debug!("[SKILL-EMB] Rate limited for '{}', retrying in {:?}", skill.name, backoff);
                    sleep(backoff).await;
                }
                Err(e) => {
                    log::warn!("[SKILL-EMB] Failed to generate embedding for skill '{}': {}", skill.name, e);
                    break;
                }
            }
        }

        if let Some(embedding) = result {
            let dims = embedding.len() as i32;
            if let Err(e) = db.upsert_skill_embedding(*skill_id, &embedding, "remote", dims) {
                log::warn!("[SKILL-EMB] Failed to store embedding for skill {}: {}", skill.name, e);
            } else {
                count += 1;
                log::debug!("[SKILL-EMB] Generated embedding for skill '{}'", skill.name);
            }
        }

        // Pace requests to avoid overwhelming the embedding server
        sleep(inter_request_delay).await;
    }

    log::info!("[SKILL-EMB] Backfilled {} skill embeddings", count);
    Ok(count)
}

/// Search skills by semantic similarity to a query string.
/// Returns matching skills with their similarity scores.
pub async fn search_skills(
    db: &Arc<Database>,
    embedding_gen: &Arc<dyn EmbeddingGenerator + Send + Sync>,
    query: &str,
    limit: usize,
    threshold: f32,
) -> Result<Vec<(DbSkill, f32)>, String> {
    // Generate query embedding
    let query_embedding = embedding_gen.generate(query).await?;

    // Load all skill embeddings
    let candidates = db.list_skill_embeddings()
        .map_err(|e| format!("Failed to list skill embeddings: {}", e))?;

    if candidates.is_empty() {
        return Ok(vec![]);
    }

    // Find similar using vector search
    let results = vector_search::find_similar(&query_embedding, &candidates, limit, threshold);

    // Map result IDs back to DbSkill objects
    let mut skills_with_scores = Vec::new();
    for result in results {
        // result.memory_id is actually skill_id here (same field name from VectorSearchResult)
        if let Ok(Some(skill)) = db.get_skill_by_id(result.memory_id) {
            if skill.enabled {
                skills_with_scores.push((skill, result.similarity));
            }
        }
    }

    Ok(skills_with_scores)
}

/// Simple text-based skill search (fallback when embeddings are unavailable).
/// Matches against skill name, description, and tags using case-insensitive substring matching.
/// Returns matching skills sorted by a simple relevance score.
pub fn search_skills_text(
    db: &Arc<Database>,
    query: &str,
    limit: usize,
) -> Result<Vec<(DbSkill, f32)>, String> {
    let skills = db.list_enabled_skills()
        .map_err(|e| format!("Failed to list skills: {}", e))?;

    let query_lower = query.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    if query_terms.is_empty() {
        return Ok(vec![]);
    }

    let mut scored: Vec<(DbSkill, f32)> = skills
        .into_iter()
        .filter_map(|skill| {
            let name_lower = skill.name.to_lowercase();
            let desc_lower = skill.description.to_lowercase();
            let tags_lower: Vec<String> = skill.tags.iter().map(|t| t.to_lowercase()).collect();
            let tags_joined = tags_lower.join(" ");

            let mut score: f32 = 0.0;

            for term in &query_terms {
                // Exact name match is strongest
                if name_lower == *term {
                    score += 0.5;
                } else if name_lower.contains(term) {
                    score += 0.3;
                }
                // Tag match
                if tags_lower.iter().any(|t| t == term) {
                    score += 0.25;
                } else if tags_joined.contains(term) {
                    score += 0.15;
                }
                // Description match
                if desc_lower.contains(term) {
                    score += 0.1;
                }
            }

            // Normalize by number of query terms to get 0..1ish range
            if score > 0.0 {
                let normalized = (score / query_terms.len() as f32).min(1.0);
                Some((skill, normalized))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    // Expand results via association edges: for each top match, pull in
    // associated skills with a decayed score so they appear after direct matches.
    let direct_ids: std::collections::HashSet<i64> = scored.iter()
        .filter_map(|(s, _)| s.id)
        .collect();

    let mut associated: Vec<(DbSkill, f32)> = Vec::new();
    for (skill, score) in &scored {
        if let Some(skill_id) = skill.id {
            if let Ok(assocs) = db.get_skill_associations(skill_id) {
                for assoc in assocs {
                    let neighbor_id = if assoc.source_skill_id == skill_id {
                        assoc.target_skill_id
                    } else {
                        assoc.source_skill_id
                    };
                    // Skip if already a direct match or already added
                    if direct_ids.contains(&neighbor_id) {
                        continue;
                    }
                    if associated.iter().any(|(s, _)| s.id == Some(neighbor_id)) {
                        continue;
                    }
                    if let Ok(Some(neighbor)) = db.get_skill_by_id(neighbor_id) {
                        if neighbor.enabled {
                            // Decay: parent score * edge strength * 0.6
                            let edge_score = score * assoc.strength as f32 * 0.6;
                            associated.push((neighbor, edge_score));
                        }
                    }
                }
            }
        }
    }

    scored.extend(associated);
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    Ok(scored)
}

/// Rebuild associations for a single skill.
/// Deletes existing associations for this skill, then creates new ones
/// based on embedding similarity with all other skills.
pub async fn rebuild_associations_for_skill(
    db: &Arc<Database>,
    skill_id: i64,
    threshold: f32,
) -> Result<usize, String> {
    // Delete existing associations for this skill
    db.delete_skill_associations_for(skill_id)
        .map_err(|e| format!("Failed to delete associations for skill {}: {}", skill_id, e))?;

    // Load all skill embeddings
    let all_embeddings = db.list_skill_embeddings()
        .map_err(|e| format!("Failed to list skill embeddings: {}", e))?;

    // Find this skill's embedding
    let this_embedding = match all_embeddings.iter().find(|(id, _)| *id == skill_id) {
        Some((_, emb)) => emb,
        None => return Ok(0), // no embedding yet
    };

    // Load all skills for tag comparison
    let all_skills: std::collections::HashMap<i64, DbSkill> = db.list_enabled_skills()
        .map_err(|e| format!("Failed to list skills: {}", e))?
        .into_iter()
        .filter_map(|s| s.id.map(|id| (id, s)))
        .collect();

    let this_skill = all_skills.get(&skill_id);
    let mut created = 0;

    for (other_id, other_embedding) in &all_embeddings {
        if *other_id == skill_id {
            continue;
        }

        let raw_similarity = vector_search::cosine_similarity(this_embedding, other_embedding);
        let other_skill = all_skills.get(other_id);
        let similarity = effective_similarity(raw_similarity, this_skill, other_skill);
        if similarity < threshold {
            continue;
        }

        let assoc_type = if let (Some(skill_a), Some(skill_b)) = (this_skill, other_skill) {
            let shared_tags = skill_a.tags.iter().any(|t| skill_b.tags.contains(t));
            if shared_tags { "complement" } else { "related" }
        } else {
            "related"
        };

        if let Err(e) = db.create_skill_association(skill_id, *other_id, assoc_type, similarity as f64, None) {
            log::warn!("[SKILL-ASSOC] Failed to create association ({} -> {}): {}", skill_id, other_id, e);
        } else {
            created += 1;
        }
    }

    log::info!("[SKILL-ASSOC] Rebuilt {} associations for skill {}", created, skill_id);
    Ok(created)
}

/// Rebuild all skill associations from embeddings.
/// Deletes existing associations, backfills missing embeddings,
/// then creates associations for skill pairs above the similarity threshold.
pub async fn rebuild_skill_associations(
    db: &Arc<Database>,
    embedding_gen: &Arc<dyn EmbeddingGenerator + Send + Sync>,
    threshold: f32,
) -> Result<usize, String> {
    // Delete all existing associations
    db.delete_all_skill_associations()
        .map_err(|e| format!("Failed to delete existing associations: {}", e))?;

    // Backfill any missing embeddings
    backfill_skill_embeddings(db, embedding_gen).await?;

    // Load all skill embeddings
    let all_embeddings = db.list_skill_embeddings()
        .map_err(|e| format!("Failed to list skill embeddings: {}", e))?;

    if all_embeddings.len() < 2 {
        return Ok(0);
    }

    // Load all skills for tag comparison
    let all_skills: std::collections::HashMap<i64, DbSkill> = db.list_enabled_skills()
        .map_err(|e| format!("Failed to list skills: {}", e))?
        .into_iter()
        .filter_map(|s| s.id.map(|id| (id, s)))
        .collect();

    let mut created = 0;

    // Compare all pairs
    for i in 0..all_embeddings.len() {
        for j in (i + 1)..all_embeddings.len() {
            let (id_a, ref emb_a) = all_embeddings[i];
            let (id_b, ref emb_b) = all_embeddings[j];

            let raw_similarity = vector_search::cosine_similarity(emb_a, emb_b);
            let skill_a = all_skills.get(&id_a);
            let skill_b = all_skills.get(&id_b);
            let similarity = effective_similarity(raw_similarity, skill_a, skill_b);
            if similarity < threshold {
                continue;
            }

            // Classify association type based on shared tags
            let assoc_type = if let (Some(a), Some(b)) = (skill_a, skill_b) {
                let shared_tags = a.tags.iter().any(|t| b.tags.contains(t));
                if shared_tags { "complement" } else { "related" }
            } else {
                "related"
            };

            if let Err(e) = db.create_skill_association(id_a, id_b, assoc_type, similarity as f64, None) {
                log::warn!("[SKILL-ASSOC] Failed to create association ({} -> {}): {}", id_a, id_b, e);
            } else {
                created += 1;
            }
        }
    }

    log::info!("[SKILL-ASSOC] Rebuilt {} skill associations", created);
    Ok(created)
}
