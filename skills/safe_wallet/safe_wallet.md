---
name: safe_wallet
description: "Create and manage Safe{Wallet} multi-sig wallets — deploy Safes, query info, propose/sign/execute multi-sig transactions, manage signers."
version: 1.0.0
author: starkbot
homepage: https://safe.global
metadata: {"requires_auth": false, "clawdbot":{"emoji":"🔐"}}
requires_tools: [set_address, web3_function_call, web3_preset_function_call, web_fetch, broadcast_web3_tx, verify_tx_broadcast, select_web3_network, define_tasks]
tags: [crypto, defi, safe, gnosis, multisig, wallet, security]
abis: [safe, safe_proxy_factory]
presets_file: presets.ron
---

# Safe{Wallet} Multi-Sig Skill

Create, manage, and transact with Safe (Gnosis Safe) multi-sig wallets. Supports deploying new Safes, querying Safe info, proposing multi-sig transactions, signing, and executing.

## CRITICAL RULES

1. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task. Do NOT work ahead.
2. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.**
3. **Sequential tool calls only.** Never call two tools in parallel when the second depends on the first.
4. **Always confirm destructive operations** (adding/removing owners, executing transactions) with the user before proceeding.
5. **On-chain signing only.** We use `approveHash()` for signing — no off-chain EIP-712 signatures.

## Key Addresses (Same on All Chains)

| Contract | Address |
|----------|---------|
| Safe Singleton v1.4.1 | `0x29fcB43b46531BcA003ddC8FCB67FFE91900C762` |
| SafeProxyFactory v1.4.1 | `0x4e1DCf7AD4e460CfD30791CCC4F9c8a4f820ec67` |
| CompatibilityFallbackHandler | `0xf48f2B2d2a534e402487b3ee7C18c33Aec0Fe5e4` |

## Safe Transaction Service API Endpoints

| Chain | URL |
|-------|-----|
| Ethereum | `https://safe-transaction-mainnet.safe.global` |
| Base | `https://safe-transaction-base.safe.global` |
| Arbitrum | `https://safe-transaction-arbitrum.safe.global` |
| Optimism | `https://safe-transaction-optimism.safe.global` |
| Polygon | `https://safe-transaction-polygon.safe.global` |

---

## Operation A: Query Safe Info

No tasks needed — direct tool calls. Use this when the user asks about an existing Safe.

### A1. Select network

```json
{"tool": "select_web3_network", "network": "<chain>"}
```

### A2. Set the Safe address

```json
{"tool": "set_address", "register": "safe_address", "address": "<safe_address>"}
```

### A3. Get owners

```json
{"tool": "web3_preset_function_call", "preset": "safe_get_owners", "network": "<chain>", "call_only": true}
```

### A4. Get threshold

```json
{"tool": "web3_preset_function_call", "preset": "safe_get_threshold", "network": "<chain>", "call_only": true}
```

### A5. Get nonce

```json
{"tool": "web3_preset_function_call", "preset": "safe_nonce", "network": "<chain>", "call_only": true}
```

### A6. Report

Report owners list, threshold (M-of-N), and current nonce to the user.

---

## Operation B: Create Safe (1-of-1)

