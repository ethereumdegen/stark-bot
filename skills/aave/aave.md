---
name: aave
description: "Aave V3 DeFi lending — view positions across chains, find best yields, supply, borrow, withdraw, repay. Powered by PayToll."
version: 3.0.0
author: starkbot
homepage: https://aave.com
metadata: {"requires_auth": false, "clawdbot":{"emoji":"👻"}}
requires_tools: [x402_post, web_fetch, token_lookup, to_raw_amount, web3_preset_function_call, web3_function_call, broadcast_web3_tx, verify_tx_broadcast, select_web3_network, define_tasks]
tags: [crypto, defi, finance, lending, aave, yield, apy, borrow, collateral, multichain, paytoll]
---

# Aave V3 — Multi-Chain Lending & Borrowing

Supply tokens for yield, borrow against collateral, check positions across Ethereum, Base, Arbitrum, Optimism, Polygon, and Avalanche. Market data powered by [PayToll](https://paytoll.io).

## CRITICAL RULES

1. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task. Do NOT work ahead.
2. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.**
3. **Sequential tool calls only.** Never call two tools in parallel when the second depends on the first.
4. **Health Factor Safety**: ALWAYS check health factor before borrowing or withdrawing collateral. Never allow HF < 1.5.

## PayToll API Reference

All reads go through PayToll (`https://api.paytoll.io`). Use `x402_post` for paid endpoints.

| Endpoint | Cost | Purpose |
|----------|------|---------|
| `/v1/aave/user-positions` | $0.01 | All positions across chains |
| `/v1/aave/health-factor` | $0.005 | Liquidation risk for a chain |
| `/v1/aave/markets` | $0.005 | Market overview & APY rates |
| `/v1/aave/best-yield` | $0.01 | Best supply APY across chains |
| `/v1/aave/best-borrow` | $0.01 | Lowest borrow APR across chains |

**Chain IDs**: 1 (Ethereum), 8453 (Base), 42161 (Arbitrum), 10 (Optimism), 137 (Polygon), 43114 (Avalanche)

## Aave Pool Contracts (for on-chain writes)

| Chain | Pool Address |
|-------|-------------|
| Base | `0xA238Dd80C259a72e81d7e4664a9801593F98d1c5` |

---

## Operation A: View Positions Across All Chains

Shows all supplied and borrowed assets across every chain.

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/user-positions", "body": {"userAddress": "<WALLET_ADDRESS>"}}
```

Read `wallet_address` from registers for the user's address.

### Present to user

```
👻 Your Aave V3 Positions

[For each chain with a position:]
━━━ [Chain Name] ━━━
  Supplied: [asset] — $X,XXX.XX (APY X.XX%)
  Borrowed: [asset] — $XXX.XX (APR X.XX%)
  Health Factor: X.XX [Safe/Caution/Danger]

Total Supplied: $XX,XXX.XX
Total Borrowed: $X,XXX.XX
```

---

## Operation B: Check Health Factor

Check liquidation risk on a specific chain.

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/health-factor", "body": {"userAddress": "<WALLET_ADDRESS>", "chainId": 8453}}
```

### Health Factor Guide

| HF | Status |
|----|--------|
| > 2.0 | Safe |
| 1.5 – 2.0 | Safe |
| 1.2 – 1.5 | Caution — monitor closely |
| 1.0 – 1.2 | Danger — high liquidation risk |
| < 1.0 | Liquidation possible |

---

## Operation C: Find Best Yield

Find the best supply APY for an asset across all chains.

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/best-yield", "body": {"asset": "USDC"}}
```

To narrow to specific chains:

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/best-yield", "body": {"asset": "ETH", "chainIds": [8453, 42161, 10]}}
```

Present as a ranked table:

```
🏆 Best USDC Supply Yields

 # │ Chain     │  APY   │ Liquidity
───┼───────────┼────────┼──────────
 1 │ Optimism  │ 4.21%  │ $42.5M
 2 │ Base      │ 3.87%  │ $38.1M
 3 │ Arbitrum  │ 3.52%  │ $55.2M
```

---

## Operation D: Find Cheapest Borrow

Find the lowest variable borrow APR for an asset.

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/best-borrow", "body": {"asset": "USDC"}}
```

---

## Operation E: Markets Overview

Get a snapshot of all Aave V3 markets.

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/markets", "body": {"topAssetsCount": 5}}
```

For a single chain:

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/markets", "body": {"chainIds": [8453], "topAssetsCount": 10}}
```

---

## Operation F: Supply Assets to Aave (Base)

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Check position & validate: query PayToll for health factor, look up token, check balance, check allowance.",
  "TASK 2 — Approve Aave Pool (SKIP if allowance sufficient): approve token, broadcast, wait.",
  "TASK 3 — Supply: convert amount, call aave_supply preset, broadcast, verify."
]}
```

### Task 1: Prepare

#### 1a. Select network

```json
{"tool": "select_web3_network", "network": "base"}
```

#### 1b. Check current position via PayToll

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/health-factor", "body": {"userAddress": "<WALLET_ADDRESS>", "chainId": 8453}}
```

#### 1c. Look up token

```json
{"tool": "token_lookup", "symbol": "USDC", "cache_as": "token_address"}
```

#### 1d. Check token balance

```tool:web3_preset_function_call
preset: erc20_balance
network: base
call_only: true
```

#### 1e. Check Aave Pool allowance

```tool:web3_preset_function_call
preset: aave_allowance_pool
network: base
call_only: true
```

Report balance, allowance status, and current position. Complete task.

---

### Task 2: Approve Token for Aave Pool

**If allowance sufficient, SKIP:**

```json
{"tool": "task_fully_completed", "summary": "Allowance already sufficient — skipping approval."}
```

**Otherwise:**

```tool:web3_preset_function_call
preset: aave_approve_pool
network: base
```

Broadcast and wait:

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_approve>"}
```

---

### Task 3: Supply Token

#### 3a. Convert amount to raw units

For USDC (6 decimals):
```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals": 6, "cache_as": "aave_supply_amount"}
```

For WETH (18 decimals):
```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals": 18, "cache_as": "aave_supply_amount"}
```

#### 3b. Execute supply

```tool:web3_preset_function_call
preset: aave_supply
network: base
```

#### 3c. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_supply>"}
```

#### 3d. Verify

```json
{"tool": "verify_tx_broadcast"}
```

Report: "Supplied [amount] [symbol] to Aave on Base."

---

## Operation G: Borrow Assets (Base)

**CRITICAL**: Always check health factor before borrowing.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Safety check: query PayToll for health factor and available borrows.",
  "TASK 2 — Borrow: look up asset, convert amount, call borrow via web3_function_call, broadcast, verify."
]}
```

### Task 1: Safety Check

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/health-factor", "body": {"userAddress": "<WALLET_ADDRESS>", "chainId": 8453}}
```

Also check positions:

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/user-positions", "body": {"userAddress": "<WALLET_ADDRESS>", "chainIds": [8453]}}
```

**Safety checks:**
- If HF < 1.5: warn that borrowing is risky
- If requested amount exceeds available borrows: block
- If projected HF after borrow < 1.5: warn

---

### Task 2: Execute Borrow

#### 2a. Look up asset

```json
{"tool": "token_lookup", "symbol": "USDC", "cache_as": "token_address"}
```

#### 2b. Convert amount

```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals": 6, "cache_as": "borrow_amount_raw"}
```

#### 2c. Execute borrow

Read `token_address` and `borrow_amount_raw` from registers, and `wallet_address` from registers:

```json
{"tool": "web3_function_call", "abi": "aave_pool", "contract": "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5", "function": "borrow", "params": ["<token_address>", "<borrow_amount_raw>", "2", "0", "<wallet_address>"], "network": "base"}
```

**Note:** `"2"` = variable interest rate mode, `"0"` = no referral.

#### 2d. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_borrow>"}
```

#### 2e. Verify

```json
{"tool": "verify_tx_broadcast"}
```

#### 2f. Check updated position

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/health-factor", "body": {"userAddress": "<WALLET_ADDRESS>", "chainId": 8453}}
```

Report updated health factor.

---

## Operation H: Withdraw from Aave (Base)

**CRITICAL**: If you have borrows, withdrawing collateral can cause liquidation.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Safety check: query PayToll for positions and health factor.",
  "TASK 2 — Withdraw: look up token, convert amount, call aave_withdraw preset, broadcast, verify."
]}
```

### Task 1: Safety Check

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/user-positions", "body": {"userAddress": "<WALLET_ADDRESS>", "chainIds": [8453]}}
```

