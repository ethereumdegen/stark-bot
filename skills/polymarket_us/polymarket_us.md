---
name: polymarket_us
description: "Trade prediction markets on Polymarket US — search events, analyze markets, place orders, manage positions and portfolio."
version: 4.0.0
author: starkbot
homepage: https://docs.polymarket.us/
metadata: {"clawdbot":{"emoji":"🎲"}}
requires_tools: [run_skill_script]
requires_binaries: [uv]
scripts: [polymarket.py]
requires_api_keys:
  POLYMARKET_KEY_ID:
    description: "Polymarket US API key ID — get one at https://polymarket.us/developer"
    secret: false
  POLYMARKET_SECRET_KEY:
    description: "Polymarket US API secret key (Ed25519) — get one at https://polymarket.us/developer"
    secret: true
tags: [polymarket, prediction-markets, trading, betting, finance, sports]
arguments:
  action:
    description: "Action to perform (search, get_market, create_order, get_positions, etc.)"
    required: false
  query:
    description: "Search query or market slug"
    required: false
  amount:
    description: "Amount in USD or number of contracts"
    required: false
---

# Polymarket US Trading Skill

Trade CFTC-regulated prediction markets on Polymarket US via the `run_skill_script` tool with `polymarket.py`.

## Call Pattern

```json
{"script": "polymarket.py", "action": "<action>", "args": { ... }, "skill_name": "polymarket_us"}
```

---

## Discovery (no auth required)

### search — Full-text search
```json
{"action": "search", "args": {"query": "bitcoin", "limit": 10}}
```
Optional: `page`, `series_ids`, `market_type`, `status`

### search_events — Browse events
```json
{"action": "search_events", "args": {"limit": 10, "active": true}}
```
Optional: `offset`, `closed`, `archived`, `featured`, `live`, `ended`, `categories` (list of slugs), `series_id`

### get_event — Event details
```json
{"action": "get_event", "args": {"slug": "super-bowl-2026"}}
```
Or by ID: `{"action": "get_event", "args": {"id": 12345}}`

### list_markets — Browse markets
```json
{"action": "list_markets", "args": {"active": true, "limit": 20}}
```
Optional: `offset`, `closed`, `archived`, `categories`, `sports_market_types` (MONEYLINE, SPREAD, TOTAL, PROP), `volume_num_min`, `liquidity_num_min`

### get_market — Market details
```json
{"action": "get_market", "args": {"slug": "will-x-happen"}}
```
Or by ID: `{"action": "get_market", "args": {"id": 12345}}`

### get_bbo — Best bid/offer (top of book)
```json
{"action": "get_bbo", "args": {"slug": "will-x-happen"}}
```
Returns: best bid, best ask, last trade price, bid/ask depth, open interest.

### get_book — Full order book
```json
{"action": "get_book", "args": {"slug": "will-x-happen"}}
```
Returns all bid and offer levels with depth.

### get_sides — Market sides with prices
```json
{"action": "get_sides", "args": {"slug": "will-x-happen"}}
```

### settlement — Resolved market result
```json
{"action": "settlement", "args": {"slug": "will-x-happen"}}
```
Returns settlement price: 0.00 (No) or 1.00 (Yes).

### list_series — Event series
```json
{"action": "list_series", "args": {"limit": 10}}
```

### get_series — Series details
```json
{"action": "get_series", "args": {"id": 5}}
```

### list_sports — Sports categories
```json
{"action": "list_sports", "args": {}}
```

### sports_teams — Teams for a sport
```json
{"action": "sports_teams", "args": {"provider": "sportradar"}}
```

---

## Trading (requires API keys)

### preview_order — Dry run (no execution)
```json
{"action": "preview_order", "args": {"market_slug": "will-x-happen", "intent": "buy_yes", "quantity": 10, "price": 0.55}}
```
Same params as `create_order` — previews cost/fills without submitting.

### create_order — Place an order
```json
{"action": "create_order", "args": {"market_slug": "will-x-happen", "intent": "buy_yes", "quantity": 10, "price": 0.55}}
```

**Intent shortcuts:** `buy_yes`, `sell_yes`, `buy_no`, `sell_no`
Full intents: `ORDER_INTENT_BUY_LONG`, `ORDER_INTENT_SELL_LONG`, `ORDER_INTENT_BUY_SHORT`, `ORDER_INTENT_SELL_SHORT`

**Order types:** `limit` (default), `market`

**Time in force:** `GOOD_TILL_CANCEL` (default), `GOOD_TILL_DATE`, `IMMEDIATE_OR_CANCEL`, `FILL_OR_KILL`

**Price:** 0.01–0.99 (represents probability). Required for limit orders.

### modify_order — Change an open order
```json
{"action": "modify_order", "args": {"order_id": "abc-123", "price": 0.60, "quantity": 15}}
```

### cancel_order — Cancel one order
```json
{"action": "cancel_order", "args": {"order_id": "abc-123"}}
```

### cancel_all — Cancel all open orders
```json
{"action": "cancel_all", "args": {}}
```

### close_position — Exit a full position at market price
```json
{"action": "close_position", "args": {"market_slug": "will-x-happen"}}
```
Optional: `current_price` (for slippage calc), `ticks` (slippage tolerance)

### get_order — Get specific order details
```json
{"action": "get_order", "args": {"order_id": "abc-123"}}
```

### list_orders — Your open orders
```json
{"action": "list_orders", "args": {}}
```

---

## Portfolio (requires API keys)

### get_positions — Your positions with P&L
```json
{"action": "get_positions", "args": {}}
```
Returns per position: netPosition, qtyBought, qtySold, cost, realized P&L, cashValue, qtyAvailable.

### get_activities — Trading history
```json
{"action": "get_activities", "args": {"limit": 20}}
```
Optional: `types` (list: `ACTIVITY_TYPE_TRADE`, `ACTIVITY_TYPE_POSITION_RESOLUTION`, `ACTIVITY_TYPE_ACCOUNT_DEPOSIT`, `ACTIVITY_TYPE_ACCOUNT_WITHDRAWAL`), `market_slug`, `sort_order` (`SORT_ORDER_DESCENDING` or `SORT_ORDER_ASCENDING`)

### get_balance — Account balances and buying power
```json
{"action": "get_balance", "args": {}}
```
Returns: currentBalance, buyingPower, assetNotional, openOrders, unsettledFunds, marginRequirement.

---

## Utility

### status — Health check
```json
{"action": "status", "args": {}}
```
Checks API connectivity and key configuration.

---

## Workflow

1. **Discover** — `search` or `search_events` to find interesting events
2. **Research** — `get_event` → `list_markets` → `get_market` to understand options
3. **Analyze** — `get_bbo` or `get_book` to check current prices and liquidity
4. **Preview** — `preview_order` to see cost before committing
5. **Trade** — `create_order` to place the bet
6. **Monitor** — `get_positions`, `list_orders`, `get_balance` to track
7. **Exit** — `close_position` to exit, or `cancel_order`/`cancel_all` to pull orders

## Order States

PENDING_NEW → PARTIALLY_FILLED → FILLED (complete)
                                → CANCELED (user cancelled)
                                → REJECTED (exchange refused)
                                → EXPIRED (time expired)

## Important Notes

- Polymarket US is CFTC-regulated. All trades are real money.
- API keys are Ed25519 keys from https://polymarket.us/developer
- Prices are 0.01–0.99 representing the probability of the outcome
- Always preview and confirm with the user before placing orders
- Use `close_position` for quick exits at market price
