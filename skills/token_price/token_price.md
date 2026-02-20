---
name: token_price
description: "Get cryptocurrency and token prices using CoinGecko (no API key required). Supports price lookup, market data, trending coins, and token search."
version: 1.0.0
author: starkbot
homepage: https://www.coingecko.com/en/api
metadata: {"clawdbot":{"emoji":"💰"}}
tags: [crypto, finance, price, token, coingecko, market]
requires_tools: [exec]
---

# Token Price (CoinGecko)

Free API, no key required. Rate limit: ~30 requests/minute.

## Quick Price Lookup

Single token:
```bash
curl -s "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=usd" | jq
# {"bitcoin":{"usd":42000}}
```

Multiple tokens:
```bash
curl -s "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin,ethereum,solana&vs_currencies=usd" | jq
```

With market cap and 24h change:
```bash
curl -s "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin,ethereum&vs_currencies=usd&include_market_cap=true&include_24hr_change=true&include_24hr_vol=true" | jq
```

## Common Token IDs

| Token | CoinGecko ID |
|-------|--------------|
| Bitcoin | bitcoin |
| Ethereum | ethereum |
| Solana | solana |
| USDC | usd-coin |
| USDT | tether |
| BNB | binancecoin |
| XRP | ripple |
| Cardano | cardano |
| Dogecoin | dogecoin |
| Avalanche | avalanche-2 |
| Polygon | matic-network |
| Chainlink | chainlink |
| Uniswap | uniswap |
| Aave | aave |

## Search for Token ID

If you don't know the CoinGecko ID:
```bash
curl -s "https://api.coingecko.com/api/v3/search?query=starkbot" | jq '.coins[:5]'
```

Returns matching coins with their IDs, symbols, and market cap rank.

## Price by Contract Address

For tokens on specific chains (useful for newer/smaller tokens):
```bash
# Ethereum mainnet
curl -s "https://api.coingecko.com/api/v3/simple/token_price/ethereum?contract_addresses=0x1f9840a85d5af5bf1d1762f925bdaddc4201f984&vs_currencies=usd" | jq

# Base
curl -s "https://api.coingecko.com/api/v3/simple/token_price/base?contract_addresses=0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913&vs_currencies=usd" | jq
```

Supported chains: `ethereum`, `base`, `arbitrum-one`, `optimistic-ethereum`, `polygon-pos`, `avalanche`, `binance-smart-chain`

## Detailed Coin Data

Full market data for a token:
```bash
curl -s "https://api.coingecko.com/api/v3/coins/bitcoin?localization=false&tickers=false&community_data=false&developer_data=false" | jq '{
  price: .market_data.current_price.usd,
  market_cap: .market_data.market_cap.usd,
  volume_24h: .market_data.total_volume.usd,
  change_24h: .market_data.price_change_percentage_24h,
  change_7d: .market_data.price_change_percentage_7d,
  ath: .market_data.ath.usd,
  ath_change: .market_data.ath_change_percentage.usd
}'
```

## Trending Coins

Get trending coins (most searched in last 24h):
```bash
curl -s "https://api.coingecko.com/api/v3/search/trending" | jq '.coins[:7] | .[].item | {name, symbol, market_cap_rank}'
```

## Historical Price

Price at specific date:
```bash
curl -s "https://api.coingecko.com/api/v3/coins/bitcoin/history?date=01-01-2024" | jq '.market_data.current_price.usd'
```

## Multiple Currencies

Get price in multiple currencies:
```bash
curl -s "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies=usd,eur,gbp,btc,eth" | jq
```

## Error Handling

- **429 Too Many Requests**: Rate limited. Wait 60 seconds.
- **404 Not Found**: Invalid coin ID. Use search endpoint to find correct ID.
- **Empty response**: Token may not be listed on CoinGecko.

## Tips

- Use `jq` to parse JSON responses
- CoinGecko IDs are lowercase, often differ from ticker symbols
- For obscure tokens, search by contract address
- Cache responses when possible (prices update every 1-2 minutes)
- Free tier is sufficient for most use cases

## IMPORTANT: Communicating Results

After fetching the price data, summarize the results to the user clearly:

```
 "Bitcoin is currently $83,876 USD (up 1.26% in 24h)"
```

Include:
- The token name and current price
- 24h change percentage (if available)
- Any other relevant data the user asked about

 