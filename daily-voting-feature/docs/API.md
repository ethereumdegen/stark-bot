# Daily Voting API Documentation

## Base URL
```
http://localhost:3000/api/v1
```

## Authentication
Currently, the API uses signature-based authentication for write operations. Users sign messages with their Ethereum wallet to prove ownership.

---

## Endpoints

### Health Check

#### `GET /health`
Check service health status.

**Response:**
```json
{
  "status": "ok",
  "version": "0.1.0",
  "current_cycle": 42,
  "database": "healthy",
  "blockchain": "healthy"
}
```

---

### Cycles

#### `GET /cycle/current`
Get the current active voting cycle.

**Response:**
```json
{
  "cycle_id": 42,
  "start_time": "2024-01-15T00:00:00Z",
  "end_time": "2024-01-16T00:00:00Z",
  "total_pool": "150000000000000000000",
  "time_remaining_seconds": 43200,
  "is_active": true,
  "post_count": 25,
  "vote_count": 150
}
```

#### `GET /cycle/:cycle_id`
Get a specific cycle by ID.

**Parameters:**
- `cycle_id` (path): Cycle ID

**Response:** Same as above

#### `POST /cycle/:cycle_id/finalize`
Finalize a cycle that has ended. Triggers reward distribution.

**Parameters:**
- `cycle_id` (path): Cycle ID to finalize

**Response:**
```json
{
  "success": true,
  "message": "Cycle 42 finalized"
}
```

**Errors:**
- `400`: Cycle has not ended yet
- `400`: Cycle already finalized

---

### Posts

#### `GET /posts`
List all posts in the current cycle.

**Query Parameters:**
- `page` (optional): Page number (default: 1)
- `limit` (optional): Items per page (default: 20)

**Response:**
```json
[
  {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "external_id": "x402_post_12345",
    "creator_address": "0x1234567890abcdef1234567890abcdef12345678",
    "total_votes": 15,
    "total_staked": "50000000000000000000",
    "rank": 1
  }
]
```

#### `POST /posts/register`
Register a new post for the current voting cycle.

**Request Body:**
```json
{
  "post_id": "x402_post_12345",
  "creator_address": "0x1234567890abcdef1234567890abcdef12345678"
}
```

**Response:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "external_id": "x402_post_12345",
  "creator_address": "0x1234567890abcdef1234567890abcdef12345678",
  "total_votes": 0,
  "total_staked": "0",
  "rank": null
}
```

**Errors:**
- `400`: Post already registered for this cycle
- `400`: Current voting cycle has ended
- `400`: Invalid creator address

#### `GET /posts/:post_id`
Get details of a specific post.

**Parameters:**
- `post_id` (path): Post UUID

**Response:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "external_id": "x402_post_12345",
  "creator_address": "0x1234567890abcdef1234567890abcdef12345678",
  "total_votes": 15,
  "total_staked": "50000000000000000000",
  "rank": 3
}
```

---

### Voting

#### `POST /vote`
Cast a vote for a post.

**Request Body:**
```json
{
  "post_id": "x402_post_12345",
  "voter_address": "0xabcdef1234567890abcdef1234567890abcdef12",
  "amount": "10000000000000000000",
  "signature": "0x..."
}
```

**Fields:**
- `post_id`: The external post ID (from x402book)
- `voter_address`: Ethereum address of the voter
- `amount`: Token amount in wei (must be >= vote price)
- `signature`: EIP-712 signed message proving wallet ownership

**Response:**
```json
{
  "success": true,
  "vote_id": "660e8400-e29b-41d4-a716-446655440001",
  "tx_hash": "0x...",
  "message": "Vote recorded successfully"
}
```

**Errors:**
- `400`: Amount below minimum vote price
- `400`: Already voted for this post
- `400`: Post not found in current cycle
- `400`: Current voting cycle has ended

#### `GET /votes/user/:address`
Get all votes by a user for the current cycle.

**Parameters:**
- `address` (path): Ethereum address

**Query Parameters:**
- `cycle_id` (optional): Specific cycle (default: current)

**Response:**
```json
{
  "address": "0xabcdef1234567890abcdef1234567890abcdef12",
  "cycle_id": 42,
  "votes": [
    {
      "post_id": "550e8400-e29b-41d4-a716-446655440000",
      "external_post_id": "x402_post_12345",
      "amount": "10000000000000000000",
      "voted_at": "2024-01-15T12:30:00Z",
      "is_winning": true
    }
  ],
  "total_staked": "10000000000000000000"
}
```

---

### Leaderboard

#### `GET /leaderboard`
Get the current cycle leaderboard.

**Query Parameters:**
- `limit` (optional): Number of entries (default: 10, max: 100)
- `cycle_id` (optional): Specific cycle (default: current)

**Response:**
```json
{
  "cycle_id": 42,
  "entries": [
    {
      "rank": 1,
      "post_id": "550e8400-e29b-41d4-a716-446655440000",
      "external_id": "x402_post_12345",
      "creator_address": "0x1234567890abcdef1234567890abcdef12345678",
      "total_votes": 25,
      "total_staked": "100000000000000000000",
      "percentage_of_pool": 35.5
    }
  ],
  "total_posts": 25,
  "total_votes": 150,
  "total_pool": "281690140845070000000",
  "time_remaining_seconds": 43200
}
```

#### `GET /leaderboard/:cycle_id`
Get leaderboard for a specific historical cycle.

---

### Rewards

#### `GET /rewards/:address`
Get all rewards (past and pending) for a user.

**Parameters:**
- `address` (path): Ethereum address

**Response:**
```json
{
  "address": "0xabcdef1234567890abcdef1234567890abcdef12",
  "rewards": [
    {
      "cycle_id": 41,
      "amount": "5000000000000000000",
      "claimed": true,
      "claim_tx_hash": "0x..."
    },
    {
      "cycle_id": 42,
      "amount": "7500000000000000000",
      "claimed": false,
      "claim_tx_hash": null
    }
  ],
  "total_earned": "12500000000000000000",
  "total_claimed": "5000000000000000000",
  "total_pending": "7500000000000000000"
}
```

#### `POST /rewards/claim`
Claim a pending reward.

**Request Body:**
```json
{
  "cycle_id": 42,
  "voter_address": "0xabcdef1234567890abcdef1234567890abcdef12",
  "signature": "0x..."
}
```

**Response:**
```json
{
  "success": true,
  "vote_id": "770e8400-e29b-41d4-a716-446655440002",
  "tx_hash": "0x...",
  "message": "Claimed 7500000000000000000 tokens"
}
```

**Errors:**
- `400`: No pending reward found
- `400`: Reward already claimed

---

## Error Responses

All errors follow this format:

```json
{
  "error": "Error message description"
}
```

**HTTP Status Codes:**
- `200`: Success
- `400`: Bad Request (invalid input)
- `404`: Not Found
- `500`: Internal Server Error

---

## Rate Limiting

- Votes: 1-minute cooldown per address
- API requests: 100 requests per minute per IP

---

## WebSocket Events (Future)

```
ws://localhost:3000/ws
```

Events:
- `vote_cast`: New vote received
- `cycle_ended`: Voting cycle ended
- `rewards_distributed`: Rewards calculated
- `leaderboard_update`: Leaderboard changed
