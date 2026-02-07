---
name: swap
description: "Swap ERC20 tokens on Base using 0x DEX aggregator via quoter.defirelay.com"
version: 8.2.0
author: starkbot
homepage: https://0x.org
metadata: {"requires_auth": false, "clawdbot":{"emoji":"🔄"}}
tags: [crypto, defi, swap, dex, base, trading, 0x]
requires_tools: [token_lookup, to_raw_amount, decode_calldata, web3_preset_function_call, x402_fetch, x402_rpc, list_queued_web3_tx, broadcast_web3_tx, verify_tx_broadcast, select_web3_network, define_tasks]
---

# Token Swap Skill
 

## Step 1: Define the four tasks

Call `define_tasks` with all 5 tasks in order:

```json
{"tool": "define_tasks", "tasks": [
  "SWAP TASK 1/5 — Prepare: select network, look up sell and buy tokens, check balances, check Permit2 allowance. Report what you found. See swap skill 'Task 1' instructions.",
  "SWAP TASK 2/5 — Approve sell token for Permit2 (SKIP if allowance was sufficient in Task 1): call erc20_approve_permit2 preset, broadcast, wait for confirmation. See swap skill 'Task 2' instructions.",
  "SWAP TASK 3/5 — Get swap quote: convert sell amount to raw_units with to_raw_amount, fetch quote with x402_fetch swap_quote preset, decode calldata with decode_calldata using cache_as 'swap'. See swap skill 'Task 3' instructions.",
  "SWAP TASK 4/5 — Execute swap: IMMEDIATELY call swap_execute preset, then IMMEDIATELY broadcast  ",
  "SWAP TASK 5/5 —  Call verify_tx_broadcast and ONLY report success if VERIFIED or CONFIRMED. See swap skill 'Task 5' instructions."
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

### 1d. Check Permit2 allowance

```json
{"tool": "web3_preset_function_call", "preset": "erc20_allowance_permit2", "network": "<network>", "call_only": true}
```

### 1e. Report findings and complete

Tell the user what you found (token addresses, balances, whether approval is needed). Then:

```json
{"tool": "task_fully_completed", "summary": "Tokens looked up. Allowance: <sufficient/insufficient>. Ready for next step."}
```

---

## Task 2: Approve sell token for Permit2

**If Task 1 determined allowance is already sufficient, SKIP this task:**

```json
{"tool": "task_fully_completed", "summary": "Allowance already sufficient — skipping approval."}
```

**Otherwise, approve:**

```json
{"tool": "web3_preset_function_call", "preset": "erc20_approve_permit2", "network": "<network>"}
```

Broadcast and wait for confirmation:
```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_approve>"}
```

After the approval is confirmed:
```json
{"tool": "task_fully_completed", "summary": "Sell token approved for Permit2. Ready for quote."}
```

 
---

## Task 3: Get swap quote

### 3a. Convert sell amount to raw units

```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals_register": "sell_token_decimals", "cache_as": "sell_amount"}
```

### 3b. Fetch swap quote

```json
{"tool": "x402_fetch", "preset": "swap_quote", "cache_as": "swap_quote", "network": "<network>"}
```

If this fails after retries, STOP and tell the user.

### 3c. Decode the quote

**Use `cache_as: "swap"` exactly.** This sets `swap_param_0`–`swap_param_4`, `swap_contract`, `swap_value`.

```json
{"tool": "decode_calldata", "abi": "0x_settler", "calldata_register": "swap_quote", "cache_as": "swap"}
```

After decoding succeeds:
```json
{"tool": "task_fully_completed", "summary": "Quote fetched and decoded. Ready to execute swap."}
```

---

## Task 4: Execute the swap

**  just execute and broadcast immediately  **

### 4a. Execute the swap transaction

```json
{"tool": "web3_preset_function_call", "preset": "swap_execute", "network": "<network>"}
```

### 4b. Immediately broadcast the swap transaction

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_4a>"}
```

### 4c. VERIFY the result

Call `verify_tx_broadcast` to poll for the receipt, decode token transfer events, and AI-verify the result matches the user's intent:

```json
{"tool": "verify_tx_broadcast"}
```

Read the output:

- **"TRANSACTION VERIFIED"** → The swap succeeded AND the AI confirmed it matches the user's intent. Report success with tx hash and explorer link.
- **"TRANSACTION CONFIRMED — INTENT MISMATCH"** → Confirmed on-chain but AI flagged a concern. Tell the user to check the explorer.
- **"TRANSACTION REVERTED"** → The swap FAILED. Tell the user. 
- **"CONFIRMATION TIMEOUT"** → Tell the user to check the explorer link.

**Only call `task_fully_completed` if verify_tx_broadcast returned VERIFIED or CONFIRMED.**
