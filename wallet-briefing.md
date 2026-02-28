---
name: wallet-briefing
description: "Fetch token balances and USD values for any EVM wallet and generate a concise daily portfolio briefing."
version: 1.0.0
author: starkbot-contributor
requires_tools: [run_skill_script]
requires_binaries: [curl, jq, python3]
scripts: [fetch_balances.sh, format_briefing.py]
requires_api_keys:
  ETHERSCAN_API_KEY:
    description: "Etherscan API key — free at https://etherscan.io/apis"
    secret: true
  COINGECKO_API_KEY:
    description: "CoinGecko API key — free tier works, get one at https://www.coingecko.com/en/api"
    secret: true
    optional: true
tags: [defi, portfolio, wallet, ethereum, briefing, autonomous]
arguments:
  wallet_address:
    description: "EVM wallet address to analyze (0x...)"
    required: true
  chain:
    description: "Chain to query: ethereum, arbitrum, base, optimism (default: ethereum)"
    required: false
  currency:
    description: "Fiat currency for valuation: usd, eur, gbp (default: usd)"
    required: false
  top_n:
    description: "Number of top tokens to include by value (default: 10)"
    required: false
---

# Wallet Briefing Skill

You are generating a **daily wallet portfolio briefing** for the provided address.

## What This Skill Does

When invoked, this skill will:
1. Fetch the native token balance (ETH/MATIC/etc.) and top ERC-20 holdings for the wallet
2. Retrieve current USD prices for each token via CoinGecko
3. Calculate total portfolio value and 24h change where available
4. Format a clean, concise briefing suitable for a morning summary or alert

## How to Invoke

The user or scheduler may say things like:
- "Give me my wallet briefing for 0xabc..."
- "What's my portfolio worth today?"
- "Run the daily briefing"
- "Check my wallet on base"

Extract the wallet address and optional chain/currency/top_n from context, then call the scripts in order.

## Execution Steps

### Step 1 — Fetch balances

Run `fetch_balances.sh` with:
```
WALLET_ADDRESS=<address> CHAIN=<chain> run_skill_script fetch_balances.sh
```

This outputs a JSON file at `/tmp/wallet_balances.json` with token symbols, raw balances, and contract addresses.

### Step 2 — Format the briefing

Run `format_briefing.py` with:
```
WALLET_ADDRESS=<address> CURRENCY=<currency> TOP_N=<n> run_skill_script format_briefing.py
```

This reads `/tmp/wallet_balances.json`, enriches with CoinGecko prices, and prints the final briefing to stdout.

### Step 3 — Present the briefing

Output the briefing text to the user. If a notification channel is configured (e.g. Discord, Telegram), you may also send it there using the appropriate skill.

## Output Format

The briefing will look like:

```
📊 Wallet Briefing — 0xabc...f43c
🕐 2026-02-27 08:00 UTC  |  Chain: Ethereum

💼 Total Portfolio Value: $12,340.55  (▲ +2.3% 24h)

Top Holdings:
  1. ETH      2.450      $6,125.00   (49.6%)
  2. USDC     3,000.00   $3,000.00   (24.3%)
  3. WBTC     0.0412     $2,800.00   (22.7%)
  4. LINK     85.00        $935.00    (7.6%)
  ...

⚠️  Alerts:
  - ETH price up >5% in 24h
  - Low ETH balance (<0.05 ETH) — consider topping up for gas

🔗 View on Etherscan: https://etherscan.io/address/0xabc...f43c
```

## Error Handling

- If the API key is missing or invalid, tell the user to set `ETHERSCAN_API_KEY` and retry.
- If a token price is unavailable, show the balance without USD value and note "price unavailable."
- If the wallet has no ERC-20 holdings, report only the native balance and note "no ERC-20 tokens found."
- On network errors, retry once after 3 seconds before failing gracefully.

## Scheduling

This skill works well as a scheduled task. To run it every morning at 8am UTC:
```
schedule wallet-briefing daily at 08:00 UTC for wallet 0xYOUR_ADDRESS
```

## Composability

After running this skill, you can pipe the output into:
- `notify-discord` skill — to post the briefing to a Discord channel
- `portfolio-rebalance` skill — to trigger rebalancing if allocations drift
- `csv-log` skill — to append daily snapshots to a spreadsheet
