---
name: spot_trader
description: "Autonomous DeFi spot trader — scans DexScreener and Bankr signals, makes trade decisions, executes swaps on Base"
version: 1.0.0
author: starkbot
requires_tools: [local_rpc, sign_raw_tx]
---

# Spot Trader — RPC Reference

The `spot_trader` module exposes these RPC endpoints via `local_rpc`.
All endpoints are at `http://127.0.0.1:9104`.

## Decision

Submit a trading decision after evaluating the market.

```
local_rpc(url="http://127.0.0.1:9104/rpc/decision", method="POST", body={
  "decision": "BUY" | "SELL" | "HOLD",
  "token_address": "0x...",
  "token_symbol": "SYMBOL",
  "reason": "brief explanation of why"
})
```

- **BUY**: Module constructs an unsigned swap tx (WETH → token) via 0x API, stores it, and fires the `spot_trader_sign_tx` hook with tx details.
- **SELL**: Module constructs an unsigned swap tx (token → WETH) via 0x API, same flow.
- **HOLD**: No tx is constructed. Decision is logged for audit.

Response includes `decision_id` and, for BUY/SELL, the unsigned tx fields.

## Sign

After signing a transaction with `sign_raw_tx`, submit the signed hex:

```
local_rpc(url="http://127.0.0.1:9104/rpc/sign", method="POST", body={
  "tx_id": 123,
  "signed_tx": "0x..."
})
```

Module broadcasts via `eth_sendRawTransaction`, polls for receipt, and updates status. On confirmed execution, updates portfolio with cost basis and records in trade_history.

## History

Query recent trade decisions:

```
local_rpc(url="http://127.0.0.1:9104/rpc/history", method="POST", body={
  "limit": 20,
  "status": "executed"
})
```

Optional filters: `limit` (default 20), `status` ("pending", "executed", "failed", "all").

## Stats

Aggregate trading statistics:

```
local_rpc(url="http://127.0.0.1:9104/rpc/stats", method="GET")
```

Returns total decisions, buys, sells, holds, executed count, and failed count.

## P&L

Get aggregate profit & loss summary:

```
local_rpc(url="http://127.0.0.1:9104/rpc/pnl", method="GET")
```

Returns:
- `total_realized_pnl` — from closed trades
- `total_unrealized_pnl` — from open positions
- `total_pnl` — combined
- `win_count`, `loss_count`, `win_rate`
- `total_trades`
- `best_trade`, `worst_trade` — with token symbol and P&L amount

## Refresh Prices

Refresh portfolio prices from DexScreener and update unrealized P&L:

```
local_rpc(url="http://127.0.0.1:9104/rpc/refresh", method="POST")
```

Returns `eth_price_usd`, `positions_refreshed`, and updated `portfolio`.

## Trade History

View closed and open trade records with realized P&L:

```
local_rpc(url="http://127.0.0.1:9104/rpc/trade_history", method="GET")
```

Returns list of trade_history entries with token, side (BUY/SELL), value_usd, realized_pnl, tx_hash, and timestamp.

## Config

View or update trader configuration:

```
local_rpc(url="http://127.0.0.1:9104/rpc/config", method="GET")
local_rpc(url="http://127.0.0.1:9104/rpc/config", method="POST", body={
  "key": "pulse_interval",
  "value": "240"
})
```

Config keys:
- `pulse_interval` — seconds between pulse fires (default 240)
- `max_trade_usd` — max trade size in USD (default 20)
- `chain` — chain name (default "base")
- `enabled` — "true" / "false"
- `weth_address` — WETH contract address on chain
- `signal_mode` — `dexscreener` (default) or `bankr` — controls which signal source the pulse uses
- `bankr_min_confidence` — minimum confidence % for Bankr signals (default 70)
- `bankr_providers` — comma-separated provider addresses to filter Bankr signals (empty = accept all)
- `eth_price_usd` — cached ETH price in USD (auto-updated on each pulse)

## Control

Control the trading loop:

```
local_rpc(url="http://127.0.0.1:9104/rpc/control", method="POST", body={
  "action": "start" | "stop" | "trigger"
})
```

- **start**: Enable the background pulse timer.
- **stop**: Disable it.
- **trigger**: Fire a pulse immediately (ignores timer). Includes portfolio risk context.

## Portfolio

View current token holdings with P&L:

```
local_rpc(url="http://127.0.0.1:9104/rpc/portfolio", method="GET")
```

Returns list of held tokens with addresses, symbols, amounts, cost basis (`total_cost_usd`), current price, unrealized P&L, and buy count.

## Backup

Export all data:

```
local_rpc(url="http://127.0.0.1:9104/rpc/backup/export", method="POST")
```

Returns decisions, executions, config, portfolio, and trade_history.

Restore from backup:

```
local_rpc(url="http://127.0.0.1:9104/rpc/backup/restore", method="POST", body={
  "data": { ... }
})
```
