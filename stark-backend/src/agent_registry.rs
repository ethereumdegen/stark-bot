//! Agent registry — syncs Starflask agents and manages local capability mappings.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use starflask::Starflask;
use uuid::Uuid;

use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::StarflaskSeed;

/// Load the pack definition JSON for a capability from the seed-packs directory.
///
/// Returns the full JSON object (with `soul`, `personas`, `pack` fields)
/// ready to be sent to `Starflask::provision_pack`.
fn load_pack_definition(capability: &str) -> Option<Value> {
    let candidates = [
        PathBuf::from(format!("seed-packs/packs/{}.json", capability)),
        PathBuf::from(format!("../seed-packs/packs/{}.json", capability)),
    ];
    for path in &candidates {
        if path.exists() {
            let content = std::fs::read_to_string(path).ok()?;
            let value: Value = serde_json::from_str(&content).ok()?;
            return Some(value);
        }
    }
    None
}

pub struct AgentRegistry {
    starflask: Arc<Starflask>,
    db: Arc<Database>,
    broadcaster: Arc<EventBroadcaster>,
}

impl AgentRegistry {
    pub fn new(
        starflask: Arc<Starflask>,
        db: Arc<Database>,
        broadcaster: Arc<EventBroadcaster>,
    ) -> Self {
        Self { starflask, db, broadcaster }
    }

    /// Sync agents from the remote Starflask account into the local DB.
    /// Maps each remote agent to a capability based on its name/description.
    /// If no agents exist remotely, creates a "General Assistant".
    pub async fn sync_remote_agents(&self) -> Result<Vec<String>, String> {
        let agents = self.starflask.list_agents().await
            .map_err(|e| format!("Failed to list remote agents: {}", e))?;

        if agents.is_empty() {
            log::info!("[AgentRegistry] No remote agents found, creating a General Assistant");
            let agent = self.starflask.create_agent("General Assistant").await
                .map_err(|e| format!("Failed to create agent: {}", e))?;

            self.db.upsert_starflask_agent(
                "general", &agent.id, "General Assistant",
                "General-purpose AI assistant", &[], "synced",
            )?;

            self.broadcaster.broadcast(GatewayEvent::new(
                "starflask.agent_synced",
                serde_json::json!({
                    "capability": "general",
                    "agent_id": agent.id.to_string(),
                    "name": "General Assistant",
                }),
            ));

            return Ok(vec!["general".to_string()]);
        }

        let mut synced = Vec::new();
        let existing = self.db.list_starflask_agents().unwrap_or_default();
        let remote_agent_ids: Vec<String> = agents.iter().map(|a| a.id.to_string()).collect();

        // Prune local agents that no longer exist remotely
        for local in &existing {
            if !remote_agent_ids.contains(&local.agent_id) {
                log::info!(
                    "[AgentRegistry] Pruning ghost agent: {} ({})",
                    local.capability, local.agent_id
                );
                let _ = self.db.delete_starflask_agent(&local.capability);
                self.broadcaster.broadcast(GatewayEvent::new(
                    "starflask.agent_removed",
                    serde_json::json!({
                        "capability": &local.capability,
                        "agent_id": &local.agent_id,
                    }),
                ));
            }
        }

        // Re-read after pruning
        let existing = self.db.list_starflask_agents().unwrap_or_default();
        let existing_agent_ids: Vec<String> = existing.iter().map(|a| a.agent_id.clone()).collect();

        for agent in &agents {
            // Skip if already tracked by agent_id
            if existing_agent_ids.contains(&agent.id.to_string()) {
                continue;
            }

            // Infer capability from agent name
            let name_lower = agent.name.to_lowercase();
            let capability = self.infer_capability(&name_lower);

            // If this capability already exists with a DIFFERENT agent_id, update it
            // rather than creating a slugged duplicate
            let description = agent.description.as_deref().unwrap_or("");

            if let Some(existing_agent) = self.db.get_starflask_agent(&capability).ok().flatten() {
                if existing_agent.agent_id != agent.id.to_string() {
                    log::info!(
                        "[AgentRegistry] Updating capability '{}': {} -> {}",
                        capability, existing_agent.agent_id, agent.id
                    );
                    self.db.upsert_starflask_agent_str(
                        &capability,
                        &agent.id.to_string(),
                        &agent.name,
                        description,
                        &existing_agent.pack_hashes,
                        "synced",
                    )?;
                    synced.push(capability);
                    continue;
                }
            }

            self.db.upsert_starflask_agent_str(
                &capability,
                &agent.id.to_string(),
                &agent.name,
                description,
                &[],
                "synced",
            )?;

            synced.push(capability.clone());

            self.broadcaster.broadcast(GatewayEvent::new(
                "starflask.agent_synced",
                serde_json::json!({
                    "capability": &capability,
                    "agent_id": agent.id.to_string(),
                    "name": &agent.name,
                }),
            ));
        }

        // Deduplicate: if multiple capabilities point to the same agent_id, keep only the first
        let all_agents = self.db.list_starflask_agents().unwrap_or_default();
        let mut seen_ids = std::collections::HashSet::new();
        for agent in &all_agents {
            if !seen_ids.insert(agent.agent_id.clone()) {
                log::info!(
                    "[AgentRegistry] Removing duplicate capability '{}' (agent_id {} already mapped)",
                    agent.capability, agent.agent_id
                );
                let _ = self.db.delete_starflask_agent(&agent.capability);
            }
        }

        // Ensure at least a "general" capability exists
        if self.db.get_starflask_agent("general").ok().flatten().is_none() {
            // Assign the first available agent as "general"
            if let Some(first) = agents.first() {
                let description = first.description.as_deref().unwrap_or("");
                self.db.upsert_starflask_agent_str(
                    "general",
                    &first.id.to_string(),
                    &first.name,
                    description,
                    &[],
                    "synced",
                )?;
                synced.push("general".to_string());
            }
        }

        if !synced.is_empty() {
            log::info!("[AgentRegistry] Synced {} agents: {:?}", synced.len(), synced);
        } else {
            log::info!("[AgentRegistry] All remote agents already tracked");
        }

        Ok(synced)
    }

