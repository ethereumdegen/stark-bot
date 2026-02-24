---
name: wallet_monitor
description: "Monitor ETH wallets for on-chain activity, detect whale trades, and track transaction history on Ethereum Mainnet and Base"
version: 2.1.0
author: starkbot
tags: [crypto, defi, monitoring, wallets, whale, alerts]
requires_tools: [local_rpc, dexscreener, token_lookup]
---

# Wallet Monitor Skill

You are helping the user manage their wallet monitoring setup. This skill tracks on-chain activity for watched wallets using Alchemy Enhanced APIs, detecting transfers, swaps, and large trades on Ethereum Mainnet and Base.

The wallet monitor runs as a separate microservice at `http://127.0.0.1:9100`. Use the `local_rpc` tool to call its RPC endpoints directly.

## API Endpoints

All endpoints are on the wallet-monitor-service at `http://127.0.0.1:9100`.

### 1. Watchlist Management

**List all watched wallets:**
```
local_rpc(url="http://127.0.0.1:9100/rpc/watchlist/list")
```

**Add a wallet:**
```
local_rpc(url="http://127.0.0.1:9100/rpc/watchlist/add", method="POST", body={
  "address": "0x...",
  "label": "Whale Alpha",
  "chain": "mainnet",
  "threshold_usd": 50000
})
```
- `address` (required): 0x + 40 hex chars
- `label` (optional): human-readable name
- `chain` (optional): "mainnet" or "base" (default: "mainnet")
- `threshold_usd` (optional): large trade threshold in USD (default: 1000)

**Remove a wallet:**
```
local_rpc(url="http://127.0.0.1:9100/rpc/watchlist/remove", method="POST", body={"id": 1})
```

**Update a wallet:**
```
local_rpc(url="http://127.0.0.1:9100/rpc/watchlist/update", method="POST", body={
  "id": 1,
  "label": "New Label",
  "threshold_usd": 10000,
  "monitor_enabled": true,
  "notes": "Interesting trader"
})
```

### 2. Activity Queries

**Query recent activity:**
```
local_rpc(url="http://127.0.0.1:9100/rpc/activity/query", method="POST", body={
  "limit": 25
})
```

**Large trades only:**
```
local_rpc(url="http://127.0.0.1:9100/rpc/activity/query", method="POST", body={
  "large_only": true,
  "limit": 25
})
```

**Filter by address/chain/type:**
```
local_rpc(url="http://127.0.0.1:9100/rpc/activity/query", method="POST", body={
  "address": "0x...",
  "chain": "mainnet",
  "activity_type": "swap",
  "limit": 50
})
```
Filter fields (all optional): `address`, `chain`, `activity_type` (eth_transfer, erc20_transfer, swap, internal), `large_only` (bool), `limit` (int).

**Activity statistics:**
```
local_rpc(url="http://127.0.0.1:9100/rpc/activity/stats")
```

### 3. Service Control

**Check service status:**
```
local_rpc(url="http://127.0.0.1:9100/rpc/status")
```

## Workflow

1. First check status: `local_rpc(url="http://127.0.0.1:9100/rpc/status")`
2. Add wallets: `local_rpc(url="http://127.0.0.1:9100/rpc/watchlist/add", method="POST", body={...})`
3. The background worker automatically polls every 60 seconds
4. Query activity: `local_rpc(url="http://127.0.0.1:9100/rpc/activity/query", method="POST", body={"limit": 25})`

## Important Notes

- The wallet monitor runs as a standalone service (wallet-monitor-service)
- Dashboard available at http://127.0.0.1:9100/
- Supported chains: "mainnet" (Ethereum) and "base" (Base)
- Each wallet has its own threshold_usd for large trade detection (default $1,000)
- Swap detection: transactions with both outgoing and incoming ERC-20 transfers are classified as swaps
- USD values are estimated using DexScreener price data (cached 60s)
- The worker uses block-number cursors for gap-free incremental polling
- All responses are wrapped in `{"success": true, "data": ...}` or `{"success": false, "error": "..."}`
