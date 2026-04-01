---
name: defi-risk-guardian
description: >
  Autonomous DeFi position monitoring, risk scoring, and protective action execution
  across Starknet protocols (Nostra, zkLend, Ekubo, Vesu, Opus, STRK Staking).
  Use when the user wants to: protect open lending/borrowing positions from liquidation,
  monitor health factors in real time, auto-rebalance collateral, receive risk alerts,
  generate portfolio risk reports, or set up autonomous guardian workflows that act
  without manual intervention. Also triggers for phrases like "watch my position",
  "protect my collateral", "am I close to liquidation", "set a guardian", or
  "auto-rebalance if health factor drops". Composable: outputs standardized RiskReport
  JSON that other skills (yield-optimizer, portfolio-rebalancer, alert-dispatcher) can
  consume directly.
license: MIT
metadata:
  author: starkbot-community
  version: "1.0.0"
  tracks:
    - defi-integration
    - autonomous-operation
  protocols:
    - nostra
    - zklend
    - ekubo
    - vesu
    - opus
    - strk-staking
  chain: starknet
  requires_wallet: true
  outputs: RiskReport
---

# DeFi Risk Guardian

An autonomous skill that watches over open DeFi positions 24/7, scores their
health in real time, and executes protective actions before losses occur.
Think of it as hiring a full-time risk desk — except it never sleeps, never
misses an alert, and costs fractions of a cent per check via x402 micropayments.

---

## Overview

DeFi liquidations are one of the most avoidable losses in crypto. A position
worth $50 000 can be liquidated because the user was asleep when the health
factor crossed 1.05. This skill solves that permanently by turning StarkBot
into an always-on guardian that:

1. **Monitors** health factors, LTV ratios, and liquidation prices across every
   supported lending protocol on Starknet.
2. **Scores** overall portfolio risk using a composable `RiskScore` (0–100).
3. **Alerts** the user at configurable thresholds via the notification channel
   they prefer.
4. **Acts autonomously** — adds collateral, repays debt, or withdraws yield to
   restore a safe health factor — when the user grants execution permissions.
5. **Reports** every action in an immutable on-chain audit log so nothing is
   ever a black box.

---

## Supported Protocols

| Protocol     | Type              | Risk Metric Monitored          |
|-------------|-------------------|-------------------------------|
| Nostra       | Lending/Borrowing | Health Factor, LTV, Liquidation Price |
| zkLend       | Lending/Borrowing | Health Factor, LTV, Liquidation Price |
| Vesu         | CDP / Lending     | Collateral Ratio, Debt Ceiling |
| Opus         | Stablecoin CDP    | Collateral Ratio, Forge Cap    |
| Ekubo        | AMM LP            | Price Range, IL Exposure       |
| STRK Staking | Staking           | Slash Risk, Validator Health   |

---

## Configuration

Before running, set guardian parameters. These are stored in the agent's
persistent config store and can be updated at any time.

```yaml
# guardian_config.yaml (example)
guardian:
  poll_interval_seconds: 60          # How often to fetch on-chain state
  alert_threshold_health_factor: 1.4 # Alert when HF drops below this
  action_threshold_health_factor: 1.2 # Act autonomously below this
  max_gas_per_action_gwei: 50        # Spend cap per protective tx
  collateral_top_up_pct: 20          # Add this % of current collateral when acting
  emergency_full_exit: false         # If true, full exit when HF < 1.05
  notification_channel: telegram     # telegram | webhook | onchain-log
  webhook_url: ""                    # If channel is webhook
  autonomous_mode: true              # Set false to alert-only (no auto-tx)
  wallet: "0xYOUR_WALLET"
  protocols:
    - nostra
    - zklend
    - ekubo
```

---

## Workflow

### Step 1 — Discover Positions

The guardian first discovers all open positions associated with the configured
wallet across all supported protocols.

```
discover_positions(wallet_address) → PositionList
```

Each `Position` contains:
- `protocol`: string
- `position_type`: lending | borrowing | lp | staking
- `collateral_usd`: float
- `debt_usd`: float
- `health_factor`: float | null  (null for LP/staking)
- `liquidation_price`: float | null
- `current_price`: float
- `lltv`: float  (liquidation LTV, protocol-specific)

