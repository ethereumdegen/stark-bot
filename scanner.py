#!/usr/bin/env python3
"""
scanner.py — DeFi Risk Guardian
Fetches open positions from all supported Starknet protocols
for a given wallet address.

Supports: Nostra, zkLend, Vesu, Opus, Ekubo, STRK Staking
"""

from __future__ import annotations

import asyncio
import json
import os
import time
from dataclasses import dataclass, field, asdict
from typing import Literal, Optional
import httpx

# ──────────────────────────────────────────────
# Types
# ──────────────────────────────────────────────

PositionType = Literal["lending", "borrowing", "lp", "staking", "cdp"]

PRICE_CACHE: dict[str, tuple[float, float]] = {}   # asset → (price, timestamp)
PRICE_TTL = 30  # seconds


@dataclass
class Position:
    protocol: str
    position_type: PositionType
    collateral_asset: str
    collateral_amount: float
    collateral_usd: float
    debt_asset: Optional[str]
    debt_amount: Optional[float]
    debt_usd: Optional[float]
    health_factor: Optional[float]
    liquidation_price: Optional[float]
    current_price: Optional[float]
    lltv: Optional[float]         # Liquidation LTV (e.g. 0.85 for 85%)
    extra: dict = field(default_factory=dict)

    def distance_to_liquidation_pct(self) -> Optional[float]:
        if self.liquidation_price and self.current_price:
            return abs(self.current_price - self.liquidation_price) / self.current_price * 100
        return None

    def to_dict(self) -> dict:
        d = asdict(self)
        d["distance_to_liq_pct"] = self.distance_to_liquidation_pct()
        return d


# ──────────────────────────────────────────────
# Price Oracle (Pragma + fallback to CoinGecko)
# ──────────────────────────────────────────────

PRAGMA_BASE = "https://api.pragma.build/node/v1/data"
COINGECKO_IDS = {
    "ETH": "ethereum",
    "WBTC": "wrapped-bitcoin",
    "BTC": "bitcoin",
    "STRK": "starknet",
    "USDC": "usd-coin",
    "USDT": "tether",
    "DAI": "dai",
    "wstETH": "wrapped-steth",
    "LORDS": "lords",
    "EKUBO": "ekubo-protocol",
}


async def get_price(asset: str, client: httpx.AsyncClient) -> float:
    """Return USD price of asset, using cache if fresh."""
    now = time.time()
    if asset in PRICE_CACHE:
        price, ts = PRICE_CACHE[asset]
        if now - ts < PRICE_TTL:
            return price

    if asset in ("USDC", "USDT", "DAI"):
        PRICE_CACHE[asset] = (1.0, now)
        return 1.0

    # Try Pragma first
    try:
        pair = f"{asset}/USD"
        r = await client.get(f"{PRAGMA_BASE}/spot/latest?pair={pair}", timeout=5)
        if r.status_code == 200:
            data = r.json()
            price = float(data["price"]) / 10 ** data["decimals"]
            PRICE_CACHE[asset] = (price, now)
            return price
    except Exception:
        pass

    # Fallback: CoinGecko
    cg_id = COINGECKO_IDS.get(asset)
    if cg_id:
        try:
            r = await client.get(
                f"https://api.coingecko.com/api/v3/simple/price?ids={cg_id}&vs_currencies=usd",
                timeout=8,
            )
            if r.status_code == 200:
                price = r.json()[cg_id]["usd"]
                PRICE_CACHE[asset] = (price, now)
                return price
        except Exception:
            pass

    raise ValueError(f"Could not fetch price for {asset}")


# ──────────────────────────────────────────────
# Protocol Adapters
# ──────────────────────────────────────────────

RPC_URL = os.getenv("STARKNET_RPC", "https://starknet-mainnet.public.blastapi.io")

PROTOCOL_APIS = {
    "nostra":  "https://api.nostra.finance/graphql",
    "zklend":  "https://app.zklend.com/api",
    "vesu":    "https://api.vesu.xyz",
    "opus":    "https://api.opus.money",
    "ekubo":   "https://mainnet-api.ekubo.org",
    "staking": "https://staking.starknet.io/api",
}


