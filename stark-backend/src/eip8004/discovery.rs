//! Agent Discovery
//!
//! Find, index, and search for agents from EIP-8004 registries.

use super::config::Eip8004Config;
use super::identity::IdentityRegistry;
use super::reputation::ReputationRegistry;
use super::types::*;
use crate::wallet::WalletProvider;
use std::collections::HashMap;
use std::sync::Arc;

/// Agent discovery and indexing
pub struct AgentDiscovery {
    config: Eip8004Config,
    identity: IdentityRegistry,
    reputation: ReputationRegistry,
    /// Local cache of discovered agents
    cache: HashMap<(u64, String), DiscoveredAgent>,
}

impl AgentDiscovery {
    /// Create a new discovery client
    pub fn new(config: Eip8004Config) -> Self {
        let identity = IdentityRegistry::new(config.clone());
        let reputation = ReputationRegistry::new(config.clone());

        Self {
            config,
            identity,
            reputation,
            cache: HashMap::new(),
        }
    }

    /// Create with a wallet provider (for Flash/Privy mode)
    pub fn new_with_wallet_provider(config: Eip8004Config, wallet_provider: Arc<dyn WalletProvider>) -> Self {
        let identity = IdentityRegistry::new_with_wallet_provider(config.clone(), wallet_provider.clone());
        let reputation = ReputationRegistry::new_with_wallet_provider(config.clone(), wallet_provider);

        Self {
            config,
            identity,
            reputation,
            cache: HashMap::new(),
        }
    }

    /// Check if registries are deployed
    pub fn is_available(&self) -> bool {
        self.config.is_identity_deployed()
    }

    /// Get total number of registered agents
    pub async fn total_agents(&self) -> Result<u64, String> {
        self.identity.total_supply().await
    }

    /// Discover a single agent by ID
    pub async fn discover_agent(&mut self, agent_id: u64) -> Result<DiscoveredAgent, String> {
        let cache_key = (agent_id, self.config.agent_registry_string());

        // Check cache first
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        // Fetch from registry
        let mut agent = self.identity.get_agent_details(agent_id).await?;

        // Fetch reputation
        if self.config.is_reputation_deployed() {
            if let Ok(summary) = self.reputation.get_summary(agent_id, &[], "", "").await {
                agent.reputation = Some(summary);
            }
        }

        // Cache the result
        self.cache.insert(cache_key, agent.clone());

        Ok(agent)
    }

