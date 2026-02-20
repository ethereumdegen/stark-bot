---
name: ens
description: "ENS domains — check availability, lookup names/addresses, register .eth names, and renew. Powered by PayToll."
version: 1.0.0
author: starkbot
homepage: https://ens.domains
metadata: {"requires_auth": false, "clawdbot":{"emoji":"🏷️"}}
requires_tools: [x402_post, web_fetch, web3_function_call, broadcast_web3_tx, verify_tx_broadcast, select_web3_network, define_tasks]
tags: [crypto, ens, domains, identity, ethereum, names, web3, paytoll]
---

# ENS — Ethereum Name Service

Check availability, look up names and addresses, register `.eth` domains, and renew. Market data powered by [PayToll](https://paytoll.io).

## CRITICAL RULES

1. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task. Do NOT work ahead.
2. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.**
3. **Sequential tool calls only.** Never call two tools in parallel when the second depends on the first.
4. **ENS operates on Ethereum Mainnet** — always use `network: "mainnet"`.
5. **Registration is a 2-step process** — commit, wait 60+ seconds, then register.

## Key Addresses (Ethereum Mainnet)

| Contract | Address |
|----------|---------|
| ETH Registrar Controller | `0x253553366Da8546fC250F225fe3d25d0C782303b` |
| Public Resolver | `0x231b0Ee14048e9dCcD1d247744d114a4EB5E8E63` |

## PayToll API Reference

| Endpoint | Cost | Purpose |
|----------|------|---------|
| `/v1/ens/check` | Free | Check name availability |
| `/v1/crypto/ens` | $0.001 | Lookup name or reverse-resolve address |
| `/v1/ens/commit` | Free | Build commitment tx (step 1) |
| `/v1/ens/register` | Free | Build register tx (step 2) |
| `/v1/ens/renew` | Free | Build renewal tx |

---

## Operation A: Check Name Availability

```json
{"tool": "web_fetch", "url": "https://api.paytoll.io/v1/ens/check", "method": "POST", "body": {"name": "<name_without_eth>"}, "extract_mode": "raw"}
```

Present result:

```
🏷️ ENS Availability: <name>.eth

[Available]   → "name.eth is available for registration!"
[Taken]       → "name.eth is already registered."
```

---

## Operation B: Lookup ENS Name or Address

### Forward lookup (name → address)

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/crypto/ens", "body": {"name": "vitalik.eth"}}
```

### Reverse lookup (address → name)

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/crypto/ens", "body": {"address": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"}}
```

### Full profile (with avatar + text records)

```json
{"tool": "x402_post", "url": "https://api.paytoll.io/v1/crypto/ens", "body": {"name": "vitalik.eth", "resolveAvatar": true, "resolveText": ["description", "url", "twitter", "github", "email"]}}
```

Present as:

```
🏷️ vitalik.eth

Address: 0xd8dA...6045
Avatar:  [url if resolved]

Records:
  Twitter:     @VitalikButerin
  GitHub:      vbuterin
  URL:         https://vitalik.eth.limo
  Description: ...
```

---

## Operation C: Check Registration Price

Use the ENS controller directly to get the price in ETH:

```json
{"tool": "web3_function_call", "abi": "ens_registrar", "contract": "0x253553366Da8546fC250F225fe3d25d0C782303b", "function": "rentPrice", "params": ["<name_without_eth>", "31536000"], "network": "mainnet", "call_only": true}
```

**Duration**: `31536000` = 1 year in seconds. Adjust for longer:
- 2 years: `63072000`
- 3 years: `94608000`
- 5 years: `157680000`

The result is a tuple of `(base, premium)` in wei. Add both together for the total price. Report in ETH.

```
🏷️ Registration Price: <name>.eth

Duration: 1 year
Base:     0.003 ETH
Premium:  0.000 ETH
Total:    0.003 ETH (~$X.XX)

Note: 5-character+ names are cheapest. 4-char names have a premium.
3-char names have a higher premium.
```

---

## Operation D: Register a New .eth Name

Registration is a **2-step commit-reveal process** to prevent front-running.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Check availability and price.",
  "TASK 2 — Commit: generate secret, compute commitment, submit commit tx.",
  "TASK 3 — Wait 60 seconds for the commitment to mature.",
  "TASK 4 — Register: submit register tx with ETH payment, broadcast, verify."
]}
```

### Task 1: Check Availability & Price

#### 1a. Select network

```json
{"tool": "select_web3_network", "network": "mainnet"}
```

#### 1b. Check availability

```json
{"tool": "web_fetch", "url": "https://api.paytoll.io/v1/ens/check", "method": "POST", "body": {"name": "<name>"}, "extract_mode": "raw"}
```

If NOT available, stop: "This name is already registered."

#### 1c. Check price

```json
{"tool": "web3_function_call", "abi": "ens_registrar", "contract": "0x253553366Da8546fC250F225fe3d25d0C782303b", "function": "rentPrice", "params": ["<name>", "<duration_seconds>"], "network": "mainnet", "call_only": true}
```

Report availability and price. Ask user to confirm. Complete task.

---

### Task 2: Commit

#### 2a. Generate a random secret

Generate a random 32-byte hex string for the secret, e.g.: `0x` followed by 64 random hex characters. Store it — the user will need it for registration in Task 4.

#### 2b. Compute commitment hash

```json
{"tool": "web3_function_call", "abi": "ens_registrar", "contract": "0x253553366Da8546fC250F225fe3d25d0C782303b", "function": "makeCommitment", "params": ["<name>", "<wallet_address>", "<duration_seconds>", "<secret>", "0x231b0Ee14048e9dCcD1d247744d114a4EB5E8E63", "[]", "true", "0"], "network": "mainnet", "call_only": true}
```

**Parameters explained:**
- `name`: Name without `.eth`
- `wallet_address`: User's wallet (owner)
- `duration_seconds`: Registration length (default `"31536000"` = 1 year)
- `secret`: The random secret from 2a
- `resolver`: Public Resolver (`0x231b0Ee14048e9dCcD1d247744d114a4EB5E8E63`)
- `data`: Empty (`[]`)
- `reverseRecord`: `true` — sets as primary ENS name
- `ownerControlledFuses`: `0`

#### 2c. Submit commit transaction

```json
{"tool": "web3_function_call", "abi": "ens_registrar", "contract": "0x253553366Da8546fC250F225fe3d25d0C782303b", "function": "commit", "params": ["<commitment_hash>"], "network": "mainnet"}
```

#### 2d. Broadcast commit

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_commit>"}
```

