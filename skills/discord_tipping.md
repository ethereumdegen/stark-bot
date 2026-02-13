---
name: discord_tipping
description: "Tip Discord users with tokens. Resolves Discord mentions to wallet addresses and executes ERC20 transfers."
version: 2.4.0
author: starkbot
metadata: {"clawdbot":{"emoji":"💸"}}
tags: [discord, tipping, crypto, transfer, erc20]
sets_agent_subtype: finance
requires_tools: [discord_resolve_user, token_lookup, to_raw_amount, web3_preset_function_call, list_queued_web3_tx, broadcast_web3_tx, verify_tx_broadcast, define_tasks]
---

# Discord Tipping

Send tokens to Discord users by resolving their mention to a registered wallet address.

**Amount shorthand:** Users can use "k" for thousands (1k = 1,000) and "m" for millions (1m = 1,000,000). For example: "tip @user 5k STARKBOT" means 5,000 tokens.

## CRITICAL RULES

1. **EXECUTE IMMEDIATELY.** Do NOT ask the user for confirmation. Do NOT narrate what you plan to do. Call `define_tasks` as your VERY FIRST action, then execute each task by calling the prescribed tools. Never output a text response before starting the tool flow.
2. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task. Do NOT work ahead.
3. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.** Using `finished_task: true` advances the task queue — if you use it prematurely, tasks get skipped.
4. **Use `say_to_user` WITHOUT `finished_task`** for progress updates. Only set `finished_task: true` OR call `task_fully_completed` when ALL steps in the current task are done.
5. **Sequential tool calls only.** Never call two tools in parallel when the second depends on the first.
6. **Register pattern prevents hallucination.** Never pass raw addresses/amounts directly — always use registers set by the tools.
7. **Ignore chat context for addresses.** Even if you see an address in recent chat history, you MUST call `discord_resolve_user` to resolve it properly. Never skip tool calls because you think you already know the answer.

## Step 1: Define the seven tasks

Call `define_tasks` with all 7 tasks in order:

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Resolve Discord mention to wallet address. See discord_tipping skill 'Task 1'.",
  "TASK 2 — Look up token contract. See discord_tipping skill 'Task 2'.",
  "TASK 3 — Check token balance. See discord_tipping skill 'Task 3'.",
  "TASK 4 — Convert amount to raw units. See discord_tipping skill 'Task 4'.",
  "TASK 5 — Create transfer transaction. See discord_tipping skill 'Task 5'.",
  "TASK 6 — Broadcast transaction. See discord_tipping skill 'Task 6'.",
  "TASK 7 — Verify transfer and report result. See discord_tipping skill 'Task 7'."
]}
```

---

## Task 1: Resolve Discord mention

Extract the Discord user ID from the mention and resolve it to a wallet address:

```json
{"tool": "discord_resolve_user", "user_mention": "<numeric_user_id>"}
```

**Note:** Pass the numeric user ID, not the raw mention format. Extract the numbers from mentions like `<@1234567890>`.

- If `registered: true` → proceed (the `recipient_address` register is automatically set)
- If error/not registered → tell user they need to register with `@starkbot register 0x...` and stop

After success:
```json
{"tool": "task_fully_completed", "summary": "Resolved user to wallet address."}
```

---

## Task 2: Look up token

```json
{"tool": "token_lookup", "symbol": "<TOKEN>", "network": "base", "cache_as": "token_address"}
```

This sets registers: `token_address` and `token_address_decimals`.

After success:
```json
{"tool": "task_fully_completed", "summary": "Token looked up and registers set."}
```

---

## Task 3: Check token balance

```json
{"tool": "web3_preset_function_call", "preset": "erc20_balance", "network": "base", "call_only": true}
```

Verify the sender has enough tokens for the tip. If insufficient, tell the user and stop.

After success:
```json
{"tool": "task_fully_completed", "summary": "Balance verified — sufficient funds."}
```

---

## Task 4: Convert amount to raw units

```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "cache_as": "transfer_amount"}
```

This reads `token_address_decimals` automatically and sets the `transfer_amount` register.

After success:
```json
{"tool": "task_fully_completed", "summary": "Amount converted. Ready to execute transfer."}
```

---

## Task 5: Create transfer transaction

```json
{"tool": "web3_preset_function_call", "preset": "erc20_transfer", "network": "base"}
```

The `erc20_transfer` preset reads `token_address`, `recipient_address`, and `transfer_amount` from registers automatically.

Wait for the result. Extract the `uuid` from the response.

After success:
```json
{"tool": "task_fully_completed", "summary": "Transfer transaction created."}
```

---

## Task 6: Broadcast transaction

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_previous_task>"}
```

After broadcast succeeds:
```json
{"tool": "task_fully_completed", "summary": "Tip broadcast. Verifying next."}
```

---

## Task 7: Verify transfer

Call `verify_tx_broadcast` to poll for the receipt and confirm the result:

```json
{"tool": "verify_tx_broadcast"}
```

Read the output:

- **"TRANSACTION VERIFIED"** → The tip succeeded AND the AI confirmed it matches the user's intent. Report success with tx hash and explorer link.
- **"TRANSACTION CONFIRMED — INTENT MISMATCH"** → Confirmed on-chain but AI flagged a concern. Tell the user to check the explorer.
- **"TRANSACTION REVERTED"** → The tip failed. Tell the user.
- **"CONFIRMATION TIMEOUT"** → Tell the user to check the explorer link.

Call `task_fully_completed` when verify_tx_broadcast returned VERIFIED or CONFIRMED.