    /// Discover all agents (paginated)
    pub async fn discover_all(
        &mut self,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<DiscoveredAgent>, String> {
        let total = self.total_agents().await?;

        if offset >= total {
            return Ok(Vec::new());
        }

        let end = (offset + limit).min(total);
        let mut agents = Vec::new();

        for agent_id in (offset + 1)..=end {
            match self.discover_agent(agent_id).await {
                Ok(agent) => agents.push(agent),
                Err(e) => {
                    log::warn!("Failed to discover agent {}: {}", agent_id, e);
                }
            }
        }

        Ok(agents)
    }

    /// Search for agents with specific criteria
    pub async fn search(
        &mut self,
        criteria: SearchCriteria,
    ) -> Result<Vec<DiscoveredAgent>, String> {
        // First, discover all agents (or use cached)
        let total = self.total_agents().await.unwrap_or(0);
        let mut results = Vec::new();

        for agent_id in 1..=total {
            if let Ok(agent) = self.discover_agent(agent_id).await {
                if criteria.matches(&agent) {
                    results.push(agent);

                    // Limit results
                    if let Some(limit) = criteria.limit {
                        if results.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }

        // Sort by reputation if requested
        if criteria.sort_by_reputation {
            results.sort_by(|a, b| {
                let score_a = a.reputation.as_ref().map(|r| r.average_score).unwrap_or(0.0);
                let score_b = b.reputation.as_ref().map(|r| r.average_score).unwrap_or(0.0);
                score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        Ok(results)
    }

    /// Find agents with x402 support
    pub async fn find_x402_agents(&mut self) -> Result<Vec<DiscoveredAgent>, String> {
        self.search(SearchCriteria {
            x402_required: true,
            active_only: true,
            min_trust_level: Some(TrustLevel::Low),
            sort_by_reputation: true,
            limit: Some(50),
            ..Default::default()
        })
        .await
    }

    /// Find agents with a specific service
    pub async fn find_by_service(&mut self, service_name: &str) -> Result<Vec<DiscoveredAgent>, String> {
        self.search(SearchCriteria {
            required_service: Some(service_name.to_string()),
            active_only: true,
            sort_by_reputation: true,
            limit: Some(50),
            ..Default::default()
        })
        .await
    }

    /// Get agent reputation summary
    pub async fn get_reputation(&self, agent_id: u64) -> Result<ReputationSummary, String> {
        self.reputation.get_summary(agent_id, &[], "", "").await
    }

    /// Check if an agent should be trusted
    pub async fn check_trust(&self, agent_id: u64) -> Result<TrustLevel, String> {
        let summary = self.get_reputation(agent_id).await?;
        Ok(summary.trust_level())
    }

    /// Clear the discovery cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Get cached agents
    pub fn cached_agents(&self) -> Vec<&DiscoveredAgent> {
        self.cache.values().collect()
    }

    /// Refresh a cached agent
    pub async fn refresh_agent(&mut self, agent_id: u64) -> Result<DiscoveredAgent, String> {
        let cache_key = (agent_id, self.config.agent_registry_string());
        self.cache.remove(&cache_key);
        self.discover_agent(agent_id).await
    }
}

/// Search criteria for agent discovery
#[derive(Debug, Clone, Default)]
pub struct SearchCriteria {
    /// Only return agents with x402 support
    pub x402_required: bool,
    /// Only return active agents
    pub active_only: bool,
    /// Minimum trust level required
    pub min_trust_level: Option<TrustLevel>,
    /// Minimum reputation count
    pub min_reputation_count: Option<u64>,
    /// Required service name (e.g., "mcp", "swap", "chat")
    pub required_service: Option<String>,
    /// Name contains (case-insensitive)
    pub name_contains: Option<String>,
    /// Sort results by reputation score
    pub sort_by_reputation: bool,
    /// Maximum number of results
    pub limit: Option<usize>,
}

impl SearchCriteria {
    /// Check if an agent matches the criteria
    pub fn matches(&self, agent: &DiscoveredAgent) -> bool {
        // Check x402 support
        if self.x402_required && !agent.is_x402_enabled() {
            return false;
        }

        // Check active status
        if self.active_only && !agent.is_active() {
            return false;
        }

        // Check trust level
        if let Some(ref min_level) = self.min_trust_level {
            let agent_level = agent.trust_level();
            if !self.trust_level_meets_minimum(&agent_level, min_level) {
                return false;
            }
        }

        // Check reputation count
        if let Some(min_count) = self.min_reputation_count {
            let count = agent.reputation.as_ref().map(|r| r.count).unwrap_or(0);
            if count < min_count {
                return false;
            }
        }

        // Check required service
        if let Some(ref service) = self.required_service {
            if !agent.has_service(service) {
                return false;
            }
        }

        // Check name contains
        if let Some(ref name_filter) = self.name_contains {
            let name = agent
                .registration
                .as_ref()
                .map(|r| r.name.to_lowercase())
                .unwrap_or_default();
            if !name.contains(&name_filter.to_lowercase()) {
                return false;
            }
        }

        true
    }

    fn trust_level_meets_minimum(&self, level: &TrustLevel, minimum: &TrustLevel) -> bool {
        let level_value = match level {
            TrustLevel::High => 4,
            TrustLevel::Medium => 3,
            TrustLevel::Low => 2,
            TrustLevel::Unverified => 1,
            TrustLevel::Negative => 0,
        };

        let min_value = match minimum {
            TrustLevel::High => 4,
            TrustLevel::Medium => 3,
            TrustLevel::Low => 2,
            TrustLevel::Unverified => 1,
            TrustLevel::Negative => 0,
        };

        level_value >= min_value
    }
}

/// Agent index for efficient searching (database-backed)
pub struct AgentIndex {
    agents: Vec<IndexedAgent>,
}

#[derive(Debug, Clone)]
pub struct IndexedAgent {
    pub agent_id: u64,
    pub agent_registry: String,
    pub name: String,
    pub description: String,
    pub x402_support: bool,
    pub is_active: bool,
    pub services: Vec<String>,
    pub reputation_score: f64,
    pub reputation_count: u64,
    pub wallet_address: Option<String>,
    pub last_updated: String,
}

impl AgentIndex {
    pub fn new() -> Self {
        Self { agents: Vec::new() }
    }

    /// Add or update an agent in the index
    pub fn upsert(&mut self, agent: &DiscoveredAgent) {
        let indexed = IndexedAgent {
            agent_id: agent.identifier.agent_id,
            agent_registry: agent.identifier.agent_registry.clone(),
            name: agent
                .registration
                .as_ref()
                .map(|r| r.name.clone())
                .unwrap_or_default(),
            description: agent
                .registration
                .as_ref()
                .map(|r| r.description.clone())
                .unwrap_or_default(),
            x402_support: agent.is_x402_enabled(),
            is_active: agent.is_active(),
            services: agent
                .registration
                .as_ref()
                .map(|r| r.services.iter().map(|s| s.name.clone()).collect())
                .unwrap_or_default(),
            reputation_score: agent
                .reputation
                .as_ref()
                .map(|r| r.average_score)
                .unwrap_or(0.0),
            reputation_count: agent.reputation.as_ref().map(|r| r.count).unwrap_or(0),
            wallet_address: agent.wallet_address.clone(),
            last_updated: agent.last_updated.clone(),
        };

        // Update or insert
        if let Some(existing) = self
            .agents
            .iter_mut()
            .find(|a| a.agent_id == indexed.agent_id && a.agent_registry == indexed.agent_registry)
        {
            *existing = indexed;
        } else {
            self.agents.push(indexed);
        }
    }

    /// Search the index
    pub fn search(&self, query: &str) -> Vec<&IndexedAgent> {
        let query_lower = query.to_lowercase();

        self.agents
            .iter()
            .filter(|a| {
                a.name.to_lowercase().contains(&query_lower)
                    || a.description.to_lowercase().contains(&query_lower)
                    || a.services.iter().any(|s| s.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    /// Get all x402-enabled agents
    pub fn x402_agents(&self) -> Vec<&IndexedAgent> {
        self.agents.iter().filter(|a| a.x402_support && a.is_active).collect()
    }

    /// Get agents by service
    pub fn by_service(&self, service: &str) -> Vec<&IndexedAgent> {
        self.agents
            .iter()
            .filter(|a| a.services.iter().any(|s| s == service) && a.is_active)
            .collect()
    }

    /// Get top agents by reputation
    pub fn top_by_reputation(&self, limit: usize) -> Vec<&IndexedAgent> {
        let mut sorted: Vec<_> = self.agents.iter().filter(|a| a.is_active).collect();
        sorted.sort_by(|a, b| {
            b.reputation_score
                .partial_cmp(&a.reputation_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted.into_iter().take(limit).collect()
    }
}

impl Default for AgentIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_criteria() {
        let criteria = SearchCriteria {
            x402_required: true,
            active_only: true,
            ..Default::default()
        };

        // Create a mock agent
        let registration = RegistrationFile::new("TestAgent", "A test")
            .with_service("x402", "https://test.com", "1.0");

        let agent = DiscoveredAgent {
            identifier: AgentIdentifier::new(1, 8453, "0x1234"),
            registration: Some(registration),
            owner_address: "0x5678".to_string(),
            wallet_address: Some("0x9abc".to_string()),
            reputation: None,
            discovered_at: "2024-01-01".to_string(),
            last_updated: "2024-01-01".to_string(),
        };

        assert!(criteria.matches(&agent));
    }

    #[test]
    fn test_agent_index() {
        let mut index = AgentIndex::new();

        let registration = RegistrationFile::new("SwapBot", "Token swaps")
            .with_service("swap", "https://swap.com", "1.0");

        let agent = DiscoveredAgent {
            identifier: AgentIdentifier::new(1, 8453, "0x1234"),
            registration: Some(registration),
            owner_address: "0x5678".to_string(),
            wallet_address: None,
            reputation: Some(ReputationSummary {
                agent_id: 1,
                agent_registry: "test".to_string(),
                count: 10,
                total_value: 800,
                value_decimals: 0,
                average_score: 80.0,
                total_payments_usdc: None,
            }),
            discovered_at: "2024-01-01".to_string(),
            last_updated: "2024-01-01".to_string(),
        };

        index.upsert(&agent);

        let results = index.search("swap");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "SwapBot");

        let by_service = index.by_service("swap");
        assert_eq!(by_service.len(), 1);
    }
}
