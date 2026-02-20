---
name: bankr
description: "Interact with Bankr - check token info, wallet balances, and use the Agent API to execute prompts and transactions."
version: 2.0.0
author: starkbot
homepage: https://bankr.bot
metadata: {"requires_auth": true, "clawdbot":{"emoji":"🏦"}}
tags: [crypto, defi, bankr, bnkr, base, wallet, yield, token, agent]
requires_tools: [api_keys_check, exec]
requires_api_keys:
  BANKR_API_KEY:
    description: "Bankr API Key"
    secret: true
---

# Bankr Integration

Bankr is an AI-powered crypto banking agent.

## How to Use This Skill

**First, check if BANKR_API_KEY is configured:**
```tool:api_keys_check
key_name: BANKR_API_KEY
```

If not configured, ask the user to get one from https://bankr.bot/api (enable "Agent API access").

**Then use the `exec` tool** with **timeout: 120** to run this single command that handles everything:

```bash
PROMPT='USER_PROMPT_HERE' && \
JOB_ID=$(curl -sS -X POST "https://api.bankr.bot/agent/prompt" \
  -H "X-API-Key: $BANKR_API_KEY" \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": \"$PROMPT\"}" | jq -r '.jobId') && \
echo "Job submitted: $JOB_ID" && \
for i in {1..30}; do \
  sleep 3; \
  RESULT=$(curl -sS "https://api.bankr.bot/agent/job/$JOB_ID" -H "X-API-Key: $BANKR_API_KEY"); \
  STATUS=$(echo "$RESULT" | jq -r '.status'); \
  echo "Poll $i: $STATUS"; \
  if [ "$STATUS" = "completed" ]; then \
    echo "=== BANKR RESPONSE ==="; \
    echo "$RESULT" | jq -r '.response'; \
    exit 0; \
  elif [ "$STATUS" = "failed" ]; then \
    echo "=== ERROR ==="; \
    echo "$RESULT" | jq -r '.error // .message // "Unknown error"'; \
    exit 1; \
  fi; \
done; \
echo "Timeout: Job did not complete in 90 seconds"
```

**Replace `USER_PROMPT_HERE` with the user's actual request** (properly escaped for JSON).

### Example Usage

User says: "buy 1 $starkbot"

Call `exec` tool with parameters:
- **command**: (the bash script below with PROMPT set)
- **timeout**: 120

```bash
PROMPT='buy 1 $starkbot' && \
JOB_ID=$(curl -sS -X POST "https://api.bankr.bot/agent/prompt" \
  -H "X-API-Key: $BANKR_API_KEY" \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": \"$PROMPT\"}" | jq -r '.jobId') && \
echo "Job submitted: $JOB_ID" && \
for i in {1..30}; do \
  sleep 3; \
  RESULT=$(curl -sS "https://api.bankr.bot/agent/job/$JOB_ID" -H "X-API-Key: $BANKR_API_KEY"); \
  STATUS=$(echo "$RESULT" | jq -r '.status'); \
  echo "Poll $i: $STATUS"; \
  if [ "$STATUS" = "completed" ]; then \
    echo "=== BANKR RESPONSE ==="; \
    echo "$RESULT" | jq -r '.response'; \
    exit 0; \
  elif [ "$STATUS" = "failed" ]; then \
    echo "=== ERROR ==="; \
    echo "$RESULT" | jq -r '.error // .message // "Unknown error"'; \
    exit 1; \
  fi; \
done; \
echo "Timeout: Job did not complete in 90 seconds"
```

This single command:
1. Submits the prompt to Bankr
2. Polls every 3 seconds for up to 90 seconds
3. Returns the response when complete
4. Handles errors and timeouts

**DO NOT** manually poll with multiple exec calls. Use this single command.

---

## Example Prompts for Bankr