    /// Provision agents from seed config (creates new agents with packs).
    /// Only useful when real pack hashes are configured.
    pub async fn provision_from_seed(&self) -> Result<Vec<String>, String> {
        let seed = match StarflaskSeed::load() {
            Some(s) => s,
            None => {
                log::info!("[AgentRegistry] No seed config found, skipping provisioning");
                return Ok(vec![]);
            }
        };

        // Skip if seed has placeholder hashes
        let has_real_hashes = seed.agents.iter().any(|a|
            a.pack_hashes.iter().any(|h| !h.contains("..."))
        );
        if !has_real_hashes {
            log::info!("[AgentRegistry] Seed config has placeholder hashes, skipping pack provisioning");
            return Ok(vec![]);
        }

        let mut provisioned = Vec::new();

        for agent_seed in &seed.agents {
            // Check if this capability already exists in DB
            let existing = self.db.get_starflask_agent(&agent_seed.capability).ok().flatten();

            if let Some(ref existing_agent) = existing {
                // Verify the agent still exists on Starflask
                let agent_exists = if let Ok(agent_id) = Uuid::parse_str(&existing_agent.agent_id) {
                    self.starflask.get_agent(&agent_id).await.is_ok()
                } else {
                    false
                };

                if !agent_exists {
                    // Ghost agent — delete local row and fall through to create new
                    log::info!(
                        "[AgentRegistry] Agent '{}' no longer exists on Starflask, re-provisioning",
                        agent_seed.capability
                    );
                    let _ = self.db.delete_starflask_agent(&agent_seed.capability);
                } else if !existing_agent.pack_hashes.is_empty() {
                    // Already fully provisioned
                    continue;
                } else {
                    // Agent exists but has no packs — provision pack on it
                    log::info!(
                        "[AgentRegistry] Installing seed packs on existing agent: {} ({})",
                        agent_seed.name, agent_seed.capability
                    );
                    if let Ok(agent_id) = Uuid::parse_str(&existing_agent.agent_id) {
                        if let Err(e) = self.provision_or_install_pack(&agent_id, &agent_seed.capability, &agent_seed.pack_hashes).await {
                            log::warn!("[AgentRegistry] Failed to install pack on '{}': {}", agent_seed.name, e);
                        }
                        if existing_agent.description.is_empty() {
                            let _ = self.starflask.update_agent(&agent_id, None, Some(&agent_seed.description)).await;
                        }
                        let _ = self.db.upsert_starflask_agent(
                            &agent_seed.capability, &agent_id, &agent_seed.name,
                            &agent_seed.description, &agent_seed.pack_hashes, "provisioned",
                        );
                        provisioned.push(agent_seed.capability.clone());
                        self.broadcaster.broadcast(GatewayEvent::new(
                            "starflask.agent_provisioned",
                            serde_json::json!({
                                "capability": &agent_seed.capability,
                                "agent_id": agent_id.to_string(),
                                "name": &agent_seed.name,
                            }),
                        ));
                    }
                    continue;
                }
            }

            // No existing agent (or ghost was pruned) — create a new one
            log::info!("[AgentRegistry] Provisioning agent: {} ({})", agent_seed.name, agent_seed.capability);

            let agent = match self.starflask.create_agent(&agent_seed.name).await {
                Ok(a) => a,
                Err(e) => {
                    log::error!("[AgentRegistry] Failed to create agent '{}': {}", agent_seed.name, e);
                    continue;
                }
            };

            if let Err(e) = self.starflask.update_agent(&agent.id, None, Some(&agent_seed.description)).await {
                log::warn!("[AgentRegistry] Failed to set description for '{}': {}", agent_seed.name, e);
            }

            if let Err(e) = self.provision_or_install_pack(&agent.id, &agent_seed.capability, &agent_seed.pack_hashes).await {
                log::error!("[AgentRegistry] Failed to install pack on '{}': {}", agent_seed.name, e);
            }

            if let Err(e) = self.db.upsert_starflask_agent(
                &agent_seed.capability, &agent.id, &agent_seed.name,
                &agent_seed.description, &agent_seed.pack_hashes, "provisioned",
            ) {
                log::error!("[AgentRegistry] Failed to save agent '{}' to DB: {}", agent_seed.name, e);
                continue;
            }

            provisioned.push(agent_seed.capability.clone());

            self.broadcaster.broadcast(GatewayEvent::new(
                "starflask.agent_provisioned",
                serde_json::json!({
                    "capability": &agent_seed.capability,
                    "agent_id": agent.id.to_string(),
                    "name": &agent_seed.name,
                }),
            ));
        }

        if !provisioned.is_empty() {
            log::info!("[AgentRegistry] Provisioned {} agents: {:?}", provisioned.len(), provisioned);
        }

        Ok(provisioned)
    }

