//! Reputation Registry interactions
//!
//! Submit feedback, query reputation, manage responses.

use super::abi::common::keccak256;
use super::abi::reputation::*;
use super::config::Eip8004Config;
use super::types::*;
use crate::wallet::WalletProvider;
use crate::x402::X402EvmRpc;
use ethers::types::Address;
use std::str::FromStr;
use std::sync::Arc;

/// Reputation Registry client
pub struct ReputationRegistry {
    config: Eip8004Config,
    rpc: Option<X402EvmRpc>,
    wallet_provider: Option<Arc<dyn WalletProvider>>,
}

impl ReputationRegistry {
    /// Create a new Reputation Registry client
    pub fn new(config: Eip8004Config) -> Self {
        Self { config, rpc: None, wallet_provider: None }
    }

    /// Create with a wallet provider (for Flash/Privy mode)
    pub fn new_with_wallet_provider(config: Eip8004Config, wallet_provider: Arc<dyn WalletProvider>) -> Self {
        Self {
            config,
            rpc: None,
            wallet_provider: Some(wallet_provider),
        }
    }

    /// Create with an existing RPC client
    pub fn with_rpc(config: Eip8004Config, rpc: X402EvmRpc) -> Self {
        Self {
            config,
            rpc: Some(rpc),
            wallet_provider: None,
        }
    }

    /// Get or create RPC client
    fn get_rpc(&self) -> Result<X402EvmRpc, String> {
        let network = if self.config.chain_id == 1 { "mainnet" } else { "base" };

        // Prefer wallet provider (works in both Standard and Flash/Privy mode)
        if let Some(ref wp) = self.wallet_provider {
            return X402EvmRpc::new_with_wallet_provider(wp.clone(), network, None, true);
        }

        // Fall back to raw private key (Standard mode only)
        let private_key = crate::config::burner_wallet_private_key()
            .ok_or("BURNER_WALLET_BOT_PRIVATE_KEY not set")?;
        X402EvmRpc::new(&private_key, network)
    }

    /// Get the registry contract address
    pub fn registry_address(&self) -> &str {
        &self.config.reputation_registry
    }

    /// Check if the registry is deployed
    pub fn is_deployed(&self) -> bool {
        self.config.is_reputation_deployed()
    }

    /// Parse registry address
    fn parse_registry_address(&self) -> Result<Address, String> {
        Address::from_str(&self.config.reputation_registry)
            .map_err(|e| format!("Invalid registry address: {}", e))
    }

    /// Get reputation summary for an agent
    pub async fn get_summary(
        &self,
        agent_id: u64,
        client_addresses: &[String],
        tag1: &str,
        tag2: &str,
    ) -> Result<ReputationSummary, String> {
        if !self.is_deployed() {
            return Err("Reputation Registry not deployed".to_string());
        }

        let rpc = self.get_rpc()?;
        let registry_addr = self.parse_registry_address()?;
        let calldata = encode_get_summary(agent_id, client_addresses, tag1, tag2);

        let result = rpc.eth_call(registry_addr, &calldata).await?;

        let (count, total_value, value_decimals) = decode_summary_result(&result)?;

        // Calculate average score
        let average_score = if count > 0 {
            let divisor = 10_f64.powi(value_decimals as i32);
            (total_value as f64 / divisor) / count as f64
        } else {
            0.0
        };

        Ok(ReputationSummary {
            agent_id,
            agent_registry: self.config.agent_registry_string(),
            count,
            total_value,
            value_decimals,
            average_score,
            total_payments_usdc: None,
        })
    }

    /// Get encoded calldata for giveFeedback - for use with web3_tx tool
    pub fn encode_give_feedback(
        &self,
        agent_id: u64,
        value: i64,
        value_decimals: u8,
        tag1: &str,
        tag2: &str,
        endpoint: &str,
        feedback_uri: &str,
        feedback_content: Option<&str>,
    ) -> String {
        // Compute feedback hash if content provided
        let feedback_hash = feedback_content.map(|content| {
            let hash = keccak256(content.as_bytes());
            hash
        });

        let calldata = encode_give_feedback(
            agent_id,
            value as i128,
            value_decimals,
            tag1,
            tag2,
            endpoint,
            feedback_uri,
            feedback_hash,
        );

        format!("0x{}", hex::encode(&calldata))
    }

    /// Get encoded calldata for revokeFeedback - for use with web3_tx tool
    pub fn encode_revoke_feedback(&self, agent_id: u64, feedback_index: u64) -> String {
        let calldata = encode_revoke_feedback(agent_id, feedback_index);
        format!("0x{}", hex::encode(&calldata))
    }

    /// Get encoded calldata for appendResponse - for use with web3_tx tool
    pub fn encode_append_response(
        &self,
        agent_id: u64,
        client_address: &str,
        feedback_index: u64,
        response_uri: &str,
        response_content: Option<&str>,
    ) -> String {
        // Compute response hash if content provided
        let response_hash = response_content
            .map(|content| keccak256(content.as_bytes()))
            .unwrap_or([0u8; 32]);

        let calldata = encode_append_response(
            agent_id,
            client_address,
            feedback_index,
            response_uri,
            response_hash,
        );

        format!("0x{}", hex::encode(&calldata))
    }

    /// Create a feedback file with proof of payment
    pub fn create_feedback_file(
        &self,
        agent_id: u64,
        client_address: &str,
        value: i64,
        tag1: Option<&str>,
        tag2: Option<&str>,
        endpoint: Option<&str>,
        payment: Option<&X402PaymentRecord>,
    ) -> FeedbackFile {
        let now = chrono::Utc::now().to_rfc3339();

        let proof_of_payment = payment.and_then(|p| p.to_proof());

        FeedbackFile {
            agent_registry: self.config.agent_registry_string(),
            agent_id,
            client_address: format!("eip155:{}:{}", self.config.chain_id, client_address),
            created_at: now,
            value,
            value_decimals: 0,
            tag1: tag1.map(String::from),
            tag2: tag2.map(String::from),
            endpoint: endpoint.map(String::from),
            proof_of_payment,
        }
    }

