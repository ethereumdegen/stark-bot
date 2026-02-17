# Safety Features

This document describes two key safety mechanisms in the stark-backend that help prevent AI hallucination and improve data integrity during agent operations.

## Register Store

**Location:** `src/tools/register.rs`

The Register Store is a CPU-like "register" system that allows tool outputs to be cached and retrieved by other tools *without flowing through the agent's reasoning*. This is critical for financial transactions where data integrity must be preserved.

### The Problem

When an agent processes a swap quote (e.g., from 0x API), the quote contains critical data like:
- `to` address (contract to call)
- `data` (encoded calldata)
- `value` (ETH amount to send)

If this data passes through the agent's reasoning to be used by another tool, there's a risk of hallucination - the agent might subtly modify addresses, amounts, or calldata, leading to failed transactions or loss of funds.

### The Solution

Tools can write their outputs directly to named registers, and other tools can read from those registers - bypassing the agent entirely for critical data transfer.

```rust
// Tool 1 (e.g., x402_preset_fetch) caches its output
context.registers.set("swap_quote", json!({
    "to": "0x...",
    "data": "0x...",
    "value": "1000000000000000"
}), "x402_preset_fetch");

// Tool 2 (e.g., web3_tx) reads from the register
let quote = context.registers.get("swap_quote")?;
let to = quote.get("to").unwrap();
```

### Key Features

| Feature | Description |
|---------|-------------|
| **Thread-safe** | Uses `Arc<RwLock<>>` for concurrent access |
| **Metadata tracking** | Stores source tool name and creation timestamp |
| **Nested field access** | Can retrieve nested fields via dot notation (`quote.transaction.data`) |
| **Staleness detection** | Can check if a register is older than a threshold |
| **Intrinsic registers** | Special registers like `wallet_address` that are lazily computed |
| **UI broadcasting** | Register updates are broadcast to connected clients via WebSocket |

### Blocked Registers

Some registers cannot be set via the `register_set` tool and must use specific tools instead:

| Register | Required Tool |
|----------|---------------|
| `sell_token` | `token_lookup` with `cache_as: 'sell_token'` |
| `buy_token` | `token_lookup` with `cache_as: 'buy_token'` |
| `wallet_address` | Intrinsic - auto-resolved from wallet config |

### PresetOrCustom Pattern

The codebase uses a `PresetOrCustom<T>` monad that enforces mutual exclusivity at the type level - a tool parameter can either read from a preset register OR use custom values, but never both:

```rust
#[derive(Deserialize)]
struct MyToolParams {
    #[serde(flatten)]
    mode: PresetOrCustom<CustomParams>,
    network: String,
}
```

---

## Context Bank

**Location:** `src/tools/context_bank.rs`

The Context Bank automatically extracts and stores key terms from user input, making them available to the agent without relying on the agent to correctly parse them.

### What It Scans For

| Item Type | Pattern | Example |
|-----------|---------|---------|
| Ethereum addresses | `0x` + 40 hex chars | `0x742d35Cc6634C0532925a3b844Bc9e7595f8FdF0` |
| Token symbols | Whole-word matches from `config/tokens.ron` | `ETH`, `USDC`, `WBTC` |

### How It Works

1. When a user message arrives, `scan_input()` is called on the text
2. Regex patterns extract Ethereum addresses
3. Token symbols are matched as whole words against the configured token list
4. Detected items are stored in a thread-safe `HashSet`
5. The context bank is included in the agent's system context
6. Updates are broadcast to the UI via WebSocket

### ContextBankItem Structure

```rust
pub struct ContextBankItem {
    pub value: String,        // The detected value
    pub item_type: String,    // "eth_address" or "token_symbol"
    pub label: Option<String>, // Optional extra info (e.g., token name)
}
```

### Agent Context Format

When included in the agent's context, the context bank formats like:

```
Addresses: 0x742d35cc..., 0xa0b86991...
Tokens: ETH (Ethereum), USDC (USD Coin)
```

### Why This Matters

Without the context bank, the agent must parse addresses and token symbols from the user's message, which can lead to:
- Typos in addresses (catastrophic for transactions)
- Confusion between similar token symbols
- Missing addresses mentioned early in a conversation

The context bank provides a reliable, pre-parsed reference that tools and the agent can trust.

---

## Integration

Both systems are integrated into `ToolContext`, which is passed to every tool during execution:

```rust
pub struct ToolContext {
    // ... other fields ...

    /// Register store for passing data between tools safely
    pub registers: RegisterStore,

    /// Context bank for key terms extracted from user input
    pub context_bank: ContextBank,
}
```

Tools access them via the context:

```rust
async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
    // Read from register
    let quote = context.registers.get("swap_quote")?;

    // Check context bank
    if let Some(formatted) = context.context_bank.format_for_agent() {
        // Use extracted terms
    }

    // Write to register (with automatic UI broadcast)
    context.set_register("result", json!({...}), "my_tool");
}
```
