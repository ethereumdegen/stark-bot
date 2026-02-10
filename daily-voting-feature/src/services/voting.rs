use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::Database;
use crate::models::*;
use crate::services::blockchain::BlockchainService;

#[derive(Clone)]
pub struct VotingService {
    db: Database,
    blockchain: BlockchainService,
}

impl VotingService {
    pub fn new(pool: PgPool, blockchain: BlockchainService) -> Self {
        Self {
            db: Database::new(pool),
            blockchain,
        }
    }

    // ============ Cycle Operations ============

    /// Get the current active cycle, creating one if none exists
    pub async fn get_or_create_current_cycle(&self) -> Result<Cycle> {
        if let Some(cycle) = self.db.get_current_cycle().await? {
            return Ok(cycle);
        }

        // No active cycle, create a new one
        let now = Utc::now();
        let end_time = now + Duration::hours(24);
        
        // Get the next cycle number
        let cycle_number = 1; // In production, query for max cycle_number + 1
        
        let cycle = self.db.create_cycle(cycle_number, now, end_time).await?;
        
        tracing::info!("Created new voting cycle: {}", cycle.id);
        
        Ok(cycle)
    }

    /// Get current cycle response with all stats
    pub async fn get_current_cycle_response(&self) -> Result<CycleResponse> {
        let cycle = self.get_or_create_current_cycle().await?;
        
        let post_count = self.db.get_cycle_post_count(cycle.id).await?;
        let vote_count = self.db.get_cycle_vote_count(cycle.id).await?;
        
        let now = Utc::now();
        let time_remaining = if cycle.end_time > now {
            (cycle.end_time - now).num_seconds()
        } else {
            0
        };
        
        Ok(CycleResponse {
            cycle_id: cycle.id,
            start_time: cycle.start_time,
            end_time: cycle.end_time,
            total_pool: cycle.total_pool,
            time_remaining_seconds: time_remaining,
            is_active: !cycle.finalized && time_remaining > 0,
            post_count,
            vote_count,
        })
    }

    /// Get a specific cycle by ID
    pub async fn get_cycle(&self, cycle_id: i64) -> Result<Option<CycleResponse>> {
        let cycle = match self.db.get_cycle(cycle_id).await? {
            Some(c) => c,
            None => return Ok(None),
        };

        let post_count = self.db.get_cycle_post_count(cycle.id).await?;
        let vote_count = self.db.get_cycle_vote_count(cycle.id).await?;
        
        let now = Utc::now();
        let time_remaining = if cycle.end_time > now {
            (cycle.end_time - now).num_seconds()
        } else {
            0
        };

        Ok(Some(CycleResponse {
            cycle_id: cycle.id,
            start_time: cycle.start_time,
            end_time: cycle.end_time,
            total_pool: cycle.total_pool,
            time_remaining_seconds: time_remaining,
            is_active: !cycle.finalized && time_remaining > 0,
            post_count,
            vote_count,
        }))
    }

    // ============ Post Operations ============

    /// Register a new post for the current voting cycle
    pub async fn register_post(&self, request: RegisterPostRequest) -> Result<PostResponse> {
        let cycle = self.get_or_create_current_cycle().await?;
        
        // Check if cycle is still active
        if cycle.finalized || Utc::now() >= cycle.end_time {
            return Err(anyhow!("Current voting cycle has ended"));
        }

        // Check if post already registered
        if self.db.get_post_by_external_id(&request.post_id, cycle.id).await?.is_some() {
            return Err(anyhow!("Post already registered for this cycle"));
        }

        // Validate creator address
        if !request.creator_address.starts_with("0x") || request.creator_address.len() != 42 {
            return Err(anyhow!("Invalid creator address"));
        }

        let post = self.db.register_post(
            &request.post_id,
            cycle.id,
            &request.creator_address,
            None, // tx_hash - would be set after blockchain confirmation
        ).await?;

        tracing::info!(
            "Registered post {} by {} for cycle {}",
            request.post_id, request.creator_address, cycle.id
        );

        Ok(PostResponse {
            id: post.id.to_string(),
            external_id: post.external_id,
            creator_address: post.creator_address,
            total_votes: post.total_votes,
            total_staked: post.total_staked,
            rank: None,
        })
    }

    /// Get a post by ID
    pub async fn get_post(&self, post_id: &str) -> Result<Option<PostResponse>> {
        let uuid = Uuid::parse_str(post_id)?;
        
        let post = match self.db.get_post(uuid).await? {
            Some(p) => p,
            None => return Ok(None),
        };

        Ok(Some(PostResponse {
            id: post.id.to_string(),
            external_id: post.external_id,
            creator_address: post.creator_address,
            total_votes: post.total_votes,
            total_staked: post.total_staked,
            rank: None,
        }))
    }

