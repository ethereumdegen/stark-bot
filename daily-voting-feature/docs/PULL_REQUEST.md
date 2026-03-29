# Pull Request: Daily Post Voting System with Token Rewards

## Summary

This PR implements the daily voting system proposed in [Issue #39](https://github.com/ethereumdegen/stark-bot/issues/39), enabling users to vote for posts using tokens with automatic reward distribution.

## Features

### Core Functionality
- ✅ 24-hour voting cycles
- ✅ Token-based voting (configurable price)
- ✅ Automatic reward distribution:
  - 30% to winning post creator
  - 50% to winning voters (proportional to stake)
  - 20% burned (deflationary)
- ✅ Real-time leaderboard
- ✅ Reward claiming system

### Technical Implementation
- ✅ Solidity smart contract for BASE blockchain
- ✅ Rust backend API (Axum framework)
- ✅ PostgreSQL database schema
- ✅ Comprehensive test suite
- ✅ API documentation

### Security Features
- Anti-sybil: 1-minute vote cooldown
- Authorized backend whitelist for post registration
- Signature verification for votes
- Pausable contract for emergencies
- Reentrancy protection

## Files Changed

```
daily-voting-feature/
├── contracts/
│   └── DailyVoting.sol          # Smart contract
├── src/
│   ├── main.rs                  # Entry point
│   ├── api/
│   │   ├── mod.rs
│   │   └── handlers.rs          # API endpoints
│   ├── services/
│   │   ├── mod.rs
│   │   ├── voting.rs            # Business logic
│   │   └── blockchain.rs        # BASE interaction
│   ├── models/
│   │   └── mod.rs               # Data types
│   └── db/
│       └── mod.rs               # Database operations
├── migrations/
│   └── 001_create_voting_tables.sql
├── tests/
│   └── DailyVoting.t.sol        # Contract tests
├── script/
│   └── Deploy.s.sol             # Deployment script
├── docs/
│   └── API.md                   # API documentation
├── Cargo.toml
├── foundry.toml
├── .env.example
└── README.md
```

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/cycle/current` | Current cycle info |
| POST | `/api/v1/posts/register` | Register post |
| POST | `/api/v1/vote` | Cast vote |
| GET | `/api/v1/leaderboard` | Get rankings |
| GET | `/api/v1/rewards/:address` | User rewards |
| POST | `/api/v1/rewards/claim` | Claim reward |

## Smart Contract

**Target Chain:** BASE (Chain ID: 8453)
**Token:** `0x587Cd533F418825521f3A1daa7CCd1E7339A1B07`

Key functions:
- `registerPost(bytes32 postId, address creator)`
- `vote(bytes32 postId, uint256 amount)`
- `finalizeCycle()`
- `claimReward(uint256 cycleId)`

## Testing

```bash
# Smart contract tests
forge test -vvv

# Backend tests
cargo test
```

## Deployment Steps

1. Deploy smart contract to BASE
2. Configure `.env` with contract address
3. Run database migrations
4. Start backend server
5. Authorize backend address on contract

## Breaking Changes

None - this is a new feature.

## Checklist

- [x] Code compiles without warnings
- [x] Tests pass
- [x] Documentation updated
- [x] Migration scripts included
- [x] Environment example provided

## Screenshots / Demo

*Add screenshots of the voting UI here when frontend is implemented*

## Related Issues

- Closes #39

## Questions for Reviewers

1. Should the vote price be configurable via admin or hardcoded?
2. Preference on cycle duration (currently 24 hours)?
3. Should we add WebSocket support for real-time updates?

---

**Note:** This implementation focuses on the backend infrastructure. Frontend integration will require additional work to connect the UI to these API endpoints.
