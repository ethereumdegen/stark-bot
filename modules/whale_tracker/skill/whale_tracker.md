---
name: whale_tracker
description: "Track whale wallet movements on Ethereum and Base — detect exchange deposits/withdrawals, score confidence, and verify signal accuracy"
version: 1.1.0
author: starkbot
requires_tools: [local_rpc]
---

# Whale Tracker — RPC Reference

The `whale_tracker` module exposes these RPC endpoints via `local_rpc`.

**After reading these instructions, call `local_rpc` directly to fulfill the user's request. Do NOT call `use_skill` again.**

Use `module="whale_tracker"` — the port is resolved automatically.

## Whale Registry

Manage the whale watchlist — add, remove, list, and update tracked whale wallets.

```
local_rpc(module="whale_tracker", path="/rpc/tools/whales", method="POST", body={
  "action": "add",
  "address": "0x...",
  "chain": "ethereum",
  "label": "Whale Name",
  "category": "individual",
  "tags": ["defi", "early-investor"],
  "notes": "Known early ETH accumulator"
})
```

### Actions

**add** — Add a whale to the watchlist.
- `address` (required): Wallet address
- `chain`: "ethereum" or "base" (default: "ethereum")
- `label`: Human-readable name
- `category`: "individual", "market_maker", "exchange", "fund", "deployer" (default: "individual")
- `tags`: Array of string tags
- `notes`: Free-form notes

**remove** — Remove a whale from the watchlist.
- `address` (required): Wallet address to remove

**list** — List all tracked whales with accuracy stats.
Returns array of whales with joined accuracy data (total_signals, correct_signals, accuracy_pct).

**update** — Update whale metadata.
- `address` (required): Wallet address
- `label`, `category`, `tags`, `enabled`, `notes`: Fields to update

**import_exchanges** — Seed the exchange address database with known exchange hot wallets for Binance, Coinbase, Kraken, OKX, Gemini, and Bybit on Ethereum and Base. Run this once after setup.

```
local_rpc(module="whale_tracker", path="/rpc/tools/whales", method="POST", body={
  "action": "import_exchanges"
})
```

## Signals

Query whale movement signals with scoring and historical accuracy context.

```
local_rpc(module="whale_tracker", path="/rpc/tools/signals", method="POST", body={
  "action": "recent",
  "min_confidence": 60,
  "limit": 20
})
```

### Actions

**recent** — Recent signals, newest first.
- `whale_address`: Filter by whale
- `token`: Filter by token (e.g., "ETH")
- `signal_type`: "bearish", "bullish", or "neutral"
- `min_confidence`: Minimum confidence score (0-100)
- `limit`: Max results (default 50, max 200)

**accuracy** — Per-whale accuracy leaderboard.
Returns array of whales ranked by accuracy percentage, including total signals, correct signals, and average 24h price impact.

**history** — Full signal history for a specific whale with movement details.
- `whale_address` (required): Wallet address
- `limit`: Max results (default 100)

## Worker Control

Control the background polling worker.

```
local_rpc(module="whale_tracker", path="/rpc/tools/control", method="POST", body={
  "action": "status"
})
```

### Actions

**status** — Worker status including last poll time, stats, and configuration.

**trigger** — Force an immediate poll cycle (runs in background thread).

## How Movement Classification Works

The worker classifies each whale movement by checking the counterparty address:

1. **exchange_deposit**: Whale sends tokens to a known exchange address — **bearish** (likely preparing to sell)
2. **exchange_withdrawal**: Whale receives tokens from a known exchange — **bullish** (likely accumulating)
3. **wallet_transfer**: Transfer to/from an unknown wallet — **neutral** (portfolio rebalancing, cold storage, etc.)

Exchange addresses are pre-seeded via `import_exchanges` and cover major CEXs (Binance, Coinbase, Kraken, OKX, Gemini, Bybit) on Ethereum and Base.

## How Confidence Scoring Works

Each signal gets a confidence score (0-100) based on:

| Factor | Points |
|--------|--------|
| Exchange deposit (bearish) | +40 |
| Exchange withdrawal (bullish) | +30 |
| Wallet transfer (neutral) | +10 |
| Size: whale (>$100M) | +30 |
| Size: massive ($10M-$100M) | +20 |
| Size: large ($1M-$10M) | +10 |
| Size: medium ($100K-$1M) | +5 |
| Whale accuracy >= 70% | +20 |
| Whale accuracy >= 50% | +10 |

Signals only fire for movements sized "large" or above ($1M+).

## Size Tiers

| Tier | USD Range |
|------|-----------|
| small | < $100K |
| medium | $100K - $1M |
| large | $1M - $10M |
| massive | $10M - $100M |
| whale | > $100M |

## Outcome Tracking

Signals are verified against actual price movement:
- After 24h, the token price is compared to `price_at_signal`
- Bearish signal + price dropped >= 1% — **correct**
- Bullish signal + price rose >= 1% — **correct**
- After 72h with no confirming move — **incorrect**
- Accuracy stats are rebuilt from resolved signals

## Backup & Restore

Export whale registry and exchange addresses:
```
local_rpc(module="whale_tracker", path="/rpc/backup/export", method="POST")
```

Restore from backup:
```
local_rpc(module="whale_tracker", path="/rpc/backup/restore", method="POST", body={
  "whales": [...],
  "exchange_addresses": [...]
})
```
