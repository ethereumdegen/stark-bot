# Swap Tokens via Uniswap Trading API

Execute token swaps using Uniswap's official Trading API with optimized routing.

## CRITICAL RULES

1. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task. Do NOT work ahead.
2. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.**
3. **Sequential tool calls only.** Never call two tools in parallel when the second depends on the first.
4. **Always use the Trading API base URL:** `https://trade-api.gateway.uniswap.org/v1`
5. **Always include the API key header:** `"x-api-key": "$UNISWAP_API_KEY"`

---

## Step 1: Define the five tasks

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Prepare: select network, look up sell + buy tokens, resolve addresses. See swap_via_api flow 'Task 1'.",
  "TASK 2 — Check approval: call /check_approval, approve if needed. See swap_via_api flow 'Task 2'.",
  "TASK 3 — Get quote: call /quote to get optimized route and price. See swap_via_api flow 'Task 3'.",
  "TASK 4 — Execute swap: call /swap to get tx calldata, broadcast. See swap_via_api flow 'Task 4'.",
  "TASK 5 — Verify the swap result and report to the user. See swap_via_api flow 'Task 5'."
]}
```

---

## Task 1: Prepare — look up tokens, resolve addresses

### 1a. Select network

```json
{"tool": "select_web3_network", "network": "<network>"}
```

Default to `base` if the user doesn't specify a chain.

### 1b. Look up SELL token

```json
{"tool": "token_lookup", "symbol": "<SELL_TOKEN>", "cache_as": "sell_token"}
```

If selling native ETH, use WETH address for the API call but set `"isNative": true` in later requests.

### 1c. Look up BUY token

```json
{"tool": "token_lookup", "symbol": "<BUY_TOKEN>", "cache_as": "buy_token"}
```

### 1d. Convert amount to raw units

```json
{"tool": "to_raw_amount", "amount": "<human_amount>", "decimals_register": "sell_token_decimals", "cache_as": "sell_amount"}
```

Report token addresses, decimals, and raw amount. Complete task.

---

## Task 2: Check and handle approval

### 2a. Check approval status

```json
{
  "tool": "web_fetch",
  "url": "https://trade-api.gateway.uniswap.org/v1/check_approval",
  "method": "POST",
  "headers": {"x-api-key": "$UNISWAP_API_KEY"},
  "body": {
    "token": "<sell_token_address>",
    "amount": "<raw_sell_amount>",
    "walletAddress": "<wallet_address>",
    "chainId": "<chain_id>"
  },
  "extract_mode": "raw"
}
```

The response includes:
- `approval.isRequired` — whether approval is needed
- `approval.txData` — if needed, the approval transaction data (to, data, value)

### 2b. If approval required, broadcast the approval tx

Use the `txData` from the response to build and broadcast the approval transaction.

If approval is NOT required:

```json
{"tool": "task_fully_completed", "summary": "Token already approved — skipping."}
```

---

## Task 3: Get swap quote

Fetch an optimized route and price quote from Uniswap.

```json
{
  "tool": "web_fetch",
  "url": "https://trade-api.gateway.uniswap.org/v1/quote",
  "method": "POST",
  "headers": {"x-api-key": "$UNISWAP_API_KEY"},
  "body": {
    "type": "EXACT_INPUT",
    "tokenIn": "<sell_token_address>",
    "tokenInChainId": "<chain_id>",
    "tokenOut": "<buy_token_address>",
    "tokenOutChainId": "<chain_id>",
    "amount": "<raw_sell_amount>",
    "swapper": "<wallet_address>",
    "slippageTolerance": 0.5
  },
  "extract_mode": "raw",
  "cache_as": "uniswap_quote"
}
```

**Quote types:**
- `EXACT_INPUT` — specify exact sell amount, get variable buy amount
- `EXACT_OUTPUT` — specify exact buy amount, get variable sell amount

The response includes:
- `quote.output.amount` — expected output amount
- `quote.output.amountReadable` — human-readable output
- `quote.gasFeeUSD` — estimated gas cost in USD
- `routing` — CLASSIC, DUTCH_V2, etc.

Report the quote details to the user (expected output, price impact, gas estimate). Ask for confirmation before proceeding.

```json
{"tool": "task_fully_completed", "summary": "Quote received: <output_amount> <buy_token>. User confirmed."}
```

---

## Task 4: Execute the swap

### 4a. Get executable transaction

```json
{
  "tool": "web_fetch",
  "url": "https://trade-api.gateway.uniswap.org/v1/swap",
  "method": "POST",
  "headers": {"x-api-key": "$UNISWAP_API_KEY"},
  "body": {
    "type": "EXACT_INPUT",
    "tokenIn": "<sell_token_address>",
    "tokenInChainId": "<chain_id>",
    "tokenOut": "<buy_token_address>",
    "tokenOutChainId": "<chain_id>",
    "amount": "<raw_sell_amount>",
    "swapper": "<wallet_address>",
    "slippageTolerance": 0.5
  },
  "extract_mode": "raw",
  "cache_as": "uniswap_swap_tx"
}
```

The response includes a `swap` object with `{to, data, value}` — the ready-to-sign transaction.

### 4b. Broadcast the transaction

Use the transaction data from the response to broadcast via the bot's web3 infrastructure.

```json
{"tool": "broadcast_web3_tx", "uuid": "<uuid>"}
```

Wait for confirmation.

```json
{"tool": "task_fully_completed", "summary": "Swap transaction confirmed on-chain."}
```

---

## Task 5: Verify and report

```json
{"tool": "verify_tx_broadcast"}
```

Report the result:
- **VERIFIED**: Swap succeeded. Report tx hash, explorer link, amounts swapped.
- **REVERTED**: Swap failed. Possible reasons: slippage exceeded, insufficient balance, approval expired.
- **TIMEOUT**: Tell user to check explorer.

```json
{"tool": "task_fully_completed", "summary": "Swap completed: <sell_amount> <sell_token> -> <buy_amount> <buy_token>."}
```

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| `INSUFFICIENT_BALANCE` | Not enough sell token | Check balance, reduce amount |
| `NO_ROUTE_FOUND` | No liquidity path exists | Try a different pair or chain |
| `SLIPPAGE_TOO_HIGH` | Price impact too large | Reduce amount or increase slippage tolerance |
| `APPROVAL_REQUIRED` | Token not approved for Permit2 | Run approval flow (Task 2) |
| `API_KEY_INVALID` | Missing or bad API key | Check $UNISWAP_API_KEY env var |