Deploy a new Safe with a single owner (the user's wallet). Simplest case — good for personal multi-sig or as a starting point.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select network, confirm owner address with user. See safe_wallet skill 'Create 1-of-1 Task 1'.",
  "TASK 2 — Deploy: call createProxyWithNonce on SafeProxyFactory, broadcast, verify. See safe_wallet skill 'Create 1-of-1 Task 2'.",
  "TASK 3 — Verify: confirm deployment, query the new Safe. See safe_wallet skill 'Create 1-of-1 Task 3'."
]}
```

### Task 1: Prepare

#### 1a. Select network

```json
{"tool": "select_web3_network", "network": "<chain>"}
```

#### 1b. Confirm with user

Tell the user you will deploy a 1-of-1 Safe with their wallet as the sole owner. Ask them to confirm. Complete with `finished_task: true`.

### Task 2: Deploy

The `setup` function initializer must be ABI-encoded and passed as the `initializer` bytes parameter to `createProxyWithNonce`.

**Setup parameters for 1-of-1:**
- `_owners`: `[<user_wallet>]`
- `_threshold`: `1`
- `to`: `0x0000000000000000000000000000000000000000` (no delegate call)
- `data`: `0x` (empty)
- `fallbackHandler`: `0xf48f2B2d2a534e402487b3ee7C18c33Aec0Fe5e4`
- `paymentToken`: `0x0000000000000000000000000000000000000000`
- `payment`: `0`
- `paymentReceiver`: `0x0000000000000000000000000000000000000000`

#### 2a. Build the initializer

You must ABI-encode a call to `setup(address[],uint256,address,bytes,address,address,uint256,address)`.

The function selector for `setup` is `0xb63e800d`.

Construct the initializer hex manually:

```
0xb63e800d
0000000000000000000000000000000000000000000000000000000000000100  // offset to _owners array (256 bytes)
0000000000000000000000000000000000000000000000000000000000000001  // _threshold = 1
0000000000000000000000000000000000000000000000000000000000000000  // to = address(0)
0000000000000000000000000000000000000000000000000000000000000140  // offset to data (320 bytes)
000000000000000000000000f48f2B2d2a534e402487b3ee7C18c33Aec0Fe5e4  // fallbackHandler
0000000000000000000000000000000000000000000000000000000000000000  // paymentToken = address(0)
0000000000000000000000000000000000000000000000000000000000000000  // payment = 0
0000000000000000000000000000000000000000000000000000000000000000  // paymentReceiver = address(0)
0000000000000000000000000000000000000000000000000000000000000001  // _owners array length = 1
000000000000000000000000<OWNER_ADDRESS_20_BYTES_NO_0x>              // _owners[0]
0000000000000000000000000000000000000000000000000000000000000000  // data length = 0
```

Replace `<OWNER_ADDRESS_20_BYTES_NO_0x>` with the user's wallet address (lowercase, no 0x prefix).

Concatenate all lines (remove whitespace and comments) into a single hex string starting with `0x`.

#### 2b. Deploy via createProxyWithNonce

Use a random salt nonce (e.g. current unix timestamp):

```json
{"tool": "web3_function_call", "abi": "safe_proxy_factory", "contract": "0x4e1DCf7AD4e460CfD30791CCC4F9c8a4f820ec67", "function": "createProxyWithNonce", "params": ["0x29fcB43b46531BcA003ddC8FCB67FFE91900C762", "<initializer_hex>", "<salt_nonce>"]}
```

#### 2c. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

Complete with `finished_task: true` after broadcast succeeds.

### Task 3: Verify

#### 3a. Verify deployment

```json
{"tool": "verify_tx_broadcast"}
```

#### 3b. Query the new Safe

The deployed Safe address is in the transaction receipt logs (the `ProxyCreation` event). Extract it from the receipt, then query it:

```json
{"tool": "set_address", "register": "safe_address", "address": "<new_safe_address>"}
```

```json
{"tool": "web3_preset_function_call", "preset": "safe_get_owners", "network": "<chain>", "call_only": true}
```

```json
{"tool": "web3_preset_function_call", "preset": "safe_get_threshold", "network": "<chain>", "call_only": true}
```

Report the new Safe address, owners, and threshold to the user. Complete the task.

---

## Operation C: Create Multi-Owner Safe

Same as Operation B, but with multiple owners and a higher threshold.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select network, collect owner addresses and threshold from user. See safe_wallet skill 'Multi-Owner Task 1'.",
  "TASK 2 — Deploy: build initializer with N owners and M threshold, call createProxyWithNonce, broadcast. See safe_wallet skill 'Multi-Owner Task 2'.",
  "TASK 3 — Verify: confirm deployment, query the new Safe. See safe_wallet skill 'Create 1-of-1 Task 3'."
]}
```

### Multi-Owner Task 1: Prepare

Select network. Ask the user for:
- List of owner addresses (including their own)
- Threshold (M-of-N) — must be >= 1 and <= number of owners

Validate all addresses and confirm with the user. Complete with `finished_task: true`.

### Multi-Owner Task 2: Deploy

Same as Operation B Task 2, but adjust the initializer:
- `_owners` array length and entries match the provided list
- `_threshold` matches the user's chosen value
- The `_owners` array offset and `data` offset shift based on array length

**Initializer layout for N owners:**

```
0xb63e800d
<offset to _owners array = 0x100>                                    // 256
<_threshold>
<to = 0x0>
<offset to data = 0x100 + 0x20 + N*0x20>                            // shifts with N
<fallbackHandler>
<paymentToken = 0x0>
<payment = 0>
<paymentReceiver = 0x0>
<N>                                                                   // array length
<owner_1 padded to 32 bytes>
<owner_2 padded to 32 bytes>
...
<owner_N padded to 32 bytes>
<0>                                                                   // data length = 0
```