async def scan_nostra(wallet: str, client: httpx.AsyncClient) -> list[Position]:
    """Scan Nostra Finance lending/borrowing positions."""
    positions: list[Position] = []
    query = """
    query UserPositions($address: String!) {
      userPositions(address: $address) {
        market { collateralAsset debtAsset lltv }
        collateralAmount
        debtAmount
        healthFactor
      }
    }
    """
    try:
        r = await client.post(
            PROTOCOL_APIS["nostra"],
            json={"query": query, "variables": {"address": wallet}},
            timeout=10,
        )
        r.raise_for_status()
        data = r.json().get("data", {}).get("userPositions", [])
    except Exception as e:
        print(f"[nostra] scan failed: {e}")
        return []

    for pos in data:
        if pos["collateralAmount"] == 0 and pos["debtAmount"] == 0:
            continue
        col_asset = pos["market"]["collateralAsset"]
        dbt_asset = pos["market"]["debtAsset"]
        col_price = await get_price(col_asset, client)
        dbt_price = await get_price(dbt_asset, client)
        col_usd = pos["collateralAmount"] * col_price
        dbt_usd = pos["debtAmount"] * dbt_price
        hf = float(pos["healthFactor"])
        lltv = float(pos["market"]["lltv"])
        # Liquidation price = debt / (collateral_amount * lltv)
        liq_price = dbt_usd / (pos["collateralAmount"] * lltv) if pos["collateralAmount"] else None

        positions.append(Position(
            protocol="nostra",
            position_type="borrowing" if pos["debtAmount"] > 0 else "lending",
            collateral_asset=col_asset,
            collateral_amount=pos["collateralAmount"],
            collateral_usd=col_usd,
            debt_asset=dbt_asset,
            debt_amount=pos["debtAmount"],
            debt_usd=dbt_usd,
            health_factor=hf,
            liquidation_price=liq_price,
            current_price=col_price,
            lltv=lltv,
        ))

    return positions


async def scan_zklend(wallet: str, client: httpx.AsyncClient) -> list[Position]:
    """Scan zkLend lending/borrowing positions."""
    positions: list[Position] = []
    try:
        r = await client.get(
            f"{PROTOCOL_APIS['zklend']}/users/{wallet}/positions",
            timeout=10,
        )
        r.raise_for_status()
        data = r.json()
    except Exception as e:
        print(f"[zklend] scan failed: {e}")
        return []

    for pos in data.get("positions", []):
        if pos.get("collateral_amount", 0) == 0:
            continue
        col_asset = pos["collateral_token"]
        dbt_asset = pos.get("debt_token")
        col_price = await get_price(col_asset, client)
        col_usd = pos["collateral_amount"] * col_price
        dbt_usd = 0.0
        dbt_price = 1.0

        if dbt_asset and pos.get("debt_amount", 0) > 0:
            dbt_price = await get_price(dbt_asset, client)
            dbt_usd = pos["debt_amount"] * dbt_price

        hf = pos.get("health_factor")
        lltv = pos.get("liquidation_threshold", 0.8)
        liq_price = (dbt_usd / (pos["collateral_amount"] * lltv)) if pos.get("debt_amount") else None

        positions.append(Position(
            protocol="zklend",
            position_type="borrowing" if dbt_usd > 0 else "lending",
            collateral_asset=col_asset,
            collateral_amount=pos["collateral_amount"],
            collateral_usd=col_usd,
            debt_asset=dbt_asset,
            debt_amount=pos.get("debt_amount"),
            debt_usd=dbt_usd,
            health_factor=float(hf) if hf else None,
            liquidation_price=liq_price,
            current_price=col_price,
            lltv=lltv,
        ))

    return positions


