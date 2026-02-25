[HyperClaw Pulse — {timestamp}]

Pulse data:
```json
{data}
```

**Setup checks (from account_status in data above):**

1. If `account_status.registered` is `false`: register with Orderly first via `local_rpc(url="http://127.0.0.1:9108/rpc/register", method="POST")`. Then call `task_fully_completed` — the sign hook will handle the EIP-712 signature.
2. If `account_status.key_added` is `false`: add trading key via `local_rpc(url="http://127.0.0.1:9108/rpc/add_key", method="POST")`. Then call `task_fully_completed` — the sign hook will handle the EIP-712 signature.
3. If `balances` is empty or has no USDC: deposit via `local_rpc(url="http://127.0.0.1:9108/rpc/deposit", method="POST", body={"amount": 50})`. Then call `task_fully_completed`.

**Risk checks (from position_summary in data above):**

1. If `liquidation_alerts` is non-empty: positions within 5% of liquidation. **Close them immediately** — submit a CLOSE decision for each.
2. If `stop_loss_alerts` is non-empty: positions at or past -25%. **Close immediately** — submit CLOSE for each before considering anything else.
3. If `take_profit_alerts` is non-empty: positions at or past +50%. **Consider closing** to lock in gains.
4. Review `pnl` for your overall performance context.

**Signal evaluation (only after setup and risk checks):**

If `can_open` is `true` and `bankr_signals` is non-empty:
- Evaluate each signal. Each has: provider, action (LONG/SHORT), token, confidence %, reasoning, entry price, leverage.
- Map LONG → OPEN_LONG, SHORT → OPEN_SHORT.
- Map token to Orderly symbol: ETH → PERP_ETH_USDC, BTC → PERP_BTC_USDC, SOL → PERP_SOL_USDC.
- Respect leverage limits: max 20x, default 5x if not specified.
- If no signals are actionable, submit HOLD.

If `can_open` is `false`: you have max positions open. Only CLOSE or HOLD.

**Submit your decision via local_rpc:**

```
local_rpc(url="http://127.0.0.1:9108/rpc/decision", method="POST", body={
  "decision": "OPEN_LONG" | "OPEN_SHORT" | "CLOSE" | "HOLD",
  "symbol": "PERP_ETH_USDC",
  "leverage": 5,
  "collateral": 10.0,
  "reason": "brief explanation"
})
```

For CLOSE, include `symbol` from the position data.

If HOLD, still submit with a reason explaining why no trade.

After submitting, call `task_fully_completed` with a summary of your decision.
