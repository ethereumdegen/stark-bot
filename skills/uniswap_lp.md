---
name: uniswap_lp
description: "Provide liquidity on Uniswap V4 (Base) — deposit to pools, withdraw, collect fees."
version: 1.1.0
author: starkbot
homepage: https://app.uniswap.org
metadata: {"requires_auth": false, "clawdbot":{"emoji":"🦄"}}
requires_tools: [token_lookup, to_raw_amount, web3_function_call, web3_preset_function_call, decode_calldata, web_fetch, broadcast_web3_tx, verify_tx_broadcast, select_web3_network, define_tasks]
tags: [crypto, defi, liquidity, uniswap, lp, base, yield]
---

# Uniswap V4 LP Skill

Provide liquidity on Uniswap V4 pools on Base — create positions, increase/decrease liquidity, and collect fees.

## CRITICAL RULES

1. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task. Do NOT work ahead.
2. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.**
3. **Sequential tool calls only.** Never call two tools in parallel when the second depends on the first.
4. **Use exact parameter values shown.** Especially `cache_as` values — use exactly what is specified.
5. **Always use WETH, not native ETH.** If the user wants to LP with ETH, wrap it to WETH first using the `weth_deposit` preset.

## Pool Configuration

| Pool | Pool ID | Token0 | Token1 | Fee | Tick Spacing | Hooks |
|------|---------|--------|--------|-----|-------------|-------|
| STARKBOT/WETH | `0x0d64a8e0d28626511cc23fc75b81c2f03e222b14f9b944b60eecc3f4ddabeddc` | WETH (`0x4200000000000000000000000000000000000006`) | STARKBOT (`0x587Cd533F418825521f3A1daa7CCd1E7339A1B07`) | 10000 (1%) | 200 | `0x0000000000000000000000000000000000000000` |

> Token0/token1 order is determined by address sort (lower address first). WETH < STARKBOT by address.

## Key Addresses (Base)

| Contract | Address |
|----------|---------|
| V4 PositionManager | `0x7c5f5a4bbd8fd63184577525326123b519429bdc` |
| V4 StateView | `0xa3c0c9b65bad0b08107aa264b0f3db444b867a71` |
| Permit2 | `0x000000000022D473030F116dDEE9F6B43aC78BA3` |
| WETH | `0x4200000000000000000000000000000000000006` |
| STARKBOT | `0x587Cd533F418825521f3A1daa7CCd1E7339A1B07` |

## Tick Range Guidance

When creating a position, the user must choose a tick range. Offer these options:

- **Full range**: tickLower = -887200, tickUpper = 887200 (like V2, simple but less capital efficient)
- **Wide range**: approximately ±50% around current tick (balanced risk/efficiency)
- **Narrow range**: approximately ±10% around current tick (high capital efficiency, more IL risk)

**Tick alignment**: tickLower and tickUpper must be multiples of the pool's tick spacing (200 for STARKBOT/WETH). Round to the nearest multiple.

To compute ticks from price ratios around the current tick:
- For ±X% range: tickLower = currentTick - (X_ticks), tickUpper = currentTick + (X_ticks)
- Round both to nearest multiple of tickSpacing (200)

---

## Operation A: Get Pool Info

Read pool state to get current tick, price, and liquidity. No tasks needed — just direct tool calls.

### A1. Select network

```json
{"tool": "select_web3_network", "network": "base"}
```

### A2. Read pool slot0

```json
{"tool": "web3_function_call", "abi": "uniswap_v4_state_view", "contract": "0xa3c0c9b65bad0b08107aa264b0f3db444b867a71", "function": "getSlot0", "params": ["0x0d64a8e0d28626511cc23fc75b81c2f03e222b14f9b944b60eecc3f4ddabeddc"], "call_only": true}
```

Returns: sqrtPriceX96, tick, protocolFee, lpFee.

### A3. Read pool liquidity

```json
{"tool": "web3_function_call", "abi": "uniswap_v4_state_view", "contract": "0xa3c0c9b65bad0b08107aa264b0f3db444b867a71", "function": "getLiquidity", "params": ["0x0d64a8e0d28626511cc23fc75b81c2f03e222b14f9b944b60eecc3f4ddabeddc"], "call_only": true}
```

