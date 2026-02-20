---
name: pendle
description: "Trade yield on Pendle Finance (Base) — browse markets, buy PT for fixed yield, buy YT for leveraged yield, provide LP, check positions."
version: 1.0.0
author: starkbot
homepage: https://pendle.finance
metadata: {"requires_auth": false, "clawdbot":{"emoji":"🔮"}}
requires_tools: [web_fetch, token_lookup, to_raw_amount, web3_preset_function_call, list_queued_web3_tx, broadcast_web3_tx, verify_tx_broadcast, select_web3_network, define_tasks]
tags: [crypto, defi, finance, yield, pendle, base, fixed-income, pt, yt]
---

# Pendle Finance — Yield Trading on Base

Trade yield on Pendle. Buy PT for fixed yield, buy YT for leveraged yield exposure, provide LP, and check positions.

## CRITICAL RULES

1. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task. Do NOT work ahead.
2. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.**
3. **Sequential tool calls only.** Never call two tools in parallel when the second depends on the first.

## Key Info

| Item | Value |
|------|-------|
| Chain | Base (8453) |
| Router | `0x888888888889758F76e7103c6CbF23ABbF58F946` |
| API Base | `https://api-v2.pendle.finance/core` |

## Pendle Concepts

- **PT (Principal Token)**: Buy at a discount, redeems 1:1 for underlying at maturity. Discount = fixed APY.
- **YT (Yield Token)**: Earns all variable yield on the notional value until maturity. Leveraged yield exposure (10-50x).
- **LP**: Provide liquidity to Pendle AMM (PT + SY pool). Earn swap fees + PENDLE incentives + underlying yield.
- **SY (Standardized Yield)**: Wrapper around yield-bearing assets.

---

## Operation A: List Active Markets on Base

```tool:web_fetch
url: https://api-v2.pendle.finance/core/v1/markets/all?chainId=8453&isActive=true
method: GET
extract_mode: raw
```

From the response, present a table with:
- Market name / underlying asset
- Market address
- Implied APY (fixed yield if you buy PT)
- Underlying APY (variable yield)
- TVL
- Expiry date

---

## Operation B: Get Market Details

For a specific market (replace `MARKET_ADDRESS`):

```tool:web_fetch
url: https://api-v2.pendle.finance/core/v2/8453/markets/MARKET_ADDRESS/data
method: GET
extract_mode: raw
```

Report: implied APY, underlying APY, TVL, PT/YT prices, liquidity, and expiry.

---

## Operation C: Check User Positions

```tool:web_fetch
url: https://api-v2.pendle.finance/core/v1/dashboard/positions/database/WALLET_ADDRESS
method: GET
extract_mode: raw
```

Replace `WALLET_ADDRESS` with the user's wallet address. Present:
- PT positions (fixed yield locked in)
- YT positions (variable yield exposure)
- LP positions (liquidity provided)
- USD values and unrealized PnL

---

## Operation D: Buy PT (Fixed Yield)