#### 2e. Verify commit

```json
{"tool": "verify_tx_broadcast"}
```

Report: "Commitment submitted! You must wait at least 60 seconds before registering."

**IMPORTANT**: Store the `secret` — it's needed in Task 4.

---

### Task 3: Wait for Commitment to Mature

Tell the user:

```
Waiting 70 seconds for the commitment to mature...
(ENS requires at least 60 seconds between commit and register to prevent front-running.)
```

The agent should wait before proceeding to Task 4. Use a say_to_user to tell the user to wait and come back in ~70 seconds, then complete this task.

---

### Task 4: Register

#### 4a. Submit register transaction

```json
{"tool": "web3_function_call", "abi": "ens_registrar", "contract": "0x253553366Da8546fC250F225fe3d25d0C782303b", "function": "register", "params": ["<name>", "<wallet_address>", "<duration_seconds>", "<secret>", "0x231b0Ee14048e9dCcD1d247744d114a4EB5E8E63", "[]", "true", "0"], "value": "<price_in_wei_with_10pct_buffer>", "network": "mainnet"}
```

**IMPORTANT**: The `value` field must be the registration price PLUS a 10% buffer to account for price fluctuations. Any excess ETH is refunded by the contract.

Compute: `value = (base + premium) * 1.1` — round up to nearest wei.

#### 4b. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_register>"}
```

#### 4c. Verify

```json
{"tool": "verify_tx_broadcast"}
```

Report:

```
🏷️ <name>.eth — Registered!

Owner:    <wallet_address>
Duration: 1 year
Expires:  [date]
Resolver: Public Resolver
Primary:  Yes (reverse record set)
```

---

## Operation E: Renew a .eth Name

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Check renewal price.",
  "TASK 2 — Submit renew tx, broadcast, verify."
]}
```

### Task 1: Check Price

```json
{"tool": "web3_function_call", "abi": "ens_registrar", "contract": "0x253553366Da8546fC250F225fe3d25d0C782303b", "function": "rentPrice", "params": ["<name>", "<duration_seconds>"], "network": "mainnet", "call_only": true}
```

Report price. Ask user to confirm.

---

### Task 2: Execute Renewal

#### 2a. Submit renew transaction

```json
{"tool": "web3_function_call", "abi": "ens_registrar", "contract": "0x253553366Da8546fC250F225fe3d25d0C782303b", "function": "renew", "params": ["<name>", "<duration_seconds>"], "value": "<price_in_wei_with_10pct_buffer>", "network": "mainnet"}
```

#### 2b. Broadcast + Verify

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid_from_renew>"}
```

```json
{"tool": "verify_tx_broadcast"}
```

Report: "Renewed <name>.eth for [duration]."

---

## Error Handling

| Error | Solution |
|-------|----------|
| Name not available | Choose a different name |
| Insufficient ETH | Need ETH on mainnet for registration + gas |
| Commitment too new | Wait at least 60 seconds after commit |
| Commitment expired | Commitment expires after 24 hours — re-commit |
| Name too short | Names must be 3+ characters |

---

## Pricing Guide

ENS registration costs vary by name length:
- **5+ characters**: ~$5/year
- **4 characters**: ~$160/year
- **3 characters**: ~$640/year

Plus Ethereum gas fees for the commit and register transactions.
