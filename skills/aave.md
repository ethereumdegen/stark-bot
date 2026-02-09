---
name: aave
description: "Lend, borrow, and earn yield on Aave V3 (Base) — supply USDC for APY, borrow against collateral, check positions, withdraw, repay debt."
version: 2.0.0
author: starkbot
homepage: https://aave.com
metadata: {"requires_auth": false, "clawdbot":{"emoji":"👻"}}
requires_tools: [token_lookup, to_raw_amount, web3_preset_function_call, list_queued_web3_tx, broadcast_web3_tx, verify_tx_broadcast, select_web3_network, define_tasks, web_fetch, set_address]
tags: [crypto, defi, finance, lending, aave, yield, apy, base, usdc, borrow, collateral]
---

# Aave V3 Lending & Borrowing on Base

Supply USDC (or other tokens) to Aave V3 on Base to earn yield, borrow against your collateral, check your lending positions, withdraw, and repay debt.

## CRITICAL RULES

1. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task. Do NOT work ahead.
2. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.**
3. **Sequential tool calls only.** Never call two tools in parallel when the second depends on the first.
4. **Health Factor Safety**: ALWAYS check health factor before borrowing or withdrawing collateral. Never allow HF < 1.5.

## Key Addresses (Base)

| Contract | Address |
|----------|---------|
| Aave V3 Pool | `0xA238Dd80C259a72e81d7e4664a9801593F98d1c5` |
| Pool Data Provider | `0xd82a47fdebB5bf5329b09441C3DaB4b5df2153Ad` |
| USDC | `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913` |
| WETH | `0x4200000000000000000000000000000000000006` |
| USDbC | `0xd9aAEc86B65D86f6A7B5B1b0c42FFA531710b6CA` |
| cbETH | `0x2Ae3F1Ec7F1F5012CFEab0185bfc7aa3cf0DEc22` |
| aBasUSDC (aToken) | `0x4e65fE4DbA92790696d040ac24Aa414708F5c0AB` |

## Understanding Aave

### How It Works
1. **Supply (Deposit)**: Deposit assets to earn interest. Receive aTokens (e.g., aUSDC) that grow in value.
2. **Borrow**: Use supplied assets as collateral to borrow other assets. Pay interest on borrowed amount.
3. **Health Factor**: Ratio of your collateral to debt. **Must stay above 1.0** to avoid liquidation.
4. **Withdraw**: Redeem aTokens for underlying asset + interest (if not being used as active collateral).
5. **Repay**: Pay back borrowed assets to reduce debt and free up collateral.

### Health Factor Rules
- **HF > 2.0**: ✅ Very Safe
- **HF 1.5-2.0**: ✅ Safe  
- **HF 1.2-1.5**: ⚠️ Caution - monitor closely
- **HF 1.0-1.2**: ⚠️ Danger - high liquidation risk
- **HF < 1.0**: ❌ Can be liquidated

**Formula**: HF = (Total Collateral × Liquidation Threshold) / Total Debt

---

## Operation A: View Complete Aave Position

Shows everything: supplied assets, borrowed assets, health factor, available borrow capacity.

### Use the preset

```tool:web3_preset_function_call
preset: aave_get_user_account_data
network: base
call_only: true
```

This returns:
- `totalCollateralBase`: Total collateral in USD (base units, 8 decimals)
- `totalDebtBase`: Total debt in USD (base units, 8 decimals)
- `availableBorrowsBase`: How much more you can borrow in USD (8 decimals)
- `currentLiquidationThreshold`: Weighted average liquidation threshold
- `ltv`: Loan-to-value ratio
- `healthFactor`: Position health (18 decimals, divide by 1e18 to get actual HF)

### Present to user

```markdown
📊 **Your Aave Position on Base:**

💰 Supplied (Collateral): $X,XXX.XX
📉 Borrowed (Debt): $XXX.XX
📈 Available to Borrow: $XXX.XX
🏥 Health Factor: X.XX [Safe ✅ / Caution ⚠️ / Danger ❌]

Liquidation Threshold: XX%
Current LTV: XX%
```

If health factor < 1.5, add warning:
```
⚠️ WARNING: Your health factor is low. Consider repaying debt or adding collateral to avoid liquidation.
```

---

## Operation B: Check Current APY

Use DeFi Llama to get current USDC supply APY on Aave V3 Base:

```tool:web_fetch
url: https://yields.llama.fi/pools
method: GET
extract_mode: raw
```

