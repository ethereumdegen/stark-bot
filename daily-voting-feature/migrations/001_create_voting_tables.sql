-- Daily Voting System Database Schema
-- Migration: 001_create_voting_tables

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ============ Cycles Table ============
-- Tracks 24-hour voting periods
CREATE TABLE IF NOT EXISTS cycles (
    id BIGSERIAL PRIMARY KEY,
    cycle_number BIGINT NOT NULL UNIQUE,
    start_time TIMESTAMPTZ NOT NULL,
    end_time TIMESTAMPTZ NOT NULL,
    total_pool VARCHAR(78) NOT NULL DEFAULT '0',
    winning_post_id VARCHAR(255),
    finalized BOOLEAN NOT NULL DEFAULT FALSE,
    rewards_distributed BOOLEAN NOT NULL DEFAULT FALSE,
    tx_hash VARCHAR(66),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_cycles_finalized ON cycles(finalized);
CREATE INDEX idx_cycles_end_time ON cycles(end_time);

-- ============ Posts Table ============
-- Posts registered for voting in each cycle
CREATE TABLE IF NOT EXISTS posts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    external_id VARCHAR(255) NOT NULL,
    cycle_id BIGINT NOT NULL REFERENCES cycles(id),
    creator_address VARCHAR(42) NOT NULL,
    total_votes BIGINT NOT NULL DEFAULT 0,
    total_staked VARCHAR(78) NOT NULL DEFAULT '0',
    registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    tx_hash VARCHAR(66),
    UNIQUE(external_id, cycle_id)
);

CREATE INDEX idx_posts_cycle_id ON posts(cycle_id);
CREATE INDEX idx_posts_creator ON posts(creator_address);
CREATE INDEX idx_posts_votes ON posts(cycle_id, total_votes DESC);

-- ============ Votes Table ============
-- Individual votes cast by users
CREATE TABLE IF NOT EXISTS votes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    cycle_id BIGINT NOT NULL REFERENCES cycles(id),
    post_id UUID NOT NULL REFERENCES posts(id),
    voter_address VARCHAR(42) NOT NULL,
    amount VARCHAR(78) NOT NULL,
    voted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    tx_hash VARCHAR(66),
    UNIQUE(cycle_id, post_id, voter_address)
);

CREATE INDEX idx_votes_cycle_id ON votes(cycle_id);
CREATE INDEX idx_votes_post_id ON votes(post_id);
CREATE INDEX idx_votes_voter ON votes(voter_address);

-- ============ Voter Rewards Table ============
-- Rewards allocated to winning voters
CREATE TABLE IF NOT EXISTS voter_rewards (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    cycle_id BIGINT NOT NULL REFERENCES cycles(id),
    voter_address VARCHAR(42) NOT NULL,
    amount VARCHAR(78) NOT NULL,
    claimed BOOLEAN NOT NULL DEFAULT FALSE,
    claim_tx_hash VARCHAR(66),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    UNIQUE(cycle_id, voter_address)
);

CREATE INDEX idx_voter_rewards_voter ON voter_rewards(voter_address);
CREATE INDEX idx_voter_rewards_unclaimed ON voter_rewards(claimed) WHERE claimed = FALSE;

-- ============ Creator Rewards Table ============
-- Rewards for winning post creators
CREATE TABLE IF NOT EXISTS creator_rewards (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    cycle_id BIGINT NOT NULL REFERENCES cycles(id),
    creator_address VARCHAR(42) NOT NULL,
    post_id UUID NOT NULL REFERENCES posts(id),
    amount VARCHAR(78) NOT NULL,
    tx_hash VARCHAR(66),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(cycle_id, creator_address)
);

CREATE INDEX idx_creator_rewards_creator ON creator_rewards(creator_address);

-- ============ Burn Records Table ============
-- Track burned tokens for deflationary mechanism
CREATE TABLE IF NOT EXISTS burn_records (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    cycle_id BIGINT NOT NULL REFERENCES cycles(id),
    amount VARCHAR(78) NOT NULL,
    tx_hash VARCHAR(66),
    burned_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_burn_records_cycle ON burn_records(cycle_id);

-- ============ Anti-Sybil: Vote Cooldowns ============
CREATE TABLE IF NOT EXISTS vote_cooldowns (
    voter_address VARCHAR(42) PRIMARY KEY,
    last_vote_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============ Trigger for updated_at ============
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_cycles_updated_at
    BEFORE UPDATE ON cycles
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