### Step 2 — Score Risk

Run each position through the risk scoring engine:

```
score_position(position) → RiskScore {
  score: 0–100,          # 0 = safe, 100 = immediately liquidatable
  category: safe | watch | warning | critical | emergency,
  distance_to_liq_pct: float,
  time_to_liq_estimate: str,  # e.g. "~6h at current volatility"
  recommended_actions: Action[]
}
```

**Scoring formula:**

```
base_score = (1 / health_factor) * 50          # Inverted HF, max 50
vol_penalty = 30-day_realized_vol * 20         # Up to +20 for volatile collateral
concentration_penalty = (position_size / portfolio_total) * 10  # Up to +10
risk_score = min(100, base_score + vol_penalty + concentration_penalty)
```

| Score | Category  | Default Behavior                          |
|-------|-----------|-------------------------------------------|
| 0–30  | Safe      | Log only                                  |
| 31–55 | Watch     | Alert user                                |
| 56–74 | Warning   | Alert + suggest actions                   |
| 75–89 | Critical  | Alert + auto-act if autonomous_mode=true  |
| 90+   | Emergency | Auto-act regardless of autonomous_mode    |

### Step 3 — Generate RiskReport

After scoring all positions, the guardian emits a standardized `RiskReport`
that other skills can consume:

```json
{
  "report_id": "rg-20240219-a3f2",
  "timestamp": "2024-02-19T14:32:00Z",
  "wallet": "0xABC...",
  "portfolio_risk_score": 42,
  "positions": [
    {
      "protocol": "nostra",
      "collateral_asset": "WBTC",
      "collateral_usd": 25000,
      "debt_asset": "USDC",
      "debt_usd": 15000,
      "health_factor": 1.67,
      "liquidation_price_btc": 38200,
      "current_price_btc": 51500,
      "distance_to_liq_pct": 25.8,
      "risk_score": 38,
      "category": "watch"
    }
  ],
  "actions_taken": [],
  "next_poll": "2024-02-19T14:33:00Z"
}
```

### Step 4 — Execute Protective Actions

When a position enters **Critical** or **Emergency**, the guardian selects and
executes the optimal protective action:

```
select_action(position, risk_score, config) → Action {
  type: add_collateral | repay_debt | partial_exit | full_exit,
  amount_usd: float,
  expected_new_hf: float,
  estimated_gas_gwei: float,
  protocol_calldata: hex
}
```

**Action priority order:**
1. Repay debt with available stablecoins (cheapest gas, most effective)
2. Add collateral from the wallet's idle balance
3. Withdraw from yield positions and convert (if 1 and 2 not possible)
4. Partial exit (sell collateral portion to repay debt)
5. Full exit (emergency only — closes the entire position)

Every executed action is written to the on-chain audit log using a minimal
Cairo contract call so it's permanently verifiable.

### Step 5 — Loop

