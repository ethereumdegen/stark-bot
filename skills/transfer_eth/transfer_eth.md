---
name: transfer_eth
description: "Transfer (Send) native ETH on Base/Ethereum using the burner wallet"
version: 3.1.0
author: starkbot
homepage: https://basescan.org
metadata: {"requires_auth": false, "clawdbot":{"emoji":"💸"}}
tags: [crypto, transfer, send, eth, base, wallet]
requires_tools: [set_address, to_raw_amount, send_eth, list_queued_web3_tx, broadcast_web3_tx, x402_rpc, verify_tx_broadcast, select_web3_network, define_tasks]
---

# ETH Transfer Skill

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
  "TASK 1 — Prepare: select network (if specified), check ETH balance, report to user. See transfer_eth skill 'Task 1'.",
  "TASK 2 — Set up: set recipient address, convert amount to wei. See transfer_eth skill 'Task 2'.",
  "TASK 3 — Execute: call send_eth, then broadcast_web3_tx. See transfer_eth skill 'Task 3'.",
  "TASK 4 — Verify the transfer result and report to the user. See transfer_eth skill 'Task 4'."
]}
```

---

## Task 1: Prepare — check ETH balance

### 1a. Select network (if user specified one)

```json
{"tool": "select_web3_network", "network": "<network>"}
```

If no network specified, skip this step (default is base).

### 1b. Check ETH balance

```json
{"tool": "x402_rpc", "preset": "get_balance", "network": "<network>"}
```

### 1c. Report findings and complete

Tell the user their balance and whether they have enough (including gas) using `say_to_user` with `finished_task: true`:

```json
{"tool": "say_to_user", "message": "ETH Balance: ... ETH\nRequested transfer: ... ETH\nSufficient funds: yes/no", "finished_task": true}
```

**Do NOT proceed to setting address or converting amounts in this task. Just report findings.**

---

## Task 2: Set recipient address and convert amount

### 2a. Set recipient address

```json
{"tool": "set_address", "register": "send_to", "address": "<RECIPIENT_ADDRESS>"}
```

### 2b. Convert amount to wei

```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals": 18, "cache_as": "amount_raw"}
```

ETH always uses 18 decimals. This sets the `amount_raw` register.

After both succeed:
```json
{"tool": "task_fully_completed", "summary": "Recipient set and amount converted to wei. Ready to execute transfer."}
```

---

## Task 3: Execute the transfer

**Exactly 2 tool calls, SEQUENTIALLY (one at a time, NOT in parallel):**

### 3a. Create the ETH transfer transaction (FIRST call)

```json
{"tool": "send_eth", "network": "<network>"}
```

The tool reads `send_to` and `amount_raw` from registers automatically. Gas is auto-estimated.

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