If user has debt, also check health factor and verify withdrawal won't drop HF below 1.5.

---

### Task 2: Execute Withdrawal

#### 2a. Look up token

```json
{"tool": "token_lookup", "symbol": "USDC", "cache_as": "token_address"}
```

#### 2b. Convert amount

For specific amount:
```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals": 6, "cache_as": "aave_withdraw_amount"}
```

To withdraw ALL (max uint256):
```json
{"tool": "to_raw_amount", "amount": "115792089237316195423570985008687907853269984665640564039457584007913129639935", "decimals": 0, "cache_as": "aave_withdraw_amount"}
```

#### 2c. Execute withdraw

```tool:web3_preset_function_call
preset: aave_withdraw
network: base
```

#### 2d. Broadcast + Verify

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_withdraw>"}
```

```json
{"tool": "verify_tx_broadcast"}
```

---

## Operation I: Repay Borrowed Assets (Base)

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Check debt: query PayToll for positions, check token balance, check allowance.",
  "TASK 2 — Approve (SKIP if sufficient): approve token for Aave Pool.",
  "TASK 3 — Repay: convert amount, call repay via web3_function_call, broadcast, verify."
]}
```

### Task 1: Prepare

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/user-positions", "body": {"userAddress": "<WALLET_ADDRESS>", "chainIds": [8453]}}
```

If no debt: "You have no outstanding debt on Aave!" — skip remaining tasks.

Check balance and allowance (same as Supply Task 1c–1e).

---

### Task 2: Approve (if needed)

Same as Supply Task 2.

---

### Task 3: Execute Repay

#### 3a. Convert amount

For specific amount:
```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals": 6, "cache_as": "repay_amount_raw"}
```

To repay ALL (max uint256):
```json
{"tool": "to_raw_amount", "amount": "115792089237316195423570985008687907853269984665640564039457584007913129639935", "decimals": 0, "cache_as": "repay_amount_raw"}
```

#### 3b. Execute repay

Read `token_address`, `repay_amount_raw`, and `wallet_address` from registers:

```json
{"tool": "web3_function_call", "abi": "aave_pool", "contract": "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5", "function": "repay", "params": ["<token_address>", "<repay_amount_raw>", "2", "<wallet_address>"], "network": "base"}
```

**Note:** `"2"` = variable interest rate mode.

#### 3c. Broadcast + Verify

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_repay>"}
```

```json
{"tool": "verify_tx_broadcast"}
```

#### 3d. Check updated position

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/aave/health-factor", "body": {"userAddress": "<WALLET_ADDRESS>", "chainId": 8453}}
```

---

## Error Handling

| Error | Solution |
|-------|----------|
| Insufficient balance | Check balance first, reduce amount |
| Insufficient gas | Need ETH on Base for gas fees |
| Allowance too low | Run approval task first |
| HF too low | Supply more collateral or repay debt |
| Reserve frozen | Wait or use different asset |

---

## Quick Reference

**Supply → Earn**:  Deposit asset → receive aToken → earn yield automatically
**Borrow**: Requires collateral → pay variable interest → keep HF > 1.5
**Withdraw**: Redeem aToken → get asset + interest (check HF if borrowing)
**Repay**: Pay back debt → frees collateral → improves HF