From the response, filter for pools where `project` is `"aave-v3"` and `chain` is `"Base"` and `symbol` contains `"USDC"`. Report the `apy` field as the current supply APY and `apyBorrow` for borrow rate.

---

## Operation C: Check Supplied Balance (Specific Asset)

To check how much the user has supplied to Aave for a specific asset (e.g., USDC), query the aToken balance:

### 1. Look up the underlying token

```json
{"tool": "token_lookup", "symbol": "USDC", "cache_as": "token_address"}
```

### 2. Set the aToken address

For USDC on Base:
```json
{"tool": "set_address", "register": "token_address", "address": "0x4e65fE4DbA92790696d040ac24Aa414708F5c0AB"}
```

### 3. Check aToken balance

```tool:web3_preset_function_call
preset: erc20_balance
network: base
call_only: true
```

The aToken balance represents the user's supplied amount plus accrued interest. USDC has 6 decimals, so divide the raw value by 1e6 for the human-readable amount.

---

## Operation D: Supply Assets to Aave (Enter Position)

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select Base network, look up token, check balance, check Aave Pool allowance.",
  "TASK 2 — Approve Aave Pool (SKIP if allowance sufficient): approve token for Aave Pool, broadcast, wait.",
  "TASK 3 — Supply: convert amount, call aave_supply preset, broadcast, verify."
]}
```

### Task 1: Prepare — look up token, check balance, check allowance

#### 1a. Select network

```json
{"tool": "select_web3_network", "network": "base"}
```

#### 1b. Look up token (e.g., USDC)

```json
{"tool": "token_lookup", "symbol": "USDC", "cache_as": "token_address"}
```

#### 1c. Check token balance

```tool:web3_preset_function_call
preset: erc20_balance
network: base
call_only: true
```

#### 1d. Check Aave Pool allowance

```tool:web3_preset_function_call
preset: aave_allowance_pool
network: base
call_only: true
```

#### 1e. Report and complete

Tell the user their balance and whether approval is needed. Complete with `finished_task: true`.

---

### Task 2: Approve Token for Aave Pool

**If Task 1 determined allowance is already sufficient, SKIP:**

```json
{"tool": "task_fully_completed", "summary": "Allowance already sufficient — skipping approval."}
```

**Otherwise, approve:**

```tool:web3_preset_function_call
preset: aave_approve_pool
network: base
```

Broadcast and wait for confirmation:

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_approve>"}
```

After confirmed:

```json
{"tool": "task_fully_completed", "summary": "Token approved for Aave V3 Pool. Ready to supply."}
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

Report result: "✅ Successfully supplied [amount] [symbol] to Aave! You'll start earning [APY]% APY."

Complete task.

---

## Operation E: Borrow Assets (Enter Position - Borrow)

**CRITICAL**: Before borrowing, ALWAYS check the user's position and calculate projected health factor.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Safety Check: select Base network, check current position and health factor.",
  "TASK 2 — Borrow: look up asset, convert amount, call aave_borrow preset, broadcast, verify."
]}
```

### Task 1: Safety Check

#### 1a. Select network

```json
{"tool": "select_web3_network", "network": "base"}
```

#### 1b. Get current position

```tool:web3_preset_function_call
preset: aave_get_user_account_data
network: base
call_only: true
```

#### 1c. Calculate and warn

Extract:
- Current health factor (divide healthFactor by 1e18)
- Available to borrow (availableBorrowsBase / 1e8 for USD)
- Current collateral and debt

**Safety checks:**
1. If healthFactor < 1.5e18: "⚠️ WARNING: Your health factor is already low. Borrowing is risky."
2. If requested borrow amount > availableBorrowsBase: "❌ ERROR: You can only borrow up to $XXX. Reduce amount or add more collateral."
3. Calculate projected HF after borrow - if would be < 1.5: "⚠️ WARNING: This borrow would drop your health factor to X.XX. Risky!"

Report current position and whether it's safe to proceed. Complete with `finished_task: true`.

---

### Task 2: Execute Borrow

**Only proceed if Task 1 confirmed it's safe!**

#### 2a. Look up asset to borrow

```json
{"tool": "token_lookup", "symbol": "USDC", "cache_as": "token_address"}
```

#### 2b. Convert amount to raw units

For USDC (6 decimals):
```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals": 6, "cache_as": "aave_borrow_amount"}
```

#### 2c. Execute borrow

```tool:web3_preset_function_call
preset: aave_borrow
network: base
```

The preset uses interest rate mode 2 (variable rate) by default, which is recommended.