The guardian runs continuously at the configured `poll_interval_seconds`. In
x402 micropayment mode, each poll costs a fraction of a cent (charged to the
agent's wallet), enabling indefinite autonomous operation without manual
intervention.

---

## Usage Examples

### Example 1 — Quick Risk Check (manual, one-shot)

```
User: check my defi risk
```

StarkBot runs a single scan, returns a formatted RiskReport summary:

```
🛡️ DeFi Risk Guardian — Snapshot Report
Portfolio Risk Score: 42/100 (Watch)

📍 Nostra — WBTC/USDC
   Health Factor: 1.67  ✅ Safe
   Liq. Price: $38,200 (BTC @ $51,500 — 25.8% buffer)
   Risk Score: 38/100

📍 zkLend — ETH/USDT
   Health Factor: 1.29  ⚠️ Warning
   Liq. Price: $2,810 (ETH @ $3,250 — 13.5% buffer)
   Risk Score: 61/100 ← Consider repaying ~$800 USDT to reach HF 1.5

No autonomous actions taken (autonomous_mode=false).
Run `enable guardian autonomous` to allow auto-protective actions.
```

---

### Example 2 — Enable Autonomous Guardian

```
User: enable guardian autonomous mode, alert me on telegram if HF drops below 1.4, act if below 1.2
```

StarkBot writes config, starts the background polling loop, and confirms:

```
✅ Guardian activated (autonomous mode ON)
   Monitoring: nostra, zklend, ekubo
   Alert at: HF < 1.4 → Telegram
   Act at:   HF < 1.2 → auto repay/add collateral (max 50 GWEI gas)
   Poll: every 60s via x402 micropayment

Your positions are now protected. You'll only hear from me if something needs attention.
```

---

### Example 3 — Emergency Response (autonomous)

The ETH price drops 18% in 4 hours. The guardian detects the zkLend position
entering Critical (HF = 1.08, Risk Score = 88).

**Without user intervention, the guardian:**
1. Fetches available USDT balance: $1,200
2. Calculates optimal repayment: $950 USDT restores HF to 1.55
3. Executes the repayment transaction on zkLend
4. Writes action to on-chain audit log
5. Sends Telegram alert:

```
🚨 Guardian Action Taken — zkLend ETH/USDT

ETH dropped to $2,665 → Health Factor fell to 1.08 (CRITICAL)

Action: Repaid $950 USDT
New Health Factor: 1.55 ✅
Gas Used: 0.0003 ETH ($0.87)
Tx: 0x7f2a...c839

Your position is safe. ETH liquidation price is now $2,215 (16.8% buffer).
```

---

### Example 4 — Composing with Other Skills

The `RiskReport` output is designed to be consumed by other skills:

```
# yield-optimizer skill reads RiskReport to avoid
# recommending higher-yield (riskier) positions when portfolio risk > 60
risk = guardian.get_latest_report()
if risk.portfolio_risk_score > 60:
    yield_optimizer.set_max_risk_tier("conservative")

# alert-dispatcher skill routes by category
for position in risk.positions:
    if position.category in ["critical", "emergency"]:
        alert_dispatcher.send(channel="pagerduty", payload=position)
```

---

## Error Handling

| Error | Detection | Recovery |
|-------|-----------|----------|
| RPC node timeout | 3 retries with exponential backoff | Fall back to secondary RPC |
| Price feed stale (>5 min) | Check oracle timestamp | Use secondary oracle or skip action, alert user |
| Gas spike (>max_gas_per_action) | Pre-check before tx | Delay action 60s, retry 3x, alert user if still spiked |
| Insufficient wallet balance | Check before action | Alert user, skip action, increase alert urgency |
| Protocol paused/emergency | Check protocol status flag | Skip actions, alert user, suggest manual intervention |
| Action reverts on-chain | Catch revert reason | Log revert, escalate alert, retry with lower amount |
| x402 micropayment failure | Check payment receipt | Retry once, then pause guardian and alert user |

---

## Bundled Scripts

| Script | Purpose |
|--------|---------|
| `scripts/scanner.py` | Fetch positions from all supported protocols via their ABIs |
| `scripts/risk_engine.py` | Score positions and build RiskReport JSON |
| `scripts/executor.py` | Simulate, sign, and broadcast protective transactions |
| `scripts/guardian_loop.py` | Orchestrator — runs the full poll → score → act loop |
| `scripts/audit_log.py` | Write actions to on-chain audit log contract |

Run the guardian manually:
```bash
python scripts/guardian_loop.py \
  --wallet 0xYOUR_WALLET \
  --config guardian_config.yaml \
  --once   # omit for continuous loop
```

---

## Composability Contract

Any skill that wants to consume `RiskReport` data can do so by calling:

```
guardian.get_latest_report(wallet) → RiskReport
guardian.get_position_risk(wallet, protocol) → RiskScore
guardian.subscribe(wallet, callback_skill, min_category="warning") → subscription_id
```

The guardian also emits a lightweight event stream that other skills can subscribe
to without polling, enabling event-driven multi-skill coordination.

---

## Security Notes

- The guardian **never** holds private keys. It signs via the agent's existing
  wallet session.
- Autonomous actions are bounded by `max_gas_per_action_gwei` and
  `collateral_top_up_pct` caps — it cannot drain the wallet.
- Setting `autonomous_mode: false` makes the guardian **alert-only** — it will
  never submit transactions without explicit user approval.
- All actions are pre-simulated before broadcasting. If simulation fails, the
  action is aborted and the user is alerted.
- The on-chain audit log contract is read-only after write — actions cannot be
  modified or deleted.