### A4. Calculate and report

Calculate approximate price from sqrtPriceX96:
- price = (sqrtPriceX96 / 2^96)^2
- Adjust for decimals: WETH has 18 decimals, STARKBOT has 18 decimals
- This gives STARKBOT per WETH price

Report: current tick, sqrtPriceX96, liquidity, approximate price.

---

## Operation B: Create Position (Deposit) — 5 Tasks

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select Base, look up both tokens, check balances, read pool state (slot0 + liquidity). See LP skill 'Task 1'.",
  "TASK 2 — Approve: approve both tokens for Permit2 (skip if sufficient). See LP skill 'Task 2'.",
  "TASK 3 — Build tx: POST to Uniswap API /lp/create, cache response. See LP skill 'Task 3'.",
  "TASK 4 — Execute: decode calldata, call LP preset, then broadcast_web3_tx. See LP skill 'Task 4'.",
  "TASK 5 — Verify the LP position result and report to the user. See LP skill 'Task 5'."
]}
```

### Task 1: Prepare — look up tokens, check balances, read pool state

#### 1a. Select network

```json
{"tool": "select_web3_network", "network": "base"}
```

#### 1b. Look up WETH (token0)

```json
{"tool": "token_lookup", "symbol": "WETH", "cache_as": "sell_token"}
```

#### 1c. Check WETH balance

```json
{"tool": "web3_preset_function_call", "preset": "weth_balance", "network": "base", "call_only": true}
```

#### 1d. Look up STARKBOT (token1)

```json
{"tool": "token_lookup", "symbol": "STARKBOT", "cache_as": "sell_token"}
```

#### 1e. Check STARKBOT balance

```json
{"tool": "web3_preset_function_call", "preset": "erc20_balance", "network": "base", "call_only": true}
```

Note: `sell_token` register now holds STARKBOT address (from 1d). The erc20_balance preset reads from `token_address` — you need to set it:

```json
{"tool": "token_lookup", "symbol": "STARKBOT", "cache_as": "token_address"}
```

Then check balance:

```json
{"tool": "web3_preset_function_call", "preset": "erc20_balance", "network": "base", "call_only": true}
```

#### 1f. Read pool state

```json
{"tool": "web3_function_call", "abi": "uniswap_v4_state_view", "contract": "0xa3c0c9b65bad0b08107aa264b0f3db444b867a71", "function": "getSlot0", "params": ["0x0d64a8e0d28626511cc23fc75b81c2f03e222b14f9b944b60eecc3f4ddabeddc"], "call_only": true}
```

```json
{"tool": "web3_function_call", "abi": "uniswap_v4_state_view", "contract": "0xa3c0c9b65bad0b08107aa264b0f3db444b867a71", "function": "getLiquidity", "params": ["0x0d64a8e0d28626511cc23fc75b81c2f03e222b14f9b944b60eecc3f4ddabeddc"], "call_only": true}
```

#### 1g. Report and suggest tick range

Report balances, current tick, price, and liquidity. Suggest tick ranges (full/wide/narrow). Ask user to confirm amounts and range. Complete with `finished_task: true`.

---

### Task 2: Approve tokens for Permit2

Uniswap V4 uses Permit2. Check and approve BOTH tokens if needed.

#### 2a. Check WETH allowance for Permit2

```json
{"tool": "token_lookup", "symbol": "WETH", "cache_as": "sell_token"}
```

```json
{"tool": "web3_preset_function_call", "preset": "erc20_allowance_permit2", "network": "base", "call_only": true}
```

#### 2b. Approve WETH if needed

If allowance is insufficient:

```json
{"tool": "web3_preset_function_call", "preset": "erc20_approve_permit2", "network": "base"}
```

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

Wait for confirmation.

#### 2c. Check STARKBOT allowance for Permit2

```json
{"tool": "token_lookup", "symbol": "STARKBOT", "cache_as": "sell_token"}
```

```json
{"tool": "web3_preset_function_call", "preset": "erc20_allowance_permit2", "network": "base", "call_only": true}
```

#### 2d. Approve STARKBOT if needed

If allowance is insufficient:

```json
{"tool": "web3_preset_function_call", "preset": "erc20_approve_permit2", "network": "base"}
```

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

Wait for confirmation.

#### 2e. Complete

```json
{"tool": "task_fully_completed", "summary": "Both tokens approved for Permit2. Ready to create position."}
```

If both were already approved:

```json
{"tool": "task_fully_completed", "summary": "Both tokens already approved for Permit2 — skipping."}
```

---

### Task 3: Build LP transaction via Uniswap API

**IMPORTANT**: Use the pool state values from Task 1 (currentTick, sqrtRatioX96, poolLiquidity).

Convert amounts to raw units first:

For WETH (token0, 18 decimals):
```json
{"tool": "to_raw_amount", "amount": "<weth_amount>", "decimals": 18, "cache_as": "lp_amount0"}
```

For STARKBOT (token1, 18 decimals):
```json
{"tool": "to_raw_amount", "amount": "<starkbot_amount>", "decimals": 18, "cache_as": "lp_amount1"}
```

Then call the Uniswap API:

```json
{
  "tool": "web_fetch",
  "url": "https://trade-api.gateway.uniswap.org/v1/lp/create",
  "method": "POST",
  "headers": {"x-api-key": "$UNISWAP_API_KEY"},
  "body": {
    "protocol": "V4",
    "walletAddress": "<wallet_address>",
    "chainId": 8453,
    "position": {
      "pool": {
        "token0": "0x4200000000000000000000000000000000000006",
        "token1": "0x587Cd533F418825521f3A1daa7CCd1E7339A1B07",
        "fee": 10000,
        "tickSpacing": 200,
        "hooks": "0x0000000000000000000000000000000000000000"
      },
      "tickLower": "<tick_lower>",
      "tickUpper": "<tick_upper>"
    },
    "amount0": "<raw_amount0_from_register>",
    "amount1": "<raw_amount1_from_register>",
    "poolLiquidity": "<from_task1>",
    "currentTick": "<from_task1>",
    "sqrtRatioX96": "<from_task1>",
    "slippageTolerance": 50
  },
  "extract_mode": "raw",
  "cache_as": "uni_lp_tx",
  "json_path": "create"
}
```

The `json_path: "create"` extracts the transaction object from the API response's `create` field. The cached register will contain `{to, data, value}`.

After success:
```json
{"tool": "task_fully_completed", "summary": "LP create transaction built and cached. Ready to execute."}
```

---

### Task 4: Decode and execute LP transaction

#### 4a. Decode the cached transaction

```json
{"tool": "decode_calldata", "abi": "uniswap_v4_position_manager", "calldata_register": "uni_lp_tx", "cache_as": "uni_lp"}
```

This extracts `uni_lp_contract`, `uni_lp_param_0`, `uni_lp_param_1`, `uni_lp_value` from the cached transaction.

#### 4b. Execute via preset

```json
{"tool": "web3_preset_function_call", "preset": "uni_v4_modify_liquidities", "network": "base"}
```

#### 4c. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

The task auto-completes when `broadcast_web3_tx` succeeds.

---

### Task 5: Verify

```json
{"tool": "verify_tx_broadcast"}
```

Report the result:
- **VERIFIED**: Position created successfully. Report tx hash and explorer link.
- **REVERTED**: Transaction failed. Tell the user.
- **TIMEOUT**: Tell user to check explorer.

```json
{"tool": "task_fully_completed", "summary": "LP position created successfully."}
```

---

## Operation C: Increase Position — 4 Tasks

Used to add more liquidity to an existing position.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select Base, look up tokens, check balances, read pool state. Get tokenId from user. See LP skill 'Increase Task 1'.",
  "TASK 2 — Approve: approve both tokens for Permit2 (skip if sufficient). See LP skill 'Task 2' (same as create).",
  "TASK 3 — Build + Execute: POST to /lp/increase, decode, execute, then broadcast_web3_tx. See LP skill 'Increase Task 3'.",
  "TASK 4 — Verify the result and report to the user. See LP skill 'Task 5' (same as create)."
]}
```

