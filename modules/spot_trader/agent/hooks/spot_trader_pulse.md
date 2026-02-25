[Spot Trader Pulse — {timestamp}]

Pulse data:
```json
{data}
```

**Risk checks (from portfolio_summary in data above):**

1. If `stop_loss_alerts` is non-empty: these positions have hit -40% or worse. **SELL them immediately** — submit a SELL decision for each before considering anything else.
2. If `take_profit_alerts` is non-empty: these positions have doubled (2x). Consider taking profit — submit SELL.
3. If `can_buy` is `false`: you have 3 open positions (the max). Only SELL or HOLD — do NOT submit BUY.
4. Review `pnl` for your overall performance context.

**Trade mode:** Check `simulation_mode` in the data. In "partner" mode, trades are paper-simulated (no real broadcast). In "rogue" mode, trades are live.

**Instructions based on signal mode:**

If the data above contains `"signal_mode": "bankr"` and a `bankr_signals` array, you are in **Bankr Signals mode**:
- Evaluate each signal in the `bankr_signals` array. Each signal has: provider, action (LONG/SHORT), token, confidence %, reasoning, entry price, leverage.
- Map LONG → BUY, SHORT → SELL.
- Still verify liquidity and volume via `dexscreener` before acting — skip tokens that fail your minimums.
- If no signals are actionable, submit HOLD.

Otherwise, you are in **DexScreener mode**:
- Use the `dexscreener` tool to check trending tokens and new pairs on Base.

**For both modes:**

Evaluate against your trading strategy (defined in your agent.md). The pulse data already includes your portfolio summary and P&L — use that context for your decision.

Submit your decision via local_rpc:

```
local_rpc(url="http://127.0.0.1:9104/rpc/decision", method="POST", body={
  "decision": "BUY" | "SELL" | "HOLD",
  "token_address": "0x...",
  "token_symbol": "SYMBOL",
  "reason": "brief explanation"
})
```

If HOLD, still submit with a reason explaining why no trade.

After submitting, call `task_fully_completed` with a summary of your decision.
