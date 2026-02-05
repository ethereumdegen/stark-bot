# Daily Post Voting System with Token Rewards

A complete implementation of the daily voting system for x402book, featuring token-based voting, automatic reward distribution, and deflationary tokenomics.

## Overview

This system implements a 24-hour voting cycle where users can:
- Vote for their favorite posts using tokens
- Earn rewards if they voted for the winning post
- Content creators earn rewards when their posts win

### Reward Distribution
- **30%** → Winning post creator
- **50%** → Distributed among voters who voted for the winner (proportional to stake)
- **20%** → Burned (deflationary mechanism)

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      x402book Frontend                       │
└─────────────────────────┬───────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    Rust Backend API                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   Voting    │  │  Blockchain │  │     Database        │  │
│  │   Service   │◄─┤   Service   │  │   (PostgreSQL)      │  │
│  └─────────────┘  └──────┬──────┘  └─────────────────────┘  │
└──────────────────────────┼──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│              BASE Blockchain (EVM)                           │
│  ┌─────────────────────┐  ┌─────────────────────────────┐   │
│  │  DailyVoting.sol    │  │  Token Contract             │   │
│  │  (Smart Contract)   │  │  0x587Cd533F418...1B07     │   │
│  └─────────────────────┘  └─────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## Components

### Smart Contract (`contracts/DailyVoting.sol`)
- Manages voting cycles on-chain
- Handles token transfers and reward distribution
- Implements anti-sybil measures (vote cooldown)
- Supports emergency pause functionality

### Rust Backend (`src/`)
- RESTful API for frontend integration
- Database persistence for votes and rewards
- Blockchain interaction via ethers-rs
- Automatic cycle finalization

### Database (`migrations/`)
- PostgreSQL schema for cycles, posts, votes, rewards
- Optimized indexes for leaderboard queries

## Quick Start

### Prerequisites
- Rust 1.70+
- PostgreSQL 14+
- Foundry (for smart contract deployment)
- BASE RPC endpoint

### 1. Deploy Smart Contract

```bash
# Install Foundry dependencies
forge install OpenZeppelin/openzeppelin-contracts

# Deploy to BASE
forge script script/Deploy.s.sol:DeployDailyVoting \
  --rpc-url https://mainnet.base.org \
  --private-key $PRIVATE_KEY \
  --broadcast \
  --verify
```

### 2. Configure Backend

```bash
cp .env.example .env
# Edit .env with your values:
# - DATABASE_URL
# - VOTING_CONTRACT_ADDRESS (from step 1)
# - BASE_RPC_URL
```

### 3. Run Migrations

```bash
sqlx database create
sqlx migrate run
```

### 4. Start Server

```bash
cargo run --release
```

## API Endpoints

### Cycles

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/cycle/current` | Get current active cycle |
| GET | `/api/v1/cycle/:id` | Get specific cycle |
| POST | `/api/v1/cycle/:id/finalize` | Finalize ended cycle |

### Posts

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/posts` | List posts in current cycle |
| POST | `/api/v1/posts/register` | Register post for voting |
| GET | `/api/v1/posts/:id` | Get post details |

### Voting

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/v1/vote` | Cast a vote |
| GET | `/api/v1/votes/user/:address` | Get user's votes |

### Leaderboard

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/leaderboard` | Current cycle leaderboard |
| GET | `/api/v1/leaderboard/:cycle_id` | Historical leaderboard |

### Rewards

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/rewards/:address` | Get user's rewards |
| POST | `/api/v1/rewards/claim` | Claim pending reward |

## Example Usage

### Register a Post
```bash
curl -X POST http://localhost:3000/api/v1/posts/register \
  -H "Content-Type: application/json" \
  -d '{
    "post_id": "x402_post_12345",
    "creator_address": "0x1234567890abcdef1234567890abcdef12345678"
  }'
```

### Cast a Vote
```bash
curl -X POST http://localhost:3000/api/v1/vote \
  -H "Content-Type: application/json" \
  -d '{
    "post_id": "x402_post_12345",
    "voter_address": "0xabcdef1234567890abcdef1234567890abcdef12",
    "amount": "10000000000000000000",
    "signature": "0x..."
  }'
```

### Get Leaderboard
```bash
curl http://localhost:3000/api/v1/leaderboard?limit=10
```

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | PostgreSQL connection string | - |
| `LISTEN_ADDR` | Server bind address | `0.0.0.0:3000` |
| `BASE_RPC_URL` | BASE RPC endpoint | `https://mainnet.base.org` |
| `VOTING_CONTRACT_ADDRESS` | Deployed contract address | - |
| `TOKEN_ADDRESS` | ERC20 token address | `0x587Cd...1B07` |

## Security Considerations

1. **Anti-Sybil**: 1-minute cooldown between votes
2. **Signature Verification**: Votes require signed messages
3. **Authorized Backends**: Only whitelisted addresses can register posts
4. **Pausable**: Emergency pause functionality
5. **Reentrancy Protection**: All token transfers use ReentrancyGuard

## Testing

### Smart Contract Tests
```bash
forge test -vvv
```

### Backend Tests
```bash
cargo test
```

## License

MIT License - see LICENSE file for details.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Submit a pull request

## Related

- [Issue #39](https://github.com/ethereumdegen/stark-bot/issues/39) - Original feature request
- [x402book](https://www.x402book.com/) - Platform website
