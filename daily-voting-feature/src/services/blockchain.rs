use anyhow::{anyhow, Result};
use ethers::{
    prelude::*,
    providers::{Http, Provider},
    types::{Address, H256, U256},
};
use std::sync::Arc;

// ABI for the DailyVoting contract (simplified for key functions)
abigen!(
    DailyVotingContract,
    r#"[
        function currentCycleId() view returns (uint256)
        function votePrice() view returns (uint256)
        function cycles(uint256) view returns (uint256 id, uint256 startTime, uint256 endTime, uint256 totalPool, bytes32 winningPostId, bool finalized, bool rewardsDistributed)
        function cyclePosts(uint256,bytes32) view returns (bytes32 postId, address creator, uint256 totalVotes, uint256 totalStaked, bool exists)
        function votes(uint256,address,bytes32) view returns (address voter, bytes32 postId, uint256 amount, uint256 timestamp)
        function voterRewards(uint256,address) view returns (uint256 amount, bool claimed)
        function registerPost(bytes32 postId, address creator) external
        function vote(bytes32 postId, uint256 amount) external
        function finalizeCycle() external
        function claimReward(uint256 cycleId) external
        function getCurrentCycle() view returns (uint256 cycleId, uint256 startTime, uint256 endTime, uint256 totalPool, uint256 timeRemaining, bool isActive)
        function getLeaderboard(uint256 cycleId, uint256 limit) view returns (bytes32[] postIds, uint256[] voteCounts, uint256[] stakedAmounts)
        function hasVoted(uint256 cycleId, address voter, bytes32 postId) view returns (bool)
        function getPendingReward(uint256 cycleId, address voter) view returns (uint256 amount, bool claimed)
        event CycleStarted(uint256 indexed cycleId, uint256 startTime, uint256 endTime)
        event PostRegistered(uint256 indexed cycleId, bytes32 indexed postId, address indexed creator)
        event VoteCast(uint256 indexed cycleId, bytes32 indexed postId, address indexed voter, uint256 amount)
        event CycleFinalized(uint256 indexed cycleId, bytes32 indexed winningPostId, uint256 totalPool)
        event CreatorRewarded(uint256 indexed cycleId, address indexed creator, uint256 amount)
        event VoterRewardAllocated(uint256 indexed cycleId, address indexed voter, uint256 amount)
        event VoterRewardClaimed(uint256 indexed cycleId, address indexed voter, uint256 amount)
        event TokensBurned(uint256 indexed cycleId, uint256 amount)
    ]"#
);

// ERC20 ABI for token interactions
abigen!(
    IERC20,
    r#"[
        function balanceOf(address) view returns (uint256)
        function allowance(address,address) view returns (uint256)
        function approve(address,uint256) returns (bool)
        function transfer(address,uint256) returns (bool)
        function transferFrom(address,address,uint256) returns (bool)
    ]"#
);

#[derive(Clone)]
pub struct BlockchainService {
    provider: Arc<Provider<Http>>,
    contract_address: Address,
    token_address: Address,
    chain_id: u64,
}

impl BlockchainService {
    pub async fn new(rpc_url: &str, contract_address: &str, token_address: &str) -> Result<Self> {
        let provider = Provider::<Http>::try_from(rpc_url)?;
        let chain_id = provider.get_chainid().await?.as_u64();

        let contract_addr: Address = contract_address.parse()
            .map_err(|_| anyhow!("Invalid contract address"))?;
        let token_addr: Address = token_address.parse()
            .map_err(|_| anyhow!("Invalid token address"))?;

        tracing::info!(
            "Blockchain service initialized - Chain ID: {}, Contract: {}, Token: {}",
            chain_id, contract_address, token_address
        );

        Ok(Self {
            provider: Arc::new(provider),
            contract_address: contract_addr,
            token_address: token_addr,
            chain_id,
        })
    }

    /// Get the current cycle information from the contract
    pub async fn get_current_cycle(&self) -> Result<CycleInfo> {
        let contract = DailyVotingContract::new(self.contract_address, self.provider.clone());
        
        let (cycle_id, start_time, end_time, total_pool, time_remaining, is_active) = 
            contract.get_current_cycle().call().await?;

        Ok(CycleInfo {
            cycle_id: cycle_id.as_u64(),
            start_time: start_time.as_u64(),
            end_time: end_time.as_u64(),
            total_pool: total_pool.to_string(),
            time_remaining: time_remaining.as_u64(),
            is_active,
        })
    }

    /// Get the current vote price
    pub async fn get_vote_price(&self) -> Result<U256> {
        let contract = DailyVotingContract::new(self.contract_address, self.provider.clone());
        let price = contract.vote_price().call().await?;
        Ok(price)
    }

