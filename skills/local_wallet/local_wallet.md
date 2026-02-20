---
name: local_wallet
description: "Check balances and interact with the local burner wallet using RPC calls."
version: 2.4.0
author: starkbot
metadata: {"clawdbot":{"emoji":"wallet"}}
tags: [wallet, crypto, finance, local, burner, address, base, ethereum, rpc]
requires_tools: [token_lookup, ask_user, x402_rpc, web3_preset_function_call]
---

# Local Wallet Access

Check balances and interact with the local burner wallet via RPC calls.

## CRITICAL: YOU MUST CALL THE ACTUAL TOOLS

**DO NOT call `use_skill` again.** This skill file contains instructions. You must now:
1. Read these instructions
2. Call the actual tools directly (e.g., `x402_rpc`, `ask_user`, `token_lookup`, `web3_preset_function_call`)
3. Look for context about the currently selected network

**Example:** To check ETH balance, call `x402_rpc`:
```tool:x402_rpc
preset: get_balance
network: base / polygon / mainnet
```

---

## IMPORTANT: Intrinsic Registers

The `wallet_address` register is **always available** - it's automatically derived from the configured private key. You do NOT need to fetch it separately.

## Step 1: Ask Which Network (REQUIRED)

If the user doesn't specify a network, **ALWAYS** use `ask_user` first:

```tool:ask_user
question: Which network would you like to check your balance on?
options: ["Base", "Ethereum Mainnet"]
context: Your wallet address is the same across all EVM networks, but balances differ.
```

## Step 2: Check ETH Balance

Use `x402_rpc` with the `get_balance` preset. It automatically reads `wallet_address` from registers:

```tool:x402_rpc
preset: get_balance
network: base
```

The result is a hex value in wei. Convert to ETH by dividing by 10^18.

## Step 3: Check ERC20 Token Balance

**ALWAYS use the `erc20_balance` preset - NEVER manually construct a balanceOf call!**

The preset automatically uses your `wallet_address` (from intrinsic register). If you manually call balanceOf with the wrong address, you'll get incorrect results.

### 3a. Look up the token address using `token_lookup`

**ALWAYS use `token_lookup` to get the token address** - never hardcode or guess addresses:

```tool:token_lookup
symbol: STARKBOT
network: base
cache_as: token_address
```

This will automatically cache the address in the `token_address` register for the next step.

### 3b. Get the balance using `web3_preset_function_call`

```tool:web3_preset_function_call
preset: erc20_balance
network: base
call_only: true
```

The preset reads `wallet_address` and `token_address` from registers automatically.

## Available Tokens (use `token_lookup` to get addresses)

### Base
STARKBOT, USDC, WETH, USDbC, DAI, cbBTC, BNKR, AERO, DEGEN, BRETT, TOSHI, ETH

### Ethereum Mainnet
ETH, WETH, USDC, USDT, DAI, WBTC

## Example Flow: "How much ETH do I have?"

1. **Ask user for network:**
```tool:ask_user
question: Which network would you like me to check?
options: ["Base", "Ethereum Mainnet", "Polygon"]
default:  < current network >
```

2. **After user responds**, call `x402_rpc`:
```tool:x402_rpc
preset: get_balance
network:  < current network >
```

3. **Convert and report**: Result is hex wei (e.g., `"0x2386f26fc10000"` = 0.01 ETH).

## Example Flow: "Show me my USDC balance on Base"

1. **Look up token:**
```tool:token_lookup
symbol: USDC
network: < current network >
cache_as: token_address
```

2. **Get balance:**
```tool:web3_preset_function_call
preset: erc20_balance
network:  < current network >
call_only: true
```

3. **Convert**: USDC has 6 decimals, so divide result by 10^6.

## Example Flow: "How much STARKBOT do I have?"

1. **Look up STARKBOT token:**
```tool:token_lookup
symbol: STARKBOT
network: < current network >
cache_as: token_address
```

2. **Get balance:**
```tool:web3_preset_function_call
preset: erc20_balance
network: < current network >
call_only: true
```

3. **Convert**: STARKBOT has 18 decimals, so divide result by 10^18.

## Available Presets

### x402_rpc presets
| Preset | Description | Registers Used |
|--------|-------------|----------------|
| `get_balance` | Get ETH balance | `wallet_address` (intrinsic) |
| `get_nonce` | Get transaction count | `wallet_address` (intrinsic) |
| `gas_price` | Get current gas price | none |
| `block_number` | Get current block | none |

### web3_preset_function_call presets
| Preset | Description | Registers Used |
|--------|-------------|----------------|
| `erc20_balance` | Get any ERC20 token balance | `wallet_address`, `token_address` |
| `weth_balance` | Get WETH balance specifically | `wallet_address` |

## Important Notes

- This is a **burner wallet** - only use for testing/small amounts
- The wallet address is the same across all EVM networks
- `wallet_address` is an **intrinsic register** - always available automatically
- **ALWAYS ask for network if not specified** - don't assume!
- Remember decimals: ETH/WETH = 18, USDC = 6

## COMMON MISTAKE TO AVOID

**NEVER manually construct a balanceOf call — always use presets:**
```
# WRONG - Do NOT manually specify contract/params, use web3_preset_function_call instead!
# The preset "erc20_balance" reads token_address and wallet_address from registers automatically.
```

**ALWAYS use the preset instead:**
```tool:web3_preset_function_call
# CORRECT - Preset automatically uses wallet_address register
preset: erc20_balance
network:  < current network >   (base, polygon, mainnet, etc)
call_only: true
```

The `erc20_balance` preset reads `wallet_address` from registers automatically, ensuring you always check YOUR wallet balance.
