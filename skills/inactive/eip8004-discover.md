---
name: eip8004-discover
description: "Discover and interact with EIP-8004 registered agents"
version: 1.0.0
author: starkbot
homepage: https://eips.ethereum.org/EIPS/eip-8004
metadata: {"requires_auth": false, "clawdbot":{"emoji":"🔍"}}
tags: [crypto, discovery, eip8004, agents, trust, interop]
---

# EIP-8004 Agent Discovery

Discover, evaluate, and interact with agents registered via EIP-8004 Identity Registry.

## Overview

Agent discovery enables:
- **Finding Agents**: Browse the Identity Registry
- **Evaluating Trust**: Check reputation before interaction
- **Service Discovery**: Find agents with specific capabilities
- **Interoperability**: Connect with agents across organizations

## Contract Addresses (Base Mainnet)

| Contract | Address |
|----------|---------|
| Identity Registry | `0x...` (TBD) |
| Reputation Registry | `0x...` (TBD) |

---

## Discovering Agents

### Get Total Registered Agents

```solidity
function totalSupply() external view returns (uint256)
```

**Using x402_rpc**:
```json
{
  "method": "eth_call",
  "params": [{
    "to": "<IDENTITY_REGISTRY>",
    "data": "0x18160ddd"
  }, "latest"],
  "network": "base"
}
```

### Get Agent by ID

For each agentId from 1 to totalSupply:

1. **Get Token URI**:
```solidity
function tokenURI(uint256 tokenId) external view returns (string memory)
```

2. **Fetch Registration File** from the URI (IPFS, HTTPS, etc.)

3. **Parse Registration JSON**:
```json
{
  "type": "https://eips.ethereum.org/EIPS/eip-8004#registration-v1",
  "name": "ExampleAgent",
  "description": "An AI agent for...",
  "services": [...],
  "x402Support": true,
  "active": true
}
```

### Get Agent Owner

```solidity
function ownerOf(uint256 tokenId) external view returns (address)
```

### Get Agent Wallet (Payment Address)

```solidity
function getAgentWallet(uint256 agentId) external view returns (address)
```

---

## Registration File Schema

```json
{
  "type": "https://eips.ethereum.org/EIPS/eip-8004#registration-v1",
  "name": "string (required)",
  "description": "string (required)",
  "image": "url (optional)",
  "services": [
    {
      "name": "service_type",
      "endpoint": "url",
      "version": "string"
    }
  ],
  "x402Support": "boolean",
  "active": "boolean",
  "registrations": [
    {
      "agentId": "number",
      "agentRegistry": "eip155:chainId:address"
    }
  ],
  "supportedTrust": ["reputation", "crypto-economic", "tee-attestation"]
}
```

### Service Types

| Service | Description |
|---------|-------------|
| `mcp` | Model Context Protocol server |
| `a2a` | Agent-to-Agent protocol |
| `chat` | Chat/conversation endpoint |
| `x402` | x402 payment-enabled API |
| `oasf` | Open Agent Semantic Framework |

---

## Evaluating Agents

### Step 1: Check Active Status

From registration file:
```javascript
if (!registration.active) {
  // Agent is inactive, skip
}
```

### Step 2: Check Required Services

```javascript
const hasX402 = registration.x402Support === true;
const hasMcp = registration.services.some(s => s.name === 'mcp');
```

### Step 3: Query Reputation

Use the `eip8004-reputation` skill:

```solidity
// Get summary with no filters
getSummary(agentId, [], "", "")
```

**Trust Decision Matrix**:

| Reputation Score | Feedback Count | Trust Level |
|-----------------|----------------|-------------|
| >= 75 | >= 10 | High |
| >= 50 | >= 5 | Medium |
| >= 25 | >= 3 | Low |
| < 25 or count < 3 | Any | Verify First |

### Step 4: Verify Payment Address

Ensure `getAgentWallet(agentId)` matches expected payment recipient.

---

## Interacting with Discovered Agents

### Via x402 (Payment-Enabled APIs)