### Increase Task 1: Prepare

Same as Create Task 1, but also ask the user for their position `tokenId` (they can find it on the Uniswap UI or from their wallet).

### Increase Task 3: Build + Execute

Convert amounts to raw units, then call the API:

```json
{
  "tool": "web_fetch",
  "url": "https://trade-api.gateway.uniswap.org/v1/lp/increase",
  "method": "POST",
  "headers": {"x-api-key": "$UNISWAP_API_KEY"},
  "body": {
    "protocol": "V4",
    "tokenId": "<token_id>",
    "walletAddress": "<wallet_address>",
    "chainId": 8453,
    "position": {
      "pool": {
        "token0": "0x4200000000000000000000000000000000000006",
        "token1": "0x587Cd533F418825521f3A1daa7CCd1E7339A1B07",
        "fee": 10000,
        "tickSpacing": 200,
        "hooks": "0x0000000000000000000000000000000000000000"
      },
      "tickLower": "<tick_lower>",
      "tickUpper": "<tick_upper>"
    },
    "amount0": "<raw_amount0>",
    "amount1": "<raw_amount1>",
    "poolLiquidity": "<from_task1>",
    "currentTick": "<from_task1>",
    "sqrtRatioX96": "<from_task1>",
    "slippageTolerance": 50
  },
  "extract_mode": "raw",
  "cache_as": "uni_lp_tx",
  "json_path": "increase"
}
```

