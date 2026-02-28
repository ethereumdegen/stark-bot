# Token Discovery

Find and research tokens using DexScreener, DefiLlama, and web search. Generate Uniswap deep links for quick action.

## No tasks needed — direct tool calls

Token discovery is read-only research. No `define_tasks` needed — just use the tools directly and report findings.

---

## Method 1: Search by keyword (DexScreener)

Search for tokens by name, symbol, or keyword:

```json
{
  "tool": "web_fetch",
  "url": "https://api.dexscreener.com/latest/dex/search?q=<keyword>",
  "method": "GET",
  "extract_mode": "raw"
}
```

The response includes matching pairs with:
- `baseToken.name`, `baseToken.symbol`, `baseToken.address`
- `priceUsd`, `priceChange.h24`, `volume.h24`
- `liquidity.usd`, `fdv`
- `chainId`, `dexId`, `pairAddress`

### Interpret results

- **High volume + high liquidity** = established, safer to trade
- **Low liquidity** = high slippage risk, be cautious
- **New pairs** (recently created) = higher risk, do additional research
- **Multiple DEXes** = more established token

---

## Method 2: Look up by address (DexScreener)

Get pool data for a specific token address:

```json
{
  "tool": "web_fetch",
  "url": "https://api.dexscreener.com/token-pairs/v1/<network>/<token_address>",
  "method": "GET",
  "extract_mode": "raw"
}
```

Network values: `ethereum`, `base`, `arbitrum`, `optimism`, `polygon`, `bsc`, `avalanche`

---

## Method 3: Trending / Boosted tokens (DexScreener)

Get tokens with active promotions (boosted on DexScreener):

```json
{
  "tool": "web_fetch",
  "url": "https://api.dexscreener.com/token-boosts/top/v1",
  "method": "GET",
  "extract_mode": "raw"
}
```

---

## Method 4: Web search for broader research

Use web search for context beyond on-chain data:

```json
{"tool": "web_search", "query": "<token_name> token <chain> site:twitter.com OR site:dexscreener.com"}
```

Look for: project website, team info, audit status, community sentiment, recent news.

---

## Generate Uniswap Deep Link

After discovering a token, generate a ready-to-use swap link:

```
https://app.uniswap.org/swap?chain=<chain>&inputCurrency=<input_address>&outputCurrency=<output_address>&value=<amount>&field=INPUT
```

**Chain values for deep links:** `ethereum`, `base`, `arbitrum`, `optimism`, `polygon`, `bnb`, `avalanche`, `celo`, `blast`, `zora`, `unichain`, `worldchain`

**Example:**
```
https://app.uniswap.org/swap?chain=base&inputCurrency=ETH&outputCurrency=0x587Cd533F418825521f3A1daa7CCd1E7339A1B07&value=0.1&field=INPUT
```

Use `ETH` as the inputCurrency for native ETH (not the WETH address).

---

## Reporting

When reporting discovered tokens, include:
- Token name, symbol, contract address
- Chain and DEX
- Current price (USD)
- 24h price change (%)
- 24h volume
- Liquidity (USD)
- FDV (Fully Diluted Valuation) if available
- Uniswap deep link for quick swap
- Risk warnings for low liquidity or newly created tokens