    /// Get all posts for a cycle
    pub async fn get_cycle_posts(&self, cycle_id: Option<i64>) -> Result<Vec<PostResponse>> {
        let cid = match cycle_id {
            Some(id) => id,
            None => self.get_or_create_current_cycle().await?.id,
        };

        let posts = self.db.get_cycle_posts(cid).await?;

        Ok(posts.into_iter().enumerate().map(|(idx, p)| PostResponse {
            id: p.id.to_string(),
            external_id: p.external_id,
            creator_address: p.creator_address,
            total_votes: p.total_votes,
            total_staked: p.total_staked,
            rank: Some((idx + 1) as i32),
        }).collect())
    }

    // ============ Voting Operations ============

    /// Cast a vote for a post
    pub async fn cast_vote(&self, request: CastVoteRequest) -> Result<VoteResponse> {
        let cycle = self.get_or_create_current_cycle().await?;
        
        // Check if cycle is still active
        if cycle.finalized || Utc::now() >= cycle.end_time {
            return Err(anyhow!("Current voting cycle has ended"));
        }

        // Validate voter address
        if !request.voter_address.starts_with("0x") || request.voter_address.len() != 42 {
            return Err(anyhow!("Invalid voter address"));
        }

        // Find the post
        let post = self.db.get_post_by_external_id(&request.post_id, cycle.id).await?
            .ok_or_else(|| anyhow!("Post not found in current cycle"))?;

        // Check if already voted for this post
        if self.db.has_voted_for_post(&request.voter_address, post.id).await? {
            return Err(anyhow!("Already voted for this post"));
        }

        // Verify signature (optional but recommended)
        // In production, you'd verify the signature matches the voter
        // self.blockchain.verify_vote_signature(...)?;

        // Parse and validate amount
        let amount: u128 = request.amount.parse()
            .map_err(|_| anyhow!("Invalid amount"))?;
        
        let vote_price = self.blockchain.get_vote_price().await?;
        if amount < vote_price.as_u128() {
            return Err(anyhow!("Amount below minimum vote price"));
        }

        // Record the vote
        let vote = self.db.record_vote(
            cycle.id,
            post.id,
            &request.voter_address,
            &request.amount,
            None, // tx_hash - set after blockchain confirmation
        ).await?;

        // Update post stats
        let new_votes = post.total_votes + 1;
        let current_staked: u128 = post.total_staked.parse().unwrap_or(0);
        let new_staked = current_staked + amount;
        
        self.db.update_post_stats(post.id, new_votes, &new_staked.to_string()).await?;

        // Update cycle pool
        let current_pool: u128 = cycle.total_pool.parse().unwrap_or(0);
        let new_pool = current_pool + amount;
        self.db.update_cycle_pool(cycle.id, &new_pool.to_string()).await?;

        tracing::info!(
            "Vote cast: {} voted for post {} with {} tokens",
            request.voter_address, request.post_id, request.amount
        );

        Ok(VoteResponse {
            success: true,
            vote_id: vote.id.to_string(),
            tx_hash: None,
            message: "Vote recorded successfully".to_string(),
        })
    }

    /// Get all votes by a user for the current cycle
    pub async fn get_user_votes(&self, address: &str, cycle_id: Option<i64>) -> Result<UserVotesResponse> {
        let cid = match cycle_id {
            Some(id) => id,
            None => self.get_or_create_current_cycle().await?.id,
        };

        let cycle = self.db.get_cycle(cid).await?
            .ok_or_else(|| anyhow!("Cycle not found"))?;

        let votes = self.db.get_user_votes(address, cid).await?;
        
        let mut user_votes = Vec::new();
        let mut total_staked: u128 = 0;

        for vote in votes {
            let post = self.db.get_post(vote.post_id).await?;
            let external_id = post.map(|p| p.external_id).unwrap_or_default();
            
            let is_winning = cycle.winning_post_id.as_ref()
                .map(|w| w == &vote.post_id.to_string())
                .unwrap_or(false);

            let amount: u128 = vote.amount.parse().unwrap_or(0);
            total_staked += amount;

            user_votes.push(UserVote {
                post_id: vote.post_id.to_string(),
                external_post_id: external_id,
                amount: vote.amount,
                voted_at: vote.voted_at,
                is_winning,
            });
        }

        Ok(UserVotesResponse {
            address: address.to_lowercase(),
            cycle_id: cid,
            votes: user_votes,
            total_staked: total_staked.to_string(),
        })
    }

    // ============ Leaderboard ============

    /// Get the leaderboard for a cycle
    pub async fn get_leaderboard(&self, cycle_id: Option<i64>, limit: i64) -> Result<LeaderboardResponse> {
        let cid = match cycle_id {
            Some(id) => id,
            None => self.get_or_create_current_cycle().await?.id,
        };

        let cycle = self.db.get_cycle(cid).await?
            .ok_or_else(|| anyhow!("Cycle not found"))?;

        let entries = self.db.get_leaderboard(cid, limit).await?;
        let total_posts = self.db.get_cycle_post_count(cid).await?;
        let total_votes = self.db.get_cycle_vote_count(cid).await?;
        
        let now = Utc::now();
        let time_remaining = if cycle.end_time > now {
            (cycle.end_time - now).num_seconds()
        } else {
            0
        };

        Ok(LeaderboardResponse {
            cycle_id: cid,
            entries,
            total_posts,
            total_votes,
            total_pool: cycle.total_pool,
            time_remaining_seconds: time_remaining,
        })
    }

