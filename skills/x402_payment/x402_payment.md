---
name: x402_payment
description: "Pay for x402-enabled AI agent endpoints using USDC on Base"
version: 1.1.2
author: starkbot
homepage: https://x402.org
metadata: {"clawdbot":{"emoji":"💳"}}
tags: [crypto, payments, x402, agents, api, usdc, base]
requires_tools: [x402_agent_invoke, web_fetch]
arguments:
  agent_url:
    description: "Base URL of the x402 agent (e.g., https://dad-jokes-agent-production.up.railway.app)"
    required: false
  endpoint:
    description: "Entrypoint name to invoke (e.g., 'joke', 'health')"
    required: false
---

# x402 Payment Protocol for AI Agents

Invoke x402-enabled AI agent endpoints with automatic USDC micropayments on Base.

## Quick Start

Use `x402_agent_invoke` to call any x402 agent endpoint with automatic payment:

```tool:x402_agent_invoke
agent_url: https://dad-jokes-agent-production.up.railway.app
entrypoint: joke
input: {"category": "programming"}
```

That's it! The tool handles the entire x402 payment flow automatically.

---

## How It Works

The `x402_agent_invoke` tool:

1. Makes POST request to `/entrypoints/{name}/invoke`
2. If 402 returned, parses payment requirements from response
3. Signs EIP-3009 USDC authorization with burner wallet
4. Retries with `X-PAYMENT` header containing signed payment
5. Returns the response

## Prerequisites

- **Burner Wallet**: `BURNER_WALLET_BOT_PRIVATE_KEY` environment variable
- **USDC on Base**: Wallet needs USDC on Base mainnet (chain ID 8453)
- **USDC Contract**: `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913`

---

## Tool Reference

### x402_agent_invoke

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `agent_url` | string | Yes | Base URL of the agent |
| `entrypoint` | string | Yes | Entrypoint name to invoke |
| `input` | object | No | Input data (default: `{}`) |
| `network` | string | No | `base` or `base-sepolia` (default: `base`) |

### Example: Get a Dad Joke

```tool:x402_agent_invoke
agent_url: https://dad-jokes-agent-production.up.railway.app
entrypoint: joke
input: {}
```

### Example: Get Multiple Jokes

```tool:x402_agent_invoke
agent_url: https://dad-jokes-agent-production.up.railway.app
entrypoint: jokes
input: {"count": 3}
```

### Example: Health Check (Free)

```tool:x402_agent_invoke
agent_url: https://dad-jokes-agent-production.up.railway.app
entrypoint: health
input: {}
```

---

## Agent Discovery

Before invoking, discover available endpoints:

### Fetch Agent Manifest

```tool:web_fetch
url: {{agent_url}}/.well-known/agent.json
extract_mode: raw
```

### List Entrypoints

```tool:web_fetch
url: {{agent_url}}/entrypoints
extract_mode: markdown
```

Each entrypoint shows:
- **Path**: `/entrypoints/{name}/invoke`
- **Pricing**: Cost in USDC (e.g., 0.001)
- **Network**: Usually `base`
- **Input Schema**: Expected JSON format

---

## Example: Dad Jokes Agent

**Agent URL**: `https://dad-jokes-agent-production.up.railway.app`

| Endpoint | Price | Description |
|----------|-------|-------------|
| `health` | Free | Health check |
| `joke` | 0.001 USDC | Get a random dad joke |
| `jokes` | 0.005 USDC | Get multiple jokes (count: 1-10) |

### Get a Joke

```tool:x402_agent_invoke
agent_url: https://dad-jokes-agent-production.up.railway.app
entrypoint: joke
input: {"category": "programming"}
```

---

## Cost Reference

| Amount (raw) | USDC Value | Typical Use |
|--------------|------------|-------------|
| 1000 | $0.001 | Single API call |
| 5000 | $0.005 | Multiple results |
| 10000 | $0.01 | Premium request |
| 1000000 | $1.00 | Large batch |

---

## Understanding 402 Responses

When payment is required, agents return:

```json
{
  "error": "X-PAYMENT header is required",
  "accepts": [{
    "scheme": "exact",
    "network": "base",
    "maxAmountRequired": "1000",
    "payTo": "0xFb92D3310dd97a18d88a17E448325b97664EF467",
    "asset": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
  }],
  "x402Version": 1
}
```

The `x402_agent_invoke` tool handles this automatically.

---

## Network Reference

| Network | Chain ID | USDC Contract |
|---------|----------|---------------|
| Base Mainnet | 8453 | 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913 |
| Base Sepolia | 84532 | 0x036CbD53842c5426634e7929541eC2318f3dCF7e |

---

## Troubleshooting

### "BURNER_WALLET_BOT_PRIVATE_KEY not set"

Set the environment variable with your wallet's private key:
```bash
export BURNER_WALLET_BOT_PRIVATE_KEY=0x...
```

### "Payment request failed"

Check that your wallet has sufficient USDC on Base:
- Go to `https://basescan.org/address/{wallet_address}`
- Check USDC token balance

### "No compatible payment option found"

The agent may only support specific networks. Check the 402 response's `accepts` array.

### "Settlement failed" / "Facilitator error"

Payment relay issues. Wait 30 seconds and retry.

---

## Related Skills

- **swap**: Token swaps using x402-enabled DEX aggregator
- **transfer**: USDC transfers on Base
- **local_wallet**: Check burner wallet balance

---

## Other x402 Tools

### x402_preset_fetch (Preset-based)

For DEX/DeFi operations via DeFi Relay:

```tool:x402_preset_fetch
preset: swap_quote
network: base
cache_as: quote_result
```

### x402_rpc (Blockchain RPC)

For paid RPC calls:

```tool:x402_rpc
preset: get_balance
network: base
```
