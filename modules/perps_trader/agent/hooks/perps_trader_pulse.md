[Perps Trader Pulse — {timestamp}]

Pulse data:
```json
{data}
```

**Risk checks (from position_summary in data above):**

1. If `usdc_status.approved` is `false`: approve USDC first via `local_rpc(url="http://127.0.0.1:9105/rpc/approve_usdc", method="POST")`. Then call `task_fully_completed` — the sign hook will handle the approval tx.
2. If `liquidation_alerts` is non-empty: positions within 5% of liquidation. **Close them immediately** — submit a CLOSE decision for each.
3. If `stop_loss_alerts` is non-empty: positions at or past -25%. **Close immediately** — submit CLOSE for each before considering anything else.
4. If `take_profit_alerts` is non-empty: positions at or past +50%. **Consider closing** to lock in gains.
5. Review `pnl` for your overall performance context.

**Signal evaluation (only after risk checks):**

If `can_open` is `true` and `bankr_signals` is non-empty:
- Evaluate each signal. Each has: provider, action (LONG/SHORT), token, confidence %, reasoning, entry price, leverage.
- Map LONG → OPEN_LONG, SHORT → OPEN_SHORT.
- Only act on signals for allowed pairs: ETH/USD, BTC/USD, SOL/USD, DOGE/USD.
- Respect leverage limits: max 25x, default 10x if not specified.
- If no signals are actionable, submit HOLD.

If `can_open` is `false`: you have max positions open. Only CLOSE or HOLD.

**Submit your decision via local_rpc:**

```
local_rpc(url="http://127.0.0.1:9105/rpc/decision", method="POST", body={
  "decision": "OPEN_LONG" | "OPEN_SHORT" | "CLOSE" | "HOLD",
  "pair_name": "ETH/USD",
  "leverage": 10,
  "collateral": 10.0,
  "reason": "brief explanation"
})
```

For CLOSE, include `pair_index` and `trade_index` from the position data.

If HOLD, still submit with a reason explaining why no trade.

After submitting, call `task_fully_completed` with a summary of your decision.