Then decode and execute (same as Create Task 4):

```json
{"tool": "decode_calldata", "abi": "uniswap_v4_position_manager", "calldata_register": "uni_lp_tx", "cache_as": "uni_lp"}
```

```json
{"tool": "web3_preset_function_call", "preset": "uni_v4_modify_liquidities", "network": "base"}
```

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

---

## Operation D: Decrease Position (Withdraw) — 4 Tasks

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select Base, get tokenId and withdrawal percentage from user, read pool state. See LP skill 'Decrease Task 1'.",
  "TASK 2 — Build + Execute: POST to /lp/decrease, decode, execute, then broadcast_web3_tx. See LP skill 'Decrease Task 2'.",
  "TASK 3 — Verify the result and report to the user. See LP skill 'Task 5'.",
  "TASK 4 — Report: report withdrawn amounts. See LP skill 'Decrease Task 4'."
]}
```

### Decrease Task 1: Prepare

Select network, read pool state, ask user for:
- `tokenId`: their position NFT token ID
- How much to withdraw: percentage (e.g., 100 for full withdrawal, 50 for half)

### Decrease Task 2: Build + Execute

```json
{
  "tool": "web_fetch",
  "url": "https://trade-api.gateway.uniswap.org/v1/lp/decrease",
  "method": "POST",
  "headers": {"x-api-key": "$UNISWAP_API_KEY"},
  "body": {
    "protocol": "V4",
    "tokenId": "<token_id>",
    "walletAddress": "<wallet_address>",
    "chainId": 8453,
    "position": {
      "pool": {
        "token0": "0x4200000000000000000000000000000000000006",
        "token1": "0x587Cd533F418825521f3A1daa7CCd1E7339A1B07",
        "fee": 10000,
        "tickSpacing": 200,
        "hooks": "0x0000000000000000000000000000000000000000"
      },
      "tickLower": "<tick_lower>",
      "tickUpper": "<tick_upper>"
    },
    "liquidityPercentageToDecrease": "<percentage>",
    "poolLiquidity": "<from_task1>",
    "currentTick": "<from_task1>",
    "sqrtRatioX96": "<from_task1>",
    "slippageTolerance": 50
  },
  "extract_mode": "raw",
  "cache_as": "uni_lp_tx",
  "json_path": "decrease"
}
```

Then decode and execute:

```json
{"tool": "decode_calldata", "abi": "uniswap_v4_position_manager", "calldata_register": "uni_lp_tx", "cache_as": "uni_lp"}
```

```json
{"tool": "web3_preset_function_call", "preset": "uni_v4_modify_liquidities", "network": "base"}
```

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

### Decrease Task 4: Report

After verification, report the withdrawn token amounts and remaining position (if partial withdrawal).

---

## Operation E: Collect Fees — 3 Tasks

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select Base, get tokenId from user, read pool state. See LP skill 'Collect Task 1'.",
  "TASK 2 — Build + Execute: POST to /lp/claim, decode, execute, then broadcast_web3_tx. See LP skill 'Collect Task 2'.",
  "TASK 3 — Verify the result and report collected fees. See LP skill 'Task 5'."
]}
```

