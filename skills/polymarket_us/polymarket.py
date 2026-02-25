#!/usr/bin/env python3
"""Polymarket US — comprehensive prediction market skill.

Uses the official polymarket-us Python SDK for all operations.

CLI convention:  python3 polymarket.py <action> '<json_args>'

Env vars (injected by starkbot):
  POLYMARKET_KEY_ID      — API key ID from https://polymarket.us/developer
  POLYMARKET_SECRET_KEY  — Ed25519 secret from https://polymarket.us/developer
"""

import json
import os
import subprocess
import sys

# ---------------------------------------------------------------------------
# SDK bootstrap
# ---------------------------------------------------------------------------

_SDK_VERSION = "polymarket-us>=0.1.2"


def _ensure_sdk():
    try:
        import polymarket_us  # noqa: F401
    except ModuleNotFoundError:
        subprocess.check_call(
            ["uv", "pip", "install", _SDK_VERSION, "-q"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def _get_client(authenticated=False):
    from polymarket_us import PolymarketUS

    key_id = os.environ.get("POLYMARKET_KEY_ID", "")
    secret = os.environ.get("POLYMARKET_SECRET_KEY", "")

    if authenticated and (not key_id or not secret):
        return None, (
            "API keys required for this action. "
            "Get yours at https://polymarket.us/developer "
            "and install via Settings > API Keys."
        )

    if key_id and secret:
        return PolymarketUS(key_id=key_id, secret_key=secret), None
    return PolymarketUS(), None


# ---------------------------------------------------------------------------
# Serialization helpers
# ---------------------------------------------------------------------------

def _ser(obj):
    """Recursively serialize SDK response objects to plain dicts/lists."""
    if isinstance(obj, (str, int, float, bool, type(None))):
        return obj
    if hasattr(obj, "model_dump"):
        return obj.model_dump()
    if isinstance(obj, dict):
        return {k: _ser(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return [_ser(item) for item in obj]
    if hasattr(obj, "__dict__"):
        return {k: _ser(v) for k, v in obj.__dict__.items() if not k.startswith("_")}
    return str(obj)


def _ok(data):
    return _ser(data)


def _err(msg):
    return {"error": msg}


def _require(args, *fields):
    """Return error dict if any required field is missing, else None."""
    missing = [f for f in fields if f not in args or args[f] is None]
    if missing:
        return _err(f"Required fields missing: {', '.join(missing)}")
    return None


# ═══════════════════════════════════════════════════════════════════════════
# DISCOVERY — no auth required
# ═══════════════════════════════════════════════════════════════════════════

def search(args):
    """Full-text search across events and markets.

    Args: query (str, required), limit (int), page (int),
          series_ids (list[int]), market_type (list[str]), status (str)
    """
    query = args.get("query", "")
    if not query:
        return _err("query is required")
    client, err = _get_client()
    if err:
        return _err(err)
    params = {"query": query}
    for k in ("limit", "page", "seriesIds", "series_ids", "marketType", "market_type", "status"):
        if k in args and args[k] is not None:
            # normalize snake_case to camelCase for SDK
            camel = k.replace("_i", "I").replace("_t", "T")
            params[camel] = args[k]
    return _ok(client.search.query(params))


def search_events(args):
    """List/browse events with filters.

    Args: limit (int), offset (int), active (bool), closed (bool),
          archived (bool), featured (bool), live (bool), ended (bool),
          categories (list[str]), series_id (list[int])
    """
    client, err = _get_client()
    if err:
        return _err(err)
    params = {}
    for k in ("limit", "offset", "active", "closed", "archived", "featured",
              "live", "ended", "categories", "seriesId", "series_id"):
        if k in args and args[k] is not None:
            camel = k.replace("_i", "I")
            params[camel] = args[k]
    return _ok(client.events.list(params if params else None))


def get_event(args):
    """Get event details by ID or slug.

    Args: slug (str) OR id (int/str)
    """
    client, err = _get_client()
    if err:
        return _err(err)
    if "slug" in args:
        return _ok(client.events.retrieve_by_slug(args["slug"]))
    elif "id" in args:
        return _ok(client.events.retrieve(args["id"]))
    return _err("Provide 'id' or 'slug'")


def list_markets(args):
    """List markets with rich filtering.

    Args: limit (int), offset (int), active (bool), closed (bool),
          archived (bool), categories (list[str]),
          sports_market_types (list[str]: MONEYLINE|SPREAD|TOTAL|PROP),
          volume_num_min (float), liquidity_num_min (float)
    """
    client, err = _get_client()
    if err:
        return _err(err)
    params = {}
    field_map = {
        "limit": "limit", "offset": "offset", "active": "active",
        "closed": "closed", "archived": "archived", "categories": "categories",
        "sports_market_types": "sportsMarketTypes",
        "sportsMarketTypes": "sportsMarketTypes",
        "volume_num_min": "volumeNumMin", "volumeNumMin": "volumeNumMin",
        "liquidity_num_min": "liquidityNumMin", "liquidityNumMin": "liquidityNumMin",
    }
    for snake, camel in field_map.items():
        if snake in args and args[snake] is not None:
            params[camel] = args[snake]
    return _ok(client.markets.list(params if params else None))


def get_market(args):
    """Get market details by slug or ID.

    Args: slug (str) OR id (int/str)
    """
    client, err = _get_client()
    if err:
        return _err(err)
    if "slug" in args:
        return _ok(client.markets.retrieve_by_slug(args["slug"]))
    elif "id" in args:
        return _ok(client.markets.retrieve(args["id"]))
    return _err("Provide 'slug' or 'id'")


def get_sides(args):
    """Get market sides (Yes/No) with current prices via BBO.

    Args: slug (str, required)
    """
    check = _require(args, "slug")
    if check:
        return check
    client, err = _get_client()
    if err:
        return _err(err)
    return _ok(client.markets.bbo(args["slug"]))


def get_book(args):
    """Get full order book for a market.

    Args: slug (str, required)
    """
    check = _require(args, "slug")
    if check:
        return check
    client, err = _get_client()
    if err:
        return _err(err)
    return _ok(client.markets.book(args["slug"]))


def get_bbo(args):
    """Get best bid/offer (top of book) for a market.

    Returns: best bid, best ask, last trade price, bid/ask depth, open interest.

    Args: slug (str, required)
    """
    check = _require(args, "slug")
    if check:
        return check
    client, err = _get_client()
    if err:
        return _err(err)
    return _ok(client.markets.bbo(args["slug"]))


def settlement(args):
    """Get settlement price for a resolved market (0.00 or 1.00).

    Args: slug (str, required)
    """
    check = _require(args, "slug")
    if check:
        return check
    client, err = _get_client()
    if err:
        return _err(err)
    return _ok(client.markets.settlement(args["slug"]))


def list_series(args):
    """List available series (groupings of related events).

    Args: limit (int), offset (int)
    """
    client, err = _get_client()
    if err:
        return _err(err)
    params = {k: v for k, v in args.items() if v is not None}
    return _ok(client.series.list(params if params else None))


def get_series(args):
    """Get series details by ID.

    Args: id (int/str, required)
    """
    check = _require(args, "id")
    if check:
        return check
    client, err = _get_client()
    if err:
        return _err(err)
    return _ok(client.series.retrieve(args["id"]))


def list_sports(args):
    """List available sports categories."""
    client, err = _get_client()
    if err:
        return _err(err)
    return _ok(client.sports.list())


def sports_teams(args):
    """Get teams for a sports provider.

    Args: provider (str, required)
    """
    check = _require(args, "provider")
    if check:
        return check
    client, err = _get_client()
    if err:
        return _err(err)
    return _ok(client.sports.teams({"provider": args["provider"]}))


# ═══════════════════════════════════════════════════════════════════════════
# TRADING — requires API keys
# ═══════════════════════════════════════════════════════════════════════════

def preview_order(args):
    """Preview an order without submitting (dry run). Requires auth.

    Args: market_slug (str), intent (str), type (str), price (float),
          quantity (int), tif (str)
    """
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    order = _build_order_params(args)
    if "error" in order:
        return order
    return _ok(client.orders.preview(order))


def create_order(args):
    """Place an order on a market. Requires auth.

    Args:
        market_slug (str, required): market slug
        intent (str, required): ORDER_INTENT_BUY_LONG | ORDER_INTENT_SELL_LONG |
                                ORDER_INTENT_BUY_SHORT | ORDER_INTENT_SELL_SHORT
                                (shortcuts: buy_yes, sell_yes, buy_no, sell_no)
        type (str): ORDER_TYPE_LIMIT (default) | ORDER_TYPE_MARKET
        price (float): limit price in USD (required for limit orders)
        quantity (int, required): number of contracts
        tif (str): GOOD_TILL_CANCEL (default) | GOOD_TILL_DATE |
                   IMMEDIATE_OR_CANCEL | FILL_OR_KILL
    """
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    order = _build_order_params(args)
    if "error" in order:
        return order
    return _ok(client.orders.create(order))


def modify_order(args):
    """Modify an existing open order. Requires auth.

    Args: order_id (str, required), price (float), quantity (int)
    """
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    check = _require(args, "order_id")
    if check:
        return check
    params = {}
    if "price" in args:
        params["price"] = {"value": str(args["price"]), "currency": "USD"}
    if "quantity" in args:
        params["quantity"] = int(args["quantity"])
    return _ok(client.orders.modify(args["order_id"], params))


def cancel_order(args):
    """Cancel a specific order. Requires auth.

    Args: order_id (str, required)
    """
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    check = _require(args, "order_id")
    if check:
        return check
    return _ok(client.orders.cancel(args["order_id"], {}))


def cancel_all(args):
    """Cancel all open orders. Requires auth."""
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    return _ok(client.orders.cancel_all())


def close_position(args):
    """Close an entire position at market price. Requires auth.

    Args:
        market_slug (str, required): market to close
        current_price (float, optional): current price for slippage calc
        ticks (int, optional): slippage tolerance in ticks
    """
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    check = _require(args, "market_slug")
    if check:
        return check
    params = {"marketSlug": args["market_slug"]}
    if "current_price" in args:
        params["currentPrice"] = str(args["current_price"])
    if "ticks" in args:
        params["ticks"] = int(args["ticks"])
    return _ok(client.orders.close_position(params))


def get_order(args):
    """Get details of a specific order. Requires auth.

    Args: order_id (str, required)
    """
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    check = _require(args, "order_id")
    if check:
        return check
    return _ok(client.orders.retrieve(args["order_id"]))


def list_orders(args):
    """List your open orders. Requires auth.

    Args: limit (int), cursor (str)
    """
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    params = {k: v for k, v in args.items() if v is not None}
    return _ok(client.orders.list(params if params else None))


# ═══════════════════════════════════════════════════════════════════════════
# PORTFOLIO — requires API keys
# ═══════════════════════════════════════════════════════════════════════════

def get_positions(args):
    """Get your current positions with P&L. Requires auth.

    Args: limit (int), cursor (str)
    """
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    params = {k: v for k, v in args.items() if v is not None}
    return _ok(client.portfolio.positions(params if params else None))


def get_activities(args):
    """Get your trading activity history. Requires auth.

    Args:
        limit (int), cursor (str),
        types (list[str]): ACTIVITY_TYPE_TRADE | ACTIVITY_TYPE_POSITION_RESOLUTION |
                           ACTIVITY_TYPE_ACCOUNT_DEPOSIT | ACTIVITY_TYPE_ACCOUNT_WITHDRAWAL
        market_slug (str): filter to specific market
        sort_order (str): SORT_ORDER_DESCENDING (default) | SORT_ORDER_ASCENDING
    """
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    params = {}
    field_map = {
        "limit": "limit", "cursor": "cursor", "types": "types",
        "market_slug": "marketSlug", "marketSlug": "marketSlug",
        "sort_order": "sortOrder", "sortOrder": "sortOrder",
    }
    for snake, camel in field_map.items():
        if snake in args and args[snake] is not None:
            params[camel] = args[snake]
    return _ok(client.portfolio.activities(params if params else None))


def get_balance(args):
    """Get account balances including buying power. Requires auth.

    Returns: currentBalance, currency, buyingPower, assetNotional,
             assetAvailable, openOrders, unsettledFunds, marginRequirement,
             pendingWithdrawals
    """
    client, err = _get_client(authenticated=True)
    if err:
        return _err(err)
    return _ok(client.account.balances())


# ═══════════════════════════════════════════════════════════════════════════
# UTILITY
# ═══════════════════════════════════════════════════════════════════════════

def status(args):
    """Check API connectivity and key configuration."""
    result = {
        "sdk": _SDK_VERSION,
        "api_keys_configured": bool(
            os.environ.get("POLYMARKET_KEY_ID") and os.environ.get("POLYMARKET_SECRET_KEY")
        ),
    }
    try:
        client, _ = _get_client()
        if client:
            client.search.query({"query": "test", "limit": 1})
            result["api_status"] = "healthy"
    except Exception as e:
        result["api_status"] = f"error: {str(e)[:200]}"

    if result["api_keys_configured"]:
        try:
            client, _ = _get_client(authenticated=True)
            if client:
                client.account.balances()
                result["auth_status"] = "healthy"
        except Exception as e:
            result["auth_status"] = f"error: {str(e)[:200]}"

    return result


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

_INTENT_SHORTCUTS = {
    "buy_yes": "ORDER_INTENT_BUY_LONG",
    "sell_yes": "ORDER_INTENT_SELL_LONG",
    "buy_no": "ORDER_INTENT_BUY_SHORT",
    "sell_no": "ORDER_INTENT_SELL_SHORT",
    "buy_long": "ORDER_INTENT_BUY_LONG",
    "sell_long": "ORDER_INTENT_SELL_LONG",
    "buy_short": "ORDER_INTENT_BUY_SHORT",
    "sell_short": "ORDER_INTENT_SELL_SHORT",
}


def _build_order_params(args):
    """Build order params dict from user-friendly args."""
    check = _require(args, "market_slug", "intent", "quantity")
    if check:
        return check

    intent = args["intent"]
    intent = _INTENT_SHORTCUTS.get(intent.lower(), intent)
    if not intent.startswith("ORDER_INTENT_"):
        return _err(
            f"Invalid intent '{args['intent']}'. "
            "Use: buy_yes, sell_yes, buy_no, sell_no "
            "(or full: ORDER_INTENT_BUY_LONG, etc.)"
        )

    order_type = args.get("type", "ORDER_TYPE_LIMIT")
    if order_type.lower() in ("limit", "market"):
        order_type = f"ORDER_TYPE_{order_type.upper()}"

    tif = args.get("tif", "GOOD_TILL_CANCEL")

    quantity = int(args["quantity"])
    if quantity <= 0:
        return _err("quantity must be positive")

    params = {
        "marketSlug": args["market_slug"],
        "intent": intent,
        "type": order_type,
        "quantity": quantity,
        "tif": tif,
    }

    if order_type == "ORDER_TYPE_LIMIT":
        if "price" not in args:
            return _err("price is required for limit orders")
        price = float(args["price"])
        if not (0.01 <= price <= 0.99):
            return _err(f"price must be between 0.01 and 0.99, got {price}")
        params["price"] = {"value": str(price), "currency": "USD"}

    return params


# ---------------------------------------------------------------------------
# Action registry
# ---------------------------------------------------------------------------

ACTIONS = {
    # Discovery
    "search": search,
    "search_events": search_events,
    "get_event": get_event,
    "list_markets": list_markets,
    "get_market": get_market,
    "get_sides": get_sides,
    "get_book": get_book,
    "get_bbo": get_bbo,
    "settlement": settlement,
    "list_series": list_series,
    "get_series": get_series,
    "list_sports": list_sports,
    "sports_teams": sports_teams,
    # Trading
    "preview_order": preview_order,
    "create_order": create_order,
    "modify_order": modify_order,
    "cancel_order": cancel_order,
    "cancel_all": cancel_all,
    "close_position": close_position,
    "get_order": get_order,
    "list_orders": list_orders,
    # Portfolio
    "get_positions": get_positions,
    "get_activities": get_activities,
    "get_balance": get_balance,
    # Utility
    "status": status,
}


# ---------------------------------------------------------------------------
# Main dispatch
# ---------------------------------------------------------------------------

def main():
    if len(sys.argv) < 2:
        print(json.dumps({
            "error": "Usage: polymarket.py <action> [json_args]",
            "available_actions": sorted(ACTIONS.keys()),
        }))
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
        print(json.dumps({
            "error": f"Unknown action: {action}",
            "available_actions": sorted(ACTIONS.keys()),
        }))
        sys.exit(1)

    _ensure_sdk()

    try:
        result = ACTIONS[action](args)
        print(json.dumps(result, default=str))
    except Exception as e:
        print(json.dumps({"error": str(e)}), file=sys.stdout)
        sys.exit(1)


if __name__ == "__main__":
    main()
