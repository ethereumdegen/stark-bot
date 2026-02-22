#!/usr/bin/env python3
"""GeckoTerminal — Search pools and return price data with chart links.

Usage: python3 geckoterminal.py search '{"query":"PEPE","network":"base"}'
"""

import json
import sys
import urllib.request
import urllib.parse

API_BASE = "https://api.geckoterminal.com/api/v2"

NETWORK_MAP = {
    "ethereum": "eth", "eth": "eth", "mainnet": "eth",
    "base": "base",
    "solana": "solana", "sol": "solana",
    "bsc": "bsc", "binance": "bsc",
    "polygon": "polygon_pos", "matic": "polygon_pos",
    "arbitrum": "arbitrum", "arb": "arbitrum",
    "optimism": "optimism", "op": "optimism",
    "avalanche": "avax", "avax": "avax",
}


def fmt(n):
    if n >= 1_000_000_000:
        return f"${n / 1_000_000_000:.2f}B"
    if n >= 1_000_000:
        return f"${n / 1_000_000:.2f}M"
    if n >= 1_000:
        return f"${n / 1_000:.2f}K"
    return f"${n:.2f}"


def api_get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "StarkBot/1.0"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read())


def search(args):
    query = args.get("query", "").strip()
    if not query:
        print("Error: 'query' is required (token symbol, name, or address)")
        sys.exit(1)

    network = args.get("network", "").strip()

    url = f"{API_BASE}/search/pools?query={urllib.parse.quote(query)}"
    if network:
        norm = NETWORK_MAP.get(network.lower(), network)
        url += f"&network={urllib.parse.quote(norm)}"

    data = api_get(url)
    pools = data.get("data") or []

    if not pools:
        print(f"No pools found for '{query}'. Try a different query or specify a network.")
        return

    pool = pools[0]
    attrs = pool.get("attributes") or {}
    net_id = (
        ((pool.get("relationships") or {}).get("network") or {}).get("data") or {}
    ).get("id", "unknown")

    name = attrs.get("name", "Unknown Pool")
    address = attrs.get("address", "")

    lines = [f"**{name}** on {net_id}"]

    price = attrs.get("base_token_price_usd")
    if price:
        change_str = ""
        pcp = attrs.get("price_change_percentage") or {}
        h24 = pcp.get("h24")
        if h24 is not None:
            c = float(h24)
            change_str = f" ({c:+.1f}% 24h)"
        lines.append(f"  Price: ${price}{change_str}")

    reserve = attrs.get("reserve_in_usd")
    if reserve:
        lines.append(f"  Liquidity: {fmt(float(reserve))}")

    vol = (attrs.get("volume_usd") or {}).get("h24")
    if vol:
        lines.append(f"  24h Vol: {fmt(float(vol))}")

    fdv = attrs.get("fdv_usd")
    if fdv:
        lines.append(f"  FDV: {fmt(float(fdv))}")

    lines.append(f"  https://www.geckoterminal.com/{net_id}/pools/{address}")

    print("\n".join(lines))


if __name__ == "__main__":
    action = sys.argv[1] if len(sys.argv) > 1 else "search"
    args = json.loads(sys.argv[2]) if len(sys.argv) > 2 else {}

    if action == "search":
        search(args)
    else:
        print(f"Unknown action: {action}. Use: search")
        sys.exit(1)
