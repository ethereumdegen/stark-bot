use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{
    Cycle, Post, Vote, VoterReward, CreatorReward, BurnRecord,
    LeaderboardEntry,
};

pub struct Database {
    pool: PgPool,
}

impl Database {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // ============ Cycle Operations ============

    pub async fn get_current_cycle(&self) -> Result<Option<Cycle>> {
        let cycle = sqlx::query_as::<_, Cycle>(
            r#"
            SELECT * FROM cycles 
            WHERE finalized = false 
            ORDER BY cycle_number DESC 
            LIMIT 1
            "#
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(cycle)
    }

    pub async fn get_cycle(&self, cycle_id: i64) -> Result<Option<Cycle>> {
        let cycle = sqlx::query_as::<_, Cycle>(
            "SELECT * FROM cycles WHERE id = $1"
        )
        .bind(cycle_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(cycle)
    }

    pub async fn get_cycle_by_number(&self, cycle_number: i64) -> Result<Option<Cycle>> {
        let cycle = sqlx::query_as::<_, Cycle>(
            "SELECT * FROM cycles WHERE cycle_number = $1"
        )
        .bind(cycle_number)
        .fetch_optional(&self.pool)
        .await?;

        Ok(cycle)
    }

    pub async fn create_cycle(&self, cycle_number: i64, start_time: DateTime<Utc>, end_time: DateTime<Utc>) -> Result<Cycle> {
        let cycle = sqlx::query_as::<_, Cycle>(
            r#"
            INSERT INTO cycles (cycle_number, start_time, end_time, total_pool, finalized, rewards_distributed)
            VALUES ($1, $2, $3, '0', false, false)
            RETURNING *
            "#
        )
        .bind(cycle_number)
        .bind(start_time)
        .bind(end_time)
        .fetch_one(&self.pool)
        .await?;

        Ok(cycle)
    }

    pub async fn finalize_cycle(&self, cycle_id: i64, winning_post_id: Option<String>, tx_hash: Option<String>) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE cycles 
            SET finalized = true, 
                winning_post_id = $2, 
                tx_hash = $3,
                updated_at = NOW()
            WHERE id = $1
            "#
        )
        .bind(cycle_id)
        .bind(winning_post_id)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_cycle_pool(&self, cycle_id: i64, new_total: &str) -> Result<()> {
        sqlx::query(
            "UPDATE cycles SET total_pool = $2, updated_at = NOW() WHERE id = $1"
        )
        .bind(cycle_id)
        .bind(new_total)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ============ Post Operations ============

    pub async fn register_post(&self, external_id: &str, cycle_id: i64, creator_address: &str, tx_hash: Option<&str>) -> Result<Post> {
        let post = sqlx::query_as::<_, Post>(
            r#"
            INSERT INTO posts (id, external_id, cycle_id, creator_address, total_votes, total_staked, tx_hash)
            VALUES ($1, $2, $3, $4, 0, '0', $5)
            RETURNING *
            "#
        )
        .bind(Uuid::new_v4())
        .bind(external_id)
        .bind(cycle_id)
        .bind(creator_address.to_lowercase())
        .bind(tx_hash)
        .fetch_one(&self.pool)
        .await?;

        Ok(post)
    }

    pub async fn get_post(&self, post_id: Uuid) -> Result<Option<Post>> {
        let post = sqlx::query_as::<_, Post>(
            "SELECT * FROM posts WHERE id = $1"
        )
        .bind(post_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(post)
    }

    pub async fn get_post_by_external_id(&self, external_id: &str, cycle_id: i64) -> Result<Option<Post>> {
        let post = sqlx::query_as::<_, Post>(
            "SELECT * FROM posts WHERE external_id = $1 AND cycle_id = $2"
        )
        .bind(external_id)
        .bind(cycle_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(post)
    }

    pub async fn get_cycle_posts(&self, cycle_id: i64) -> Result<Vec<Post>> {
        let posts = sqlx::query_as::<_, Post>(
            "SELECT * FROM posts WHERE cycle_id = $1 ORDER BY total_votes DESC"
        )
        .bind(cycle_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(posts)
    }

    pub async fn update_post_stats(&self, post_id: Uuid, total_votes: i64, total_staked: &str) -> Result<()> {
        sqlx::query(
            "UPDATE posts SET total_votes = $2, total_staked = $3 WHERE id = $1"
        )
        .bind(post_id)
        .bind(total_votes)
        .bind(total_staked)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_cycle_post_count(&self, cycle_id: i64) -> Result<i64> {
        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM posts WHERE cycle_id = $1"
        )
        .bind(cycle_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(result.0)
    }

    // ============ Vote Operations ============

    pub async fn record_vote(&self, cycle_id: i64, post_id: Uuid, voter_address: &str, amount: &str, tx_hash: Option<&str>) -> Result<Vote> {
        let vote = sqlx::query_as::<_, Vote>(
            r#"
            INSERT INTO votes (id, cycle_id, post_id, voter_address, amount, tx_hash)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#
        )
        .bind(Uuid::new_v4())
        .bind(cycle_id)
        .bind(post_id)
        .bind(voter_address.to_lowercase())
        .bind(amount)
        .bind(tx_hash)
        .fetch_one(&self.pool)
        .await?;

        Ok(vote)
    }

    pub async fn get_user_votes(&self, voter_address: &str, cycle_id: i64) -> Result<Vec<Vote>> {
        let votes = sqlx::query_as::<_, Vote>(
            r#"
            SELECT * FROM votes 
            WHERE voter_address = $1 AND cycle_id = $2 
            ORDER BY voted_at DESC
            "#
        )
        .bind(voter_address.to_lowercase())
        .bind(cycle_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(votes)
    }

    pub async fn has_voted_for_post(&self, voter_address: &str, post_id: Uuid) -> Result<bool> {
        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM votes WHERE voter_address = $1 AND post_id = $2"
        )
        .bind(voter_address.to_lowercase())
        .bind(post_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(result.0 > 0)
    }

    pub async fn get_cycle_vote_count(&self, cycle_id: i64) -> Result<i64> {
        let result: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM votes WHERE cycle_id = $1"
        )
        .bind(cycle_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(result.0)
    }

    pub async fn get_post_voters(&self, post_id: Uuid) -> Result<Vec<Vote>> {
        let votes = sqlx::query_as::<_, Vote>(
            "SELECT * FROM votes WHERE post_id = $1"
        )
        .bind(post_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(votes)
    }

    // ============ Leaderboard ============

    pub async fn get_leaderboard(&self, cycle_id: i64, limit: i64) -> Result<Vec<LeaderboardEntry>> {
        let entries = sqlx::query_as::<_, (String, String, String, i64, String)>(
            r#"
            SELECT 
                p.id::text,
                p.external_id,
                p.creator_address,
                p.total_votes,
                p.total_staked
            FROM posts p
            WHERE p.cycle_id = $1
            ORDER BY p.total_votes DESC, p.total_staked DESC
            LIMIT $2
            "#
        )
        .bind(cycle_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        // Get total pool for percentage calculation
        let cycle = self.get_cycle(cycle_id).await?;
        let total_pool: f64 = cycle
            .map(|c| c.total_pool.parse::<f64>().unwrap_or(0.0))
            .unwrap_or(0.0);

        let leaderboard: Vec<LeaderboardEntry> = entries
            .into_iter()
            .enumerate()
            .map(|(idx, (post_id, external_id, creator_address, total_votes, total_staked))| {
                let staked: f64 = total_staked.parse().unwrap_or(0.0);
                let percentage = if total_pool > 0.0 {
                    (staked / total_pool) * 100.0
                } else {
                    0.0
                };

                LeaderboardEntry {
                    rank: (idx + 1) as i32,
                    post_id,
                    external_id,
                    creator_address,
                    total_votes,
                    total_staked,
                    percentage_of_pool: percentage,
                }
            })
            .collect();

        Ok(leaderboard)
    }

    // ============ Rewards ============

    pub async fn create_voter_reward(&self, cycle_id: i64, voter_address: &str, amount: &str) -> Result<VoterReward> {
        let reward = sqlx::query_as::<_, VoterReward>(
            r#"
            INSERT INTO voter_rewards (id, cycle_id, voter_address, amount, claimed)
            VALUES ($1, $2, $3, $4, false)
            RETURNING *
            "#
        )
        .bind(Uuid::new_v4())
        .bind(cycle_id)
        .bind(voter_address.to_lowercase())
        .bind(amount)
        .fetch_one(&self.pool)
        .await?;

        Ok(reward)
    }

    pub async fn get_user_rewards(&self, voter_address: &str) -> Result<Vec<VoterReward>> {
        let rewards = sqlx::query_as::<_, VoterReward>(
            r#"
            SELECT * FROM voter_rewards 
            WHERE voter_address = $1 
            ORDER BY cycle_id DESC
            "#
        )
        .bind(voter_address.to_lowercase())
        .fetch_all(&self.pool)
        .await?;

        Ok(rewards)
    }

    pub async fn get_pending_reward(&self, cycle_id: i64, voter_address: &str) -> Result<Option<VoterReward>> {
        let reward = sqlx::query_as::<_, VoterReward>(
            "SELECT * FROM voter_rewards WHERE cycle_id = $1 AND voter_address = $2 AND claimed = false"
        )
        .bind(cycle_id)
        .bind(voter_address.to_lowercase())
        .fetch_optional(&self.pool)
        .await?;

        Ok(reward)
    }

    pub async fn mark_reward_claimed(&self, reward_id: Uuid, tx_hash: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE voter_rewards 
            SET claimed = true, claim_tx_hash = $2, claimed_at = NOW() 
            WHERE id = $1
            "#
        )
        .bind(reward_id)
        .bind(tx_hash)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn create_creator_reward(&self, cycle_id: i64, creator_address: &str, post_id: Uuid, amount: &str, tx_hash: &str) -> Result<CreatorReward> {
        let reward = sqlx::query_as::<_, CreatorReward>(
            r#"
            INSERT INTO creator_rewards (id, cycle_id, creator_address, post_id, amount, tx_hash)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#
        )
        .bind(Uuid::new_v4())
        .bind(cycle_id)
        .bind(creator_address.to_lowercase())
        .bind(post_id)
        .bind(amount)
        .bind(tx_hash)
        .fetch_one(&self.pool)
        .await?;

        Ok(reward)
    }

    pub async fn record_burn(&self, cycle_id: i64, amount: &str, tx_hash: &str) -> Result<BurnRecord> {
        let burn = sqlx::query_as::<_, BurnRecord>(
            r#"
            INSERT INTO burn_records (id, cycle_id, amount, tx_hash)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#
        )
        .bind(Uuid::new_v4())
        .bind(cycle_id)
        .bind(amount)
        .bind(tx_hash)
        .fetch_one(&self.pool)
        .await?;

        Ok(burn)
    }
}