Buying PT locks in a fixed APY. At maturity, PT redeems 1:1 for the underlying.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select Base, list markets, let user pick one. Look up input token. See pendle skill 'Buy PT Task 1'.",
  "TASK 2 — Quote: call Pendle convert API to get PT swap quote and required approvals. See pendle skill 'Buy PT Task 2'.",
  "TASK 3 — Approve (if needed): approve input token for Pendle Router, broadcast. See pendle skill 'Buy PT Task 3'.",
  "TASK 4 — Execute: broadcast the swap tx from the quote, verify. See pendle skill 'Buy PT Task 4'."
]}
```

### Buy PT Task 1: Prepare

#### 1a. Select network

```json
{"tool": "select_web3_network", "network": "base"}
```

#### 1b. List markets

```tool:web_fetch
url: https://api-v2.pendle.finance/core/v1/markets/all?chainId=8453&isActive=true
method: GET
extract_mode: raw
```

Present markets to user with implied APY (this is the fixed yield they'd lock in). Let user choose a market and specify the input token + amount.

#### 1c. Look up input token

```json
{"tool": "token_lookup", "symbol": "<INPUT_TOKEN>", "cache_as": "token_address"}
```

#### 1d. Check balance

```tool:web3_preset_function_call
preset: erc20_balance
network: base
call_only: true
```

Report findings. Complete with `finished_task: true`.

---

### Buy PT Task 2: Get Quote from Pendle SDK

#### 2a. Convert amount to raw

```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals_register": "sell_token_decimals", "cache_as": "pendle_amount"}
```

#### 2b. Call Pendle convert endpoint

The PT address for a market can be found in the market data response (`pt.address` field). Replace placeholders:

```tool:web_fetch
url: https://api-v2.pendle.finance/core/v2/sdk/8453/convert?receiver=WALLET_ADDRESS&slippage=0.01&tokensIn=INPUT_TOKEN_ADDRESS&amountsIn=RAW_AMOUNT&tokensOut=PT_ADDRESS&enableAggregator=true&additionalData=impliedApy,effectiveApy
method: GET
extract_mode: raw
```

From the response, extract:
- `routes[0].tx` — the transaction to execute (data, to, value)
- `requiredApprovals` — any tokens that need approval first
- `routes[0].data.impliedApy` — the fixed APY being locked in
- `routes[0].data.priceImpact` — price impact

Show the user: amount in, expected PT out, fixed APY locked, price impact. **Ask for confirmation before proceeding.**

Complete with `finished_task: true`.

---

### Buy PT Task 3: Approve (if needed)

Check `requiredApprovals` from Task 2 response:

**If no approvals needed, SKIP:**

```json
{"tool": "task_fully_completed", "summary": "No approvals needed — skipping."}
```

**If approval needed**, approve the input token for the Pendle Router (`0x888888888889758F76e7103c6CbF23ABbF58F946`). Use the existing ERC20 approve pattern:

The `requiredApprovals` array contains `{token, amount}`. Queue an ERC20 approve transaction for that token to the Router address with the specified amount.

Broadcast and wait for confirmation, then complete.

---

### Buy PT Task 4: Execute Swap

Use the `tx` object from the Task 2 convert response. The tx has `to`, `data`, `value`, and `from` fields.

Broadcast the transaction using the raw tx data from the convert API response.

After broadcast, verify:

```json
{"tool": "verify_tx_broadcast"}
```

Report the result: tx hash, fixed APY locked, PT amount received.

---

## Operation E: Buy YT (Leveraged Yield)

Same flow as Buy PT, but use the YT address as `tokensOut` instead of PT address.

The YT address is found in market data response (`yt.address` field).

**Key difference**: YT gives leveraged exposure to variable yield. The user earns all yield on the notional value but the YT token itself trends toward zero at maturity.

Use the same task structure as Buy PT but replace PT_ADDRESS with YT_ADDRESS in the convert call.

---

## Operation F: Provide LP

Same flow but use the market address itself as `tokensOut` (market address = LP token address).

LP earns: swap fees + PENDLE incentives + underlying yield. Minimal IL because PT converges to SY at maturity.

---

## Operation G: Sell / Remove Position

To sell PT, YT, or remove LP, use the convert endpoint with:
- `tokensIn` = the PT/YT/LP token address
- `tokensOut` = desired output token (e.g., USDC, WETH)

Same task flow: quote via convert API, approve if needed, execute.

---

## Operation H: Claim Rewards

Claim accrued yield from YT positions and LP incentives:

```tool:web_fetch
url: https://api-v2.pendle.finance/core/v1/sdk/8453/redeem-interests-and-rewards?receiver=WALLET_ADDRESS&yts=YT_ADDRESS_1,YT_ADDRESS_2&markets=MARKET_ADDRESS_1
method: GET
extract_mode: raw
```

This returns a transaction to claim all pending rewards. Broadcast it.

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| Market not found | Invalid market address | List markets first to get valid addresses |
| Insufficient balance | Not enough input token | Check balance, reduce amount |
| High price impact | Amount too large for liquidity | Reduce amount or try different market |
| Market expired | Past maturity date | Can still redeem PT 1:1, but no new entries |
| Slippage exceeded | Market moved during execution | Retry with higher slippage |

---

## Tips

1. **PT for safety**: Fixed yield, no variable risk. Best for stablecoin yields.
2. **YT for conviction**: If you believe yield will stay high or increase, YT amplifies returns.
3. **LP for balanced**: Earn from multiple sources with minimal IL risk.
4. **Check expiry**: Markets have expiry dates. PT redeems at maturity; YT stops earning at maturity.
5. **Start small**: Test with small amounts first to verify the flow.
