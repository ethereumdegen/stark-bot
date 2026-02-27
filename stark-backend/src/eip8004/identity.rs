//! Identity Registry interactions
//!
//! Register agents, query identities, manage metadata.

use super::abi::identity::*;
use super::config::Eip8004Config;
use super::types::*;
use crate::x402::X402EvmRpc;
use ethers::types::Address;
use std::str::FromStr;

/// Identity Registry client
pub struct IdentityRegistry {
    config: Eip8004Config,
    rpc: X402EvmRpc,
}

impl IdentityRegistry {
    /// Create with a pre-resolved RPC client.
    /// Callers should use `resolve_rpc_from_context` or `resolve_rpc` to build the RPC.
    pub fn new(config: Eip8004Config, rpc: X402EvmRpc) -> Self {
        Self { config, rpc }
    }

    /// Get the registry contract address
    pub fn registry_address(&self) -> &str {
        &self.config.identity_registry
    }

    /// Get the chain ID
    pub fn chain_id(&self) -> u64 {
        self.config.chain_id
    }

    /// Check if the registry is deployed
    pub fn is_deployed(&self) -> bool {
        self.config.is_identity_deployed()
    }

    /// Parse registry address
    fn parse_registry_address(&self) -> Result<Address, String> {
        Address::from_str(&self.config.identity_registry)
            .map_err(|e| format!("Invalid registry address: {}", e))
    }

    /// Get total number of registered agents
    pub async fn total_supply(&self) -> Result<u64, String> {
        if !self.is_deployed() {
            return Err("Identity Registry not deployed".to_string());
        }

        let registry_addr = self.parse_registry_address()?;
        let calldata = encode_total_supply();

        let result = self.rpc.eth_call(registry_addr, &calldata).await?;

        decode_uint256_result(&result)
    }

    /// Get agent URI (registration file location)
    pub async fn get_agent_uri(&self, agent_id: u64) -> Result<String, String> {
        if !self.is_deployed() {
            return Err("Identity Registry not deployed".to_string());
        }

        let registry_addr = self.parse_registry_address()?;
        let calldata = encode_token_uri(agent_id);

        let result = self.rpc.eth_call(registry_addr, &calldata).await?;

        decode_token_uri_result(&result)
    }

    /// Get agent owner address
    pub async fn get_owner(&self, agent_id: u64) -> Result<String, String> {
        if !self.is_deployed() {
            return Err("Identity Registry not deployed".to_string());
        }

        let registry_addr = self.parse_registry_address()?;
        let calldata = encode_owner_of(agent_id);

        let result = self.rpc.eth_call(registry_addr, &calldata).await?;

        decode_address_result(&result)
    }

    /// Get agent wallet (payment receiving address)
    pub async fn get_agent_wallet(&self, agent_id: u64) -> Result<String, String> {
        if !self.is_deployed() {
            return Err("Identity Registry not deployed".to_string());
        }

        let registry_addr = self.parse_registry_address()?;
        let calldata = encode_get_agent_wallet(agent_id);

        let result = self.rpc.eth_call(registry_addr, &calldata).await?;

        decode_address_result(&result)
    }

    /// Get number of identity NFTs owned by an address
    pub async fn balance_of(&self, owner: &str) -> Result<u64, String> {
        if !self.is_deployed() {
            return Err("Identity Registry not deployed".to_string());
        }

        let registry_addr = self.parse_registry_address()?;
        let calldata = encode_balance_of(owner);

        let result = self.rpc.eth_call(registry_addr, &calldata).await?;

        decode_uint256_result(&result)
    }

    /// Get token ID by owner address and index (ERC-721 Enumerable)
    pub async fn token_of_owner_by_index(&self, owner: &str, index: u64) -> Result<u64, String> {
        if !self.is_deployed() {
            return Err("Identity Registry not deployed".to_string());
        }

        let registry_addr = self.parse_registry_address()?;
        let calldata = encode_token_of_owner_by_index(owner, index);

        let result = self.rpc.eth_call(registry_addr, &calldata).await?;

        decode_uint256_result(&result)
    }

    /// Check if an agent exists
    pub async fn agent_exists(&self, agent_id: u64) -> Result<bool, String> {
        match self.get_owner(agent_id).await {
            Ok(owner) => {
                // Check if owner is not zero address
                Ok(!owner.contains("0x0000000000000000000000000000000000000000"))
            }
            Err(_) => Ok(false),
        }
    }