    /// Provision or install a pack on an agent.
    ///
    /// Prefers `provision_pack` (sends full pack definition from seed-packs JSON)
    /// because it also sets `axoniac_agent_hash` + `inference_source` on the
    /// Starflask agent record. Falls back to `install_agent_pack` (hash-only)
    /// if the pack definition file isn't available.
    async fn provision_or_install_pack(
        &self,
        agent_id: &Uuid,
        capability: &str,
        pack_hashes: &[String],
    ) -> Result<(), String> {
        // Try provision_pack with full definition first
        if let Some(pack_def) = load_pack_definition(capability) {
            log::info!("[AgentRegistry] Using provision_pack for '{}' (full definition)", capability);
            match self.starflask.provision_pack(agent_id, pack_def).await {
                Ok(result) => {
                    log::info!(
                        "[AgentRegistry] Pack provisioned on '{}': hash={}",
                        capability,
                        result.content_hash
                    );
                    return Ok(());
                }
                Err(e) => {
                    log::warn!(
                        "[AgentRegistry] provision_pack failed for '{}': {} — falling back to install_agent_pack",
                        capability, e
                    );
                }
            }
        }

        // Fallback: install by hash
        for hash in pack_hashes {
            if let Err(e) = self.starflask.install_agent_pack(agent_id, hash).await {
                log::error!(
                    "[AgentRegistry] install_agent_pack failed for '{}' hash {}: {}",
                    capability, hash, e
                );
                return Err(format!("Pack install failed for hash {}: {}", hash, e));
            }
        }
        Ok(())
    }

