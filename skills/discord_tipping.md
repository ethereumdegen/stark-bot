---
name: discord_tipping
description: "Tip Discord users with tokens. Resolves Discord mentions to wallet addresses and executes ERC20 transfers."
version: 1.1.0
author: starkbot
metadata: {"clawdbot":{"emoji":"💸"}}
tags: [discord, tipping, crypto, transfer, erc20]
requires_tools: [discord_resolve_user, token_lookup, to_raw_amount, web3_function_call, list_queued_web3_tx, broadcast_web3_tx]
---

# Discord Tipping

Send tokens to Discord users by resolving their mention to a registered wallet address.

## Quick Start

When a user says "tip @someone X TOKEN", follow these 4 steps in order:

1. **Resolve the mention** → Get wallet address
2. **Look up the token** → Get contract address and decimals
3. **Convert amount** → Human readable to raw units
4. **Transfer** → Execute the ERC20 transfer

**Amount shorthand:** Users can use "k" for thousands (1k = 1,000) and "m" for millions (1m = 1,000,000). For example: "tip @user 5k STARKBOT" means 5,000 tokens.

## Step 1: Resolve Discord Mention

Extract the Discord user ID from the mention and resolve it to a wallet address:

```tool:discord_resolve_user
user_mention: "1234567890"
```

**Note:** Pass the numeric user ID, not the raw mention format. Extract the numbers from mentions like `<@1234567890>`.

- If `registered: true` → proceed with the address
- If `registered: false` → tell user they need to register with `@starkbot register 0x...`

## Step 2: Look Up Token

```tool:token_lookup
symbol: "STARKBOT"
network: base
cache_as: token_address
```

This caches:
- `token_address` → contract address
- `token_address_decimals` → decimals (e.g., 18)

## Step 3: Convert Amount

```tool:to_raw_amount
amount: "1"
cache_as: "transfer_amount"
```

Reads `token_address_decimals` automatically and outputs raw amount.

## Step 4: Transfer

```tool:web3_function_call
abi: erc20
contract: "<token_address from step 2>"
function: transfer
params: ["<wallet_address from step 1>", "<transfer_amount from step 3>"]
network: base
```

## Example: "tip @jimmy 1 STARKBOT"

1. Extract user ID from `<@987654321>` → `987654321`

2. Resolve:
```tool:discord_resolve_user
user_mention: "987654321"
```
→ `{"public_address": "0x04abc...", "registered": true}`

3. Token lookup:
```tool:token_lookup
symbol: "STARKBOT"
network: base
cache_as: token_address
```
→ Address: `0x1234...`, Decimals: 18

4. Convert:
```tool:to_raw_amount
amount: "1"
cache_as: "transfer_amount"
```
→ `1000000000000000000`

5. Transfer:
```tool:web3_function_call
abi: erc20
contract: "0x1234..."
function: transfer
params: ["0x04abc...", "1000000000000000000"]
network: base
```

6. Confirm: "Sent 1 STARKBOT to @jimmy!"

## Common Tokens (Base Network)

| Token | Address | Decimals |
|-------|---------|----------|
| USDC | `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913` | 6 |
| WETH | `0x4200000000000000000000000000000000000006` | 18 |
| BNKR | `0x22aF33FE49fD1Fa80c7149773dDe5890D3c76F3b` | 18 |

For other tokens, use `token_lookup` to get the address.