Then call `createProxyWithNonce` and broadcast as in Operation B.

---

## Operation D: Check Safe Balances

Use the Safe Transaction Service API to check ETH and token balances.

```json
{"tool": "web_fetch", "url": "https://safe-transaction-<chain>.safe.global/api/v1/safes/<safe_address>/balances/?trusted=true", "method": "GET", "extract_mode": "raw"}
```

Parse the response and report:
- ETH balance (native token)
- ERC20 token balances with symbols and human-readable amounts (divide by 10^decimals)

---

## Operation E: Propose Multi-Sig Transaction

Build a Safe transaction, sign it on-chain via `approveHash`, and POST to the Transaction Service for other signers.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select network, set safe_address, get nonce, confirm tx details with user. See safe_wallet skill 'Propose Task 1'.",
  "TASK 2 — Compute hash and approve: compute getTransactionHash, then approveHash on-chain, broadcast. See safe_wallet skill 'Propose Task 2'.",
  "TASK 3 — Post to TX Service: POST the proposed transaction to the Safe Transaction Service. See safe_wallet skill 'Propose Task 3'."
]}
```

### Propose Task 1: Prepare

#### 1a. Select network and set Safe address

```json
{"tool": "select_web3_network", "network": "<chain>"}
```

```json
{"tool": "set_address", "register": "safe_address", "address": "<safe_address>"}
```

#### 1b. Get current nonce

```json
{"tool": "web3_preset_function_call", "preset": "safe_nonce", "network": "<chain>", "call_only": true}
```

#### 1c. Confirm transaction details

Ask the user for:
- **to**: destination address
- **value**: ETH value to send (in wei), or 0
- **data**: calldata (0x for plain ETH transfer)
- **operation**: 0 for Call, 1 for DelegateCall (almost always 0)

Report the Safe address, nonce, and proposed tx details. Complete with `finished_task: true`.

### Propose Task 2: Compute hash and approve on-chain

#### 2a. Compute the Safe transaction hash

Use `getTransactionHash` on the Safe contract with these parameters:
- `to`: destination address
- `value`: wei value
- `data`: calldata bytes
- `operation`: 0 (Call)
- `safeTxGas`: 0
- `baseGas`: 0
- `gasPrice`: 0
- `gasToken`: `0x0000000000000000000000000000000000000000`
- `refundReceiver`: `0x0000000000000000000000000000000000000000`
- `_nonce`: current nonce from Task 1

```json
{"tool": "web3_function_call", "abi": "safe", "contract": "<safe_address>", "function": "getTransactionHash", "params": ["<to>", "<value>", "<data>", "0", "0", "0", "0", "0x0000000000000000000000000000000000000000", "0x0000000000000000000000000000000000000000", "<nonce>"], "call_only": true}
```

Save the returned bytes32 hash.

#### 2b. Approve the hash on-chain

```json
{"tool": "web3_function_call", "abi": "safe", "contract": "<safe_address>", "function": "approveHash", "params": ["<safe_tx_hash>"]}
```

#### 2c. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

Wait for confirmation. Save the tx hash. Complete with `finished_task: true`.

### Propose Task 3: Post to Transaction Service

POST the transaction to the Safe Transaction Service so other signers can see it:

```json
{
  "tool": "web_fetch",
  "url": "https://safe-transaction-<chain>.safe.global/api/v1/safes/<safe_address>/multisig-transactions/",
  "method": "POST",
  "headers": {"Content-Type": "application/json"},
  "body": {
    "to": "<to>",
    "value": "<value>",
    "data": "<data>",
    "operation": 0,
    "safeTxGas": "0",
    "baseGas": "0",
    "gasPrice": "0",
    "gasToken": "0x0000000000000000000000000000000000000000",
    "refundReceiver": "0x0000000000000000000000000000000000000000",
    "nonce": <nonce>,
    "contractTransactionHash": "<safe_tx_hash>",
    "sender": "<wallet_address>",
    "signature": null,
    "origin": "starkbot"
  },
  "extract_mode": "raw"
}
```

Note: `signature` is null because we used on-chain `approveHash` instead of off-chain signing.

Report success. The transaction is now visible in the Safe UI for other signers. Complete the task.

---

## Operation F: Confirm/Sign a Pending Transaction

List pending transactions and approve one on-chain.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — List pending: fetch pending transactions from TX Service, show to user. See safe_wallet skill 'Confirm Task 1'.",
  "TASK 2 — Approve: approveHash on-chain for the selected transaction, broadcast. See safe_wallet skill 'Confirm Task 2'."
]}
```