    /// Determine feedback value based on operation success
    pub fn calculate_feedback_value(
        success: bool,
        response_time_ms: Option<u64>,
        cost_usdc: Option<f64>,
    ) -> i64 {
        let mut score: i64 = if success { 75 } else { -50 };

        // Bonus for fast responses (< 1 second)
        if let Some(time) = response_time_ms {
            if time < 1000 {
                score += 15;
            } else if time < 3000 {
                score += 5;
            } else if time > 10000 {
                score -= 10;
            }
        }

        // Slight bonus for reasonable pricing
        if let Some(cost) = cost_usdc {
            if cost < 0.001 {
                score += 10;
            } else if cost > 0.1 {
                score -= 5;
            }
        }

        // Clamp to -100 to 100
        score.clamp(-100, 100)
    }

    /// Get trust level from reputation summary
    pub fn get_trust_level(&self, summary: &ReputationSummary) -> TrustLevel {
        summary.trust_level()
    }

    /// Check if an agent should be trusted based on reputation
    pub fn should_trust(&self, summary: &ReputationSummary) -> bool {
        matches!(
            summary.trust_level(),
            TrustLevel::High | TrustLevel::Medium
        )
    }
}

/// Feedback submission builder
pub struct FeedbackBuilder {
    agent_id: u64,
    value: i64,
    value_decimals: u8,
    tag1: String,
    tag2: String,
    endpoint: String,
    feedback_uri: String,
    feedback_content: Option<String>,
    payment: Option<X402PaymentRecord>,
}

impl FeedbackBuilder {
    pub fn new(agent_id: u64) -> Self {
        Self {
            agent_id,
            value: 50, // Default neutral-positive
            value_decimals: 0,
            tag1: String::new(),
            tag2: String::new(),
            endpoint: String::new(),
            feedback_uri: String::new(),
            feedback_content: None,
            payment: None,
        }
    }

    pub fn value(mut self, value: i64) -> Self {
        self.value = value.clamp(-100, 100);
        self
    }

    pub fn positive(self) -> Self {
        self.value(100)
    }

    pub fn negative(self) -> Self {
        self.value(-100)
    }

    pub fn neutral(self) -> Self {
        self.value(0)
    }

    pub fn tag1(mut self, tag: &str) -> Self {
        self.tag1 = tag.to_string();
        self
    }

    pub fn tag2(mut self, tag: &str) -> Self {
        self.tag2 = tag.to_string();
        self
    }

    pub fn tags(self, tag1: &str, tag2: &str) -> Self {
        self.tag1(tag1).tag2(tag2)
    }

    pub fn endpoint(mut self, endpoint: &str) -> Self {
        self.endpoint = endpoint.to_string();
        self
    }

    pub fn feedback_uri(mut self, uri: &str) -> Self {
        self.feedback_uri = uri.to_string();
        self
    }

    pub fn feedback_content(mut self, content: &str) -> Self {
        self.feedback_content = Some(content.to_string());
        self
    }

    pub fn with_payment(mut self, payment: X402PaymentRecord) -> Self {
        self.payment = Some(payment);
        self
    }

    /// Build the feedback parameters
    pub fn build(self) -> FeedbackParams {
        FeedbackParams {
            agent_id: self.agent_id,
            value: self.value,
            value_decimals: self.value_decimals,
            tag1: self.tag1,
            tag2: self.tag2,
            endpoint: self.endpoint,
            feedback_uri: self.feedback_uri,
            feedback_content: self.feedback_content,
            payment: self.payment,
        }
    }
}

/// Parameters for submitting feedback
#[derive(Debug, Clone)]
pub struct FeedbackParams {
    pub agent_id: u64,
    pub value: i64,
    pub value_decimals: u8,
    pub tag1: String,
    pub tag2: String,
    pub endpoint: String,
    pub feedback_uri: String,
    pub feedback_content: Option<String>,
    pub payment: Option<X402PaymentRecord>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_feedback_value() {
        // Successful fast response
        let score = ReputationRegistry::calculate_feedback_value(true, Some(500), Some(0.0001));
        assert_eq!(score, 100); // 75 + 15 + 10

        // Failed slow response
        let score = ReputationRegistry::calculate_feedback_value(false, Some(15000), None);
        assert_eq!(score, -60); // -50 - 10

        // Successful normal response
        let score = ReputationRegistry::calculate_feedback_value(true, Some(2000), Some(0.01));
        assert_eq!(score, 80); // 75 + 5
    }

    #[test]
    fn test_feedback_builder() {
        let params = FeedbackBuilder::new(42)
            .positive()
            .tags("api", "swap")
            .endpoint("https://api.example.com/swap")
            .build();

        assert_eq!(params.agent_id, 42);
        assert_eq!(params.value, 100);
        assert_eq!(params.tag1, "api");
        assert_eq!(params.tag2, "swap");
    }

    #[test]
    fn test_trust_levels() {
        let high = ReputationSummary {
            agent_id: 1,
            agent_registry: "test".to_string(),
            count: 15,
            total_value: 1200,
            value_decimals: 0,
            average_score: 80.0,
            total_payments_usdc: None,
        };

        let config = Eip8004Config::base_mainnet();
        let registry = ReputationRegistry::new(config);

        assert!(registry.should_trust(&high));
        assert_eq!(registry.get_trust_level(&high), TrustLevel::High);
    }
}