    /// Get encoded calldata for register(string) - for use with web3_tx tool
    pub fn encode_register(&self, agent_uri: &str) -> String {
        let calldata = encode_register(agent_uri);
        format!("0x{}", hex::encode(&calldata))
    }

    /// Get encoded calldata for setAgentURI - for use with web3_tx tool
    pub fn encode_set_agent_uri(&self, agent_id: u64, new_uri: &str) -> String {
        let calldata = encode_set_agent_uri(agent_id, new_uri);
        format!("0x{}", hex::encode(&calldata))
    }

    /// Create an agent identifier
    pub fn create_identifier(&self, agent_id: u64) -> AgentIdentifier {
        AgentIdentifier::new(agent_id, self.config.chain_id, &self.config.identity_registry)
    }

    /// Fetch full agent details
    pub async fn get_agent_details(&self, agent_id: u64) -> Result<DiscoveredAgent, String> {
        let owner = self.get_owner(agent_id).await?;
        let uri = self.get_agent_uri(agent_id).await.ok();
        let wallet = self.get_agent_wallet(agent_id).await.ok();

        // Fetch and parse registration file if URI is available
        let registration = if let Some(ref uri) = uri {
            self.fetch_registration(uri).await.ok()
        } else {
            None
        };

        let now = chrono::Utc::now().to_rfc3339();

        Ok(DiscoveredAgent {
            identifier: self.create_identifier(agent_id),
            registration,
            owner_address: owner,
            wallet_address: wallet,
            reputation: None, // Filled in by discovery module
            discovered_at: now.clone(),
            last_updated: now,
        })
    }

    /// Fetch and parse registration file from URI
    pub async fn fetch_registration(&self, uri: &str) -> Result<RegistrationFile, String> {
        let url = self.resolve_uri(uri);

        let client = crate::http::shared_client();
        let response = client
            .get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("Failed to fetch registration: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let text = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse registration JSON: {}", e))
    }

    /// Resolve IPFS or other URIs to HTTP URLs
    fn resolve_uri(&self, uri: &str) -> String {
        resolve_uri(uri)
    }
}

/// Resolve IPFS or other URIs to HTTP URLs
pub fn resolve_uri(uri: &str) -> String {
    if uri.starts_with("ipfs://") {
        let cid = uri.trim_start_matches("ipfs://");
        format!("https://ipfs.io/ipfs/{}", cid)
    } else if uri.starts_with("ar://") {
        let tx_id = uri.trim_start_matches("ar://");
        format!("https://arweave.net/{}", tx_id)
    } else if !uri.starts_with("http://") && !uri.starts_with("https://") {
        format!("https://{}", uri)
    } else {
        uri.to_string()
    }
}

/// Builder for creating a registration file
pub struct RegistrationBuilder {
    registration: RegistrationFile,
}

impl RegistrationBuilder {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            registration: RegistrationFile::new(name, description),
        }
    }

    pub fn image(mut self, url: &str) -> Self {
        self.registration.image = Some(url.to_string());
        self
    }

    pub fn service(mut self, name: &str, endpoint: &str, version: &str) -> Self {
        self.registration.services.push(ServiceEntry {
            name: name.to_string(),
            endpoint: endpoint.to_string(),
            version: version.to_string(),
        });
        self
    }

    pub fn x402_support(mut self, enabled: bool) -> Self {
        self.registration.x402_support = enabled;
        self
    }

    pub fn trust_method(mut self, method: &str) -> Self {
        if !self.registration.supported_trust.contains(&method.to_string()) {
            self.registration.supported_trust.push(method.to_string());
        }
        self
    }

    pub fn build(self) -> RegistrationFile {
        self.registration
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&self.registration)
            .map_err(|e| format!("Failed to serialize: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registration_builder() {
        let reg = RegistrationBuilder::new("TestBot", "A test bot")
            .service("chat", "https://api.example.com/chat", "1.0")
            .service("x402", "https://api.example.com/x402", "1.0")
            .x402_support(true)
            .build();

        assert_eq!(reg.name, "TestBot");
        assert_eq!(reg.services.len(), 2);
        assert!(reg.x402_support);
    }

    #[test]
    fn test_resolve_uri() {
        assert!(resolve_uri("ipfs://QmTest").contains("ipfs.io/ipfs/QmTest"));
        assert!(resolve_uri("https://example.com/reg.json").contains("example.com"));
    }
}