If agent has `x402Support: true`:

1. Find service endpoint from registration
2. Use `x402_preset_fetch` or `x402_rpc` tool
3. Payment handled automatically via EIP-3009
4. Submit feedback after successful interaction

### Via MCP

If agent has MCP service:

```json
{
  "name": "mcp",
  "endpoint": "https://agent.example.com/mcp",
  "version": "1.0"
}
```

Connect using MCP protocol and call available tools/resources.

### Via A2A (Agent-to-Agent)

If agent has A2A service:

```json
{
  "name": "a2a",
  "endpoint": "https://agent.example.com/a2a",
  "version": "1.0"
}
```

Use A2A protocol for structured agent communication.

---

## Building an Agent Index

For efficient discovery, build a local index:

### Indexing Process

```
1. Get totalSupply() from Identity Registry
2. For each agentId 1..totalSupply:
   a. Get tokenURI(agentId)
   b. Fetch registration file
   c. Parse and validate schema
   d. Get reputation summary
   e. Store in local database
3. Periodically refresh (e.g., every hour)
```

### Index Schema

```sql
CREATE TABLE known_agents (
    agent_id INTEGER,
    agent_registry TEXT,
    name TEXT,
    description TEXT,
    registration_uri TEXT,
    x402_support INTEGER,
    services TEXT,  -- JSON array
    is_active INTEGER,
    reputation_score INTEGER,
    reputation_count INTEGER,
    last_refreshed_at TEXT,
    UNIQUE(agent_id, agent_registry)
);
```

### Searching the Index

```sql
-- Find x402-enabled agents with good reputation
SELECT * FROM known_agents
WHERE x402_support = 1
  AND is_active = 1
  AND reputation_score >= 50
  AND reputation_count >= 5
ORDER BY reputation_score DESC;

-- Find agents with specific service
SELECT * FROM known_agents
WHERE services LIKE '%"name":"mcp"%'
  AND is_active = 1;
```

---

## Discovery Workflow

### Use Case: Find a Swap Agent

```
User: "Find an agent that can do token swaps"

1. Search index for agents with swap-related services
2. Filter by x402Support = true (for payment)
3. Sort by reputation
4. Return top candidates with:
   - Name, description
   - Service endpoints
   - Reputation score
   - Payment address
```

### Use Case: Verify Agent Before Interaction

```
User: "Is agent 42 on registry 0x... trustworthy?"

1. Fetch registration file
2. Check active status
3. Query reputation summary
4. Check for payment proofs in feedback
5. Return trust assessment
```

---

## Monitoring New Agents

Listen for new registrations:

```solidity
event Transfer(
    address indexed from,
    address indexed to,
    uint256 indexed tokenId
);
```

When `from == address(0)`, a new agent was registered:
1. Fetch the new agent's registration
2. Add to local index
3. Optionally alert user about new capabilities

---

## Security Considerations

1. **URI Validation**: Sanitize and validate all URIs before fetching
2. **Content Verification**: Check registration file schema
3. **Reputation Weight**: Don't trust agents with no/low reputation
4. **Endpoint Security**: Use HTTPS for all service endpoints
5. **Payment Verification**: Verify agentWallet before sending payments

---

## Example: Full Discovery Flow

```
1. Query Identity Registry
   → totalSupply() = 150 agents

2. Fetch agent #42 registration
   → tokenURI(42) = "ipfs://Qm.../reg.json"
   → Fetch and parse JSON

3. Check capabilities
   → x402Support: true
   → services: [{"name": "swap", "endpoint": "..."}]

4. Query reputation
   → getSummary(42, [], "", "")
   → count: 25, score: 85

5. Decision: HIGH TRUST
   → Proceed with x402 interaction
   → Submit feedback after success
```

---

## Related Skills

- `eip8004-register` - Register your own agent
- `eip8004-reputation` - Manage reputation feedback
- `swap` - Token swaps (common agent service)
- `x402_preset_fetch` - Make x402 API calls
