---
name: uniswap
description: "Swap tokens via Uniswap Trading API, discover tokens, research pools, and get V4 hooks security guidance"
version: 1.0.0
author: starkbot
homepage: https://app.uniswap.org
tags: [crypto, defi, uniswap, swap, trading-api, pools, hooks, v4, token-discovery]
requires_tools: [token_lookup, to_raw_amount, web_fetch, web3_preset_function_call, broadcast_web3_tx, verify_tx_broadcast, select_web3_network, define_tasks]
arguments:
  action:
    description: "What to do: swap, discover, research, hooks"
    required: false
  token:
    description: "Token symbol or address to act on"
    required: false
  chain:
    description: "Chain to use (ethereum, base, arbitrum, optimism, polygon, unichain)"
    required: false
---

# Uniswap Skill

Interact with the Uniswap ecosystem — swap tokens via the Trading API, discover new tokens, research pool metrics, and get V4 hooks security guidance.

**Uniswap Trading API:** `https://trade-api.gateway.uniswap.org/v1`
**API Key:** `$UNISWAP_API_KEY` (required for Trading API calls)

---

## Supported Chains

| Chain | Chain ID |
|-------|----------|
| Ethereum | 1 |
| Base | 8453 |
| Arbitrum | 42161 |
| Optimism | 10 |
| Polygon | 137 |
| BNB Chain | 56 |
| Avalanche | 43114 |
| Celo | 42220 |
| Blast | 81457 |
| Zora | 7777777 |
| Unichain | 130 |
| World Chain | 480 |

---

## ROUTING: Read the correct flow file FIRST

Determine user intent, then `read_file` the matching flow document **before doing anything else**.

| User Intent | Flow File |
|-------------|-----------|
| "swap tokens" / "trade ETH for USDC" / "buy TOKEN via uniswap" / "sell TOKEN on uniswap" | `read_file` -> `{baseDir}/flows/swap_via_api.md` |
| "find tokens" / "discover memecoins" / "what tokens are trending" / "search for TOKEN" | `read_file` -> `{baseDir}/flows/token_discovery.md` |
| "pool info" / "pool APY" / "what pools exist for TOKEN" / "liquidity stats" / "TVL" | `read_file` -> `{baseDir}/flows/pool_research.md` |
| "v4 hooks" / "hook security" / "build a hook" / "hook permissions" / "beforeSwap" / "afterSwap" | `read_file` -> `{baseDir}/flows/v4_hooks_guide.md` |

**Example:** User says "swap 0.1 ETH for USDC on Base":

```json
{"tool": "read_file", "path": "{baseDir}/flows/swap_via_api.md"}
```

Then follow the instructions in that flow file exactly.

---

## IMPORTANT: This Skill vs Other Skills

- **This skill** uses the **Uniswap Trading API** for swaps (direct Uniswap routing)
- **The `swap` skill** uses the **0x DEX aggregator** (multi-DEX routing via quoter.defirelay.com)
- **The `uniswap_lp` skill** handles **liquidity positions** (create/increase/decrease/collect fees)
- If the user specifically says "uniswap" or wants Uniswap-native routing, use THIS skill
- If the user wants best-price aggregation across DEXes, use the `swap` skill
- If the user wants to provide liquidity, use the `uniswap_lp` skill

---

## Trading API Overview

The Uniswap Trading API provides three endpoints for executing swaps:

| Endpoint | Purpose |
|----------|---------|
| `POST /v1/check_approval` | Check if token spending is approved |
| `POST /v1/quote` | Get optimized swap route and price quote |
| `POST /v1/swap` | Get executable transaction calldata |

**Routing types:** CLASSIC, DUTCH_V2, DUTCH_V3, PRIORITY, WRAP, UNWRAP, BRIDGE

**Permit2:** The Trading API uses Permit2 (`0x000000000022D473030F116dDEE9F6B43aC78BA3`) for token approvals.
