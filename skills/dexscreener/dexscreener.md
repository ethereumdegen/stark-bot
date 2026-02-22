---
name: dexscreener
description: "Get DEX token prices, pair info, and liquidity data from DexScreener"
version: 1.2.0
author: starkbot
homepage: https://docs.dexscreener.com/api/reference
metadata: {"clawdbot":{"emoji":"📈"}}
requires_tools: [run_skill_script]
requires_binaries: [python3]
scripts: [dexscreener.py]
tags: [crypto, dex, price, token, liquidity, trading, defi, market-data]
arguments:
  query:
    description: "Search query (token name, symbol, or address)"
    required: false
  chain:
    description: "Chain (ethereum, base, solana, bsc, polygon, arbitrum, etc.)"
    required: false
  address:
    description: "Token or pair contract address"
    required: false
---

# DexScreener Market Data

Use `run_skill_script` with `dexscreener.py` to get real-time DEX trading data across all major chains.

## IMPORTANT: Avoid Paid Promotions

**DO NOT use the `boosted` action unless the user explicitly asks for paid promotions.**

When users ask for "trending", "hot", or "popular" tokens, they want tokens with real trading activity - NOT paid advertisements. Use the `search` action instead and evaluate results by:
- High 24h volume
- High liquidity
- High transaction counts
- Significant price movement

The `boosted` action shows tokens that PAID DexScreener for visibility. These are often scams or low-quality projects trying to attract attention.

---

## Actions

### 1. Search for Tokens (PRIMARY ACTION)

Use this for most queries including "trending" requests:

```json
{"tool": "run_skill_script", "script": "dexscreener.py", "action": "search", "args": {"query": "PEPE"}}
```

```json
{"tool": "run_skill_script", "script": "dexscreener.py", "action": "search", "args": {"query": "0x6982508145454ce325ddbe47a25d4ec3d2311933"}}
```

### 2. Get Token by Address

```json
{"tool": "run_skill_script", "script": "dexscreener.py", "action": "token", "args": {"chain": "base", "address": "0x532f27101965dd16442e59d40670faf5ebb142e4"}}
```

### 3. Get Pair/Pool Info

```json
{"tool": "run_skill_script", "script": "dexscreener.py", "action": "pair", "args": {"chain": "ethereum", "address": "0x..."}}
```

### 4. Boosted Tokens (ONLY IF EXPLICITLY REQUESTED)

Only use this if the user specifically asks for "boosted", "promoted", or "paid promotion" tokens.

```json
{"tool": "run_skill_script", "script": "dexscreener.py", "action": "boosted", "args": {"chain": "base"}}
```

---

## Supported Chains

| Chain | ID |
|-------|-----|
| Ethereum | `ethereum` |
| Base | `base` |
| Solana | `solana` |
| BSC | `bsc` |
| Polygon | `polygon` |
| Arbitrum | `arbitrum` |
| Optimism | `optimism` |
| Avalanche | `avalanche` |

---

## Understanding the Output

- **Price** - Current USD price with 24h change %
- **MCap** - Market capitalization
- **Liquidity** - Total liquidity in USD (important for slippage)
- **24h Vol** - Trading volume (key indicator of real activity!)
- **24h Txns** - Buy/sell transaction counts
- **Token address** - Contract address
- **DexScreener URL** - Link to chart

---

## Tips

1. **Multiple pairs** - Tokens often have multiple pools; the script shows the top ones sorted by liquidity
2. **Low liquidity warning** - If liquidity is under $10K, warn user about high slippage
3. **Chain matters** - Same token name can exist on different chains; verify the chain
4. **Search is fuzzy** - Works with partial matches and addresses
5. **Never trust "boosted"** - Paid promotions are NOT an indicator of quality or legitimacy