    /// Look up agent_id for a capability.
    pub fn get_agent_id(&self, capability: &str) -> Option<Uuid> {
        self.db
            .get_starflask_agent(capability)
            .ok()?
            .and_then(|a| Uuid::parse_str(&a.agent_id).ok())
    }

    /// Get any available agent (fallback when no specific capability match).
    pub fn get_any_agent_id(&self) -> Option<Uuid> {
        self.db
            .list_starflask_agents()
            .ok()?
            .first()
            .and_then(|a| Uuid::parse_str(&a.agent_id).ok())
    }

    /// Delete a single agent by capability (remote + local).
    pub async fn delete_agent(&self, capability: &str) -> Result<(), String> {
        if let Ok(Some(existing)) = self.db.get_starflask_agent(capability) {
            if let Ok(uuid) = Uuid::parse_str(&existing.agent_id) {
                if let Err(e) = self.starflask.delete_agent(&uuid).await {
                    log::warn!("[AgentRegistry] Failed to delete agent on Starflask: {}", e);
                }
            }
            self.db.delete_starflask_agent(capability)
                .map_err(|e| format!("Failed to delete local agent: {}", e))?;

            self.broadcaster.broadcast(GatewayEvent::new(
                "starflask.agent_deleted",
                serde_json::json!({
                    "capability": capability,
                }),
            ));

            Ok(())
        } else {
            Err(format!("Agent '{}' not found", capability))
        }
    }

    /// Re-provision a single capability (delete + re-create).
    pub async fn reprovision(&self, capability: &str) -> Result<Uuid, String> {
        let seed = StarflaskSeed::load().ok_or("No seed config found")?;
        let agent_seed = seed
            .agents
            .iter()
            .find(|a| a.capability == capability)
            .ok_or_else(|| format!("Capability '{}' not found in seed config", capability))?;

        if let Ok(Some(existing)) = self.db.get_starflask_agent(capability) {
            if let Ok(uuid) = Uuid::parse_str(&existing.agent_id) {
                let _ = self.starflask.delete_agent(&uuid).await;
            }
            let _ = self.db.delete_starflask_agent(capability);
        }

        let agent = self.starflask.create_agent(&agent_seed.name).await
            .map_err(|e| format!("Failed to create agent: {}", e))?;

        let _ = self.starflask.update_agent(&agent.id, None, Some(&agent_seed.description)).await;

        self.provision_or_install_pack(&agent.id, capability, &agent_seed.pack_hashes).await
            .map_err(|e| format!("Agent created (id={}) but pack install failed: {}", agent.id, e))?;

        self.db.upsert_starflask_agent(
            capability, &agent.id, &agent_seed.name,
            &agent_seed.description, &agent_seed.pack_hashes, "provisioned",
        )?;

        self.broadcaster.broadcast(GatewayEvent::new(
            "starflask.agent_reprovisioned",
            serde_json::json!({
                "capability": capability,
                "agent_id": agent.id.to_string(),
            }),
        ));

        Ok(agent.id)
    }

    /// List all provisioned/synced agents.
    pub fn list_agents(&self) -> Result<Vec<crate::db::tables::starflask_agents::StarflaskAgent>, String> {
        self.db.list_starflask_agents()
    }

    /// Infer capability from agent name.
    fn infer_capability(&self, name_lower: &str) -> String {
        if name_lower.contains("crypto") || name_lower.contains("wallet") || name_lower.contains("swap") {
            "crypto".to_string()
        } else if name_lower.contains("image") || name_lower.contains("art") || name_lower.contains("picture") {
            "image_gen".to_string()
        } else if name_lower.contains("video") || name_lower.contains("clip") {
            "video_gen".to_string()
        } else if name_lower.contains("discord") {
            "discord_moderator".to_string()
        } else if name_lower.contains("telegram") {
            "telegram_moderator".to_string()
        } else {
            "general".to_string()
        }
    }
}
