---
name: alchemy
description: "Query Ethereum & Base wallet balances, token holdings, NFTs, and transaction history using the Alchemy API."
version: 1.0.0
author: starkbot
homepage: https://docs.alchemy.com
metadata: {"requires_auth": true, "clawdbot":{"emoji":"🧪"}}
tags: [crypto, finance, wallet, ethereum, base, tokens, nfts, alchemy, defi, portfolio]
requires_tools: [api_keys_check, exec]
---

# Alchemy Wallet & Portfolio

Query on-chain wallet data across Ethereum and Base using the Alchemy Enhanced APIs.

## Authentication

**First, check if ALCHEMY_API_KEY is configured:**

```tool:api_keys_check
key_name: ALCHEMY_API_KEY
```

If not configured, ask the user to get one from https://dashboard.alchemy.com/ (free tier works).

## Base URLs

All endpoints use JSON-RPC POST requests to the chain-specific URL:

| Chain | URL |
|-------|-----|
| Ethereum | `https://eth-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY` |
| Base | `https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY` |

**Default to Base** unless the user specifies Ethereum.

---

## Native ETH Balance

```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":["WALLET_ADDRESS","latest"]}' | jq -r '.result' | xargs printf "%d\n" | awk '{printf "%.6f ETH\n", $1/1e18}'
```

Replace `WALLET_ADDRESS` with the target address. The result is hex — the pipeline converts it to human-readable ETH.

---

## All ERC-20 Token Balances

Get every token a wallet holds in a single call:

```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"alchemy_getTokenBalances","params":["WALLET_ADDRESS"]}' | jq '.result.tokenBalances[] | select(.tokenBalance != "0x0000000000000000000000000000000000000000000000000000000000000000")'
```

This returns contract addresses and raw hex balances. To get readable names/symbols, look up each token with the metadata endpoint below.

### Token Balances with Metadata (Full Portfolio)

To get a complete portfolio with names and human-readable amounts, use this two-step approach:

**Step 1** — Get all non-zero token balances:
```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"alchemy_getTokenBalances","params":["WALLET_ADDRESS"]}' | jq '[.result.tokenBalances[] | select(.tokenBalance != "0x0000000000000000000000000000000000000000000000000000000000000000") | {contract: .contractAddress, balance: .tokenBalance}]'
```

**Step 2** — For each contract address, get token metadata:
```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"alchemy_getTokenMetadata","params":["CONTRACT_ADDRESS"]}' | jq '{name: .result.name, symbol: .result.symbol, decimals: .result.decimals, logo: .result.logo}'
```

Then convert the raw balance: `human_amount = hex_balance / 10^decimals`

---

## Token Metadata

Look up name, symbol, decimals, and logo for any token contract:

```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"alchemy_getTokenMetadata","params":["CONTRACT_ADDRESS"]}' | jq '.result'
```

Example response:
```json
{"decimals": 6, "logo": "https://...", "name": "USD Coin", "symbol": "USDC"}
```

### Common Base Token Contracts

| Token | Contract Address |
|-------|-----------------|
| USDC | `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913` |
| USDbC (bridged) | `0xd9aAEc86B65D86f6A7B5B1b0c42FFA531710b6Da` |
| WETH | `0x4200000000000000000000000000000000000006` |
| DAI | `0x50c5725949A6F0c72E6C4a641F24049A917DB0Cb` |
| cbETH | `0x2Ae3F1Ec7F1F5012CFEab0185bfc7aa3cf0DEc22` |

### Common Ethereum Token Contracts

| Token | Contract Address |
|-------|-----------------|
| USDC | `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48` |
| USDT | `0xdAC17F958D2ee523a2206206994597C13D831ec7` |
| WETH | `0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2` |
| DAI | `0x6B175474E89094C44Da98b954EedeAC495271d0F` |
| LINK | `0x514910771AF9Ca656af840dff83E8264EcF986CA` |

---

## Transfer History

Get recent token transfers to/from a wallet:

### Incoming Transfers

```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"alchemy_getAssetTransfers","params":[{"fromBlock":"0x0","toBlock":"latest","toAddress":"WALLET_ADDRESS","category":["external","internal","erc20"],"withMetadata":true,"maxCount":"0x14","order":"desc"}]}' | jq '.result.transfers[] | {from, to, value, asset, category, timestamp: .metadata.blockTimestamp}'
```

### Outgoing Transfers

```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"alchemy_getAssetTransfers","params":[{"fromBlock":"0x0","toBlock":"latest","fromAddress":"WALLET_ADDRESS","category":["external","internal","erc20"],"withMetadata":true,"maxCount":"0x14","order":"desc"}]}' | jq '.result.transfers[] | {from, to, value, asset, category, timestamp: .metadata.blockTimestamp}'
```

### Parameters

