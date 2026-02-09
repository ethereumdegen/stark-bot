---
name: transfer_erc20
description: "Transfer (Send) ERC20 tokens on Base/Ethereum using the burner wallet"
version: 2.1.0
author: starkbot
homepage: https://basescan.org
metadata: {"requires_auth": false, "clawdbot":{"emoji":"🪙"}}
tags: [crypto, transfer, send, erc20, tokens, base, wallet]
requires_tools: [set_address, token_lookup, to_raw_amount, web3_preset_function_call, list_queued_web3_tx, broadcast_web3_tx, verify_tx_broadcast, select_web3_network, define_tasks]
---

# ERC20 Token Transfer Skill

## CRITICAL RULES

1. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task. Do NOT work ahead.
2. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.** Using `finished_task: true` advances the task queue — if you use it prematurely, tasks get skipped.
3. **Use `say_to_user` WITHOUT `finished_task`** for progress updates. Only set `finished_task: true` OR call `task_fully_completed` when ALL steps in the current task are done.
4. **Sequential tool calls only.** Never call two tools in parallel when the second depends on the first.
5. **Register pattern prevents hallucination.** Never pass raw addresses/amounts directly — always use registers set by the tools.

## Step 1: Define the four tasks

Call `define_tasks` with all 4 tasks in order:

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select network (if specified), look up token, check token balance, check ETH for gas. See transfer_erc20 skill 'Task 1'.",
  "TASK 2 — Set up: set recipient address, convert amount to raw units. See transfer_erc20 skill 'Task 2'.",
  "TASK 3 — Execute: call erc20_transfer preset, then broadcast_web3_tx. See transfer_erc20 skill 'Task 3'.",
  "TASK 4 — Verify the transfer result and report to the user. See transfer_erc20 skill 'Task 4'."
]}
```

---

## Task 1: Prepare — look up token, check balances

### 1a. Select network (if user specified one)

```json
{"tool": "select_web3_network", "network": "<network>"}
```

If no network specified, skip this step (default is base).

### 1b. Look up the token

```json
{"tool": "token_lookup", "symbol": "<TOKEN>", "cache_as": "token_address"}
```

This sets registers: `token_address` and `token_address_decimals`.

### 1c. Check token balance

```json
{"tool": "web3_preset_function_call", "preset": "erc20_balance", "network": "<network>", "call_only": true}
```

### 1d. Report findings and complete

Tell the user what you found (token address, balance, whether they have enough) using `say_to_user` with `finished_task: true`:

```json
{"tool": "say_to_user", "message": "Found token: <TOKEN>=0x...\nBalance: ...\nReady to transfer.", "finished_task": true}
```

**Do NOT proceed to setting address or converting amounts in this task. Just report findings.**

---

## Task 2: Set recipient address and convert amount

### 2a. Set recipient address

```json
{"tool": "set_address", "register": "recipient_address", "address": "<RECIPIENT_ADDRESS>"}
```

### 2b. Convert amount to raw units

```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "cache_as": "transfer_amount"}
```

This reads `token_address_decimals` automatically and sets the `transfer_amount` register.

After both succeed:
```json
{"tool": "task_fully_completed", "summary": "Recipient set and amount converted. Ready to execute transfer."}
```

---

## Task 3: Execute the transfer

**Exactly 2 tool calls, SEQUENTIALLY (one at a time, NOT in parallel):**

### 3a. Create the transfer transaction (FIRST call)

```json
{"tool": "web3_preset_function_call", "preset": "erc20_transfer", "network": "<network>"}
```

The `erc20_transfer` preset reads `token_address`, `recipient_address`, and `transfer_amount` from registers automatically.

Wait for the result. Extract the `uuid` from the response.

### 3b. Broadcast it (SECOND call — after 3a succeeds)

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_3a>"}
```

The task auto-completes when `broadcast_web3_tx` succeeds.

---

## Task 4: Verify the transfer

Call `verify_tx_broadcast` to poll for the receipt and confirm the result:

```json
{"tool": "verify_tx_broadcast"}
```

Read the output:

- **"TRANSACTION VERIFIED"** → The transfer succeeded AND the AI confirmed it matches the user's intent. Report success with tx hash and explorer link.
- **"TRANSACTION CONFIRMED — INTENT MISMATCH"** → Confirmed on-chain but AI flagged a concern. Tell the user to check the explorer.
- **"TRANSACTION REVERTED"** → The transfer failed. Tell the user.
- **"CONFIRMATION TIMEOUT"** → Tell the user to check the explorer link.

Call `task_fully_completed` when verify_tx_broadcast returned VERIFIED or CONFIRMED.