### Collect Task 1: Prepare

Select network, read pool state, get tokenId from user.

### Collect Task 2: Build + Execute

```json
{
  "tool": "web_fetch",
  "url": "https://trade-api.gateway.uniswap.org/v1/lp/claim",
  "method": "POST",
  "headers": {"x-api-key": "$UNISWAP_API_KEY"},
  "body": {
    "protocol": "V4",
    "tokenId": "<token_id>",
    "walletAddress": "<wallet_address>",
    "chainId": 8453,
    "position": {
      "pool": {
        "token0": "0x4200000000000000000000000000000000000006",
        "token1": "0x587Cd533F418825521f3A1daa7CCd1E7339A1B07",
        "fee": 10000,
        "tickSpacing": 200,
        "hooks": "0x0000000000000000000000000000000000000000"
      },
      "tickLower": "<tick_lower>",
      "tickUpper": "<tick_upper>"
    },
    "poolLiquidity": "<from_task1>",
    "currentTick": "<from_task1>",
    "sqrtRatioX96": "<from_task1>",
    "slippageTolerance": 50
  },
  "extract_mode": "raw",
  "cache_as": "uni_lp_tx",
  "json_path": "claim"
}
```

Then decode and execute:

```json
{"tool": "decode_calldata", "abi": "uniswap_v4_position_manager", "calldata_register": "uni_lp_tx", "cache_as": "uni_lp"}
```

```json
{"tool": "web3_preset_function_call", "preset": "uni_v4_modify_liquidities", "network": "base"}
```

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| Insufficient token balance | Not enough WETH or STARKBOT | Check balances, reduce amounts or wrap ETH |
| Insufficient gas | Not enough ETH for gas | Need ETH on Base for gas |
| Allowance too low | Token not approved for Permit2 | Run approval task |
| Invalid tick range | Ticks not aligned to tickSpacing | Round ticks to nearest multiple of 200 |
| Pool not found | Wrong pool parameters | Verify pool config matches on-chain state |
| API error | Uniswap API issue | Check API key ($UNISWAP_API_KEY), retry |
| Slippage exceeded | Price moved too much | Increase slippageTolerance or retry |

## V1 Limitations

- **WETH only** — not native ETH. Wrap ETH first using the `weth_deposit` preset.
- **Manual tokenId** — user must provide their position tokenId for increase/decrease/claim operations (findable on Uniswap UI or from wallet).
- **Single pool** — STARKBOT/WETH only initially. Extensible by adding to `config/uniswap_pools.ron`.
- **No position enumeration** — cannot list all positions automatically yet.

## How Uniswap V4 LP Works

1. **Create**: Deposit token0 and token1 into a price range. You receive a position NFT (tokenId).
2. **Earn fees**: When swaps happen through your price range, you earn proportional fees.
3. **Increase**: Add more liquidity to your existing position.
4. **Decrease**: Remove some or all liquidity. Receive tokens back.
5. **Collect fees**: Claim accumulated trading fees without changing your position.

Key concepts:
- **Tick range**: Defines the price range where your liquidity is active. Narrower = more capital efficient but more impermanent loss risk.
- **Concentrated liquidity**: Unlike V2, your capital only works within your chosen range.
- **Full range** (tickLower=-887200, tickUpper=887200): Mimics V2 behavior, always active, less efficient.
