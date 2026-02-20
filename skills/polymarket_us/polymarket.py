#!/usr/bin/env python3
"""Polymarket US skill script.

CLI convention: python3 polymarket.py <action> '<json_args>'

Requires env vars:
  POLYMARKET_KEY_ID     — API key ID from https://poly.market/dev-portal
  POLYMARKET_SECRET_KEY — Ed25519 secret key from https://poly.market/dev-portal
"""

import json
import os
import sys
import subprocess


def ensure_sdk():
    """Auto-install polymarket-us SDK if not found."""
    try:
        import polymarket  # noqa: F401
    except ModuleNotFoundError:
        print("Installing polymarket-us SDK...", file=sys.stderr)
        subprocess.check_call(
            ["uv", "pip", "install", "polymarket-us", "-q"],
            stdout=subprocess.DEVNULL,
        )


def get_client(authenticated=False):
    """Create a Polymarket US client."""
    from polymarket import Client

    key_id = os.environ.get("POLYMARKET_KEY_ID", "")
    secret = os.environ.get("POLYMARKET_SECRET_KEY", "")

    if authenticated and (not key_id or not secret):
        print(
            json.dumps({"error": "POLYMARKET_KEY_ID and POLYMARKET_SECRET_KEY are required for this action. Get your keys at https://poly.market/dev-portal and install them via Settings > API Keys."}),
            file=sys.stdout,
        )
        sys.exit(1)

    if key_id and secret:
        return Client(api_key=key_id, secret=secret)
    return Client()


# ---------------------------------------------------------------------------
# Actions
# ---------------------------------------------------------------------------


def search(args):
    """Full-text search across events and markets."""
    client = get_client()
    query = args.get("query", "")
    if not query:
        return {"error": "query is required for search"}
    results = client.search(query)
    return results


def search_events(args):
    """List/search events with optional filters."""
    client = get_client()
    params = {k: v for k, v in args.items() if v is not None}
    events = client.events.list(**params)
    return events


def get_event(args):
    """Get event details by ID or slug."""
    client = get_client()
    if "slug" in args:
        return client.events.retrieve_by_slug(args["slug"])
    elif "id" in args:
        return client.events.retrieve(args["id"])
    else:
        return {"error": "Provide 'id' or 'slug'"}


def list_markets(args):
    """List markets, optionally filtered by event_id."""
    client = get_client()
    params = {k: v for k, v in args.items() if v is not None}
    return client.markets.list(**params)


def get_market(args):
    """Get market details by slug."""
    client = get_client()
    slug = args.get("slug")
    if not slug:
        return {"error": "slug is required"}
    return client.markets.retrieve_by_slug(slug)


def get_sides(args):
    """Get market sides (Yes/No) with current prices."""
    client = get_client()
    market_id = args.get("market_id")
    if not market_id:
        return {"error": "market_id is required"}
    return client.markets.sides(market_id)


def get_book(args):
    """Get order book for a market."""
    client = get_client()
    slug = args.get("slug")
    if not slug:
        return {"error": "slug is required"}
    return client.markets.book(slug)


def get_bbo(args):
    """Get best bid/offer for a market."""
    client = get_client()
    slug = args.get("slug")
    if not slug:
        return {"error": "slug is required"}
    return client.markets.bbo(slug)


def create_order(args):
    """Place an order on a market."""
    client = get_client(authenticated=True)
    required = ["market_id", "side", "size", "price"]
    for field in required:
        if field not in args:
            return {"error": f"{field} is required for create_order"}
    return client.orders.create(**{k: args[k] for k in required})


def cancel_order(args):
    """Cancel a specific order."""
    client = get_client(authenticated=True)
    order_id = args.get("order_id")
    if not order_id:
        return {"error": "order_id is required"}
    return client.orders.cancel(order_id)


def cancel_all(args):
    """Cancel all open orders."""
    client = get_client(authenticated=True)
    return client.orders.cancel_all()


def list_orders(args):
    """List your orders."""
    client = get_client(authenticated=True)
    return client.orders.list()


def get_positions(args):
    """Get your current positions."""
    client = get_client(authenticated=True)
    return client.portfolio.positions()


def get_balance(args):
    """Get account balances."""
    client = get_client(authenticated=True)
    return client.account.balances()


# ---------------------------------------------------------------------------
# Main dispatch
# ---------------------------------------------------------------------------

ACTIONS = {
    "search": search,
    "search_events": search_events,
    "get_event": get_event,
    "list_markets": list_markets,
    "get_market": get_market,
    "get_sides": get_sides,
    "get_book": get_book,
    "get_bbo": get_bbo,
    "create_order": create_order,
    "cancel_order": cancel_order,
    "cancel_all": cancel_all,
    "list_orders": list_orders,
    "get_positions": get_positions,
    "get_balance": get_balance,
}


def main():
    if len(sys.argv) < 2:
        print(
            json.dumps({
                "error": "Usage: polymarket.py <action> [json_args]",
                "available_actions": list(ACTIONS.keys()),
            })
        )
        sys.exit(1)

    action = sys.argv[1]
    args = {}
    if len(sys.argv) >= 3:
        try:
            args = json.loads(sys.argv[2])
        except json.JSONDecodeError as e:
            print(json.dumps({"error": f"Invalid JSON args: {e}"}))
            sys.exit(1)

    if action not in ACTIONS:
        print(
            json.dumps({
                "error": f"Unknown action: {action}",
                "available_actions": list(ACTIONS.keys()),
            })
        )
        sys.exit(1)

    ensure_sdk()

    try:
        result = ACTIONS[action](args)
        # Serialize result — handle both dict and SDK response objects
        if isinstance(result, (dict, list)):
            print(json.dumps(result, default=str))
        elif hasattr(result, "__dict__"):
            print(json.dumps(result.__dict__, default=str))
        else:
            print(json.dumps({"result": str(result)}))
    except Exception as e:
        print(json.dumps({"error": str(e)}), file=sys.stdout)
        sys.exit(1)


if __name__ == "__main__":
    main()
