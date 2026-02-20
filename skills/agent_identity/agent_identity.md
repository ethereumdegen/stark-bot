---
name: agent_identity
description: "Create, import, and register your EIP-8004 agent identity"
version: 2.3.0
author: starkbot
homepage: https://eips.ethereum.org/EIPS/eip-8004
tags: [crypto, identity, eip8004, registration, agent, discovery, nft]
requires_tools: [import_identity, register_new_identity, x402_rpc, web3_preset_function_call]
arguments:
  agent_name:
    description: "Name for the agent identity"
    required: false
  agent_description:
    description: "Description of the agent"
    required: false
  image_url:
    description: "URL to agent avatar/image"
    required: false
---

# EIP-8004 Agent Identity Management

Manage your on-chain agent identity using the EIP-8004 standard.

**Contract:** `0xa23a42D266653846e05d8f356a52298844537472` (Base mainnet, UUPS proxy)
**Payment token:** STARKBOT (`0x587Cd533F418825521f3A1daa7CCd1E7339A1B07`)
**Registration fee:** 1000 STARKBOT (burned on registration, mints an ERC-721 NFT)

---

## IMPORTANT: Import vs Create vs Read

- **"what is my identity?"** → use `import_identity` with NO params (returns existing DB identity)
- **"import my identity" or has an NFT** → use `import_identity` (with `agent_id` if known — forces on-chain import)
- **"create a new identity from scratch"** → use `register_new_identity`
- **NEVER** use `register_new_identity` when the user asks to import an existing NFT

## 1. Reading Your Identity

Call `import_identity` with no parameters. If identity exists in the DB, it returns it immediately without going on-chain:

```tool:import_identity
```

This is the correct tool for "what is my identity?", "show my agent info", or any read operation.

## 2. Importing an Existing Identity (from on-chain)

If you need to import/re-import from on-chain, provide the `agent_id`:

```tool:import_identity
agent_id: 5
```

This forces an on-chain lookup: verifies ownership, fetches the agent URI, persists the agent_id locally, and sets the `agent_id` register so you can immediately use on-chain presets.

To auto-discover when no identity is in the DB yet, call with no params — it will scan your wallet via `balanceOf + tokenOfOwnerByIndex`.

## 2. Creating a New Identity (from scratch)

Only use this if the user does NOT have an existing NFT and wants to create a brand-new identity:

```tool:register_new_identity
name: <your agent name>
description: <brief description of what your agent does>
image: <optional image URL>
```

This creates the identity in the database with:
- EIP-8004 registration type URL
- x402 support enabled by default
- Active status set to true
- Default trust types: reputation, x402-payments

## 3. On-Chain Registration (Base)

Registration on the StarkLicense contract mints an ERC-721 NFT that represents your agent identity. It costs 1000 STARKBOT (burned, not held).

### Step 1: Approve STARKBOT spending

First, approve the StarkLicense contract to spend 1000 STARKBOT:

```tool:web3_preset_function_call
preset: identity_approve_registry
network: base
```

### Step 2: Register with your hosted identity URL

```tool:web3_preset_function_call
preset: identity_register
network: base
```

> Before calling, set the `agent_uri` register to the URL returned from upload.

This mints an NFT and returns your `agentId`. The `Registered` event is emitted with your agentId, URI, and owner address.

### Register without URI (set later)

If you don't have a hosted URL yet:

```tool:web3_preset_function_call
preset: identity_register_no_uri
network: base
```

Then set the URI later with `identity_set_uri`.

## 4. Managing On-Chain Identity

### Update Agent URI

```tool:web3_preset_function_call
preset: identity_set_uri
network: base
```

Set `agent_id` and `agent_uri` registers first. Must be the agent owner.

### Get Agent URI

```tool:web3_preset_function_call
preset: identity_get_uri
network: base
```

Set `agent_id` register first.

### Set On-Chain Metadata

Store arbitrary key-value metadata on-chain:

```tool:web3_preset_function_call
preset: identity_set_metadata
network: base
```

Set `agent_id`, `metadata_key` (string), and `metadata_value` (hex bytes) registers first.

### Get On-Chain Metadata

```tool:web3_preset_function_call
preset: identity_get_metadata
network: base
```

Set `agent_id` and `metadata_key` registers first.

## 5. Querying the Registry

### Check registration fee

```tool:web3_preset_function_call
preset: identity_registration_fee
network: base
```

### Total registered agents

```tool:web3_preset_function_call
preset: identity_total_agents
network: base
```

### How many agents does a wallet own?

```tool:web3_preset_function_call
preset: identity_balance
network: base
```

Set `wallet_address` register first.

### Get your agent ID

```tool:web3_preset_function_call
preset: identity_token_of_owner
network: base
```

Set `wallet_address` register first. Returns the first agent ID owned.

### Who owns an agent?

```tool:web3_preset_function_call
preset: identity_owner_of
network: base
```

Set `agent_id` register first.

## Identity File Format

The IDENTITY.json file follows the EIP-8004 registration file schema:

```json
{
  "type": "https://eips.ethereum.org/EIPS/eip-8004#registration-v1",
  "name": "Agent Name",
  "description": "What this agent does",
  "image": "https://example.com/avatar.png",
  "services": [
    {
      "name": "x402",
      "endpoint": "https://agent.example.com/x402",
      "version": "1.0"
    }
  ],
  "x402Support": true,
  "active": true,
  "supportedTrust": ["reputation", "x402-payments"]
}
```

## Full Workflow Summary

### Import Existing Identity (recommended)
1. **Import** with `import_identity` (with or without specific agent_id)
2. Tool verifies ownership, fetches URI, persists locally, sets `agent_id` register
3. You can now query/update the identity using on-chain presets

### New Identity (from scratch)
1. **Create** your identity with `register_new_identity`
2. **Approve** 1000 STARKBOT → `identity_approve_registry` preset
3. **Register** on-chain → `identity_register` preset (mints NFT, burns STARKBOT)

## Available Presets

| Preset | Description |
|--------|-------------|
| `identity_approve_registry` | Approve 1000 STARKBOT for registration |
| `identity_allowance_registry` | Check STARKBOT allowance for registry |
| `identity_register` | Register with URI (requires approval) |
| `identity_register_no_uri` | Register without URI |
| `identity_set_uri` | Update agent URI |
| `identity_get_uri` | Get agent URI |
| `identity_registration_fee` | Get current fee |
| `identity_total_agents` | Get total registered agents |
| `identity_balance` | Get agent NFT count for wallet |
| `identity_owner_of` | Get owner of agent ID |
| `identity_token_of_owner` | Get first agent ID for wallet |
| `identity_set_metadata` | Set on-chain metadata |
| `identity_get_metadata` | Get on-chain metadata |