async def scan_ekubo(wallet: str, client: httpx.AsyncClient) -> list[Position]:
    """Scan Ekubo AMM liquidity positions."""
    positions: list[Position] = []
    try:
        r = await client.get(
            f"{PROTOCOL_APIS['ekubo']}/positions?owner={wallet}",
            timeout=10,
        )
        r.raise_for_status()
        data = r.json()
    except Exception as e:
        print(f"[ekubo] scan failed: {e}")
        return []

    for lp in data.get("positions", []):
        t0 = lp["token0"]["symbol"]
        t1 = lp["token1"]["symbol"]
        p0 = await get_price(t0, client)
        p1 = await get_price(t1, client)
        col_usd = lp["amount0"] * p0 + lp["amount1"] * p1

        positions.append(Position(
            protocol="ekubo",
            position_type="lp",
            collateral_asset=f"{t0}/{t1}",
            collateral_amount=1.0,
            collateral_usd=col_usd,
            debt_asset=None,
            debt_amount=None,
            debt_usd=None,
            health_factor=None,
            liquidation_price=None,
            current_price=p0,
            lltv=None,
            extra={
                "lower_tick": lp.get("lower_tick"),
                "upper_tick": lp.get("upper_tick"),
                "in_range": lp.get("in_range", True),
                "fee_tier": lp.get("fee"),
            }
        ))

    return positions


async def scan_strk_staking(wallet: str, client: httpx.AsyncClient) -> list[Position]:
    """Scan STRK staking positions."""
    positions: list[Position] = []
    try:
        r = await client.get(
            f"{PROTOCOL_APIS['staking']}/delegators/{wallet}",
            timeout=10,
        )
        r.raise_for_status()
        data = r.json()
    except Exception as e:
        print(f"[staking] scan failed: {e}")
        return []

    strk_price = await get_price("STRK", client)
    for stake in data.get("delegations", []):
        amount = stake.get("staked_amount", 0)
        positions.append(Position(
            protocol="strk-staking",
            position_type="staking",
            collateral_asset="STRK",
            collateral_amount=amount,
            collateral_usd=amount * strk_price,
            debt_asset=None,
            debt_amount=None,
            debt_usd=None,
            health_factor=None,
            liquidation_price=None,
            current_price=strk_price,
            lltv=None,
            extra={
                "validator": stake.get("validator_address"),
                "validator_uptime_pct": stake.get("uptime_pct", 100),
                "unclaimed_rewards_strk": stake.get("unclaimed_rewards", 0),
            }
        ))

    return positions


# ──────────────────────────────────────────────
# Main Scanner
# ──────────────────────────────────────────────

SCANNERS = {
    "nostra":       scan_nostra,
    "zklend":       scan_zklend,
    "ekubo":        scan_ekubo,
    "strk-staking": scan_strk_staking,
}


async def scan_all(wallet: str, protocols: list[str] | None = None) -> list[Position]:
    """
    Discover all open positions for wallet across specified protocols.
    If protocols is None, scans all supported protocols.
    """
    targets = protocols or list(SCANNERS.keys())
    async with httpx.AsyncClient(headers={"User-Agent": "StarkBot-Guardian/1.0"}) as client:
        tasks = [SCANNERS[p](wallet, client) for p in targets if p in SCANNERS]
        results = await asyncio.gather(*tasks, return_exceptions=True)

    positions: list[Position] = []
    for r in results:
        if isinstance(r, Exception):
            print(f"[scanner] protocol error: {r}")
            continue
        positions.extend(r)

    # Filter out dust positions (< $1 USD)
    return [p for p in positions if p.collateral_usd >= 1.0]


# ──────────────────────────────────────────────
# CLI Entry Point
# ──────────────────────────────────────────────

if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="DeFi Risk Guardian — Position Scanner")
    parser.add_argument("--wallet", required=True, help="Starknet wallet address (0x...)")
    parser.add_argument("--protocols", nargs="*", help="Protocols to scan (default: all)")
    parser.add_argument("--json", action="store_true", help="Output raw JSON")
    args = parser.parse_args()

    positions = asyncio.run(scan_all(args.wallet, args.protocols))

    if args.json:
        print(json.dumps([p.to_dict() for p in positions], indent=2))
    else:
        print(f"\n🔍 Found {len(positions)} position(s) for {args.wallet[:12]}...\n")
        for p in positions:
            hf_str = f"HF={p.health_factor:.2f}" if p.health_factor else "No HF (LP/Staking)"
            liq_str = f"Liq@${p.liquidation_price:,.0f}" if p.liquidation_price else ""
            buf_str = f"({p.distance_to_liquidation_pct():.1f}% buffer)" if p.distance_to_liquidation_pct() else ""
            print(f"  [{p.protocol}] {p.collateral_asset} ${p.collateral_usd:,.0f}  {hf_str}  {liq_str} {buf_str}")