    // ============ Rewards ============

    /// Get all rewards for a user
    pub async fn get_user_rewards(&self, address: &str) -> Result<RewardsResponse> {
        let rewards = self.db.get_user_rewards(address).await?;

        let mut total_earned: u128 = 0;
        let mut total_claimed: u128 = 0;
        let mut total_pending: u128 = 0;

        let entries: Vec<RewardEntry> = rewards.iter().map(|r| {
            let amount: u128 = r.amount.parse().unwrap_or(0);
            total_earned += amount;
            
            if r.claimed {
                total_claimed += amount;
            } else {
                total_pending += amount;
            }

            RewardEntry {
                cycle_id: r.cycle_id,
                amount: r.amount.clone(),
                claimed: r.claimed,
                claim_tx_hash: r.claim_tx_hash.clone(),
            }
        }).collect();

        Ok(RewardsResponse {
            address: address.to_lowercase(),
            rewards: entries,
            total_earned: total_earned.to_string(),
            total_claimed: total_claimed.to_string(),
            total_pending: total_pending.to_string(),
        })
    }

    /// Claim a reward for a specific cycle
    pub async fn claim_reward(&self, request: ClaimRewardRequest) -> Result<VoteResponse> {
        let reward = self.db.get_pending_reward(request.cycle_id, &request.voter_address).await?
            .ok_or_else(|| anyhow!("No pending reward found"))?;

        // Verify signature
        // In production, verify the claim request is signed by the voter
        
        // In a real implementation, this would:
        // 1. Submit a transaction to the blockchain
        // 2. Wait for confirmation
        // 3. Update the database with the tx hash
        
        // For now, we'll just mark it as claimed (placeholder)
        let tx_hash = "0x..."; // Would be the actual tx hash
        self.db.mark_reward_claimed(reward.id, tx_hash).await?;

        tracing::info!(
            "Reward claimed: {} claimed {} tokens from cycle {}",
            request.voter_address, reward.amount, request.cycle_id
        );

        Ok(VoteResponse {
            success: true,
            vote_id: reward.id.to_string(),
            tx_hash: Some(tx_hash.to_string()),
            message: format!("Claimed {} tokens", reward.amount),
        })
    }

    // ============ Cycle Finalization ============

    /// Finalize a cycle and distribute rewards
    pub async fn finalize_cycle(&self, cycle_id: i64) -> Result<()> {
        let cycle = self.db.get_cycle(cycle_id).await?
            .ok_or_else(|| anyhow!("Cycle not found"))?;

        if cycle.finalized {
            return Err(anyhow!("Cycle already finalized"));
        }

        if Utc::now() < cycle.end_time {
            return Err(anyhow!("Cycle has not ended yet"));
        }

        // Get posts and find winner
        let posts = self.db.get_cycle_posts(cycle_id).await?;
        
        if posts.is_empty() {
            // No posts, just finalize without rewards
            self.db.finalize_cycle(cycle_id, None, None).await?;
            tracing::info!("Cycle {} finalized with no posts", cycle_id);
            return Ok(());
        }

        // Find winning post (most votes)
        let winner = posts.into_iter()
            .max_by_key(|p| p.total_votes)
            .unwrap();

        // Calculate reward distribution
        let total_pool: u128 = cycle.total_pool.parse().unwrap_or(0);
        
        if total_pool > 0 {
            let creator_amount = (total_pool * 30) / 100;  // 30%
            let voter_pool = (total_pool * 50) / 100;      // 50%
            let burn_amount = (total_pool * 20) / 100;     // 20%

            // Record creator reward
            self.db.create_creator_reward(
                cycle_id,
                &winner.creator_address,
                winner.id,
                &creator_amount.to_string(),
                "pending", // Would be actual tx hash
            ).await?;

            // Distribute to winning voters
            let winning_votes = self.db.get_post_voters(winner.id).await?;
            let total_winning_stake: u128 = winning_votes.iter()
                .map(|v| v.amount.parse::<u128>().unwrap_or(0))
                .sum();

            if total_winning_stake > 0 {
                for vote in winning_votes {
                    let vote_amount: u128 = vote.amount.parse().unwrap_or(0);
                    let voter_share = (vote_amount * voter_pool) / total_winning_stake;
                    
                    self.db.create_voter_reward(
                        cycle_id,
                        &vote.voter_address,
                        &voter_share.to_string(),
                    ).await?;
                }
            }

            // Record burn
            self.db.record_burn(cycle_id, &burn_amount.to_string(), "pending").await?;
        }

        // Mark cycle as finalized
        self.db.finalize_cycle(cycle_id, Some(winner.id.to_string()), None).await?;

        tracing::info!(
            "Cycle {} finalized. Winner: {} with {} votes",
            cycle_id, winner.external_id, winner.total_votes
        );

        // Create new cycle
        self.get_or_create_current_cycle().await?;

        Ok(())
    }
}