    /// Check if a user has already voted for a specific post
    pub async fn has_voted(&self, cycle_id: u64, voter: &str, post_id: [u8; 32]) -> Result<bool> {
        let contract = DailyVotingContract::new(self.contract_address, self.provider.clone());
        let voter_addr: Address = voter.parse()?;
        
        let has_voted = contract
            .has_voted(U256::from(cycle_id), voter_addr, post_id)
            .call()
            .await?;
        
        Ok(has_voted)
    }

    /// Get pending reward for a voter
    pub async fn get_pending_reward(&self, cycle_id: u64, voter: &str) -> Result<RewardInfo> {
        let contract = DailyVotingContract::new(self.contract_address, self.provider.clone());
        let voter_addr: Address = voter.parse()?;
        
        let (amount, claimed) = contract
            .get_pending_reward(U256::from(cycle_id), voter_addr)
            .call()
            .await?;
        
        Ok(RewardInfo {
            amount: amount.to_string(),
            claimed,
        })
    }

    /// Get leaderboard from contract
    pub async fn get_leaderboard(&self, cycle_id: u64, limit: u64) -> Result<Vec<LeaderboardItem>> {
        let contract = DailyVotingContract::new(self.contract_address, self.provider.clone());
        
        let (post_ids, vote_counts, staked_amounts) = contract
            .get_leaderboard(U256::from(cycle_id), U256::from(limit))
            .call()
            .await?;
        
        let items: Vec<LeaderboardItem> = post_ids
            .iter()
            .zip(vote_counts.iter())
            .zip(staked_amounts.iter())
            .map(|((post_id, votes), staked)| LeaderboardItem {
                post_id: hex::encode(post_id),
                vote_count: votes.as_u64(),
                staked_amount: staked.to_string(),
            })
            .collect();
        
        Ok(items)
    }

    /// Get token balance for an address
    pub async fn get_token_balance(&self, address: &str) -> Result<U256> {
        let token = IERC20::new(self.token_address, self.provider.clone());
        let addr: Address = address.parse()?;
        let balance = token.balance_of(addr).call().await?;
        Ok(balance)
    }

    /// Get token allowance
    pub async fn get_token_allowance(&self, owner: &str, spender: &str) -> Result<U256> {
        let token = IERC20::new(self.token_address, self.provider.clone());
        let owner_addr: Address = owner.parse()?;
        let spender_addr: Address = spender.parse()?;
        let allowance = token.allowance(owner_addr, spender_addr).call().await?;
        Ok(allowance)
    }

    /// Verify a signature for a vote transaction
    pub fn verify_vote_signature(
        &self,
        voter: &str,
        post_id: &str,
        amount: &str,
        signature: &str,
    ) -> Result<bool> {
        // Construct the message that was signed
        let message = format!(
            "Vote for post {} with amount {} on x402book",
            post_id, amount
        );
        
        let signature_bytes = hex::decode(signature.trim_start_matches("0x"))?;
        let sig = Signature::try_from(signature_bytes.as_slice())?;
        
        let recovered = sig.recover(message.as_bytes())?;
        let voter_addr: Address = voter.parse()?;
        
        Ok(recovered == voter_addr)
    }

    /// Convert external post ID to bytes32 for contract
    pub fn post_id_to_bytes32(post_id: &str) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        let id_bytes = post_id.as_bytes();
        let len = std::cmp::min(id_bytes.len(), 32);
        bytes[..len].copy_from_slice(&id_bytes[..len]);
        bytes
    }

    /// Convert bytes32 to string post ID
    pub fn bytes32_to_post_id(bytes: [u8; 32]) -> String {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(32);
        String::from_utf8_lossy(&bytes[..end]).to_string()
    }

    /// Check if the contract is healthy
    pub async fn health_check(&self) -> Result<bool> {
        let block = self.provider.get_block_number().await?;
        tracing::debug!("Current block number: {}", block);
        Ok(true)
    }

    pub fn contract_address(&self) -> Address {
        self.contract_address
    }

    pub fn token_address(&self) -> Address {
        self.token_address
    }

    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }
}

// Helper structs for blockchain data
#[derive(Debug, Clone)]
pub struct CycleInfo {
    pub cycle_id: u64,
    pub start_time: u64,
    pub end_time: u64,
    pub total_pool: String,
    pub time_remaining: u64,
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub struct RewardInfo {
    pub amount: String,
    pub claimed: bool,
}

#[derive(Debug, Clone)]
pub struct LeaderboardItem {
    pub post_id: String,
    pub vote_count: u64,
    pub staked_amount: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_post_id_conversion() {
        let original = "post_123456";
        let bytes = BlockchainService::post_id_to_bytes32(original);
        let recovered = BlockchainService::bytes32_to_post_id(bytes);
        assert_eq!(original, recovered);
    }
}
