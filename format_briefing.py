#!/usr/bin/env python3
"""
format_briefing.py
Reads /tmp/wallet_balances.json, fetches prices from CoinGecko,
and prints a formatted daily wallet briefing to stdout.

Required env vars:
  WALLET_ADDRESS     — for display / Etherscan link
Optional env vars:
  CURRENCY           — fiat currency (default: usd)
  TOP_N              — number of top tokens to show (default: 10)
  COINGECKO_API_KEY  — if set, uses authenticated endpoint (higher rate limits)
"""

import json
import os
import sys
import time
import urllib.request
import urllib.parse
from datetime import datetime, timezone

# ── Config ────────────────────────────────────────────────────────────────────
INPUT_FILE = "/tmp/wallet_balances.json"
CURRENCY = os.environ.get("CURRENCY", "usd").lower()
TOP_N = int(os.environ.get("TOP_N", "10"))
CG_API_KEY = os.environ.get("COINGECKO_API_KEY", "")

CURRENCY_SYMBOLS = {"usd": "$", "eur": "€", "gbp": "£"}
CURR_SYM = CURRENCY_SYMBOLS.get(CURRENCY, CURRENCY.upper() + " ")

# ── CoinGecko symbol → coin ID map (extend as needed) ────────────────────────
# For unknown symbols the script will try a CoinGecko search fallback.
KNOWN_IDS = {
    "ETH": "ethereum",
    "BTC": "bitcoin",
    "WBTC": "wrapped-bitcoin",
    "WETH": "weth",
    "USDC": "usd-coin",
    "USDT": "tether",
    "DAI": "dai",
    "LINK": "chainlink",
    "UNI": "uniswap",
    "AAVE": "aave",
    "MKR": "maker",
    "SNX": "synthetix-network-token",
    "CRV": "curve-dao-token",
    "MATIC": "matic-network",
    "OP": "optimism",
    "ARB": "arbitrum",
    "LDO": "lido-dao",
    "RPL": "rocket-pool",
    "PEPE": "pepe",
    "SHIB": "shiba-inu",
    "STRK": "starknet",
}


def cg_get(path: str, params: dict) -> dict | None:
    """Call CoinGecko API; returns parsed JSON or None on error."""
    if CG_API_KEY:
        base = "https://pro-api.coingecko.com/api/v3"
        params["x_cg_pro_api_key"] = CG_API_KEY
    else:
        base = "https://api.coingecko.com/api/v3"

    url = f"{base}{path}?{urllib.parse.urlencode(params)}"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "starkbot-wallet-briefing/1.0"})
        with urllib.request.urlopen(req, timeout=10) as resp:
            return json.loads(resp.read())
    except Exception as e:
        print(f"  [warn] CoinGecko request failed: {e}", file=sys.stderr)
        return None


def resolve_coin_id(symbol: str) -> str | None:
    """Look up a CoinGecko coin ID by symbol, using known map first."""
    if symbol.upper() in KNOWN_IDS:
        return KNOWN_IDS[symbol.upper()]
    # Fallback: search by symbol
    result = cg_get("/search", {"query": symbol})
    if result and result.get("coins"):
        # Return the first exact match (case-insensitive)
        for coin in result["coins"][:5]:
            if coin.get("symbol", "").upper() == symbol.upper():
                return coin["id"]
    return None


def fetch_prices(coin_ids: list[str]) -> dict:
    """Fetch current prices + 24h change for a list of coin IDs."""
    if not coin_ids:
        return {}
    ids_str = ",".join(coin_ids)
    data = cg_get(
        "/simple/price",
        {
            "ids": ids_str,
            "vs_currencies": CURRENCY,
            "include_24hr_change": "true",
        },
    )
    return data or {}


def fmt_value(val: float) -> str:
    """Format a fiat value with commas and 2 decimal places."""
    return f"{CURR_SYM}{val:,.2f}"


def fmt_change(pct: float | None) -> str:
    """Format a percentage change with an arrow."""
    if pct is None:
        return ""
    arrow = "▲" if pct >= 0 else "▼"
    return f"  ({arrow} {abs(pct):.1f}% 24h)"


def truncate_address(addr: str) -> str:
    return f"{addr[:6]}...{addr[-4:]}" if len(addr) > 12 else addr


