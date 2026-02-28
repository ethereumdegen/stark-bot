# Pool Research

Research Uniswap pool metrics — TVL, APY, volume, fee tiers, and liquidity depth.

## No tasks needed — direct tool calls

Pool research is read-only. No `define_tasks` needed.

---

## Method 1: Pool data via DexScreener

### Search for pools by token

```json
{
  "tool": "web_fetch",
  "url": "https://api.dexscreener.com/token-pairs/v1/<network>/<token_address>",
  "method": "GET",
  "extract_mode": "raw"
}
```

Returns all trading pairs for that token, including:
- `pairAddress`, `dexId` (uniswap_v3, uniswap_v2, etc.)
- `priceUsd`, `priceNative`
- `txns.h24.buys`, `txns.h24.sells`
- `volume.h24`, `priceChange.h24`
- `liquidity.usd`, `liquidity.base`, `liquidity.quote`

### Search for a specific pair

```json
{
  "tool": "web_fetch",
  "url": "https://api.dexscreener.com/latest/dex/search?q=<token0>%20<token1>",
  "method": "GET",
  "extract_mode": "raw"
}
```

---

## Method 2: Pool yields via DefiLlama

Get APY, TVL, and volume data from DefiLlama:

```json
{
  "tool": "web_fetch",
  "url": "https://yields.llama.fi/pools",
  "method": "GET",
  "extract_mode": "raw"
}
```

This returns ALL pools across DeFi. Filter by:
- `project`: `"uniswap-v3"`, `"uniswap-v2"`, `"uniswap-v4"`
- `chain`: `"Ethereum"`, `"Base"`, `"Arbitrum"`, `"Optimism"`, etc.
- `symbol`: e.g., `"USDC-WETH"`

Key fields:
- `apy` — current annualized yield (%)
- `apyBase` — base APY from trading fees
- `apyReward` — reward token APY (if any)
- `tvlUsd` — total value locked in USD
- `volumeUsd1d` — 24h volume in USD
- `volumeUsd7d` — 7d volume in USD

**Tip:** The DefiLlama response is large. Use web_fetch with a filter or post-process to find the pools you need.

---

## Method 3: On-chain pool state (Uniswap V4)

Read pool state directly from the V4 StateView contract:

### Get slot0 (current price, tick, fees)

```json
{
  "tool": "web3_function_call",
  "abi": "uniswap_v4_state_view",
  "contract": "0xa3c0c9b65bad0b08107aa264b0f3db444b867a71",
  "function": "getSlot0",
  "params": ["<pool_id>"],
  "call_only": true
}
```

Returns: `sqrtPriceX96`, `tick`, `protocolFee`, `lpFee`

### Get pool liquidity

```json
{
  "tool": "web3_function_call",
  "abi": "uniswap_v4_state_view",
  "contract": "0xa3c0c9b65bad0b08107aa264b0f3db444b867a71",
  "function": "getLiquidity",
  "params": ["<pool_id>"],
  "call_only": true
}
```

### Price calculation from sqrtPriceX96

```
price = (sqrtPriceX96 / 2^96)^2
```

Adjust for decimal differences between token0 and token1.

---

## Uniswap Fee Tiers

| Fee Tier | Fee (%) | Tick Spacing | Best For |
|----------|---------|-------------|----------|
| 0.01% | 100 | 1 | Stablecoins (USDC/USDT) |
| 0.05% | 500 | 10 | Stable pairs, high volume |
| 0.30% | 3000 | 60 | Most pairs, standard |
| 1.00% | 10000 | 200 | Exotic/volatile pairs |

---

## Uniswap LP Deep Link

After researching, generate a deep link for the user to add liquidity:

```
https://app.uniswap.org/positions/create?currencyA=<token0>&currencyB=<token1>&chain=<chain>&fee=<fee_tier>&step=1
```

**Fee tier values for deep links:** `100`, `500`, `3000`, `10000`

**Example:**
```
https://app.uniswap.org/positions/create?currencyA=ETH&currencyB=0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48&chain=ethereum&fee=500&step=1
```

---

## Reporting

When reporting pool research, include:
- Pool pair (e.g., WETH/USDC)
- Chain and protocol version (V2/V3/V4)
- Fee tier
- TVL (USD)
- 24h and 7d volume
- APY (base fees + rewards if any)
- Current price and tick
- Liquidity depth assessment (shallow/moderate/deep)
- Deep link for LP or swap action
