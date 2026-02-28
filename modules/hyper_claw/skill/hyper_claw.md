---
name: hyper_claw
description: "Autonomous perpetual futures trader on HyperClaw (Orderly Network) — leveraged LONG/SHORT positions from Bankr signals"
version: 1.1.0
author: starkbot
requires_tools: [local_rpc, sign_raw_tx, sign_typed_data]
---

# HyperClaw — RPC Reference

The `hyper_claw` module exposes these RPC endpoints via `local_rpc`.

**After reading these instructions, call `local_rpc` directly to fulfill the user's request. Do NOT call `use_skill` again.**

Use `module="hyper_claw"` — the port is resolved automatically.

## Setup Flow

Before trading, complete these one-time setup steps:

### 1. Register with Orderly

```
local_rpc(module="hyper_claw", path="/rpc/register", method="POST")
```

Returns EIP-712 data and fires `hyper_claw_sign_tx` hook. After signing, the module registers with Orderly Network using broker_id `hyper_claw`.

### 2. Add Trading Key

```
local_rpc(module="hyper_claw", path="/rpc/add_key", method="POST")
```

Generates an ed25519 keypair, returns EIP-712 data to sign. After signing, the key is registered with Orderly for API authentication.

### 3. Deposit USDC

```
local_rpc(module="hyper_claw", path="/rpc/deposit", method="POST", body={
  "amount": 50
})
```

Constructs USDC approval + vault deposit transactions. Fires `hyper_claw_sign_tx` for each.

## Decision

Submit a trading decision after evaluating market signals and risk.

```
local_rpc(module="hyper_claw", path="/rpc/decision", method="POST", body={
  "decision": "OPEN_LONG",
  "symbol": "PERP_ETH_USDC",
  "leverage": 5,
  "collateral": 10.0,
  "reason": "brief explanation of why"
})
```

- **OPEN_LONG**: Places a BUY market order on Orderly. Quantity is calculated from collateral * leverage / price.
- **OPEN_SHORT**: Places a SELL market order on Orderly.
- **CLOSE**: Places a reduce-only order to close the position for the given symbol.
- **HOLD**: No order is placed. Decision is logged for audit.

Response includes `decision_id` and, for OPEN_LONG/OPEN_SHORT/CLOSE, the `order_id`.

## Sign

After signing EIP-712 data or on-chain transactions, submit:

For EIP-712 (register, add_key):
```
local_rpc(module="hyper_claw", path="/rpc/sign", method="POST", body={
  "tx_id": 123,
  "signature": "0x...",
  "eip712_data": {...}
})
```

For on-chain tx (deposit, approve_usdc):
```
local_rpc(module="hyper_claw", path="/rpc/sign", method="POST", body={
  "tx_id": 123,
  "signed_tx": "0x..."
})
```

## Positions

View open positions with unrealized P&L:

```
local_rpc(module="hyper_claw", path="/rpc/positions", method="GET")
```

Returns list of positions with symbol, side, qty, entry/mark price, liquidation price, unrealized P&L.

## Orders

View open orders on Orderly:

```
local_rpc(module="hyper_claw", path="/rpc/orders", method="GET")
```

## Account

View Orderly account info and balances:

```
local_rpc(module="hyper_claw", path="/rpc/account", method="GET")
```

## Refresh

Sync positions from Orderly and update P&L:

```
local_rpc(module="hyper_claw", path="/rpc/refresh", method="POST")
```

## History

Query recent trade decisions:

```
local_rpc(module="hyper_claw", path="/rpc/history", method="POST", body={
  "limit": 20,
  "status": "executed"
})
```

Optional filters: `limit` (default 20), `status` ("pending", "executed", "failed", "all").

## Stats

Aggregate trading statistics:

```
local_rpc(module="hyper_claw", path="/rpc/stats", method="GET")
```

## P&L

Get aggregate profit & loss summary:

```
local_rpc(module="hyper_claw", path="/rpc/pnl", method="GET")
```

Returns:
- `total_realized_pnl` — from closed trades
- `total_unrealized_pnl` — from open positions
- `total_pnl` — combined
- `win_count`, `loss_count`, `win_rate`
- `total_trades`
- `best_trade`, `worst_trade` — with symbol and P&L amount

## Trade History

View closed trade records with realized P&L:

```
local_rpc(module="hyper_claw", path="/rpc/trade_history", method="GET")
```

## Config

View or update trader configuration:

```
local_rpc(module="hyper_claw", path="/rpc/config", method="GET")

local_rpc(module="hyper_claw", path="/rpc/config", method="POST", body={
  "key": "max_collateral_per_trade",
  "value": "15"
})
```

Config keys:
- `pulse_interval` — seconds between pulse fires (default 300)
- `enabled` — "true" / "false"
- `trader_address` — wallet address (must be set before trading)
- `max_collateral_per_trade` — max USDC per trade (default 10)
- `max_leverage` — maximum leverage (default 20)
- `default_leverage` — default leverage when not specified (default 5)
- `max_positions` — max concurrent positions (default 3)
- `take_profit_pct` — default TP % from entry (default 50)
- `stop_loss_pct` — default SL % from entry (default 25)
- `bankr_min_confidence` — minimum confidence % for Bankr signals (default 70)
- `bankr_providers` — comma-separated provider addresses to filter
- `allowed_symbols` — comma-separated allowed symbols (default PERP_ETH_USDC,PERP_BTC_USDC,PERP_SOL_USDC)
- `chain_id` — chain ID for registration/deposits (default 8453 = Base)

## Control

Control the trading loop:

```
local_rpc(module="hyper_claw", path="/rpc/control", method="POST", body={
  "action": "start"
})
```

- **start**: Enable the background pulse timer.
- **stop**: Disable it.
- **trigger**: Fire a pulse immediately.

## Symbols

View available perpetual symbols:

```
local_rpc(module="hyper_claw", path="/rpc/symbols", method="GET")
```

## Backup

Export all data:

```
local_rpc(module="hyper_claw", path="/rpc/backup/export", method="POST")
```

Restore from backup:

```
local_rpc(module="hyper_claw", path="/rpc/backup/restore", method="POST", body={
  "data": { ... }
})
```
