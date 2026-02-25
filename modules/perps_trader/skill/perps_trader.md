---
name: perps_trader
description: "Autonomous perpetual futures trader on Avantis (Base) — leveraged LONG/SHORT positions from Bankr signals"
version: 1.0.0
author: starkbot
requires_tools: [local_rpc, sign_raw_tx]
---

# Perps Trader — RPC Reference

The `perps_trader` module exposes these RPC endpoints via `local_rpc`.
All endpoints are at `http://127.0.0.1:9105`.

## Decision

Submit a trading decision after evaluating market signals and risk.

```
local_rpc(url="http://127.0.0.1:9105/rpc/decision", method="POST", body={
  "decision": "OPEN_LONG" | "OPEN_SHORT" | "CLOSE" | "HOLD",
  "pair_name": "ETH/USD",
  "leverage": 10,
  "collateral": 10.0,
  "take_profit_pct": 50,
  "stop_loss_pct": 25,
  "reason": "brief explanation of why",
  "pair_index": 0,
  "trade_index": 0
})
```

- **OPEN_LONG**: Opens a long perpetual position. Module constructs an unsigned tx via Avantis SDK, stores it, and fires the `perps_trader_sign_tx` hook.
- **OPEN_SHORT**: Opens a short perpetual position. Same flow as OPEN_LONG.
- **CLOSE**: Closes an existing position. Requires `pair_index` and `trade_index` (from positions table). Module constructs close tx and fires sign hook.
- **HOLD**: No tx is constructed. Decision is logged for audit.

Response includes `decision_id` and, for OPEN_LONG/OPEN_SHORT/CLOSE, the unsigned tx fields.

## Sign

After signing a transaction with `sign_raw_tx`, submit the signed hex:

```
local_rpc(url="http://127.0.0.1:9105/rpc/sign", method="POST", body={
  "tx_id": 123,
  "signed_tx": "0x..."
})
```

Module broadcasts via `eth_sendRawTransaction`, polls for receipt, and updates status. On confirmed execution:
- **OPEN**: adds position to `positions` table with entry price, leverage, TP/SL
- **CLOSE**: calculates realized P&L, moves to `trade_history`, removes from `positions`

## Positions

View open positions with unrealized P&L:

```
local_rpc(url="http://127.0.0.1:9105/rpc/positions", method="GET")
```

Returns list of open positions with pair, direction, collateral, leverage, entry/current price, liquidation price, unrealized P&L.

## Sync Positions

Re-sync positions from on-chain via Avantis `get_trades()`:

```
local_rpc(url="http://127.0.0.1:9105/rpc/sync_positions", method="POST")
```

Catches liquidations, auto-TP/SL closures, and updates the positions table.

## History

Query recent trade decisions:

```
local_rpc(url="http://127.0.0.1:9105/rpc/history", method="POST", body={
  "limit": 20,
  "status": "executed"
})
```

Optional filters: `limit` (default 20), `status` ("pending", "executed", "failed", "all").

## Stats

Aggregate trading statistics:

```
local_rpc(url="http://127.0.0.1:9105/rpc/stats", method="GET")
```

Returns total decisions, opens, closes, holds, executed count, and failed count.

## P&L

Get aggregate profit & loss summary:

```
local_rpc(url="http://127.0.0.1:9105/rpc/pnl", method="GET")
```

Returns:
- `total_realized_pnl` — from closed trades
- `total_unrealized_pnl` — from open positions
- `total_pnl` — combined
- `win_count`, `loss_count`, `win_rate`
- `total_trades`
- `best_trade`, `worst_trade` — with pair and P&L amount

## Refresh Prices

Refresh prices via Avantis SDK and update unrealized P&L:

```
local_rpc(url="http://127.0.0.1:9105/rpc/refresh", method="POST")
```

Returns `positions_refreshed` and updated position list.

## Trade History

View closed trade records with realized P&L:

```
local_rpc(url="http://127.0.0.1:9105/rpc/trade_history", method="GET")
```

Returns list of trade_history entries with pair, direction, collateral, leverage, entry/exit price, realized P&L.

## Config

View or update trader configuration:

```
local_rpc(url="http://127.0.0.1:9105/rpc/config", method="GET")
local_rpc(url="http://127.0.0.1:9105/rpc/config", method="POST", body={
  "key": "max_collateral_per_trade",
  "value": "15"
})
```

Config keys:
- `pulse_interval` — seconds between pulse fires (default 300)
- `enabled` — "true" / "false"
- `trader_address` — wallet address (must be set before trading)
- `max_collateral_per_trade` — max USDC per trade (default 10)
- `max_leverage` — maximum leverage (default 25)
- `default_leverage` — default leverage when not specified (default 10)
- `max_positions` — max concurrent positions (default 3)
- `take_profit_pct` — default TP % from entry (default 50)
- `stop_loss_pct` — default SL % from entry (default 25)
- `bankr_min_confidence` — minimum confidence % for Bankr signals (default 70)
- `bankr_providers` — comma-separated provider addresses to filter
- `usdc_approved` — whether USDC is approved for trading (auto-updated)
- `allowed_pairs` — comma-separated allowed pairs (default ETH/USD,BTC/USD,SOL/USD,DOGE/USD)

## Control

Control the trading loop:

```
local_rpc(url="http://127.0.0.1:9105/rpc/control", method="POST", body={
  "action": "start" | "stop" | "trigger"
})
```

- **start**: Enable the background pulse timer.
- **stop**: Disable it.
- **trigger**: Fire a pulse immediately (ignores timer). Includes position risk context.

## Approve USDC

Construct a USDC approval transaction for the Avantis TradingStorage contract:

```
local_rpc(url="http://127.0.0.1:9105/rpc/approve_usdc", method="POST")
```

Constructs unsigned approval tx and fires `perps_trader_sign_tx` hook. Must be signed and broadcast before opening trades.

## Pairs

View available trading pairs with indices:

```
local_rpc(url="http://127.0.0.1:9105/rpc/pairs", method="GET")
```

Returns list of pairs with name and index from Avantis SDK.

## Backup

Export all data:

```
local_rpc(url="http://127.0.0.1:9105/rpc/backup/export", method="POST")
```

Returns decisions, executions, config, positions, and trade_history.

Restore from backup:

```
local_rpc(url="http://127.0.0.1:9105/rpc/backup/restore", method="POST", body={
  "data": { ... }
})
```