- `"What is my wallet balance?"`
- `"buy 1 $STARKBOT"` or `"buy 0.01 ETH worth of $BNKR"`
- `"swap 0.1 ETH for USDC"`
- `"What is the current price of ETH?"`
- `"Show me trending tokens"`
- `"What tokens do I hold?"`

---

## Requirements

- **API Key**: Must have `BANKR_API_KEY` configured with Agent API access enabled
- **Get API Key**: https://bankr.bot/api (enable "Agent API access")

---

# Public APIs (No API Key Required)

For read-only data, you can use public APIs without authentication.

## Key Info

- **BNKR Token**: `0x22aF33FE49fD1Fa80c7149773dDe5890D3c76F3b` (Base)
- **Chain**: Base (chainId 8453)
- **Website**: https://bankr.bot
- **Swap**: https://swap.bankr.bot

## Token Info & Price

Get BNKR token details and current price:

```bash
# Get price from DexScreener
curl -s "https://api.dexscreener.com/latest/dex/tokens/0x22aF33FE49fD1Fa80c7149773dDe5890D3c76F3b" | jq '.pairs[0] | {price: .priceUsd, priceChange24h: .priceChange.h24, volume24h: .volume.h24, liquidity: .liquidity.usd, dex: .dexId}'
```

## Check Wallet Balance

Check BNKR and ETH balance for any address on Base:

```bash
ADDRESS="0x..."

# Get ETH balance on Base
curl -s "https://api.basescan.org/api?module=account&action=balance&address=$ADDRESS&tag=latest" | jq '.result | tonumber / 1e18 | "ETH: \(.)"'

# Get BNKR token balance
curl -s "https://api.basescan.org/api?module=account&action=tokenbalance&contractaddress=0x22aF33FE49fD1Fa80c7149773dDe5890D3c76F3b&address=$ADDRESS&tag=latest" | jq '.result | tonumber / 1e18 | "BNKR: \(.)"'
```

## Explore Pools & Liquidity

Find BNKR liquidity pools:

```bash
curl -s "https://api.dexscreener.com/latest/dex/tokens/0x22aF33FE49fD1Fa80c7149773dDe5890D3c76F3b" | jq '.pairs[] | {pair: .pairAddress, dex: .dexId, baseToken: .baseToken.symbol, quoteToken: .quoteToken.symbol, price: .priceUsd, liquidity: .liquidity.usd}'
```

---

# About Bankr

Bankr is an AI-powered crypto banker that works on X (Twitter) and Farcaster. Key features:

- **Trading**: Swap tokens, trade perps, prediction markets
- **Advanced Orders**: Limit, stop loss, trailing stop, TWAP, DCA
- **Bankr Earn**: Auto-optimizes USDC yield across chains
- **NFTs**: Mint and manage NFTs via natural language

### Tokenomics
- 90% of platform revenue goes to BNKR stakers and LP providers
- Fixed 100B supply, ownership-renounced contract
- Available on Aerodrome, Uniswap (Base), and CEXs (MEXC, BingX, Gate.io)

### Supported Chains
- Base (primary)
- Ethereum
- Polygon
- Solana

---

# Resources

- **API Dashboard:** https://bankr.bot/api
- **Example Apps:** https://github.com/BankrBot/bankr-api-examples
- **Swap UI:** https://swap.bankr.bot
- **Twitter:** https://x.com/bankrbot
- **Token:** https://basescan.org/token/0x22aF33FE49fD1Fa80c7149773dDe5890D3c76F3b

---

# Best Practices

1. **Start with limited funds** - Test with small amounts first
2. **Never share your API key** - Treat it like a password
3. **Poll responsibly** - Use 2-second intervals, don't spam
4. **Handle all statuses** - Check for failed/cancelled, not just completed
5. **Check richData** - Contains valuable structured information
6. **Set timeouts** - Don't poll forever, implement max attempts
7. **Revoke compromised keys immediately** - If leaked, revoke at https://bankr.bot/api