### Confirm Task 1: List pending transactions

```json
{"tool": "web_fetch", "url": "https://safe-transaction-<chain>.safe.global/api/v1/safes/<safe_address>/multisig-transactions/?executed=false&limit=10", "method": "GET", "extract_mode": "raw"}
```

Parse the results and show the user:
- Nonce, to, value, data summary, confirmations count vs threshold, safeTxHash

Ask the user which transaction to sign. Complete with `finished_task: true`.

### Confirm Task 2: Approve on-chain

#### 2a. Approve hash

```json
{"tool": "web3_function_call", "abi": "safe", "contract": "<safe_address>", "function": "approveHash", "params": ["<safe_tx_hash>"]}
```

#### 2b. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

#### 2c. Verify

```json
{"tool": "verify_tx_broadcast"}
```

Report success. If this was the final required confirmation, tell the user the transaction is now ready to execute. Complete the task.

---

## Operation G: Execute a Confirmed Transaction

Execute a transaction that has enough approvals.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: fetch the transaction details and confirmations from TX Service. See safe_wallet skill 'Execute Task 1'.",
  "TASK 2 — Execute: call execTransaction with packed signatures, broadcast, verify. See safe_wallet skill 'Execute Task 2'."
]}
```

### Execute Task 1: Prepare

Fetch the specific transaction:

```json
{"tool": "web_fetch", "url": "https://safe-transaction-<chain>.safe.global/api/v1/multisig-transactions/<safe_tx_hash>/", "method": "GET", "extract_mode": "raw"}
```

Verify that confirmations count >= threshold. List the signers who have confirmed. Complete with `finished_task: true`.

### Execute Task 2: Execute

#### 2a. Build packed signatures

For on-chain approvals via `approveHash`, each signer's signature is a 65-byte block:
```
r = signer address padded to 32 bytes (left-padded with zeros)
s = 0x0000000000000000000000000000000000000000000000000000000000000000
v = 1 (indicates approved hash)
```

Sort signers by address (ascending, case-insensitive). Concatenate their 65-byte blocks. Prefix with `0x`.

Example for 2 signers (addr1 < addr2):
```
0x
000000000000000000000000<addr1_no_0x>  // r
0000000000000000000000000000000000000000000000000000000000000000  // s
01  // v
000000000000000000000000<addr2_no_0x>  // r
0000000000000000000000000000000000000000000000000000000000000000  // s
01  // v
```

#### 2b. Call execTransaction

```json
{"tool": "web3_function_call", "abi": "safe", "contract": "<safe_address>", "function": "execTransaction", "params": ["<to>", "<value>", "<data>", "0", "0", "0", "0", "0x0000000000000000000000000000000000000000", "0x0000000000000000000000000000000000000000", "<packed_signatures>"], "value": "<value_if_sending_eth_or_0>"}
```

Note: The `value` field on the tool call should be "0" — the Safe itself holds and sends the ETH via `to`/`value` in its internal transaction. Only set the tool-level `value` if the Safe requires ETH sent to it (rare).

#### 2c. Broadcast

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

#### 2d. Verify

```json
{"tool": "verify_tx_broadcast"}
```

Report result. Complete the task.

---

## Operation H: Add a Signer

Add a new owner to the Safe. This is a self-call: the Safe calls `addOwnerWithThreshold` on itself via `execTransaction`.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select network, query current owners/threshold, get new owner address and new threshold from user. See safe_wallet skill 'Add Signer Task 1'.",
  "TASK 2 — Propose: build addOwnerWithThreshold calldata, compute Safe tx hash, approveHash, POST to TX Service. See safe_wallet skill 'Add Signer Task 2'.",
  "TASK 3 — Execute (if 1-of-N or enough approvals): execute the self-call transaction. See safe_wallet skill 'Execute Task 2'."
]}
```

### Add Signer Task 1: Prepare

Query current Safe info (Operation A steps). Ask the user for:
- New owner address
- New threshold (default: keep current)

Confirm with user. Complete with `finished_task: true`.

### Add Signer Task 2: Propose

#### 2a. Build addOwnerWithThreshold calldata

The function selector for `addOwnerWithThreshold(address,uint256)` is `0x0d582f13`.

```
0x0d582f13
000000000000000000000000<new_owner_no_0x>
<new_threshold_padded_to_32_bytes>
```