# ── Main ──────────────────────────────────────────────────────────────────────
def main():
    # Load balance data
    if not os.path.exists(INPUT_FILE):
        print(f"ERROR: {INPUT_FILE} not found. Run fetch_balances.sh first.", file=sys.stderr)
        sys.exit(1)

    with open(INPUT_FILE) as f:
        data = json.load(f)

    wallet = data["wallet"]
    chain = data["chain"]
    fetched_at = data.get("fetched_at", datetime.now(timezone.utc).isoformat())
    native = data["native"]
    tokens = data.get("tokens", [])

    # Build a list of all holdings: (symbol, balance)
    holdings = [(native["symbol"], native["balance"])]
    for t in tokens:
        sym = t.get("symbol") or t.get("tokenSymbol", "?")
        bal = float(t.get("balance", 0))
        if bal > 0:
            holdings.append((sym, bal))

    # Resolve coin IDs and fetch prices
    id_map = {}  # symbol -> coin_id
    for sym, _ in holdings:
        cid = resolve_coin_id(sym)
        if cid:
            id_map[sym] = cid
        time.sleep(0.15)  # be polite to free tier

    coin_ids = list(set(id_map.values()))
    prices = fetch_prices(coin_ids)  # {coin_id: {currency: price, currency_24h_change: pct}}

    # Compute USD values
    enriched = []
    for sym, bal in holdings:
        cid = id_map.get(sym)
        price = None
        change_24h = None
        usd_value = None
        if cid and cid in prices:
            price = prices[cid].get(CURRENCY)
            change_24h = prices[cid].get(f"{CURRENCY}_24h_change")
            if price is not None:
                usd_value = bal * price

        enriched.append({
            "symbol": sym,
            "balance": bal,
            "price": price,
            "usd_value": usd_value,
            "change_24h": change_24h,
        })

    # Sort by USD value descending, unknowns at bottom
    enriched.sort(key=lambda x: (x["usd_value"] is None, -(x["usd_value"] or 0)))

    total_value = sum(h["usd_value"] for h in enriched if h["usd_value"] is not None)

    # ── Weighted 24h portfolio change ─────────────────────────────────────────
    weighted_change = None
    if total_value > 0:
        weighted_sum = sum(
            h["change_24h"] * h["usd_value"]
            for h in enriched
            if h["change_24h"] is not None and h["usd_value"] is not None
        )
        coverage = sum(h["usd_value"] for h in enriched if h["change_24h"] is not None)
        if coverage > 0:
            weighted_change = weighted_sum / coverage

    # ── Alerts ────────────────────────────────────────────────────────────────
    alerts = []
    # Low native balance
    if native["symbol"] == "ETH" and native["balance"] < 0.05:
        alerts.append(f"⚠️  Low ETH balance ({native['balance']:.4f} ETH) — may run low on gas")
    # Large single-asset change
    for h in enriched[:5]:
        if h["change_24h"] is not None and abs(h["change_24h"]) >= 5:
            direction = "up" if h["change_24h"] > 0 else "down"
            alerts.append(f"📈 {h['symbol']} {direction} {abs(h['change_24h']):.1f}% in 24h")
    # Stablecoin dominance warning (>80%)
    stable_syms = {"USDC", "USDT", "DAI", "FRAX", "LUSD", "BUSD", "TUSD"}
    stable_value = sum(h["usd_value"] for h in enriched if h["symbol"] in stable_syms and h["usd_value"])
    if total_value > 0 and stable_value / total_value > 0.80:
        alerts.append(f"💤 Portfolio is {stable_value/total_value*100:.0f}% stablecoins — fully deployed?")

    # ── Chain-specific Etherscan URL ──────────────────────────────────────────
    explorer_urls = {
        "ethereum": "https://etherscan.io",
        "arbitrum": "https://arbiscan.io",
        "base": "https://basescan.org",
        "optimism": "https://optimistic.etherscan.io",
    }
    explorer = explorer_urls.get(chain, "https://etherscan.io")

    # ── Format output ─────────────────────────────────────────────────────────
    display_addr = truncate_address(wallet)
    ts = fetched_at[:16].replace("T", " ") + " UTC"
    chain_label = chain.capitalize()

    lines = [
        "",
        f"📊 Wallet Briefing — {display_addr}",
        f"🕐 {ts}  |  Chain: {chain_label}",
        "",
    ]

    if total_value > 0:
        change_str = fmt_change(weighted_change)
        lines.append(f"💼 Total Portfolio Value: {fmt_value(total_value)}{change_str}")
    else:
        lines.append("💼 Total Portfolio Value: (prices unavailable)")

    lines += ["", "Top Holdings:"]

    for i, h in enumerate(enriched[:TOP_N], 1):
        sym = h["symbol"].ljust(8)
        bal = f"{h['balance']:,.4f}".rjust(14)
        if h["usd_value"] is not None:
            val = fmt_value(h["usd_value"]).rjust(14)
            pct = f"({h['usd_value']/total_value*100:.1f}%)" if total_value else ""
            lines.append(f"  {i:>2}. {sym}{bal}   {val}  {pct}")
        else:
            lines.append(f"  {i:>2}. {sym}{bal}   (price unavailable)")

    if len(enriched) > TOP_N:
        rest = len(enriched) - TOP_N
        lines.append(f"       ... and {rest} more token(s)")

    if alerts:
        lines += ["", "⚠️  Alerts:"]
        for alert in alerts:
            lines.append(f"  - {alert}")

    lines += [
        "",
        f"🔗 View on explorer: {explorer}/address/{wallet}",
        "",
    ]

    print("\n".join(lines))


if __name__ == "__main__":
    main()
