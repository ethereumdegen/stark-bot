#!/usr/bin/env python3
"""DexScreener — Real-time DEX token data across all major chains.

Usage:
  python3 dexscreener.py search  '{"query":"PEPE"}'
  python3 dexscreener.py token   '{"chain":"base","address":"0x..."}'
  python3 dexscreener.py pair    '{"chain":"ethereum","address":"0x..."}'
  python3 dexscreener.py boosted '{"chain":"base"}'
"""

import json
import sys
import urllib.request
import urllib.parse

BASE_URL = "https://api.dexscreener.com"


def fmt(n):
    if n >= 1_000_000_000:
        return f"{n / 1_000_000_000:.2f}B"
    if n >= 1_000_000:
        return f"{n / 1_000_000:.2f}M"
    if n >= 1_000:
        return f"{n / 1_000:.2f}K"
    return f"{n:.2f}"


def api_get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "StarkBot/1.0"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read())


def format_pair(p):
    base = p.get("baseToken") or {}
    quote = p.get("quoteToken") or {}
    sym = base.get("symbol", "???")
    qsym = quote.get("symbol", "???")
    name = base.get("name", "")
    chain = p.get("chainId", "?")
    dex = p.get("dexId", "?")

    lines = [f"**{sym}/{qsym}** {name} on {chain} ({dex})"]

    price = p.get("priceUsd")
    if price:
        change = (p.get("priceChange") or {}).get("h24")
        change_str = ""
        if change is not None:
            change_str = f" ({change:+.2f}% 24h)"
        lines.append(f"  Price: ${price}{change_str}")

    mc = p.get("marketCap")
    if mc:
        lines.append(f"  MCap: ${fmt(mc)}")

    liq = (p.get("liquidity") or {}).get("usd")
    if liq:
        lines.append(f"  Liquidity: ${fmt(liq)}")

    vol = (p.get("volume") or {}).get("h24")
    if vol:
        lines.append(f"  24h Vol: ${fmt(vol)}")

    txns = (p.get("txns") or {}).get("h24")
    if txns:
        buys = txns.get("buys", 0)
        sells = txns.get("sells", 0)
        lines.append(f"  24h Txns: {buys} buys / {sells} sells")

    addr = base.get("address")
    if addr:
        lines.append(f"  Token: {addr}")

    url = p.get("url")
    if url:
        lines.append(f"  {url}")

    return "\n".join(lines)


def do_search(args):
    query = args.get("query", "").strip()
    if not query:
        print("Error: 'query' is required for search")
        sys.exit(1)

    url = f"{BASE_URL}/latest/dex/search?q={urllib.parse.quote(query)}"
    data = api_get(url)
    pairs = data.get("pairs") or []

    if not pairs:
        print(f"No results for '{query}'")
        return

    print(f"Found {len(pairs)} results for '{query}':\n")
    for p in pairs[:10]:
        print(format_pair(p))
        print()


def do_token(args):
    chain = args.get("chain", "").strip()
    address = args.get("address", "").strip()

    if not chain:
        print("Error: 'chain' is required (ethereum, base, solana, etc.)")
        sys.exit(1)
    if not address:
        print("Error: 'address' is required")
        sys.exit(1)

    url = f"{BASE_URL}/tokens/v1/{chain}/{address}"
    pairs = api_get(url)

    if not pairs:
        print(f"No pairs found for {address} on {chain}")
        return

    print(f"Token {address} on {chain}:\n")
    for p in pairs[:5]:
        print(format_pair(p))
        print()


def do_pair(args):
    chain = args.get("chain", "").strip()
    address = args.get("address", "").strip()

    if not chain:
        print("Error: 'chain' is required")
        sys.exit(1)
    if not address:
        print("Error: 'address' is required (pair/pool address)")
        sys.exit(1)

    url = f"{BASE_URL}/latest/dex/pairs/{chain}/{address}"
    data = api_get(url)
    pairs = data.get("pairs") or []

    if not pairs:
        print(f"Pair {address} not found on {chain}")
        return

    for p in pairs:
        print(format_pair(p))
        print()


def do_boosted(args):
    chain_filter = args.get("chain", "").strip().lower()

    url = f"{BASE_URL}/token-boosts/top/v1"
    boosts = api_get(url)

    if not boosts:
        print("No boosted tokens found")
        return

    if chain_filter:
        boosts = [b for b in boosts if (b.get("chainId") or "").lower() == chain_filter]

    if not boosts:
        suffix = f" on {chain_filter}" if chain_filter else ""
        print(f"No boosted tokens found{suffix}")
        return

    chain_note = f" on {chain_filter}" if chain_filter else ""
    print(f"PAID PROMOTIONS (not organic trending!){chain_note}:\n")
    print("These tokens paid DexScreener for visibility. Exercise extreme caution.\n")

    for b in boosts[:15]:
        name = b.get("name", "?")
        symbol = b.get("symbol", "?")
        chain = b.get("chainId", "?")
        total = b.get("totalAmount", 0)
        print(f"**{name} ({symbol})** on {chain} - {total} boosts")
        if b.get("tokenAddress"):
            print(f"  {b['tokenAddress']}")
        if b.get("url"):
            print(f"  {b['url']}")
        print()

    print("For actual trending tokens, use 'search' action and evaluate by volume/liquidity.")


if __name__ == "__main__":
    action = sys.argv[1] if len(sys.argv) > 1 else "search"
    args = json.loads(sys.argv[2]) if len(sys.argv) > 2 else {}

    actions = {
        "search": do_search,
        "token": do_token,
        "pair": do_pair,
        "boosted": do_boosted,
        "trending": do_boosted,
    }

    fn = actions.get(action)
    if fn:
        fn(args)
    else:
        print(f"Unknown action: {action}. Use: search, token, pair, boosted")
        sys.exit(1)
