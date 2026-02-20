---
name: polymarket_us
description: "Trade prediction markets on Polymarket US - search events/markets, check prices, place orders, manage positions."
version: 3.0.0
author: starkbot
homepage: https://docs.polymarket.us/
metadata: {"clawdbot":{"emoji":"🎲"}}
requires_tools: [run_skill_script]
requires_binaries: [uv]
scripts: [polymarket.py]
requires_api_keys:
  POLYMARKET_KEY_ID:
    description: "Polymarket US API key ID — get one at https://poly.market/dev-portal"
    secret: false
  POLYMARKET_SECRET_KEY:
    description: "Polymarket US API secret key (Ed25519) — get one at https://poly.market/dev-portal"
    secret: true
tags: [polymarket, prediction-markets, trading, betting, finance]
arguments:
  action:
    description: "Action: search, market, bet, positions, orders, cancel, balance"
    required: false
  query:
    description: "Search query or market slug"
    required: false
  amount:
    description: "Amount in USD"
    required: false
---

# Polymarket US Trading Skill

You can trade on Polymarket US prediction markets using the `run_skill_script` tool with `polymarket.py`.

## Quick Reference

All calls follow this pattern:
```json
{
  "script": "polymarket.py",
  "action": "<action>",
  "args": { ... },
  "skill_name": "polymarket_us"
}
```

## Discovery Actions (no auth required for most)

### Search for events
```json
{ "action": "search", "args": { "query": "election" } }
```

### Search events (structured)
```json
{ "action": "search_events", "args": { "limit": 10 } }
```

### Get event details
```json
{ "action": "get_event", "args": { "slug": "presidential-election-2028" } }
```
Or by ID:
```json
{ "action": "get_event", "args": { "id": "12345" } }
```

### List markets in an event
```json
{ "action": "list_markets", "args": { "event_id": "12345", "limit": 20 } }
```

### Get market details
```json
{ "action": "get_market", "args": { "slug": "will-x-happen" } }
```

### Get market sides (Yes/No with prices)
```json
{ "action": "get_sides", "args": { "market_id": "12345" } }
```

### Get order book
```json
{ "action": "get_book", "args": { "slug": "will-x-happen" } }
```

### Get best bid/offer
```json
{ "action": "get_bbo", "args": { "slug": "will-x-happen" } }
```

## Trading Actions (requires API keys)

### Place an order
```json
{ "action": "create_order", "args": { "market_id": "12345", "side": "yes", "size": 10.0, "price": 0.55 } }
```
- `side`: "yes" or "no"
- `size`: amount in USD
- `price`: limit price (0.01–0.99)

### Cancel an order
```json
{ "action": "cancel_order", "args": { "order_id": "abc-123" } }
```

### Cancel all open orders
```json
{ "action": "cancel_all", "args": {} }
```

### List your orders
```json
{ "action": "list_orders", "args": {} }
```

### Get your positions
```json
{ "action": "get_positions", "args": {} }
```

### Get your balance
```json
{ "action": "get_balance", "args": {} }
```

## Workflow

1. **Discover**: Use `search` or `search_events` to find interesting events
2. **Research**: Use `get_event`, `list_markets`, `get_market` to understand the market
3. **Price check**: Use `get_bbo` or `get_book` to see current prices
4. **Trade**: Use `create_order` to place a bet
5. **Monitor**: Use `get_positions` and `list_orders` to track

## Important Notes

- Polymarket US is CFTC-regulated. All trades are real money.
- API keys are Ed25519 keys from https://poly.market/dev-portal
- Prices are between 0.01 and 0.99 (representing probability)
- Always confirm with the user before placing orders