| Param | Description |
|-------|-------------|
| `fromBlock` / `toBlock` | Block range (hex). Use `"0x0"` and `"latest"` for full history. |
| `fromAddress` / `toAddress` | Filter by sender or receiver. Use one at a time. |
| `category` | Array of `"external"`, `"internal"`, `"erc20"`, `"erc721"`, `"erc1155"`, `"specialnft"` |
| `maxCount` | Max results per page (hex). `"0x14"` = 20, `"0x64"` = 100, `"0x3e8"` = 1000. |
| `order` | `"asc"` (oldest first) or `"desc"` (newest first). |
| `withMetadata` | Set `true` to include block timestamps. |
| `pageKey` | Pagination cursor from previous response. |

### ERC-721/1155 Transfers (NFTs)

Include `"erc721"` and `"erc1155"` in the category array:

```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"alchemy_getAssetTransfers","params":[{"fromBlock":"0x0","toBlock":"latest","toAddress":"WALLET_ADDRESS","category":["erc721","erc1155"],"withMetadata":true,"maxCount":"0x14","order":"desc"}]}' | jq '.result.transfers'
```

---

## NFTs Owned by Wallet

```bash
curl -s "https://base-mainnet.g.alchemy.com/nft/v3/$ALCHEMY_API_KEY/getNFTsForOwner?owner=WALLET_ADDRESS&withMetadata=true&pageSize=20" | jq '.ownedNfts[] | {name: .name, collection: .contract.name, tokenId: .tokenId, tokenType: .tokenType, image: .image.thumbnailUrl}'
```

### Get NFTs from a Specific Collection

```bash
curl -s "https://base-mainnet.g.alchemy.com/nft/v3/$ALCHEMY_API_KEY/getNFTsForOwner?owner=WALLET_ADDRESS&contractAddresses[]=NFT_CONTRACT_ADDRESS&withMetadata=true" | jq '.ownedNfts'
```

### NFT Collection Floor Price

```bash
curl -s "https://eth-mainnet.g.alchemy.com/nft/v3/$ALCHEMY_API_KEY/getFloorPrice?contractAddress=NFT_CONTRACT_ADDRESS" | jq '.'
```

Note: Floor price data is most reliable on Ethereum mainnet.

---

## Transaction Details

Look up a specific transaction by hash:

```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionByHash","params":["TX_HASH"]}' | jq '.result | {from, to, value, gasPrice, hash, blockNumber}'
```

### Transaction Receipt (status, gas used, logs)

```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionReceipt","params":["TX_HASH"]}' | jq '.result | {status, gasUsed, blockNumber, logs: (.logs | length)}'
```

Status: `"0x1"` = success, `"0x0"` = reverted.

---

## Latest Block Number

```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"eth_blockNumber","params":[]}' | jq -r '.result' | xargs printf "%d\n"
```

---

## Token Allowances

Check how much of a token a spender is approved to use:

```bash
curl -s -X POST "https://base-mainnet.g.alchemy.com/v2/$ALCHEMY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{"to":"TOKEN_CONTRACT","data":"0xdd62ed3e000000000000000000000000OWNER_NO_PREFIX000000000000000000000000SPENDER_NO_PREFIX"},"latest"]}' | jq -r '.result'
```

`0xdd62ed3e` is the `allowance(address,address)` function selector. Strip the `0x` prefix from both addresses and left-pad to 32 bytes.

---

## Error Handling

| Error | Cause | Fix |
|-------|-------|-----|
| `"INVALID_PARAMS"` | Malformed address or hex | Ensure addresses are checksummed 0x-prefixed, block numbers are hex |
| `"METHOD_NOT_FOUND"` | Wrong endpoint for chain | Some enhanced methods only work on certain chains |
| 429 Too Many Requests | Rate limited | Free tier: 330 requests/second. Wait and retry. |
| Empty `tokenBalances` | Wallet has no ERC-20s | Normal — wallet may only hold native ETH |
| `"execution reverted"` | eth_call failed | Contract may not support the function, or params are wrong |

---

## Tips

- **Always use `jq`** to parse JSON-RPC responses
- **Default to Base** — most Starkbot wallets operate on Base
- **Hex conversion**: block numbers and balances are hex. Use `printf "%d\n"` or `awk` to convert.
- **Batch metadata lookups** — if a wallet holds many tokens, fetch metadata for each contract to build a readable portfolio
- **Combine with token_price skill** — use CoinGecko to get USD values after fetching balances
- **For the bot's own wallet**, use the `get_wallet_address` tool first to get the address

## IMPORTANT: Communicating Results

Format portfolio data clearly for the user. Example:

```
Wallet 0xABC...123 on Base:

  ETH: 0.542 ($1,355.00)
  USDC: 1,200.00 ($1,200.00)
  WETH: 0.100 ($250.00)

  Total: ~$2,805.00

  Recent Activity:
  - Received 500 USDC from 0xDEF...456 (2h ago)
  - Sent 0.1 ETH to 0x789...012 (1d ago)
```

Include:
- Token name/symbol and human-readable balance
- USD value if available (use token_price skill or CoinGecko)
- Recent transfers if the user asked about activity
