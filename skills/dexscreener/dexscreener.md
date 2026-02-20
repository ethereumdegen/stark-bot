---
name: dexscreener
description: "Get DEX token prices, pair info, and liquidity data from DexScreener"
version: 1.1.0
author: starkbot
homepage: https://docs.dexscreener.com/api/reference
metadata: {"clawdbot":{"emoji":"📈"}}
requires_tools: [dexscreener]
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

Use the `dexscreener` tool to get real-time DEX trading data across all major chains.

## IMPORTANT: Avoid Paid Promotions

**DO NOT use the `boosted` action unless the user explicitly asks for paid promotions.**

When users ask for "trending", "hot", or "popular" tokens, they want tokens with real trading activity - NOT paid advertisements. Use the `search` action instead and evaluate results by:
- High 24h volume
- High liquidity
- High transaction counts
- Significant price movement

The `boosted` action shows tokens that PAID DexScreener for visibility. These are often scams or low-quality projects trying to attract attention.

---

## Tool Actions

### 1. Search for Tokens (PRIMARY ACTION)

Use this for most queries including "trending" requests:

```json
{"tool": "dexscreener", "action": "search", "query": "PEPE"}
```

```json
{"tool": "dexscreener", "action": "search", "query": "0x6982508145454ce325ddbe47a25d4ec3d2311933"}
```

For "trending on Base" type requests, search for popular tokens on that chain and filter by volume/liquidity.

### 2. Get Token by Address

Get all trading pairs for a specific token:

```json
{"tool": "dexscreener", "action": "token", "chain": "base", "address": "0x532f27101965dd16442e59d40670faf5ebb142e4"}
```

### 3. Get Pair/Pool Info

Get details for a specific liquidity pool:

```json
{"tool": "dexscreener", "action": "pair", "chain": "ethereum", "address": "0x..."}
```

### 4. Boosted Tokens (ONLY IF EXPLICITLY REQUESTED)

⚠️ **Only use this if the user specifically asks for "boosted", "promoted", or "paid promotion" tokens.**

```json
{"tool": "dexscreener", "action": "boosted", "chain": "base"}
```

This shows tokens that paid for visibility. Always warn users these are paid ads, not organic trends.

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

The tool returns formatted data including:

- **Price** - Current USD price with 24h change %
- **MCap** - Market capitalization
- **Liquidity** - Total liquidity in USD (important for slippage)
- **24h Vol** - Trading volume (key indicator of real activity!)
- **24h Txns** - Buy/sell transaction counts
- **Token address** - Contract address
- **DexScreener URL** - Link to chart

---

## Common Workflows

### Check Token Price

User asks: "What's the price of PEPE?"

```json
{"tool": "dexscreener", "action": "search", "query": "PEPE"}
```

Report the price, 24h change, and liquidity from the top result.

### Research a Token Address

User provides a contract address:

```json
{"tool": "dexscreener", "action": "token", "chain": "base", "address": "0x..."}
```

Check:
- Is there liquidity? (>$50K is safer)
- Trading activity (buys vs sells)
- Price volatility

### Find Trending/Hot Tokens

User asks: "What's trending on Base?" or "Show me hot tokens"

**DO NOT use the boosted action!** Instead, search for known active tokens and evaluate by metrics:

```json
{"tool": "dexscreener", "action": "search", "query": "base meme"}
```

Or search for specific categories the user might be interested in. Report tokens with:
- Highest 24h volume
- Strong liquidity (>$50K)
- Active trading (many transactions)

Explain that you're showing tokens with real trading activity, not paid promotions.

---

## Tips

1. **Multiple pairs** - Tokens often have multiple pools; the tool shows the top ones sorted by liquidity
2. **Low liquidity warning** - If liquidity is under $10K, warn user about high slippage
3. **Chain matters** - Same token name can exist on different chains; verify the chain
4. **Search is fuzzy** - Works with partial matches and addresses
5. **Never trust "boosted"** - Paid promotions are NOT an indicator of quality or legitimacy
