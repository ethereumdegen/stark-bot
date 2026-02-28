# Protocol Registry — DeFi Risk Guardian

Reference documentation for all supported Starknet protocols.
Used by the scanner and executor to interface with each protocol correctly.

---

## Nostra Finance

| Field             | Value |
|------------------|-------|
| Type             | Isolated Lending / CDP |
| Chain            | Starknet Mainnet |
| API              | https://api.nostra.finance/graphql |
| Docs             | https://docs.nostra.finance |
| Health Factor    | Yes (1.0 = liquidation threshold) |
| Supported Assets | ETH, WBTC, USDC, USDT, DAI, wstETH, LORDS |
| LLTV             | 80–85% depending on market |
| Liquidation Bonus| 5–10% |
| Scanner Function | `scan_nostra()` |

**Key Contracts:**
- Core: `0x04c0a5193d58f74fbace4b74dcf65481e734ed1714121bdc571da345540efa05`

**Quirks:**
- Nostra uses isolated lending markets — each collateral/debt pair is separate.
- Health factor is computed on-chain; fetch from GraphQL is near real-time.

---

## zkLend

| Field             | Value |
|------------------|-------|
| Type             | Cross-collateral Lending |
| Chain            | Starknet Mainnet |
| API              | https://app.zklend.com/api |
| Docs             | https://docs.zklend.com |
| Health Factor    | Yes (1.0 = liquidation) |
| Supported Assets | ETH, WBTC, USDC, USDT, DAI, STRK |
| LLTV             | 70–85% depending on asset |
| Liquidation Bonus| 8–12% |
| Scanner Function | `scan_zklend()` |

**Key Contracts:**
- Market: `0x04c0a5193d58f74fbace4b74dcf65481e734ed1714121bdc571da345540efa05`

**Quirks:**
- zkLend supports cross-collateralization — multiple assets as collateral.
- Collateral factors differ per asset; use the `/markets` endpoint to fetch current values.

---

## Vesu

| Field             | Value |
|------------------|-------|
| Type             | Modular Lending |
| Chain            | Starknet Mainnet |
| API              | https://api.vesu.xyz |
| Docs             | https://docs.vesu.xyz |
| Health Factor    | Collateral Ratio (inverse of LTV) |
| Supported Assets | ETH, WBTC, USDC, USDT, wstETH |
| Scanner Function | `scan_vesu()` (planned) |

**Quirks:**
- Vesu uses a "singleton" architecture — all pools share one contract.
- Risk params are pool-specific; fetch from `/pools` endpoint.

---

## Opus

| Field             | Value |
|------------------|-------|
| Type             | CDP / Stablecoin (CASH) |
| Chain            | Starknet Mainnet |
| API              | https://api.opus.money |
| Docs             | https://docs.opus.money |
| Ratio            | Collateral Ratio (min 150% for most vaults) |
| Supported Assets | ETH, WBTC, wstETH, STRK |
| Scanner Function | `scan_opus()` (planned) |

**Quirks:**
- Opus mints CASH stablecoin.
- Liquidation is partial (up to 90% of debt) via a Dutch auction.
- Monitor `forge_cap` — minting may be halted if cap is reached.

---

## Ekubo

| Field             | Value |
|------------------|-------|
| Type             | Concentrated Liquidity AMM |
| Chain            | Starknet Mainnet |
| API              | https://mainnet-api.ekubo.org |
| Docs             | https://docs.ekubo.org |
| Health Factor    | N/A — LP positions |
| Risk Metric      | In-range status, IL exposure, fee tier |
| Scanner Function | `scan_ekubo()` |

**Quirks:**
- LP positions earn fees only when in range.
- Out-of-range positions accumulate 100% impermanent loss exposure.
- Monitor `lower_tick`/`upper_tick` vs current tick from the `/pools` endpoint.
- Fee tiers: 0.01%, 0.05%, 0.3%, 1% — higher tier = more fee income but less liquidity competition.

---

## STRK Staking

| Field             | Value |
|------------------|-------|
| Type             | L2 Staking / Delegation |
| Chain            | Starknet Mainnet |
| API              | https://staking.starknet.io/api |
| Docs             | https://docs.starknet.io/staking |
| Slash Risk       | Low (attestation slashing in phase 2) |
| Scanner Function | `scan_strk_staking()` |

**Quirks:**
- STRK staking is non-custodial; the agent monitors but cannot move staked funds without a 21-day unbonding period.
- Phase 2 introduces attestation requirements — validators with low uptime face slashing.
- Monitor `validator_uptime_pct` from the staking API; alert if < 90%.
- Unclaimed rewards are not at risk but should be claimed periodically.

---

## Price Oracles

| Oracle    | URL | Notes |
|-----------|-----|-------|
| Pragma    | https://api.pragma.build/node/v1/data | Primary; Starknet-native, on-chain verified |
| CoinGecko | https://api.coingecko.com/api/v3     | Fallback; free tier, 30-day vol data available |
| Chainlink | (via Starknet bridge)                 | Used by some protocols directly |

**Price Feed Priority:** Pragma → CoinGecko → hardcoded stablecoin ($1.00)

---

## Adding a New Protocol

To add support for a new protocol:

1. Add an entry to this registry with all fields.
2. Implement `scan_<protocol>(wallet, client) -> list[Position]` in `scanner.py`.
3. Implement `build_calldata_<protocol>(action) -> str` in `executor.py`.
4. Add the protocol key to `SCANNERS` in `scanner.py` and `CALLDATA_BUILDERS` in `executor.py`.
5. Test with `--dry-run` before enabling live execution.
6. Submit a PR to the StarkBot skill repository.
