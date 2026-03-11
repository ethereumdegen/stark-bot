use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Represents a voting cycle (24-hour period)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Cycle {
    pub id: i64,
    pub cycle_number: i64,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub total_pool: String, // Store as string to handle large numbers
    pub winning_post_id: Option<String>,
    pub finalized: bool,
    pub rewards_distributed: bool,
    pub tx_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Represents a post registered for voting
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Post {
    pub id: Uuid,
    pub external_id: String,      // x402book post ID
    pub cycle_id: i64,
    pub creator_address: String,
    pub total_votes: i64,
    pub total_staked: String,
    pub registered_at: DateTime<Utc>,
    pub tx_hash: Option<String>,
}

/// Represents a vote cast by a user
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Vote {
    pub id: Uuid,
    pub cycle_id: i64,
    pub post_id: Uuid,
    pub voter_address: String,
    pub amount: String,
    pub voted_at: DateTime<Utc>,
    pub tx_hash: Option<String>,
}

/// Represents a reward allocation for a voter
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct VoterReward {
    pub id: Uuid,
    pub cycle_id: i64,
    pub voter_address: String,
    pub amount: String,
    pub claimed: bool,
    pub claim_tx_hash: Option<String>,
    pub created_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
}

/// Creator reward record
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CreatorReward {
    pub id: Uuid,
    pub cycle_id: i64,
    pub creator_address: String,
    pub post_id: Uuid,
    pub amount: String,
    pub tx_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Burned tokens record
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BurnRecord {
    pub id: Uuid,
    pub cycle_id: i64,
    pub amount: String,
    pub tx_hash: Option<String>,
    pub burned_at: DateTime<Utc>,
}

// ============ API Request/Response Types ============

#[derive(Debug, Deserialize)]
pub struct RegisterPostRequest {
    pub post_id: String,
    pub creator_address: String,
}

#[derive(Debug, Deserialize)]
pub struct CastVoteRequest {
    pub post_id: String,
    pub voter_address: String,
    pub amount: String,
    pub signature: String, // For verification
}

#[derive(Debug, Deserialize)]
pub struct ClaimRewardRequest {
    pub cycle_id: i64,
    pub voter_address: String,
    pub signature: String,
}

#[derive(Debug, Serialize)]
pub struct CycleResponse {
    pub cycle_id: i64,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub total_pool: String,
    pub time_remaining_seconds: i64,
    pub is_active: bool,
    pub post_count: i64,
    pub vote_count: i64,
}

#[derive(Debug, Serialize)]
pub struct PostResponse {
    pub id: String,
    pub external_id: String,
    pub creator_address: String,
    pub total_votes: i64,
    pub total_staked: String,
    pub rank: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct LeaderboardEntry {
    pub rank: i32,
    pub post_id: String,
    pub external_id: String,
    pub creator_address: String,
    pub total_votes: i64,
    pub total_staked: String,
    pub percentage_of_pool: f64,
}

#[derive(Debug, Serialize)]
pub struct LeaderboardResponse {
    pub cycle_id: i64,
    pub entries: Vec<LeaderboardEntry>,
    pub total_posts: i64,
    pub total_votes: i64,
    pub total_pool: String,
    pub time_remaining_seconds: i64,
}

#[derive(Debug, Serialize)]
pub struct UserVotesResponse {
    pub address: String,
    pub cycle_id: i64,
    pub votes: Vec<UserVote>,
    pub total_staked: String,
}

#[derive(Debug, Serialize)]
pub struct UserVote {
    pub post_id: String,
    pub external_post_id: String,
    pub amount: String,
    pub voted_at: DateTime<Utc>,
    pub is_winning: bool,
}

#[derive(Debug, Serialize)]
pub struct RewardsResponse {
    pub address: String,
    pub rewards: Vec<RewardEntry>,
    pub total_earned: String,
    pub total_claimed: String,
    pub total_pending: String,
}

#[derive(Debug, Serialize)]
pub struct RewardEntry {
    pub cycle_id: i64,
    pub amount: String,
    pub claimed: bool,
    pub claim_tx_hash: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VoteResponse {
    pub success: bool,
    pub vote_id: String,
    pub tx_hash: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub current_cycle: i64,
    pub database: String,
    pub blockchain: String,
}

// ============ Query Parameters ============

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct LeaderboardParams {
    pub limit: Option<i64>,
    pub cycle_id: Option<i64>,
}
