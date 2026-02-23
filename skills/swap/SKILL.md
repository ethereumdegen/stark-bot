---
name: "swap"
description: "Swap ERC20 tokens on Base using 0x DEX aggregator via quoter.defirelay.com"
version: "9.0.5"
author: "starkbot"
homepage: "https://0x.org"
metadata: "{\"requires_auth\": false, \"clawdbot\":{\"emoji\":\"🔄\"}}"
requires_tools: [token_lookup, to_raw_amount, decode_calldata, web3_preset_function_call, x402_preset_fetch, x402_rpc, list_queued_web3_tx, broadcast_web3_tx, verify_tx_broadcast, select_web3_network, define_tasks]
tags: [crypto, defi, swap, dex, base, trading, 0x]
---

# Token Swap Skill

## CRITICAL RULES

1. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task. Do NOT work ahead.
2. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.** Using `finished_task: true` advances the task queue — if you use it prematurely, tasks get skipped.
3. **Use `say_to_user` WITHOUT `finished_task`** for progress updates. Only set `finished_task: true` OR call `task_fully_completed` when ALL steps in the current task are done.
4. **Sequential tool calls only.** Never call two tools in parallel when the second depends on the first (e.g., never call `swap_execute` and `decode_calldata` in the same response).
5. **Use exact parameter values shown.** Especially `cache_as: "swap"` — not "swap_params", not "swap_data", exactly `"swap"`.

## Step 1: Define the seven tasks

Call `define_tasks` with all 7 tasks in order:

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select network, look up sell+buy tokens, check AllowanceHolder allowance. See swap skill 'Task 1'.",
  "TASK 2 — Approve AllowanceHolder (SKIP if allowance sufficient): call erc20_approve_swap, broadcast, wait for confirmation. See swap skill 'Task 2'.",
  "TASK 3 — Convert amount: call to_raw_amount to convert sell amount to raw units. See swap skill 'Task 3'.",
  "TASK 4 — Fetch quote: call x402_preset_fetch with preset swap_quote. See swap skill 'Task 4'.",
  "TASK 5 — Decode quote: call decode_calldata with calldata_register='swap_quote' and cache_as='swap'. This sets swap_contract. See swap skill 'Task 5'.",
  "TASK 6 — Execute swap: call swap_execute preset THEN broadcast_web3_tx. Do NOT call decode_calldata here. See swap skill 'Task 6'.",
  "TASK 7 — Verify the swap result and report to the user. See swap skill 'Task 7'."
]}
```

---

## Task 1: Prepare — look up tokens, check balances, check allowance

### 1a. Select network (if user specified one)

```json
{"tool": "select_web3_network", "network": "<network>"}
```

### 1b. Look up SELL token

```json
{"tool": "token_lookup", "symbol": "<SELL_TOKEN>", "cache_as": "sell_token"}
```

**If selling ETH:** use WETH as the sell token instead:
1. Lookup WETH: `{"tool": "token_lookup", "symbol": "WETH", "cache_as": "sell_token"}`
2. Check WETH balance: `{"tool": "web3_preset_function_call", "preset": "weth_balance", "network": "<network>", "call_only": true}`
3. Check ETH balance: `{"tool": "x402_rpc", "preset": "get_balance", "network": "<network>"}`
4. If WETH insufficient, wrap:
   - `{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals": 18, "cache_as": "wrap_amount"}`
   - `{"tool": "web3_preset_function_call", "preset": "weth_deposit", "network": "<network>"}`
   - Broadcast the wrap tx and wait for confirmation

### 1c. Look up BUY token

```json
{"tool": "token_lookup", "symbol": "<BUY_TOKEN>", "cache_as": "buy_token"}
```

### 1d. Check AllowanceHolder allowance

```json
{"tool": "web3_preset_function_call", "preset": "erc20_allowance_swap", "network": "<network>", "call_only": true}
```

 

**Do NOT proceed to approval or quoting in this task. Just report findings.**

---

## Task 2: Approve sell token for AllowanceHolder

**If Task 1 determined allowance is already sufficient, SKIP this task:**

```json
{"tool": "task_fully_completed", "summary": "Allowance already sufficient — skipping approval."}
```

**Otherwise, approve:**

```json
{"tool": "web3_preset_function_call", "preset": "erc20_approve_swap", "network": "<network>"}
```

Broadcast and wait for confirmation:
```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_approve>"}
```

After the approval is confirmed:
```json
{"tool": "task_fully_completed", "summary": "Sell token approved for AllowanceHolder. Ready for quote."}
```

---

## Task 3: Convert sell amount to raw units

**One tool call (auto-completes on success):**

```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals_register": "sell_token_decimals", "cache_as": "sell_amount"}
```

---

## Task 4: Fetch swap quote

**One tool call (auto-completes on success):**

```json
{"tool": "x402_preset_fetch", "preset": "swap_quote", "cache_as": "swap_quote", "network": "<network>"}
```

If this fails after retries, STOP and tell the user.

---

## Task 5: Decode the swap quote

**IMPORTANT: Use `calldata_register` (NOT raw `calldata`). Use `cache_as: "swap"` exactly.**

This step reads the `swap_quote` register and extracts: `swap_contract`, `swap_param_0`–`swap_param_4`, `swap_value`.
Task 6 depends on these registers being set.

**One tool call (auto-completes on success):**

```json
{"tool": "decode_calldata", "abi": "0x_settler", "calldata_register": "swap_quote", "cache_as": "swap"}
```

---

## Task 6: Execute the swap

**Exactly 2 tool calls, SEQUENTIALLY (one at a time, NOT in parallel):**

**Do NOT call `decode_calldata` in this task — it was already done in Task 5.**
The registers `swap_contract`, `swap_param_0`–`swap_param_4`, `swap_value` should already be set.

### 6a. Create the swap transaction (FIRST call)

```json
{"tool": "web3_preset_function_call", "preset": "swap_execute", "network": "<network>"}
```

Wait for the result. Extract the `uuid` from the response.

### 6b. Broadcast it (SECOND call — after 6a succeeds)

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_6a>"}
```

The task auto-completes when `broadcast_web3_tx` succeeds.

---

## Task 7: Verify the swap

Call `verify_tx_broadcast` to poll for the receipt, decode token transfer events, and confirm the result matches the user's intent:

```json
{"tool": "verify_tx_broadcast"}
```

Read the output:

- **"TRANSACTION VERIFIED"** → The swap succeeded AND the AI confirmed it matches the user's intent. Report success with tx hash and explorer link.
- **"TRANSACTION CONFIRMED — INTENT MISMATCH"** → Confirmed on-chain but AI flagged a concern. Tell the user to check the explorer.
- **"TRANSACTION REVERTED"** → The swap failed. Tell the user.
- **"CONFIRMATION TIMEOUT"** → Tell the user to check the explorer link.

Call `task_fully_completed` when verify_tx_broadcast returned VERIFIED or CONFIRMED.