#### 2d. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_borrow>"}
```

#### 2e. Verify

```json
{"tool": "verify_tx_broadcast"}
```

#### 2f. Check new health factor

After successful borrow, call `aave_get_user_account_data` again to show the updated health factor.

Report result:
```
✅ Successfully borrowed [amount] [symbol]!

📊 Updated Position:
- New Debt: $XXX.XX
- Health Factor: X.XX [status emoji]
- Remaining Borrow Capacity: $XXX.XX
```

Complete task.

---

## Operation F: Withdraw from Aave (Exit Position - Partial)

**CRITICAL**: If user has borrowed assets, withdrawing collateral can cause liquidation. ALWAYS check health factor.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select Base network, check position, verify withdrawal is safe.",
  "TASK 2 — Withdraw: look up token, convert amount, call aave_withdraw, broadcast, verify."
]}
```

### Task 1: Prepare and Safety Check

#### 1a. Select network

```json
{"tool": "select_web3_network", "network": "base"}
```

#### 1b. Check current position

```tool:web3_preset_function_call
preset: aave_get_user_account_data
network: base
call_only: true
```

#### 1c. Check supplied balance for specific asset

```json
{"tool": "token_lookup", "symbol": "USDC", "cache_as": "token_address"}
```

Set aToken address (e.g., for USDC):
```json
{"tool": "set_address", "register": "token_address", "address": "0x4e65fE4DbA92790696d040ac24Aa414708F5c0AB"}
```

```tool:web3_preset_function_call
preset: erc20_balance
network: base
call_only: true
```

#### 1d. Safety checks

If totalDebtBase > 0 (user has borrowed):
- Calculate if withdrawal would drop HF below 1.5
- Warn: "⚠️ You have active borrows. Withdrawing may affect your health factor."
- If projected HF < 1.5: "❌ Cannot withdraw this amount - health factor would drop too low."

Report supplied balance and whether withdrawal is safe. Complete with `finished_task: true`.

---

### Task 2: Execute Withdrawal

#### 2a. Look up token

```json
{"tool": "token_lookup", "symbol": "USDC", "cache_as": "token_address"}
```

#### 2b. Convert amount to raw units

For specific amount (USDC, 6 decimals):
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

#### 2d. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_withdraw>"}
```

#### 2e. Verify

```json
{"tool": "verify_tx_broadcast"}
```

#### 2f. Check new health factor (if applicable)

If user has debt, check HF after withdrawal:

```tool:web3_preset_function_call
preset: aave_get_user_account_data
network: base
call_only: true
```

Report result:
```
✅ Successfully withdrew [amount] [symbol] from Aave!

[If has debt:]
📊 Updated Health Factor: X.XX [status emoji]
```

Complete task.

---

## Operation G: Repay Borrowed Assets (Exit Position - Debt)

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select Base network, check debt balance, check repayment token balance, check allowance.",
  "TASK 2 — Approve (SKIP if sufficient): approve token for Aave Pool, broadcast, wait.",
  "TASK 3 — Repay: convert amount, call aave_repay preset, broadcast, verify."
]}
```

### Task 1: Prepare

#### 1a. Select network

```json
{"tool": "select_web3_network", "network": "base"}
```

#### 1b. Check current debt

```tool:web3_preset_function_call
preset: aave_get_user_account_data
network: base
call_only: true
```

Report total debt. If totalDebtBase is 0:
```
✅ You don't have any outstanding debt on Aave!
```

Then complete and skip remaining tasks.

#### 1c. Look up repayment token

```json
{"tool": "token_lookup", "symbol": "USDC", "cache_as": "token_address"}
```

#### 1d. Check token balance

```tool:web3_preset_function_call
preset: erc20_balance
network: base
call_only: true
```

Verify user has enough to repay. If not: "❌ Insufficient balance. You have [X] but need [Y] to repay."

#### 1e. Check allowance

```tool:web3_preset_function_call
preset: aave_allowance_pool
network: base
call_only: true
```

Report debt amount and whether approval needed. Complete with `finished_task: true`.

---

### Task 2: Approve (if needed)

**If allowance is sufficient, SKIP:**

```json
{"tool": "task_fully_completed", "summary": "Allowance sufficient — skipping approval."}
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

Complete task.

---

### Task 3: Repay

#### 3a. Convert amount to raw units

For specific amount (USDC, 6 decimals):
```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals": 6, "cache_as": "aave_repay_amount"}
```

To repay ALL debt (max uint256):
```json
{"tool": "to_raw_amount", "amount": "115792089237316195423570985008687907853269984665640564039457584007913129639935", "decimals": 0, "cache_as": "aave_repay_amount"}
```

#### 3b. Execute repay

```tool:web3_preset_function_call
preset: aave_repay
network: base
```

The preset uses interest rate mode 2 (variable) by default.

#### 3c. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_repay>"}
```