#### 2b. Compute Safe transaction hash

Use `getTransactionHash` with:
- `to`: `<safe_address>` (self-call)
- `value`: `0`
- `data`: the calldata from 2a
- All gas/payment params: 0/address(0)
- `_nonce`: current nonce

```json
{"tool": "web3_function_call", "abi": "safe", "contract": "<safe_address>", "function": "getTransactionHash", "params": ["<safe_address>", "0", "<calldata>", "0", "0", "0", "0", "0x0000000000000000000000000000000000000000", "0x0000000000000000000000000000000000000000", "<nonce>"], "call_only": true}
```

#### 2c. Approve hash on-chain

```json
{"tool": "web3_function_call", "abi": "safe", "contract": "<safe_address>", "function": "approveHash", "params": ["<safe_tx_hash>"]}
```

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

#### 2d. POST to Transaction Service

Same as Propose Task 3, with `to` = `<safe_address>` and `data` = the addOwnerWithThreshold calldata.

Complete with `finished_task: true`.

### Add Signer Task 3: Execute

If this is a 1-of-1 Safe (or enough approvals), execute immediately using Operation G Execute Task 2 steps. Otherwise tell the user to have other signers approve first.

---

## Operation I: Remove a Signer

Remove an owner from the Safe. This is a self-call using `removeOwner`.

### Define tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select network, query current owners, identify owner to remove and prevOwner, get new threshold. See safe_wallet skill 'Remove Signer Task 1'.",
  "TASK 2 — Propose: build removeOwner calldata, compute Safe tx hash, approveHash, POST to TX Service. See safe_wallet skill 'Remove Signer Task 2'.",
  "TASK 3 — Execute (if enough approvals): execute the self-call transaction. See safe_wallet skill 'Execute Task 2'."
]}
```

### Remove Signer Task 1: Prepare

Query current owners (Operation A). Ask the user which owner to remove. Determine the `prevOwner`:
- The owners list is a linked list. To remove owner X, find the owner that points to X (the one before X in the `getOwners()` return array).
- If removing the first owner in the array, use `0x0000000000000000000000000000000000000001` as prevOwner (sentinel value).

Ask user for new threshold (must be <= remaining owners count). Confirm. Complete with `finished_task: true`.

### Remove Signer Task 2: Propose

#### 2a. Build removeOwner calldata

The function selector for `removeOwner(address,address,uint256)` is `0xf8dc5dd9`.

```
0xf8dc5dd9
000000000000000000000000<prev_owner_no_0x>
000000000000000000000000<owner_to_remove_no_0x>
<new_threshold_padded_to_32_bytes>
```

#### 2b-2d: Same as Add Signer Task 2 steps 2b-2d

Compute hash, approveHash on-chain, POST to TX Service.

### Remove Signer Task 3: Execute

Same as Add Signer Task 3. Execute if threshold is met.

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| Insufficient gas | Not enough ETH for gas | Need native token for gas |
| Threshold not met | Not enough approvals to execute | Wait for more signers to approve |
| Not an owner | Caller is not a Safe owner | Only owners can approve/execute |
| Invalid prevOwner | Wrong linked-list pointer for removeOwner | Re-query owners and find correct prevOwner |
| Nonce mismatch | Stale nonce value | Re-query nonce before building tx |
| TX Service 422 | Invalid transaction data | Check all parameters match on-chain state |

---

## How Safe Multi-Sig Works

1. **Deploy**: A Safe is a proxy contract pointing to the Safe Singleton. Created via SafeProxyFactory with an owner list and threshold.
2. **Propose**: Any owner builds a Safe transaction (to/value/data) and computes its hash via `getTransactionHash`.
3. **Sign**: Owners approve the hash — either off-chain (EIP-712) or on-chain via `approveHash`. We use on-chain.
4. **Execute**: Once enough owners have approved (>= threshold), anyone can call `execTransaction` with packed signatures.
5. **Self-calls**: To modify the Safe itself (add/remove owners, change threshold), the Safe calls itself — same propose/sign/execute flow with `to` = Safe address.

Key concepts:
- **Threshold**: M-of-N — how many owners must approve before execution
- **Nonce**: Sequential counter preventing replay attacks
- **On-chain approval**: `approveHash(hash)` records approval in contract storage — visible to all, no key management needed
- **Packed signatures**: For on-chain approvals, each signature is `r=address, s=0, v=1` — sorted by signer address ascending
