---
name: bridge_usdc
description: "Bridge USDC between chains (Base, Polygon, Ethereum, Arbitrum, Optimism) via Across Protocol"
version: 1.1.0
author: starkbot
homepage: https://across.to
metadata: {"requires_auth": false, "clawdbot":{"emoji":"🌉"}}
tags: [bridge, usdc, cross-chain, defi, polygon, base, ethereum, arbitrum, optimism, across]
requires_tools: [bridge_usdc, broadcast_web3_tx, list_queued_web3_tx, web3_preset_function_call]
---

# USDC Cross-Chain Bridge Skill

Bridge USDC between supported chains using Across Protocol's fast bridge (~2 second fills).

## Supported Chains

| Chain | Chain ID | USDC Address |
|-------|----------|--------------|
| Ethereum | 1 | `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48` |
| Base | 8453 | `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913` |
| Polygon | 137 | `0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359` |
| Arbitrum | 42161 | `0xaf88d065e77c8cC2239327C5EDb3A432268e5831` |
| Optimism | 10 | `0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85` |

---

## Tools Used

| Tool | Purpose |
|------|---------|
| `bridge_usdc` | Create bridge transaction via Across Protocol |
| `web3_preset_function_call` | Check USDC balance before bridging |
| `list_queued_web3_tx` | Review queued transactions |
| `broadcast_web3_tx` | Send transactions to network |

---

## Basic Bridge Flow

### Step 1: Check USDC Balance (Optional but Recommended)

First, verify you have enough USDC on the source chain:

```tool:token_lookup
symbol: "USDC"
network: base
cache_as: token_address
```

```tool:web3_preset_function_call
preset: erc20_balance
network: base
call_only: true
```

### Step 2: Bridge USDC

```tool:bridge_usdc
from_chain: base
to_chain: polygon
amount: "100"
```

This will:
1. Call Across Protocol API to get bridge quote
2. Queue approval transaction (if needed)
3. Queue bridge transaction
4. Return transaction UUIDs

### Step 3: Review Queued Transactions

```tool:list_queued_web3_tx
status: pending
```

### Step 4: Broadcast Transactions

**Important:** If an approval was queued, broadcast it first and wait for confirmation before broadcasting the bridge transaction.

```tool:broadcast_web3_tx
uuid: "<approval_uuid>"
```

Wait for confirmation, then:

```tool:broadcast_web3_tx
uuid: "<bridge_uuid>"
```

---

## Complete Example: Bridge 50 USDC from Base to Polygon

```tool:bridge_usdc
from_chain: base
to_chain: polygon
amount: "50"
```

Response will show:
- Route: base → polygon
- Amount: 50 USDC
- Expected output after fees
- Estimated fill time (~2 seconds)
- Transaction UUIDs

---

## Bridge to Different Recipient

To send bridged USDC to a different address:

```tool:bridge_usdc
from_chain: ethereum
to_chain: arbitrum
amount: "100"
recipient: "0x1234567890abcdef1234567890abcdef12345678"
```

---

## Custom Slippage

Default slippage is 0.5%. To adjust:

```tool:bridge_usdc
from_chain: base
to_chain: optimism
amount: "1000"
slippage: 0.01
```

(1% slippage for larger amounts)

---

## How Across Protocol Works

1. **Deposit**: You deposit USDC on source chain to Across spoke pool
2. **Relay**: Across relayers fill your order on destination chain (~2 seconds)
3. **Settlement**: Relayers are reimbursed from your deposit via UMA optimistic oracle

Benefits:
- Fast fills (~2 seconds on mainnet)
- Native CCTP integration for USDC
- Competitive fees
- No need to wait for finality

---

## Fee Structure

Across charges:
- **Relayer Fee**: Compensates relayers for capital lockup
- **LP Fee**: Goes to liquidity providers

Fees vary by:
- Route (chain pair)
- Amount
- Current liquidity
- Network congestion

The `bridge_usdc` tool shows expected output after fees.

---

## Pre-Bridge Checklist

Before bridging:

1. **Verify source chain balance** - Check you have enough USDC
2. **Verify ETH for gas** - Need native token for gas on source chain
3. **Double-check destination chain** - Bridges are irreversible
4. **Verify recipient address** - If using custom recipient
5. **Check fees** - Review expected output in tool response

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| "Insufficient USDC balance" | Not enough USDC | Check balance, reduce amount |
| "Gas estimation failed" | Insufficient ETH for gas | Add ETH to wallet |
| "Across API error" | Route not available | Try different route or smaller amount |
| "Same chain" | from_chain = to_chain | Pick different chains |
| "Invalid recipient" | Bad address format | Verify 0x address format |

---

## Tracking Bridge Status

After broadcasting, you can track your bridge at:
- https://across.to/transactions

Or use Across API:
```
GET https://app.across.to/api/deposit/status?depositId=<deposit_id>&originChainId=<chain_id>
```

---

## Security Notes

1. **Transactions are queued, not auto-sent** - You must explicitly broadcast
2. **Broadcast approval first** - Wait for confirmation before bridge tx
3. **Start with small test amounts** - Verify flow works
4. **Check expected output** - Fees can vary by route/amount
5. **Irreversible** - Once bridged, funds go to destination chain

---

## Supported Routes

All chains can bridge to all other chains:

| From / To | ETH | Base | Polygon | Arbitrum | Optimism |
|-----------|-----|------|---------|----------|----------|
| Ethereum | - | Y | Y | Y | Y |
| Base | Y | - | Y | Y | Y |
| Polygon | Y | Y | - | Y | Y |
| Arbitrum | Y | Y | Y | - | Y |
| Optimism | Y | Y | Y | Y | - |

---

## Related Skills

- `transfer_erc20` - Transfer tokens on same chain
- `swap` - Swap tokens on same chain
- `local_wallet` - Check wallet balances