#### 3d. Verify

```json
{"tool": "verify_tx_broadcast"}
```

#### 3e. Check updated position

```tool:web3_preset_function_call
preset: aave_get_user_account_data
network: base
call_only: true
```

Report result:
```
✅ Successfully repaid [amount] [symbol]!

📊 Updated Position:
- Remaining Debt: $XXX.XX [or "Debt fully repaid! ✅"]
- Health Factor: X.XX [improved! or N/A if no debt]
- Available Collateral to Withdraw: $XXX.XX
```

Complete task.

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| Insufficient balance | Not enough tokens to supply/repay | Check balance first, reduce amount |
| Insufficient gas | Not enough ETH for gas | Need ETH on Base for gas fees |
| Allowance too low | Token not approved for Pool | Run approval task first |
| Insufficient collateral | Trying to borrow more than allowed | Supply more collateral or reduce borrow amount |
| Health factor too low | Withdrawal/borrow would cause liquidation | Reduce operation amount or repay debt first |
| Withdraw exceeds balance | Trying to withdraw more than supplied | Use "withdraw all" or reduce amount |
| Repay exceeds debt | Trying to repay more than owed | Use max uint256 to repay exact debt |
| Reserve frozen | Asset temporarily disabled | Wait or use different asset |

---

## Best Practices

1. **Start Small**: Test with small amounts first ($10-50)
2. **Monitor Health Factor**: Keep HF > 1.5 for safety
3. **Don't Max Out**: Never borrow your full available capacity
4. **Variable Rates**: Use variable (mode 2) for most borrows - rates adjust with market
5. **Check Balances**: Always verify balances before operations
6. **Gas Fees**: Keep some ETH on Base for transaction fees
7. **Diversify**: Don't put all collateral in one asset
8. **Market Volatility**: Be extra cautious during high volatility - prices can move fast

---

## Example User Flows

### Flow 1: Earn Yield on USDC
```
User: "Supply 100 USDC to Aave"
→ Check balance ✓
→ Approve Pool ✓
→ Supply 100 USDC ✓
Result: Earning ~2-4% APY, can withdraw anytime
```

### Flow 2: Borrow Against Collateral
```
User: "I have 0.1 ETH in Aave, borrow 50 USDC"
→ Check position (HF, available borrows) ✓
→ Verify safe (HF would stay > 2.0) ✓
→ Borrow 50 USDC ✓
Result: 50 USDC borrowed, HF = 2.5 (safe)
```

### Flow 3: Repay and Exit
```
User: "Repay my USDC loan and withdraw my ETH"
→ Check debt (50.12 USDC including interest) ✓
→ Approve and repay 50.12 USDC ✓
→ Debt cleared, HF now N/A ✓
→ Withdraw 0.1 ETH ✓
Result: Fully exited, no debt, collateral withdrawn
```

---

## How Aave Works (Quick Reference)

**Supply**: 
- Deposit asset → receive aToken (e.g., aUSDC)
- aToken grows in value as interest accrues
- Can be used as collateral for borrowing

**Borrow**:
- Must have supplied collateral first
- Can borrow up to LTV% of collateral value
- Interest accrues on borrowed amount
- Must maintain HF > 1.0

**Health Factor**:
- HF = (Collateral × Liq. Threshold) / Debt
- If HF < 1.0, position can be liquidated
- Liquidators repay debt, take collateral at discount

**Interest Rates**:
- Variable: Changes based on utilization
- Stable: Fixed rate (if available)
- Supply APY < Borrow APY (difference is protocol revenue)

---

## Resources

- **Aave App**: https://app.aave.com/
- **Aave Docs**: https://docs.aave.com/
- **Base Contracts**: https://docs.aave.com/developers/deployed-contracts/v3-mainnet/base
- **DeFi Llama (APY)**: https://defillama.com/protocol/aave-v3
- **Risk Parameters**: https://docs.aave.com/risk/

---

**Version 2.0 Changes:**
- ✅ Added borrow functionality
- ✅ Added repay functionality  
- ✅ Added comprehensive health factor checks
- ✅ Added safety warnings for risky operations
- ✅ Added view complete position operation
- ✅ Added multi-asset support (WETH, cbETH, USDbC)
- ✅ Improved error handling
- ✅ Added example user flows
