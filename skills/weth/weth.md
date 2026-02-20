---
name: weth
description: "Wrap ETH to WETH or unwrap WETH to ETH on Base or Mainnet"
version: 2.6.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🔄"}}
tags: [crypto, defi, finance, weth, wrap, unwrap, base]
abis: [weth]
presets_file: presets.ron
requires_tools: [to_raw_amount, web3_preset_function_call, list_queued_web3_tx, broadcast_web3_tx]
---

# WETH Wrap/Unwrap

Convert between ETH and WETH (Wrapped Ether) using presets.

**Note:** `wallet_address` is an intrinsic register - always available automatically. No need to fetch it.

---

## Wrap ETH to WETH

### 1. Set amount to wrap (convert human amount to wei)
```json
{"tool": "to_raw_amount", "amount": "0.001", "decimals": 18, "cache_as": "wrap_amount"}
```

### 2. Execute wrap
```tool:web3_preset_function_call
preset: weth_deposit
network: base
```

---

## Unwrap WETH to ETH

### 1. Set amount to unwrap (convert human amount to wei)
```json
{"tool": "to_raw_amount", "amount": "0.001", "decimals": 18, "cache_as": "unwrap_amount"}
```

### 2. Execute unwrap
```tool:web3_preset_function_call
preset: weth_withdraw
network: base
```

---

## Check WETH Balance

```tool:web3_preset_function_call
preset: weth_balance
network: base
call_only: true
```

The `wallet_address` register is intrinsic - no need to set it first.

---

## Amount Reference (Wei)

| ETH Amount | Wei Value |
|------------|-----------|
| 0.0001 ETH | `100000000000000` |
| 0.001 ETH | `1000000000000000` |
| 0.01 ETH | `10000000000000000` |
| 0.1 ETH | `100000000000000000` |
| 1 ETH | `1000000000000000000` |

---

## Available Presets

| Preset | Description | Required Registers |
|--------|-------------|-------------------|
| `weth_deposit` | Wrap ETH to WETH | `wrap_amount` |
| `weth_withdraw` | Unwrap WETH to ETH | `unwrap_amount` |
| `weth_balance` | Check WETH balance | `wallet_address` (intrinsic) |

---

## Why Use WETH?

- Many DeFi protocols require ERC20 tokens, not native ETH
- WETH is a 1:1 wrapped version of ETH as an ERC20
- Wrapping/unwrapping is instant and costs only gas
- Some DEX swaps automatically wrap ETH, but direct WETH control is sometimes needed

---

## Transaction Queue Note

When using `web3_preset_function_call` with presets like `weth_deposit` or `weth_withdraw`, transactions are QUEUED (not broadcast immediately). Use `list_queued_web3_tx` to view queued transactions and `broadcast_web3_tx` to broadcast them.

### Verify and Broadcast
```tool:list_queued_web3_tx
status: pending
limit: 1
```

```tool:broadcast_web3_tx
```
