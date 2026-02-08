---
name: agent_identity
description: "Create, manage, and publish your EIP-8004 agent identity registration file"
version: 1.2.2
author: starkbot
homepage: https://eips.ethereum.org/EIPS/eip-8004
tags: [crypto, identity, eip8004, registration, agent, discovery, nft]
requires_tools: [modify_identity, x402_rpc, web3_preset_function_call]
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

Manage your on-chain agent identity using the EIP-8004 standard. This covers creating your identity file, updating it, adding services, publishing it, and registering on-chain via the **StarkLicense** contract on Base.

**Contract:** `0xa23a42D266653846e05d8f356a52298844537472` (Base mainnet, UUPS proxy)
**Payment token:** STARKBOT (`0x587Cd533F418825521f3A1daa7CCd1E7339A1B07`)
**Registration fee:** 1000 STARKBOT (burned on registration, mints an ERC-721 NFT)

---

## 1. Creating Your Identity File

Create a new IDENTITY.json file with your agent name and description:

```tool:modify_identity
action: create
name: <your agent name>
description: <brief description of what your agent does>
image: <optional image URL>
```

This creates `IDENTITY.json` in the soul/ directory with:
- EIP-8004 registration type URL
- x402 support enabled by default
- Active status set to true
- Default trust types: reputation, x402-payments

## 2. Reading Your Identity

View the current contents of your identity file:

```tool:modify_identity
action: read
```

## 3. Updating Fields

Update individual fields in your identity:

```tool:modify_identity
action: update_field
field: name
value: <new name>
```

Supported fields: `name`, `description`, `image`, `active`

## 4. Managing Services

### Add a Service

Register a service endpoint that your agent provides:

```tool:modify_identity
action: add_service
service_name: <service type, e.g. "mcp", "a2a", "chat", "x402", "swap">
service_endpoint: <full URL to service endpoint>
service_version: <version string, default "1.0">
```

Common service types:
- `mcp` — Model Context Protocol server
- `a2a` — Agent-to-Agent protocol
- `chat` — Chat/conversation endpoint
- `x402` — x402 payment-enabled endpoint
- `swap` — Token swap service

### Remove a Service

```tool:modify_identity
action: remove_service
service_name: <name of service to remove>
```

## 5. Publishing to identity.defirelay.com

Upload your identity file to the hosted identity registry. This costs up to 1000 STARKBOT via x402 payment.

```tool:modify_identity
action: upload
```

The server returns a hosted URL where your identity file can be accessed by other agents and registries. **Save this URL** — you'll need it for on-chain registration.

> **IMPORTANT:** If the upload fails for ANY reason (connection error, server down, payment failure), you MUST stop and report the error to the user. Do NOT proceed with on-chain registration without a successful upload — the registration requires a valid hosted URL.

## 6. On-Chain Registration (Base)

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

> Before calling, set the `agent_uri` register to the URL returned from step 5 (upload).

This mints an NFT and returns your `agentId`. The `Registered` event is emitted with your agentId, URI, and owner address.

### Register without URI (set later)

If you don't have a hosted URL yet:

```tool:web3_preset_function_call
preset: identity_register_no_uri
network: base
```

Then set the URI later with `identity_set_uri`.

## 7. Managing On-Chain Identity

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

## 8. Querying the Registry

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

1. **Create** your identity with `modify_identity` action=create
2. **Add services** as you deploy endpoints
3. **Upload** to identity.defirelay.com (`modify_identity` action=upload, x402 payment)
4. **Approve** 1000 STARKBOT → `identity_approve_registry` preset
5. **Register** on-chain → `identity_register` preset (mints NFT, burns STARKBOT)
6. **Update** fields and URI as your agent evolves

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
