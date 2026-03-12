//! Agent registry — syncs Starflask agents and manages local capability mappings.

use std::sync::Arc;

use starflask::Starflask;
use uuid::Uuid;

use crate::db::Database;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::models::StarflaskSeed;

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
        let existing_agent_ids: Vec<String> = existing.iter().map(|a| a.agent_id.clone()).collect();

        for agent in &agents {
            // Skip if already tracked
            if existing_agent_ids.contains(&agent.id.to_string()) {
                continue;
            }

            // Infer capability from agent name
            let name_lower = agent.name.to_lowercase();
            let capability = self.infer_capability(&name_lower);

            // If this capability already exists, use a unique slug
            let final_capability = if self.db.get_starflask_agent(&capability).ok().flatten().is_some() {
                format!("{}_{}", capability, &agent.id.to_string()[..8])
            } else {
                capability
            };

            let description = agent.description.as_deref().unwrap_or("");

            self.db.upsert_starflask_agent_str(
                &final_capability,
                &agent.id.to_string(),
                &agent.name,
                description,
                &[],
                "synced",
            )?;

            synced.push(final_capability.clone());

            self.broadcaster.broadcast(GatewayEvent::new(
                "starflask.agent_synced",
                serde_json::json!({
                    "capability": &final_capability,
                    "agent_id": agent.id.to_string(),
                    "name": &agent.name,
                }),
            ));
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
            // Skip if already in DB
            if let Ok(Some(_)) = self.db.get_starflask_agent(&agent_seed.capability) {
                continue;
            }

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

            for hash in &agent_seed.pack_hashes {
                if let Err(e) = self.starflask.install_agent_pack(&agent.id, hash).await {
                    log::warn!("[AgentRegistry] Failed to install pack {} on '{}': {}", hash, agent_seed.name, e);
                }
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

        for hash in &agent_seed.pack_hashes {
            let _ = self.starflask.install_agent_pack(&agent.id, hash).await;
        }

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
        } else if name_lower.contains("social") || name_lower.contains("twitter") || name_lower.contains("x.com") {
            "social_media".to_string()
        } else {
            "general".to_string()
        }
    }
}
