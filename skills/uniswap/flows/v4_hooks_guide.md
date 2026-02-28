# Uniswap V4 Hooks Security Guide

Security-first reference for building and auditing Uniswap V4 hooks. Based on the Uniswap AI security foundations.

## No tasks needed — reference material

This is a security reference guide. Use it to answer questions about V4 hooks, review hook code, or guide hook development.

---

## What Are V4 Hooks?

Hooks are smart contracts that execute custom logic at specific points in a pool's lifecycle. They attach to pools via the PoolManager and can modify swap behavior, fees, liquidity management, and more.

**PoolManager:** The singleton contract that manages all V4 pools. All hooks interact with the PoolManager.

---

## Hook Permission Flags

Each hook declares which lifecycle points it will be called at. These are encoded in the hook contract's address (specific bit positions).

| Flag | Risk Level | Description |
|------|-----------|-------------|
| `BEFORE_INITIALIZE` | LOW | Called before pool creation. Can validate pool parameters. |
| `AFTER_INITIALIZE` | LOW | Called after pool creation. Can set up hook state. |
| `BEFORE_ADD_LIQUIDITY` | MEDIUM | Called before liquidity addition. Can block or modify. |
| `AFTER_ADD_LIQUIDITY` | MEDIUM | Called after liquidity addition. Can take fees on deposits. |
| `BEFORE_REMOVE_LIQUIDITY` | HIGH | Called before liquidity removal. Can block withdrawals (rug risk). |
| `AFTER_REMOVE_LIQUIDITY` | MEDIUM | Called after liquidity removal. Can take exit fees. |
| `BEFORE_SWAP` | HIGH | Called before swap execution. Can modify swap parameters. |
| `AFTER_SWAP` | MEDIUM | Called after swap execution. Can take fees on output. |
| `BEFORE_DONATE` | LOW | Called before fee donation. |
| `AFTER_DONATE` | LOW | Called after fee donation. |
| `BEFORE_SWAP_RETURNS_DELTA` | CRITICAL | Allows hook to return a delta (modify swap amounts). Can steal ALL swap input. |
| `AFTER_SWAP_RETURNS_DELTA` | HIGH | Allows hook to modify output amounts after swap. |
| `AFTER_ADD_LIQUIDITY_RETURNS_DELTA` | MEDIUM | Allows hook to modify liquidity delta. |
| `AFTER_REMOVE_LIQUIDITY_RETURNS_DELTA` | HIGH | Allows hook to modify withdrawal amounts. |

---

## CRITICAL: The NoOp Rug Pull Attack

**`BEFORE_SWAP_RETURNS_DELTA` is the most dangerous permission.**

A malicious hook with this permission can:
1. Intercept the swap in `beforeSwap`
2. Return a delta that takes ALL input tokens
3. Return `0` output tokens
4. The swap "succeeds" but the user receives nothing

### How it works:

```solidity
function beforeSwap(address, PoolKey calldata, IPoolManager.SwapParams calldata params, bytes calldata)
    external override returns (bytes4, BeforeSwapDelta, uint24)
{
    // MALICIOUS: Take all input, give nothing back
    int128 amountSpecified = int128(params.amountSpecified);
    BeforeSwapDelta delta = toBeforeSwapDelta(amountSpecified, 0);
    return (this.beforeSwap.selector, delta, 0);
}
```

### Red flags to watch for:

- Any hook using `BEFORE_SWAP_RETURNS_DELTA` without clear justification
- Hook contracts that are upgradeable or have admin-controlled behavior
- Hooks that interact with external contracts in `beforeSwap`
- Hooks without source code verification

---

## Security Checklist

### For users interacting with hooked pools:

1. Is the hook contract verified on the block explorer?
2. Does the hook use `BEFORE_SWAP_RETURNS_DELTA`? If yes, understand exactly why.
3. Does the hook use `BEFORE_REMOVE_LIQUIDITY`? Could it block withdrawals?
4. Is the hook contract upgradeable? Who controls upgrades?
5. Are there admin/owner functions that can change behavior?
6. Has the hook been audited by a reputable firm?

### For developers building hooks:

1. **Minimize permissions** — only request the flags you actually need
2. **Access control** — verify the caller is the PoolManager
3. **Router verification** — validate that the sender (router) is legitimate
4. **Token handling** — never hold user tokens longer than a single callback
5. **Gas budget** — keep hook logic under ~500k gas to avoid griefing
6. **Reentrancy** — use reentrancy guards, especially with external calls
7. **Delta accounting** — ensure deltas are balanced; the PoolManager enforces this

---

## Base Hook Template

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {BaseHook} from "v4-periphery/src/utils/BaseHook.sol";
import {Hooks} from "v4-core/src/libraries/Hooks.sol";
import {IPoolManager} from "v4-core/src/interfaces/IPoolManager.sol";
import {PoolKey} from "v4-core/src/types/PoolKey.sol";
import {BeforeSwapDelta, BeforeSwapDeltaLibrary} from "v4-core/src/types/BeforeSwapDelta.sol";

contract MyHook is BaseHook {
    constructor(IPoolManager _poolManager) BaseHook(_poolManager) {}

    function getHookPermissions() public pure override returns (Hooks.Permissions memory) {
        return Hooks.Permissions({
            beforeInitialize: false,
            afterInitialize: false,
            beforeAddLiquidity: false,
            afterAddLiquidity: false,
            beforeRemoveLiquidity: false,
            afterRemoveLiquidity: false,
            beforeSwap: true,           // Only request what you need
            afterSwap: false,
            beforeDonate: false,
            afterDonate: false,
            beforeSwapReturnDelta: false, // AVOID unless absolutely necessary
            afterSwapReturnDelta: false,
            afterAddLiquidityReturnDelta: false,
            afterRemoveLiquidityReturnDelta: false
        });
    }

    function beforeSwap(
        address sender,
        PoolKey calldata key,
        IPoolManager.SwapParams calldata params,
        bytes calldata hookData
    ) external override returns (bytes4, BeforeSwapDelta, uint24) {
        // Verify caller is PoolManager
        require(msg.sender == address(poolManager), "Not PoolManager");

        // Your custom logic here
        // ...

        return (this.beforeSwap.selector, BeforeSwapDeltaLibrary.ZERO_DELTA, 0);
    }
}
```

---

## Risk Scoring System

When evaluating a hook's risk, score each permission:

| Risk Level | Score | Action |
|-----------|-------|--------|
| LOW | 1 | Generally safe |
| MEDIUM | 2 | Review implementation |
| HIGH | 3 | Careful audit required |
| CRITICAL | 5 | Must fully understand before interacting |

**Total risk = sum of active permission scores.**

| Total Score | Assessment |
|-------------|-----------|
| 1-3 | Low risk — standard hook |
| 4-6 | Moderate risk — review recommended |
| 7-9 | High risk — audit required |
| 10+ | Critical risk — proceed with extreme caution |

---

## Key V4 Contracts

| Contract | Description |
|----------|-------------|
| PoolManager | Singleton managing all pools |
| PositionManager | Manages LP positions (ERC-721) |
| StateView | Read-only pool state queries |
| BaseHook | Base class for hook contracts |
| Permit2 | Token approval management |

---

## Resources

- [Uniswap V4 Docs](https://docs.uniswap.org/contracts/v4/overview)
- [v4-core GitHub](https://github.com/Uniswap/v4-core)
- [v4-periphery GitHub](https://github.com/Uniswap/v4-periphery)
- [Hook Examples](https://github.com/Uniswap/v4-periphery/tree/main/contracts/hooks)
- [Uniswap AI Tools](https://github.com/Uniswap/uniswap-ai)
