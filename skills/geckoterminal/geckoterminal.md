---
name: geckoterminal
description: "Show an interactive GeckoTerminal price chart for a token"
version: 1.1.0
author: starkbot
metadata: {"clawdbot":{"emoji":"📊"}}
requires_tools: [run_skill_script]
requires_binaries: [python3]
scripts: [geckoterminal.py]
tags: [crypto, chart, price, trading, defi, geckoterminal]
---

# GeckoTerminal Price Chart

Show an interactive price chart for a token. On the web UI, a live
GeckoTerminal chart renders inline. On other channels, a link is provided.

## When to use

- User asks to "show a chart", "price chart", "graph", or "chart <token>"
- User wants to visualize price action for a specific token

## Steps

1. Call the script via `run_skill_script`:

By symbol/name:
```json
{"tool": "run_skill_script", "script": "geckoterminal.py", "action": "search", "args": {"query": "PEPE"}}
```

By symbol on a specific chain:
```json
{"tool": "run_skill_script", "script": "geckoterminal.py", "action": "search", "args": {"query": "PEPE", "network": "base"}}
```

By contract address:
```json
{"tool": "run_skill_script", "script": "geckoterminal.py", "action": "search", "args": {"query": "0x6982...", "network": "ethereum"}}
```

2. Summarize the key price data alongside the chart
3. The chart embed renders automatically on web — no extra steps
4. On Discord/Telegram, share the GeckoTerminal link so the user can view it

## Supported Chains

ethereum, base, solana, bsc, polygon, arbitrum, optimism, avalanche
