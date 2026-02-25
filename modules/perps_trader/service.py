# /// script
# requires-python = ">=3.12"
# dependencies = ["flask", "requests", "starkbot-sdk", "avantis_trader_sdk"]
#
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
Perps Trader module — autonomous perpetual futures trader on Avantis (Base).

Evaluates Bankr signals, makes leveraged LONG/SHORT decisions via the
perps_trader agent persona, constructs perpetual futures transactions using
the Avantis SDK, and broadcasts signed transactions on-chain.

RPC protocol endpoints:
  GET  /rpc/status          -> service health
  POST /rpc/decision        -> agent submits OPEN_LONG/OPEN_SHORT/CLOSE/HOLD
  POST /rpc/sign            -> agent submits signed tx hex for broadcast
  GET  /rpc/positions       -> open positions with P&L
  POST /rpc/sync_positions  -> re-sync from on-chain
  GET  /rpc/trade_history   -> closed trades with realized P&L
  POST /rpc/history         -> query trade decision history
  GET  /rpc/stats           -> aggregate trading statistics
  GET  /rpc/pnl             -> aggregate P&L summary
  POST /rpc/refresh         -> refresh prices, update unrealized P&L
  GET  /rpc/config          -> view trader config
  POST /rpc/config          -> update trader config
  POST /rpc/control         -> start/stop/trigger worker
  POST /rpc/approve_usdc    -> construct USDC approval tx
  GET  /rpc/pairs           -> available pairs with indices
  POST /rpc/backup/export   -> export all tables
  POST /rpc/backup/restore  -> restore from backup
  GET  /                    -> HTML dashboard

Launch with:  uv run service.py
"""

from flask import request, Response
from starkbot_sdk import create_app, success, error
import sqlite3
import os
import json
import time
import logging
import threading
import asyncio
import requests as http_requests
from datetime import datetime, timezone

# ---------------------------------------------------------------------------
# Async event loop for Avantis SDK (async SDK, sync Flask)
# ---------------------------------------------------------------------------

_loop = asyncio.new_event_loop()
threading.Thread(target=_loop.run_forever, daemon=True).start()


def run_async(coro):
    """Run an async coroutine from sync Flask context."""
    return asyncio.run_coroutine_threadsafe(coro, _loop).result(timeout=30)


# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

DB_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "perps_trader.db")
BACKEND_URL = os.environ.get("STARKBOT_BACKEND_URL", "http://127.0.0.1:8080")
INTERNAL_TOKEN = os.environ.get("STARKBOT_INTERNAL_TOKEN", "")
ALCHEMY_API_KEY = os.environ.get("ALCHEMY_API_KEY", "")

BASE_RPC_URL = f"https://base-mainnet.g.alchemy.com/v2/{ALCHEMY_API_KEY}" if ALCHEMY_API_KEY else ""
BASE_CHAIN_ID = 8453

# USDC on Base
USDC_ADDRESS = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
USDC_DECIMALS = 6

# Avantis execution fee (~0.00035 ETH in wei)
EXECUTION_FEE_WEI = 350000000000000  # 0.00035 ETH

# Defaults
DEFAULT_PULSE_INTERVAL = 300  # 5 minutes

# Module state
_start_time = time.time()
_worker_running = False
_worker_lock = threading.Lock()
_last_pulse_at = None

# Avantis SDK client (lazy-initialized)
_trader_client = None
_trader_client_lock = threading.Lock()


def get_trader_client():
    """Lazy-initialize the Avantis TraderClient."""
    global _trader_client
    if _trader_client is not None:
        return _trader_client
    with _trader_client_lock:
        if _trader_client is not None:
            return _trader_client
        if not BASE_RPC_URL:
            logging.warning("[PERPS_TRADER] No ALCHEMY_API_KEY — SDK unavailable")
            return None
        try:
            from avantis_trader_sdk import TraderClient
            _trader_client = run_async(TraderClient.create(provider_url=BASE_RPC_URL))
            logging.info("[PERPS_TRADER] Avantis TraderClient initialized")
        except Exception as e:
            logging.error(f"[PERPS_TRADER] Failed to init TraderClient: {e}")
            return None
    return _trader_client


# ---------------------------------------------------------------------------
# Database
# ---------------------------------------------------------------------------

def get_db():
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    return conn


def init_db():
    conn = get_db()
    conn.executescript("""
        CREATE TABLE IF NOT EXISTS trade_decisions (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            decision     TEXT NOT NULL,
            pair_name    TEXT,
            pair_index   INTEGER,
            reason       TEXT,
            status       TEXT NOT NULL DEFAULT 'pending',
            created_at   TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS trade_executions (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            decision_id  INTEGER NOT NULL REFERENCES trade_decisions(id),
            tx_type      TEXT,
            raw_tx_to    TEXT,
            raw_tx_data  TEXT,
            raw_tx_value TEXT,
            raw_tx_gas   TEXT,
            signed_tx    TEXT,
            tx_hash      TEXT,
            status       TEXT NOT NULL DEFAULT 'unsigned',
            error_msg    TEXT,
            created_at   TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS positions (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            pair_name       TEXT NOT NULL,
            pair_index      INTEGER NOT NULL,
            trade_index     INTEGER NOT NULL,
            is_long         INTEGER NOT NULL,
            collateral_usdc REAL NOT NULL,
            leverage        REAL NOT NULL,
            position_size   REAL,
            open_price      REAL NOT NULL,
            current_price   REAL,
            take_profit     REAL,
            stop_loss       REAL,
            liquidation_price REAL,
            unrealized_pnl  REAL DEFAULT 0,
            pnl_pct         REAL DEFAULT 0,
            decision_id     INTEGER,
            tx_hash         TEXT,
            opened_at       TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(pair_index, trade_index)
        );

        CREATE TABLE IF NOT EXISTS trade_history (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            pair_name       TEXT NOT NULL,
            pair_index      INTEGER NOT NULL,
            is_long         INTEGER NOT NULL,
            collateral_usdc REAL,
            leverage        REAL,
            position_size   REAL,
            open_price      REAL,
            close_price     REAL,
            realized_pnl    REAL,
            pnl_pct         REAL,
            decision_id     INTEGER,
            open_tx_hash    TEXT,
            close_tx_hash   TEXT,
            opened_at       TEXT,
            closed_at       TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS trader_config (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
    """)

    # Seed defaults if not present
    for k, v in [
        ("pulse_interval", str(DEFAULT_PULSE_INTERVAL)),
        ("enabled", "true"),
        ("trader_address", ""),
        ("max_collateral_per_trade", "10"),
        ("max_leverage", "25"),
        ("default_leverage", "10"),
        ("max_positions", "3"),
        ("take_profit_pct", "50"),
        ("stop_loss_pct", "25"),
        ("bankr_min_confidence", "70"),
        ("bankr_providers", ""),
        ("usdc_approved", "false"),
        ("allowed_pairs", "ETH/USD,BTC/USD,SOL/USD,DOGE/USD"),
    ]:
        conn.execute(
            "INSERT OR IGNORE INTO trader_config (key, value) VALUES (?, ?)", (k, v)
        )
    conn.commit()
    conn.close()


def get_config_value(key: str, default: str = "") -> str:
    conn = get_db()
    row = conn.execute("SELECT value FROM trader_config WHERE key = ?", (key,)).fetchone()
    conn.close()
    return row["value"] if row else default


def set_config_value(key: str, value: str):
    conn = get_db()
    conn.execute(
        "INSERT INTO trader_config (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (key, value),
    )
    conn.commit()
    conn.close()


def now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


# ---------------------------------------------------------------------------
# Avantis SDK helpers
# ---------------------------------------------------------------------------

def get_pairs_map() -> dict:
    """Get mapping of pair_name -> pair_index from Avantis SDK."""
    client = get_trader_client()
    if not client:
        return {}
    try:
        pairs_info = run_async(client.pairs_cache.get_pairs_info())
        result = {}
        for idx, pair in pairs_info.items():
            name = pair.get("name") or pair.get("from", "")
            if name:
                result[name] = int(idx)
        return result
    except Exception as e:
        logging.error(f"[PERPS_TRADER] get_pairs_map error: {e}")
        return {}


def resolve_pair_index(pair_name: str) -> int | None:
    """Resolve pair_name (e.g. 'ETH/USD') to pair_index."""
    pairs = get_pairs_map()
    if pair_name in pairs:
        return pairs[pair_name]
    # Try normalized forms
    normalized = pair_name.upper().replace(" ", "")
    for name, idx in pairs.items():
        if name.upper().replace(" ", "") == normalized:
            return idx
    return None


def get_current_price(pair_index: int) -> float | None:
    """Get current price for a pair from Avantis price feed."""
    client = get_trader_client()
    if not client:
        return None
    try:
        price = run_async(client.feed.get_price(pair_index))
        return float(price)
    except Exception as e:
        logging.error(f"[PERPS_TRADER] get_current_price error for pair {pair_index}: {e}")
        return None


def build_open_trade_tx(
    trader_address: str,
    pair_index: int,
    is_long: bool,
    collateral_usdc: float,
    leverage: float,
    take_profit: float,
    stop_loss: float,
    open_price: float,
) -> dict | None:
    """Build an unsigned open trade tx via Avantis SDK."""
    client = get_trader_client()
    if not client:
        return None
    try:
        from avantis_trader_sdk.types import TradeInput, TradeInputOrderType

        collateral_raw = int(collateral_usdc * 10**USDC_DECIMALS)

        trade_input = TradeInput(
            trader=trader_address,
            pair_index=pair_index,
            index=0,  # auto-assigned
            initial_collateral=collateral_raw,
            open_price=int(open_price * 1e10),  # 10-decimal fixed point
            is_long=is_long,
            leverage=int(leverage * 1e10),  # 10-decimal fixed point
            tp=int(take_profit * 1e10),
            sl=int(stop_loss * 1e10),
        )

        tx = run_async(
            client.trade.build_trade_open_tx(
                trade_input=trade_input,
                order_type=TradeInputOrderType.MARKET,
                slippage_percentage=1.0,
            )
        )

        return {
            "to": tx.get("to", ""),
            "data": tx.get("data", "0x"),
            "value": str(tx.get("value", EXECUTION_FEE_WEI)),
            "gas": str(tx.get("gas", "500000")),
        }
    except Exception as e:
        logging.error(f"[PERPS_TRADER] build_open_trade_tx error: {e}")
        return None


def build_close_trade_tx(
    trader_address: str, pair_index: int, trade_index: int, collateral_usdc: float
) -> dict | None:
    """Build an unsigned close trade tx via Avantis SDK."""
    client = get_trader_client()
    if not client:
        return None
    try:
        collateral_raw = int(collateral_usdc * 10**USDC_DECIMALS)

        tx = run_async(
            client.trade.build_trade_close_tx(
                pair_index=pair_index,
                trade_index=trade_index,
                collateral=collateral_raw,
                trader=trader_address,
            )
        )

        return {
            "to": tx.get("to", ""),
            "data": tx.get("data", "0x"),
            "value": str(tx.get("value", EXECUTION_FEE_WEI)),
            "gas": str(tx.get("gas", "500000")),
        }
    except Exception as e:
        logging.error(f"[PERPS_TRADER] build_close_trade_tx error: {e}")
        return None


def build_usdc_approve_tx(trader_address: str) -> dict | None:
    """Build an unsigned USDC approval tx for Avantis TradingStorage."""
    client = get_trader_client()
    if not client:
        return None
    try:
        from web3 import Web3

        w3 = Web3(Web3.HTTPProvider(BASE_RPC_URL))
        # ERC20 approve ABI
        approve_abi = [
            {
                "inputs": [
                    {"name": "spender", "type": "address"},
                    {"name": "amount", "type": "uint256"},
                ],
                "name": "approve",
                "outputs": [{"name": "", "type": "bool"}],
                "stateMutability": "nonpayable",
                "type": "function",
            }
        ]
        usdc = w3.eth.contract(address=Web3.to_checksum_address(USDC_ADDRESS), abi=approve_abi)

        # Get TradingStorage address from SDK
        trading_storage = None
        try:
            trading_storage = client.trade.trading_storage_address
        except Exception:
            pass
        if not trading_storage:
            # Fallback: Avantis TradingStorage on Base
            trading_storage = "0x8C09590E68207010eBF47F514b42f2f206671311"

        max_approval = 2**256 - 1
        tx_data = usdc.functions.approve(
            Web3.to_checksum_address(trading_storage), max_approval
        ).build_transaction({
            "from": Web3.to_checksum_address(trader_address),
            "chainId": BASE_CHAIN_ID,
            "gas": 60000,
            "value": 0,
        })

        return {
            "to": USDC_ADDRESS,
            "data": tx_data["data"],
            "value": "0",
            "gas": "60000",
        }
    except Exception as e:
        logging.error(f"[PERPS_TRADER] build_usdc_approve_tx error: {e}")
        return None


def fetch_onchain_trades(trader_address: str) -> list[dict]:
    """Fetch open trades from on-chain via Avantis SDK."""
    client = get_trader_client()
    if not client:
        return []
    try:
        result = run_async(client.trade.get_trades(trader_address))
        open_trades = result[0] if isinstance(result, (list, tuple)) else result
        trades = []
        for t in open_trades:
            trades.append({
                "pair_index": int(t.get("pair_index", 0)),
                "trade_index": int(t.get("index", 0)),
                "is_long": bool(t.get("is_long", True)),
                "collateral": float(t.get("initial_collateral", 0)) / 10**USDC_DECIMALS,
                "leverage": float(t.get("leverage", 0)) / 1e10,
                "open_price": float(t.get("open_price", 0)) / 1e10,
                "tp": float(t.get("tp", 0)) / 1e10,
                "sl": float(t.get("sl", 0)) / 1e10,
            })
        return trades
    except Exception as e:
        logging.error(f"[PERPS_TRADER] fetch_onchain_trades error: {e}")
        return []


# ---------------------------------------------------------------------------
# P&L helpers
# ---------------------------------------------------------------------------

def calc_pnl(is_long: bool, collateral: float, leverage: float, open_price: float, current_price: float) -> tuple[float, float]:
    """Calculate unrealized P&L and percentage.

    Long:  pnl = collateral * leverage * (current - open) / open
    Short: pnl = collateral * leverage * (open - current) / open
    """
    if open_price <= 0:
        return 0.0, 0.0
    if is_long:
        pnl = collateral * leverage * (current_price - open_price) / open_price
    else:
        pnl = collateral * leverage * (open_price - current_price) / open_price
    pnl_pct = (pnl / collateral * 100) if collateral > 0 else 0
    return round(pnl, 4), round(pnl_pct, 2)


def calc_liquidation_price(is_long: bool, open_price: float, leverage: float) -> float:
    """Estimate liquidation price (simplified: ~90% loss of collateral)."""
    if leverage <= 0 or open_price <= 0:
        return 0
    margin = 0.9 / leverage  # ~90% of collateral lost
    if is_long:
        return round(open_price * (1 - margin), 6)
    else:
        return round(open_price * (1 + margin), 6)


# ---------------------------------------------------------------------------
# Broadcast helper
# ---------------------------------------------------------------------------

def broadcast_tx(signed_tx_hex: str) -> str | None:
    """Broadcast a signed tx to Base via Alchemy and return the tx hash."""
    if not BASE_RPC_URL:
        return None
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_sendRawTransaction",
        "params": [signed_tx_hex],
    }
    try:
        resp = http_requests.post(BASE_RPC_URL, json=payload, timeout=30)
        data = resp.json()
        if "result" in data:
            return data["result"]
        err = data.get("error", {})
        logging.error(f"[PERPS_TRADER] Broadcast error: {err}")
    except Exception as e:
        logging.error(f"[PERPS_TRADER] Broadcast exception: {e}")
    return None


def poll_receipt(tx_hash: str, attempts: int = 12, delay: float = 5.0) -> dict | None:
    """Poll for a tx receipt on Base."""
    if not BASE_RPC_URL:
        return None
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
    }
    for _ in range(attempts):
        try:
            resp = http_requests.post(BASE_RPC_URL, json=payload, timeout=10)
            data = resp.json()
            receipt = data.get("result")
            if receipt:
                return receipt
        except Exception:
            pass
        time.sleep(delay)
    return None


# ---------------------------------------------------------------------------
# Hook firing
# ---------------------------------------------------------------------------

def fire_hook(event: str, data: dict | None = None):
    """Fire a custom persona hook via the backend internal API."""
    if not INTERNAL_TOKEN:
        logging.warning("[PERPS_TRADER] No STARKBOT_INTERNAL_TOKEN — cannot fire hooks")
        return
    try:
        http_requests.post(
            f"{BACKEND_URL}/api/internal/hooks/fire",
            json={"event": event, "data": data or {}},
            headers={"X-Internal-Token": INTERNAL_TOKEN},
            timeout=10,
        )
    except Exception as e:
        logging.error(f"[PERPS_TRADER] Hook fire error: {e}")


# ---------------------------------------------------------------------------
# Bankr Signals
# ---------------------------------------------------------------------------

BANKR_SIGNALS_URL = "https://bankrsignals.com/api/signals"


def fetch_bankr_signals() -> list[dict]:
    """Fetch trading signals from Bankr Signals API, filtered by config."""
    min_confidence = int(get_config_value("bankr_min_confidence", "70"))
    providers_raw = get_config_value("bankr_providers", "").strip()
    provider_whitelist = {p.strip().lower() for p in providers_raw.split(",") if p.strip()} if providers_raw else set()

    try:
        resp = http_requests.get(BANKR_SIGNALS_URL, params={"limit": 50}, timeout=15)
        if resp.status_code != 200:
            logging.warning(f"[PERPS_TRADER] Bankr signals API returned {resp.status_code}")
            return []
        signals = resp.json()
        if isinstance(signals, dict):
            # API returns {"success": true, "data": {"signals": [...]}}
            data = signals.get("data", signals)
            if isinstance(data, dict):
                signals = data.get("signals", [])
            elif isinstance(data, list):
                signals = data
            else:
                signals = signals.get("signals", [])
    except Exception as e:
        logging.error(f"[PERPS_TRADER] Bankr signals fetch error: {e}")
        return []

    # Filter by allowed pairs
    allowed_pairs_raw = get_config_value("allowed_pairs", "ETH/USD,BTC/USD,SOL/USD,DOGE/USD")
    allowed_tokens = set()
    for pair in allowed_pairs_raw.split(","):
        token = pair.strip().split("/")[0].upper()
        if token:
            allowed_tokens.add(token)

    results = []
    for sig in signals:
        if not isinstance(sig, dict):
            continue
        confidence = sig.get("confidence", 0)
        if isinstance(confidence, str):
            confidence = float(confidence.rstrip("%"))
        if confidence < min_confidence:
            continue
        if provider_whitelist:
            provider = (sig.get("provider") or "").lower()
            if provider not in provider_whitelist:
                continue
        # Filter to perps-relevant signals (LONG/SHORT actions for allowed tokens)
        action = (sig.get("action") or "").upper()
        if action not in ("LONG", "SHORT"):
            continue
        token = (sig.get("token") or sig.get("symbol") or "").upper()
        if allowed_tokens and token not in allowed_tokens:
            continue
        results.append(sig)

    logging.info(f"[PERPS_TRADER] Bankr signals: {len(results)} qualifying out of {len(signals)} total")
    return results


# ---------------------------------------------------------------------------
# Position summary builder
# ---------------------------------------------------------------------------

def _build_position_summary() -> dict:
    """Build position summary with risk alerts for the pulse hook."""
    conn = get_db()
    positions = conn.execute("SELECT * FROM positions ORDER BY opened_at DESC").fetchall()
    conn.close()

    max_positions = int(get_config_value("max_positions", "3"))
    tp_pct = float(get_config_value("take_profit_pct", "50"))
    sl_pct = float(get_config_value("stop_loss_pct", "25"))

    open_count = len(positions)
    total_collateral = 0
    total_unrealized = 0
    liquidation_alerts = []
    stop_loss_alerts = []
    take_profit_alerts = []

    position_list = []
    for p in positions:
        p = dict(p)
        total_collateral += float(p["collateral_usdc"] or 0)
        total_unrealized += float(p["unrealized_pnl"] or 0)
        pnl_pct_val = float(p["pnl_pct"] or 0)

        info = {
            "pair_name": p["pair_name"],
            "pair_index": p["pair_index"],
            "trade_index": p["trade_index"],
            "is_long": bool(p["is_long"]),
            "direction": "LONG" if p["is_long"] else "SHORT",
            "collateral_usdc": p["collateral_usdc"],
            "leverage": p["leverage"],
            "open_price": p["open_price"],
            "current_price": p["current_price"],
            "liquidation_price": p["liquidation_price"],
            "unrealized_pnl": round(float(p["unrealized_pnl"] or 0), 2),
            "pnl_pct": pnl_pct_val,
        }
        position_list.append(info)

        # Liquidation alert: within 5% of liquidation price
        liq_price = float(p["liquidation_price"] or 0)
        cur_price = float(p["current_price"] or 0)
        if liq_price > 0 and cur_price > 0:
            if p["is_long"] and cur_price <= liq_price * 1.05:
                liquidation_alerts.append(info)
            elif not p["is_long"] and cur_price >= liq_price * 0.95:
                liquidation_alerts.append(info)

        # Stop-loss alert
        if pnl_pct_val <= -sl_pct:
            stop_loss_alerts.append(info)

        # Take-profit alert
        if pnl_pct_val >= tp_pct:
            take_profit_alerts.append(info)

    return {
        "open_positions": open_count,
        "max_positions": max_positions,
        "can_open": open_count < max_positions,
        "total_collateral_deployed": round(total_collateral, 2),
        "total_unrealized_pnl": round(total_unrealized, 2),
        "positions": position_list,
        "liquidation_alerts": liquidation_alerts,
        "stop_loss_alerts": stop_loss_alerts,
        "take_profit_alerts": take_profit_alerts,
    }


# ---------------------------------------------------------------------------
# P&L computation
# ---------------------------------------------------------------------------

def compute_pnl() -> dict:
    """Compute aggregate P&L from trade_history and positions."""
    conn = get_db()

    # Realized P&L from closed trades
    closed = conn.execute(
        "SELECT pair_name, realized_pnl FROM trade_history WHERE realized_pnl IS NOT NULL"
    ).fetchall()
    total_realized = sum(float(t["realized_pnl"] or 0) for t in closed)
    win_count = sum(1 for t in closed if float(t["realized_pnl"] or 0) > 0)
    loss_count = sum(1 for t in closed if float(t["realized_pnl"] or 0) < 0)

    best_trade = max(closed, key=lambda t: float(t["realized_pnl"] or 0), default=None)
    worst_trade = min(closed, key=lambda t: float(t["realized_pnl"] or 0), default=None)

    # Unrealized P&L from open positions
    positions = conn.execute("SELECT unrealized_pnl FROM positions").fetchall()
    total_unrealized = sum(float(p["unrealized_pnl"] or 0) for p in positions)

    total_trades = len(closed)
    conn.close()

    return {
        "total_realized_pnl": round(total_realized, 2),
        "total_unrealized_pnl": round(total_unrealized, 2),
        "total_pnl": round(total_realized + total_unrealized, 2),
        "win_count": win_count,
        "loss_count": loss_count,
        "win_rate": round(win_count / total_trades, 3) if total_trades > 0 else 0,
        "total_trades": total_trades,
        "best_trade": {"pair": best_trade["pair_name"], "pnl": round(float(best_trade["realized_pnl"]), 2)} if best_trade else None,
        "worst_trade": {"pair": worst_trade["pair_name"], "pnl": round(float(worst_trade["realized_pnl"]), 2)} if worst_trade else None,
    }


# ---------------------------------------------------------------------------
# Sync positions from on-chain
# ---------------------------------------------------------------------------

def sync_positions_from_chain() -> dict:
    """Sync positions from on-chain state, catching liquidations/auto-closures."""
    trader_address = get_config_value("trader_address", "")
    if not trader_address:
        return {"error": "trader_address not set", "synced": 0}

    onchain_trades = fetch_onchain_trades(trader_address)
    pairs_map = get_pairs_map()
    reverse_pairs = {v: k for k, v in pairs_map.items()}

    conn = get_db()
    existing = conn.execute("SELECT * FROM positions").fetchall()
    existing_keys = {(r["pair_index"], r["trade_index"]): dict(r) for r in existing}

    onchain_keys = set()
    added = 0
    updated = 0

    for t in onchain_trades:
        pi = t["pair_index"]
        ti = t["trade_index"]
        onchain_keys.add((pi, ti))
        pair_name = reverse_pairs.get(pi, f"PAIR_{pi}")
        liq_price = calc_liquidation_price(t["is_long"], t["open_price"], t["leverage"])

        current_price = get_current_price(pi)
        pnl, pnl_pct = (0, 0)
        if current_price:
            pnl, pnl_pct = calc_pnl(t["is_long"], t["collateral"], t["leverage"], t["open_price"], current_price)

        if (pi, ti) in existing_keys:
            conn.execute(
                """UPDATE positions SET
                     current_price = ?, unrealized_pnl = ?, pnl_pct = ?,
                     liquidation_price = ?, updated_at = ?
                   WHERE pair_index = ? AND trade_index = ?""",
                (current_price, pnl, pnl_pct, liq_price, now_iso(), pi, ti),
            )
            updated += 1
        else:
            conn.execute(
                """INSERT INTO positions
                     (pair_name, pair_index, trade_index, is_long, collateral_usdc,
                      leverage, position_size, open_price, current_price, take_profit,
                      stop_loss, liquidation_price, unrealized_pnl, pnl_pct, opened_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (pair_name, pi, ti, int(t["is_long"]), t["collateral"],
                 t["leverage"], t["collateral"] * t["leverage"], t["open_price"],
                 current_price, t["tp"], t["sl"], liq_price, pnl, pnl_pct,
                 now_iso(), now_iso()),
            )
            added += 1

    # Positions in DB but not on-chain → were liquidated or auto-closed
    closed = 0
    for key, pos in existing_keys.items():
        if key not in onchain_keys:
            # Move to trade_history
            close_price = pos.get("current_price") or pos.get("open_price")
            pnl, pnl_pct = calc_pnl(
                bool(pos["is_long"]), float(pos["collateral_usdc"]),
                float(pos["leverage"]), float(pos["open_price"]),
                float(close_price or 0),
            )
            conn.execute(
                """INSERT INTO trade_history
                     (pair_name, pair_index, is_long, collateral_usdc, leverage,
                      position_size, open_price, close_price, realized_pnl, pnl_pct,
                      decision_id, open_tx_hash, opened_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (pos["pair_name"], pos["pair_index"], pos["is_long"],
                 pos["collateral_usdc"], pos["leverage"], pos.get("position_size"),
                 pos["open_price"], close_price, pnl, pnl_pct,
                 pos.get("decision_id"), pos.get("tx_hash"), pos.get("opened_at")),
            )
            conn.execute(
                "DELETE FROM positions WHERE pair_index = ? AND trade_index = ?",
                (key[0], key[1]),
            )
            closed += 1

    conn.commit()
    conn.close()
    return {"synced": len(onchain_trades), "added": added, "updated": updated, "closed": closed}


# ---------------------------------------------------------------------------
# Refresh prices
# ---------------------------------------------------------------------------

def refresh_positions_prices():
    """Refresh current prices and P&L for all open positions."""
    conn = get_db()
    positions = conn.execute("SELECT * FROM positions").fetchall()
    refreshed = 0
    for pos in positions:
        price = get_current_price(pos["pair_index"])
        if price is not None:
            pnl, pnl_pct = calc_pnl(
                bool(pos["is_long"]), float(pos["collateral_usdc"]),
                float(pos["leverage"]), float(pos["open_price"]), price,
            )
            conn.execute(
                """UPDATE positions SET
                     current_price = ?, unrealized_pnl = ?, pnl_pct = ?, updated_at = ?
                   WHERE id = ?""",
                (price, pnl, pnl_pct, now_iso(), pos["id"]),
            )
            refreshed += 1
    conn.commit()
    conn.close()
    return refreshed


# ---------------------------------------------------------------------------
# Background pulse worker
# ---------------------------------------------------------------------------

def _fire_pulse():
    """Build full pulse data with risk context and fire the hook."""
    trader_address = get_config_value("trader_address", "")

    # Sync positions from on-chain first
    if trader_address:
        try:
            sync_positions_from_chain()
        except Exception as e:
            logging.warning(f"[PERPS_TRADER] Position sync failed during pulse: {e}")

        # Refresh prices
        try:
            refresh_positions_prices()
        except Exception as e:
            logging.warning(f"[PERPS_TRADER] Price refresh failed during pulse: {e}")

    position_summary = _build_position_summary()
    pnl = compute_pnl()

    # USDC approval status
    usdc_approved = get_config_value("usdc_approved", "false").lower() == "true"
    usdc_status = {"approved": usdc_approved}
    if trader_address and not usdc_approved:
        # Check on-chain allowance
        client = get_trader_client()
        if client:
            try:
                allowance = run_async(client.get_usdc_allowance_for_trading(trader_address))
                if int(allowance) > 0:
                    set_config_value("usdc_approved", "true")
                    usdc_status["approved"] = True
            except Exception:
                pass

    hook_data = {
        "position_summary": position_summary,
        "pnl": pnl,
        "usdc_status": usdc_status,
        "bankr_signals": fetch_bankr_signals(),
        "trader_address": trader_address,
    }

    logging.info(
        f"[PERPS_TRADER] Firing pulse ({len(hook_data['bankr_signals'])} signals, "
        f"{position_summary['open_positions']} positions)"
    )
    fire_hook("perps_trader_pulse", hook_data)


def pulse_worker():
    global _last_pulse_at, _worker_running
    logger = logging.getLogger("perps_trader.worker")
    logger.info("[PERPS_TRADER] Pulse worker started")
    time.sleep(10)
    while _worker_running:
        interval = int(get_config_value("pulse_interval", str(DEFAULT_PULSE_INTERVAL)))
        enabled = get_config_value("enabled", "true").lower() == "true"
        if enabled:
            _fire_pulse()
            _last_pulse_at = now_iso()
        time.sleep(interval)


def start_worker():
    global _worker_running
    with _worker_lock:
        if _worker_running:
            return
        _worker_running = True
        t = threading.Thread(target=pulse_worker, daemon=True)
        t.start()


def stop_worker():
    global _worker_running
    with _worker_lock:
        _worker_running = False


# ---------------------------------------------------------------------------
# Flask app
# ---------------------------------------------------------------------------

def extra_status():
    return {
        "worker_running": _worker_running,
        "last_pulse_at": _last_pulse_at,
        "enabled": get_config_value("enabled", "true"),
        "trader_address": get_config_value("trader_address", ""),
        "usdc_approved": get_config_value("usdc_approved", "false"),
    }


app = create_app("perps_trader", status_extra_fn=extra_status)


# ----- /rpc/decision -----

@app.route("/rpc/decision", methods=["POST"])
def rpc_decision():
    body = request.get_json(silent=True) or {}
    decision = (body.get("decision") or "").upper()

    if decision not in ("OPEN_LONG", "OPEN_SHORT", "CLOSE", "HOLD"):
        return error("decision must be OPEN_LONG, OPEN_SHORT, CLOSE, or HOLD")

    pair_name = body.get("pair_name", "")
    pair_index = body.get("pair_index")
    reason = body.get("reason", "")
    trader_address = get_config_value("trader_address", "")

    conn = get_db()
    cur = conn.execute(
        "INSERT INTO trade_decisions (decision, pair_name, pair_index, reason, status) VALUES (?, ?, ?, ?, ?)",
        (decision, pair_name, pair_index, reason, "logged" if decision == "HOLD" else "pending"),
    )
    decision_id = cur.lastrowid
    conn.commit()
    conn.close()

    result = {"decision_id": decision_id, "decision": decision, "pair_name": pair_name}

    if decision in ("OPEN_LONG", "OPEN_SHORT"):
        # Validate constraints
        if not trader_address:
            return error("trader_address not set in config")
        if get_config_value("usdc_approved", "false").lower() != "true":
            return error("USDC not approved — call /rpc/approve_usdc first")

        # Check allowed pairs
        allowed_pairs = [p.strip() for p in get_config_value("allowed_pairs", "").split(",") if p.strip()]
        if allowed_pairs and pair_name not in allowed_pairs:
            return error(f"Pair {pair_name} not in allowed_pairs: {allowed_pairs}")

        # Check max positions
        conn2 = get_db()
        open_count = conn2.execute("SELECT COUNT(*) as c FROM positions").fetchone()["c"]
        conn2.close()
        max_positions = int(get_config_value("max_positions", "3"))
        if open_count >= max_positions:
            return error(f"Max positions ({max_positions}) reached")

        # Resolve pair index
        if pair_index is None:
            pair_index = resolve_pair_index(pair_name)
        if pair_index is None:
            return error(f"Cannot resolve pair_index for {pair_name}")

        # Validate leverage and collateral
        leverage = float(body.get("leverage", get_config_value("default_leverage", "10")))
        max_leverage = float(get_config_value("max_leverage", "25"))
        if leverage > max_leverage:
            return error(f"Leverage {leverage}x exceeds max {max_leverage}x")

        collateral = float(body.get("collateral", get_config_value("max_collateral_per_trade", "10")))
        max_collateral = float(get_config_value("max_collateral_per_trade", "10"))
        if collateral > max_collateral:
            return error(f"Collateral ${collateral} exceeds max ${max_collateral}")

        # Get current price
        current_price = get_current_price(pair_index)
        if not current_price:
            return error(f"Cannot get price for pair_index {pair_index}")

        # Calculate TP/SL
        is_long = decision == "OPEN_LONG"
        tp_pct = float(body.get("take_profit_pct", get_config_value("take_profit_pct", "50")))
        sl_pct = float(body.get("stop_loss_pct", get_config_value("stop_loss_pct", "25")))

        if is_long:
            take_profit = current_price * (1 + tp_pct / 100)
            stop_loss = current_price * (1 - sl_pct / 100)
        else:
            take_profit = current_price * (1 - tp_pct / 100)
            stop_loss = current_price * (1 + sl_pct / 100)

        # Build unsigned tx
        tx = build_open_trade_tx(
            trader_address, pair_index, is_long, collateral,
            leverage, take_profit, stop_loss, current_price,
        )

        if tx:
            conn3 = get_db()
            conn3.execute(
                "UPDATE trade_decisions SET pair_index = ?, updated_at = ? WHERE id = ?",
                (pair_index, now_iso(), decision_id),
            )
            cur3 = conn3.execute(
                """INSERT INTO trade_executions
                     (decision_id, tx_type, raw_tx_to, raw_tx_data, raw_tx_value, raw_tx_gas, status)
                   VALUES (?, 'open_trade', ?, ?, ?, ?, 'unsigned')""",
                (decision_id, tx["to"], tx["data"], tx["value"], tx["gas"]),
            )
            tx_id = cur3.lastrowid
            conn3.execute(
                "UPDATE trade_decisions SET status = 'tx_constructed', updated_at = ? WHERE id = ?",
                (now_iso(), decision_id),
            )
            conn3.commit()
            conn3.close()

            result["tx_id"] = tx_id
            result["tx"] = tx
            result["price"] = current_price
            result["leverage"] = leverage
            result["collateral"] = collateral
            result["take_profit"] = take_profit
            result["stop_loss"] = stop_loss

            # Fire sign hook
            fire_hook("perps_trader_sign_tx", {
                "tx_id": tx_id,
                "decision_id": decision_id,
                "decision": decision,
                "pair_name": pair_name,
                "to": tx["to"],
                "data": tx["data"],
                "value": tx["value"],
                "gas": tx["gas"],
                "chain_id": BASE_CHAIN_ID,
            })
        else:
            conn3 = get_db()
            conn3.execute(
                "UPDATE trade_decisions SET status = 'tx_failed', updated_at = ? WHERE id = ?",
                (now_iso(), decision_id),
            )
            conn3.commit()
            conn3.close()
            result["warning"] = "Failed to build trade tx via Avantis SDK"

    elif decision == "CLOSE":
        if not trader_address:
            return error("trader_address not set in config")

        trade_index = body.get("trade_index")
        if pair_index is None or trade_index is None:
            return error("pair_index and trade_index are required for CLOSE")

        # Get position from DB
        conn2 = get_db()
        pos = conn2.execute(
            "SELECT * FROM positions WHERE pair_index = ? AND trade_index = ?",
            (pair_index, trade_index),
        ).fetchone()
        conn2.close()

        if not pos:
            return error(f"No open position for pair_index={pair_index}, trade_index={trade_index}")

        collateral = float(pos["collateral_usdc"])
        tx = build_close_trade_tx(trader_address, pair_index, trade_index, collateral)

        if tx:
            conn3 = get_db()
            cur3 = conn3.execute(
                """INSERT INTO trade_executions
                     (decision_id, tx_type, raw_tx_to, raw_tx_data, raw_tx_value, raw_tx_gas, status)
                   VALUES (?, 'close_trade', ?, ?, ?, ?, 'unsigned')""",
                (decision_id, tx["to"], tx["data"], tx["value"], tx["gas"]),
            )
            tx_id = cur3.lastrowid
            conn3.execute(
                "UPDATE trade_decisions SET pair_index = ?, status = 'tx_constructed', updated_at = ? WHERE id = ?",
                (pair_index, now_iso(), decision_id),
            )
            conn3.commit()
            conn3.close()

            result["tx_id"] = tx_id
            result["tx"] = tx

            fire_hook("perps_trader_sign_tx", {
                "tx_id": tx_id,
                "decision_id": decision_id,
                "decision": decision,
                "pair_name": pair_name or pos["pair_name"],
                "to": tx["to"],
                "data": tx["data"],
                "value": tx["value"],
                "gas": tx["gas"],
                "chain_id": BASE_CHAIN_ID,
            })
        else:
            conn3 = get_db()
            conn3.execute(
                "UPDATE trade_decisions SET status = 'tx_failed', updated_at = ? WHERE id = ?",
                (now_iso(), decision_id),
            )
            conn3.commit()
            conn3.close()
            result["warning"] = "Failed to build close tx via Avantis SDK"

    return success(result)


# ----- /rpc/sign -----

@app.route("/rpc/sign", methods=["POST"])
def rpc_sign():
    body = request.get_json(silent=True) or {}
    tx_id = body.get("tx_id")
    signed_tx = body.get("signed_tx", "")

    if not tx_id:
        return error("tx_id is required")
    if not signed_tx or not signed_tx.startswith("0x"):
        return error("signed_tx must be a 0x-prefixed hex string")

    conn = get_db()
    row = conn.execute("SELECT * FROM trade_executions WHERE id = ?", (tx_id,)).fetchone()
    if not row:
        conn.close()
        return error(f"No execution found with tx_id={tx_id}", 404)

    conn.execute(
        "UPDATE trade_executions SET signed_tx = ?, status = 'signed', updated_at = ? WHERE id = ?",
        (signed_tx, now_iso(), tx_id),
    )
    conn.commit()
    conn.close()

    def do_broadcast():
        c = get_db()
        exec_row = c.execute("SELECT * FROM trade_executions WHERE id = ?", (tx_id,)).fetchone()
        decision_row = c.execute(
            "SELECT * FROM trade_decisions WHERE id = ?", (exec_row["decision_id"],)
        ).fetchone()
        tx_type = exec_row["tx_type"]
        c.close()

        tx_hash = broadcast_tx(signed_tx)
        c = get_db()
        if tx_hash:
            c.execute(
                "UPDATE trade_executions SET tx_hash = ?, status = 'broadcasted', updated_at = ? WHERE id = ?",
                (tx_hash, now_iso(), tx_id),
            )
            c.execute(
                "UPDATE trade_decisions SET status = 'broadcasted', updated_at = ? WHERE id = ?",
                (now_iso(), exec_row["decision_id"]),
            )
            c.commit()
            logging.info(f"[PERPS_TRADER] Broadcasted tx_id={tx_id} hash={tx_hash}")

            receipt = poll_receipt(tx_hash)
            if receipt:
                status_int = int(receipt.get("status", "0x0"), 16)
                final_status = "executed" if status_int == 1 else "reverted"
                c2 = get_db()
                c2.execute(
                    "UPDATE trade_executions SET status = ?, updated_at = ? WHERE id = ?",
                    (final_status, now_iso(), tx_id),
                )
                c2.execute(
                    "UPDATE trade_decisions SET status = ?, updated_at = ? WHERE id = ?",
                    (final_status, now_iso(), exec_row["decision_id"]),
                )
                c2.commit()

                if final_status == "executed":
                    if tx_type == "open_trade":
                        _handle_open_confirmed(exec_row, decision_row, tx_hash)
                    elif tx_type == "close_trade":
                        _handle_close_confirmed(exec_row, decision_row, tx_hash)
                    elif tx_type == "approve_usdc":
                        set_config_value("usdc_approved", "true")
                        logging.info("[PERPS_TRADER] USDC approval confirmed")
                c2.close()
            else:
                logging.warning(f"[PERPS_TRADER] Receipt timeout for tx_id={tx_id}")
        else:
            c.execute(
                "UPDATE trade_executions SET status = 'broadcast_failed', error_msg = 'RPC error', updated_at = ? WHERE id = ?",
                (now_iso(), tx_id),
            )
            c.execute(
                "UPDATE trade_decisions SET status = 'failed', updated_at = ? WHERE id = ?",
                (now_iso(), exec_row["decision_id"]),
            )
            c.commit()
        c.close()

    threading.Thread(target=do_broadcast, daemon=True).start()
    return success({"tx_id": tx_id, "status": "broadcasting"})


def _handle_open_confirmed(exec_row, decision_row, tx_hash: str):
    """Handle confirmed open trade — add to positions table."""
    decision = decision_row["decision"]
    pair_name = decision_row["pair_name"] or ""
    pair_index = decision_row["pair_index"]
    is_long = 1 if decision == "OPEN_LONG" else 0
    trader_address = get_config_value("trader_address", "")

    # Try to get trade details from on-chain
    trades = fetch_onchain_trades(trader_address) if trader_address else []

    # Find our new trade (latest for this pair)
    our_trade = None
    for t in trades:
        if t["pair_index"] == pair_index:
            if our_trade is None or t["trade_index"] > our_trade["trade_index"]:
                our_trade = t

    if our_trade:
        liq_price = calc_liquidation_price(our_trade["is_long"], our_trade["open_price"], our_trade["leverage"])
        current_price = get_current_price(pair_index)
        pnl, pnl_pct = (0, 0)
        if current_price:
            pnl, pnl_pct = calc_pnl(our_trade["is_long"], our_trade["collateral"], our_trade["leverage"], our_trade["open_price"], current_price)

        conn = get_db()
        conn.execute(
            """INSERT OR REPLACE INTO positions
                 (pair_name, pair_index, trade_index, is_long, collateral_usdc,
                  leverage, position_size, open_price, current_price, take_profit,
                  stop_loss, liquidation_price, unrealized_pnl, pnl_pct,
                  decision_id, tx_hash, opened_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (pair_name, pair_index, our_trade["trade_index"], int(our_trade["is_long"]),
             our_trade["collateral"], our_trade["leverage"],
             our_trade["collateral"] * our_trade["leverage"],
             our_trade["open_price"], current_price, our_trade["tp"], our_trade["sl"],
             liq_price, pnl, pnl_pct,
             decision_row["id"], tx_hash, now_iso(), now_iso()),
        )
        conn.commit()
        conn.close()
        logging.info(f"[PERPS_TRADER] Position opened: {pair_name} {'LONG' if is_long else 'SHORT'} @ {our_trade['open_price']}")
    else:
        logging.warning(f"[PERPS_TRADER] Open confirmed but couldn't find trade on-chain for pair_index={pair_index}")


def _handle_close_confirmed(exec_row, decision_row, tx_hash: str):
    """Handle confirmed close trade — move to history, remove from positions."""
    pair_index = decision_row["pair_index"]

    conn = get_db()
    # Find the position that was closed (from decision context)
    pos = conn.execute(
        "SELECT * FROM positions WHERE pair_index = ?", (pair_index,)
    ).fetchone()

    if pos:
        close_price = get_current_price(pair_index) or float(pos["current_price"] or pos["open_price"])
        pnl, pnl_pct = calc_pnl(
            bool(pos["is_long"]), float(pos["collateral_usdc"]),
            float(pos["leverage"]), float(pos["open_price"]), close_price,
        )

        conn.execute(
            """INSERT INTO trade_history
                 (pair_name, pair_index, is_long, collateral_usdc, leverage,
                  position_size, open_price, close_price, realized_pnl, pnl_pct,
                  decision_id, open_tx_hash, close_tx_hash, opened_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (pos["pair_name"], pos["pair_index"], pos["is_long"],
             pos["collateral_usdc"], pos["leverage"], pos.get("position_size"),
             pos["open_price"], close_price, pnl, pnl_pct,
             decision_row["id"], pos.get("tx_hash"), tx_hash, pos.get("opened_at")),
        )
        conn.execute(
            "DELETE FROM positions WHERE pair_index = ? AND trade_index = ?",
            (pos["pair_index"], pos["trade_index"]),
        )
        conn.commit()
        logging.info(f"[PERPS_TRADER] Position closed: {pos['pair_name']} P&L=${pnl} ({pnl_pct}%)")
    else:
        logging.warning(f"[PERPS_TRADER] Close confirmed but no position found for pair_index={pair_index}")

    conn.close()


# ----- /rpc/positions -----

@app.route("/rpc/positions", methods=["GET"])
def rpc_positions():
    conn = get_db()
    rows = conn.execute("SELECT * FROM positions ORDER BY opened_at DESC").fetchall()
    conn.close()
    return success([dict(r) for r in rows])


# ----- /rpc/sync_positions -----

@app.route("/rpc/sync_positions", methods=["POST"])
def rpc_sync_positions():
    result = sync_positions_from_chain()
    return success(result)


# ----- /rpc/pnl -----

@app.route("/rpc/pnl", methods=["GET"])
def rpc_pnl():
    return success(compute_pnl())


# ----- /rpc/refresh -----

@app.route("/rpc/refresh", methods=["POST"])
def rpc_refresh():
    refreshed = refresh_positions_prices()
    conn = get_db()
    rows = conn.execute("SELECT * FROM positions ORDER BY opened_at DESC").fetchall()
    conn.close()
    return success({
        "positions_refreshed": refreshed,
        "positions": [dict(r) for r in rows],
    })


# ----- /rpc/history -----

@app.route("/rpc/history", methods=["GET", "POST"])
def rpc_history():
    body = request.get_json(silent=True) or {}
    limit = int(body.get("limit", 20))
    status_filter = body.get("status", "all")

    conn = get_db()
    if status_filter == "all":
        rows = conn.execute(
            "SELECT * FROM trade_decisions ORDER BY created_at DESC LIMIT ?", (limit,)
        ).fetchall()
    else:
        rows = conn.execute(
            "SELECT * FROM trade_decisions WHERE status = ? ORDER BY created_at DESC LIMIT ?",
            (status_filter, limit),
        ).fetchall()
    conn.close()
    return success([dict(r) for r in rows])


# ----- /rpc/stats -----

@app.route("/rpc/stats", methods=["GET"])
def rpc_stats():
    conn = get_db()
    total = conn.execute("SELECT COUNT(*) as c FROM trade_decisions").fetchone()["c"]
    open_longs = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE decision='OPEN_LONG'").fetchone()["c"]
    open_shorts = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE decision='OPEN_SHORT'").fetchone()["c"]
    closes = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE decision='CLOSE'").fetchone()["c"]
    holds = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE decision='HOLD'").fetchone()["c"]
    executed = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE status='executed'").fetchone()["c"]
    failed = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE status IN ('failed','reverted','broadcast_failed','tx_failed')").fetchone()["c"]
    open_positions = conn.execute("SELECT COUNT(*) as c FROM positions").fetchone()["c"]
    total_collateral = conn.execute("SELECT COALESCE(SUM(collateral_usdc), 0) as c FROM positions").fetchone()["c"]
    conn.close()
    return success({
        "total_decisions": total,
        "open_longs": open_longs,
        "open_shorts": open_shorts,
        "closes": closes,
        "holds": holds,
        "executed": executed,
        "failed": failed,
        "open_positions": open_positions,
        "total_collateral_deployed": round(float(total_collateral), 2),
    })


# ----- /rpc/trade_history -----

@app.route("/rpc/trade_history", methods=["GET"])
def rpc_trade_history():
    limit = int(request.args.get("limit", 50))
    conn = get_db()
    rows = conn.execute(
        "SELECT * FROM trade_history ORDER BY closed_at DESC LIMIT ?", (limit,)
    ).fetchall()
    conn.close()
    return success([dict(r) for r in rows])


# ----- /rpc/config -----

@app.route("/rpc/config", methods=["GET", "POST"])
def rpc_config():
    if request.method == "GET":
        conn = get_db()
        rows = conn.execute("SELECT * FROM trader_config").fetchall()
        conn.close()
        return success({r["key"]: r["value"] for r in rows})

    body = request.get_json(silent=True) or {}
    key = body.get("key")
    value = body.get("value")
    if not key or value is None:
        return error("key and value are required")
    allowed_keys = {
        "pulse_interval", "enabled", "trader_address",
        "max_collateral_per_trade", "max_leverage", "default_leverage",
        "max_positions", "take_profit_pct", "stop_loss_pct",
        "bankr_min_confidence", "bankr_providers", "usdc_approved",
        "allowed_pairs",
    }
    if key not in allowed_keys:
        return error(f"Unknown config key: {key}. Allowed: {', '.join(sorted(allowed_keys))}")
    set_config_value(key, str(value))
    return success({"key": key, "value": str(value)})


# ----- /rpc/control -----

@app.route("/rpc/control", methods=["POST"])
def rpc_control():
    body = request.get_json(silent=True) or {}
    action = body.get("action", "")

    if action == "start":
        start_worker()
        return success({"action": "start", "worker_running": True})
    elif action == "stop":
        stop_worker()
        return success({"action": "stop", "worker_running": False})
    elif action == "trigger":
        _fire_pulse()
        return success({"action": "trigger", "fired": True})
    else:
        return error("action must be 'start', 'stop', or 'trigger'")


# ----- /rpc/approve_usdc -----

@app.route("/rpc/approve_usdc", methods=["POST"])
def rpc_approve_usdc():
    trader_address = get_config_value("trader_address", "")
    if not trader_address:
        return error("trader_address not set in config")

    tx = build_usdc_approve_tx(trader_address)
    if not tx:
        return error("Failed to build USDC approval tx")

    conn = get_db()
    cur = conn.execute(
        "INSERT INTO trade_decisions (decision, pair_name, reason, status) VALUES ('APPROVE_USDC', '', 'USDC approval for Avantis trading', 'pending')"
    )
    decision_id = cur.lastrowid
    cur2 = conn.execute(
        """INSERT INTO trade_executions
             (decision_id, tx_type, raw_tx_to, raw_tx_data, raw_tx_value, raw_tx_gas, status)
           VALUES (?, 'approve_usdc', ?, ?, ?, ?, 'unsigned')""",
        (decision_id, tx["to"], tx["data"], tx["value"], tx["gas"]),
    )
    tx_id = cur2.lastrowid
    conn.execute(
        "UPDATE trade_decisions SET status = 'tx_constructed', updated_at = ? WHERE id = ?",
        (now_iso(), decision_id),
    )
    conn.commit()
    conn.close()

    fire_hook("perps_trader_sign_tx", {
        "tx_id": tx_id,
        "decision_id": decision_id,
        "decision": "APPROVE_USDC",
        "pair_name": "",
        "to": tx["to"],
        "data": tx["data"],
        "value": tx["value"],
        "gas": tx["gas"],
        "chain_id": BASE_CHAIN_ID,
    })

    return success({"decision_id": decision_id, "tx_id": tx_id, "tx": tx})


# ----- /rpc/pairs -----

@app.route("/rpc/pairs", methods=["GET"])
def rpc_pairs():
    pairs = get_pairs_map()
    allowed_raw = get_config_value("allowed_pairs", "")
    allowed = {p.strip() for p in allowed_raw.split(",") if p.strip()} if allowed_raw else set()
    result = []
    for name, idx in sorted(pairs.items(), key=lambda x: x[1]):
        result.append({
            "name": name,
            "index": idx,
            "allowed": name in allowed if allowed else True,
        })
    return success(result)


# ----- /rpc/backup -----

@app.route("/rpc/backup/export", methods=["POST"])
def rpc_backup_export():
    conn = get_db()
    decisions = conn.execute("SELECT * FROM trade_decisions ORDER BY id").fetchall()
    executions = conn.execute("SELECT * FROM trade_executions ORDER BY id").fetchall()
    config = conn.execute("SELECT * FROM trader_config").fetchall()
    positions = conn.execute("SELECT * FROM positions ORDER BY id").fetchall()
    history = conn.execute("SELECT * FROM trade_history ORDER BY id").fetchall()
    conn.close()
    return success({
        "decisions": [dict(r) for r in decisions],
        "executions": [dict(r) for r in executions],
        "config": {r["key"]: r["value"] for r in config},
        "positions": [dict(r) for r in positions],
        "trade_history": [dict(r) for r in history],
    })


@app.route("/rpc/backup/restore", methods=["POST"])
def rpc_backup_restore():
    body = request.get_json(silent=True) or {}
    data = body.get("data") or body
    conn = get_db()
    restored = 0

    for d in data.get("decisions", []):
        try:
            conn.execute(
                """INSERT OR REPLACE INTO trade_decisions
                     (id, decision, pair_name, pair_index, reason, status, created_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
                (d["id"], d["decision"], d.get("pair_name"), d.get("pair_index"),
                 d.get("reason"), d.get("status", "pending"), d.get("created_at"), d.get("updated_at")),
            )
            restored += 1
        except Exception:
            pass

    for e in data.get("executions", []):
        try:
            conn.execute(
                """INSERT OR REPLACE INTO trade_executions
                     (id, decision_id, tx_type, raw_tx_to, raw_tx_data, raw_tx_value,
                      raw_tx_gas, signed_tx, tx_hash, status, error_msg, created_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (e["id"], e["decision_id"], e.get("tx_type"), e.get("raw_tx_to"),
                 e.get("raw_tx_data"), e.get("raw_tx_value"), e.get("raw_tx_gas"),
                 e.get("signed_tx"), e.get("tx_hash"), e.get("status", "unsigned"),
                 e.get("error_msg"), e.get("created_at"), e.get("updated_at")),
            )
            restored += 1
        except Exception:
            pass

    for k, v in data.get("config", {}).items():
        set_config_value(k, v)

    for p in data.get("positions", []):
        try:
            conn.execute(
                """INSERT OR REPLACE INTO positions
                     (id, pair_name, pair_index, trade_index, is_long, collateral_usdc,
                      leverage, position_size, open_price, current_price, take_profit,
                      stop_loss, liquidation_price, unrealized_pnl, pnl_pct,
                      decision_id, tx_hash, opened_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (p["id"], p["pair_name"], p["pair_index"], p["trade_index"],
                 p["is_long"], p["collateral_usdc"], p["leverage"],
                 p.get("position_size"), p["open_price"], p.get("current_price"),
                 p.get("take_profit"), p.get("stop_loss"), p.get("liquidation_price"),
                 p.get("unrealized_pnl", 0), p.get("pnl_pct", 0),
                 p.get("decision_id"), p.get("tx_hash"), p.get("opened_at"), p.get("updated_at")),
            )
            restored += 1
        except Exception:
            pass

    for h in data.get("trade_history", []):
        try:
            conn.execute(
                """INSERT OR REPLACE INTO trade_history
                     (id, pair_name, pair_index, is_long, collateral_usdc, leverage,
                      position_size, open_price, close_price, realized_pnl, pnl_pct,
                      decision_id, open_tx_hash, close_tx_hash, opened_at, closed_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (h["id"], h["pair_name"], h["pair_index"], h["is_long"],
                 h.get("collateral_usdc"), h.get("leverage"), h.get("position_size"),
                 h.get("open_price"), h.get("close_price"), h.get("realized_pnl"),
                 h.get("pnl_pct"), h.get("decision_id"), h.get("open_tx_hash"),
                 h.get("close_tx_hash"), h.get("opened_at"), h.get("closed_at")),
            )
            restored += 1
        except Exception:
            pass

    conn.commit()
    conn.close()
    return success({"restored": restored})


# ----- Dashboard -----

DASHBOARD_HTML = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Perps Trader</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;
     background:#0d1117;color:#c9d1d9;padding:24px}
h1{font-size:1.4rem;margin-bottom:16px;color:#58a6ff}
h2{font-size:1.1rem;margin:20px 0 10px;color:#79c0ff}
.stats{display:flex;gap:12px;flex-wrap:wrap;margin-bottom:20px}
.stat{background:#161b22;border:1px solid #30363d;border-radius:8px;padding:12px 18px;min-width:100px}
.stat .val{font-size:1.6rem;font-weight:bold;color:#58a6ff}
.stat .lbl{font-size:.8rem;color:#8b949e;margin-top:2px}
.stat.long .val,.stat.pos .val{color:#3fb950}
.stat.short .val,.stat.neg .val{color:#f85149}
.toolbar{display:flex;gap:8px;margin-bottom:12px;align-items:center;flex-wrap:wrap}
.btn{background:#238636;color:#fff;border:none;padding:6px 14px;border-radius:6px;
     cursor:pointer;font-size:.85rem;font-weight:500}
.btn:hover{background:#2ea043}
.btn-danger{background:#da3633}.btn-danger:hover{background:#f85149}
.btn-secondary{background:#30363d;color:#c9d1d9}.btn-secondary:hover{background:#484f58}
.badge{display:inline-block;padding:2px 8px;border-radius:10px;font-size:.75rem;font-weight:600}
.badge-long{background:#238636;color:#fff}
.badge-short{background:#da3633;color:#fff}
.badge-hold{background:#30363d;color:#8b949e}
.badge-ok{background:#238636;color:#fff}
.badge-fail{background:#da3633;color:#fff}
.badge-pending{background:#d29922;color:#000}
table{width:100%;border-collapse:collapse;margin-top:8px}
th,td{text-align:left;padding:8px 10px;border-bottom:1px solid #21262d;font-size:.85rem}
th{color:#8b949e;font-weight:600;text-transform:uppercase;font-size:.75rem}
td{font-family:"SF Mono",Consolas,monospace}
.empty{color:#484f58;padding:20px;text-align:center}
.worker-status{font-size:.85rem;color:#8b949e;flex:1}
.worker-status .dot{display:inline-block;width:8px;height:8px;border-radius:50%;margin-right:4px}
.dot-on{background:#3fb950}.dot-off{background:#f85149}
.pnl-pos{color:#3fb950}.pnl-neg{color:#f85149}.pnl-zero{color:#8b949e}
.toast{position:fixed;bottom:20px;right:20px;padding:10px 16px;border-radius:6px;
       opacity:0;transition:opacity .3s;pointer-events:none;z-index:99;color:#fff}
.toast.show{opacity:1}.toast.ok{background:#238636}.toast.err{background:#da3633}
</style>
</head>
<body>
<h1>Perps Trader</h1>

<div class="stats" id="stats"><div class="stat"><div class="val">...</div><div class="lbl">Loading</div></div></div>

<div class="stats" id="pnl-stats"></div>

<div class="toolbar">
  <div class="worker-status" id="worker-status">...</div>
  <button class="btn btn-secondary" onclick="refreshPrices()">Refresh Prices</button>
  <button class="btn btn-secondary" onclick="syncPositions()">Sync Positions</button>
  <button class="btn" onclick="ctrl('trigger')">Trigger Pulse</button>
  <button class="btn btn-secondary" onclick="ctrl('start')">Start Worker</button>
  <button class="btn btn-danger" onclick="ctrl('stop')">Stop Worker</button>
</div>

<h2>Open Positions</h2>
<table>
<thead><tr><th>Pair</th><th>Direction</th><th>Collateral</th><th>Leverage</th><th>Entry</th><th>Current</th><th>Liq. Price</th><th>P&L %</th><th>Unrealized</th></tr></thead>
<tbody id="positions"><tr><td colspan="9" class="empty">Loading...</td></tr></tbody>
</table>

<h2>Trade History</h2>
<table>
<thead><tr><th>Pair</th><th>Direction</th><th>Collateral</th><th>Leverage</th><th>Entry</th><th>Exit</th><th>Realized P&L</th><th>P&L %</th><th>Closed</th></tr></thead>
<tbody id="trade-history"><tr><td colspan="9" class="empty">Loading...</td></tr></tbody>
</table>

<h2>Recent Decisions</h2>
<table>
<thead><tr><th>ID</th><th>Decision</th><th>Pair</th><th>Reason</th><th>Status</th><th>Time</th></tr></thead>
<tbody id="decisions"><tr><td colspan="6" class="empty">Loading...</td></tr></tbody>
</table>

<div class="toast" id="toast"></div>

<script>
function api(path,opts){return fetch(path,opts).then(r=>r.json())}

function toast(msg,ok){
  const t=document.getElementById('toast');
  t.textContent=msg;t.className='toast show '+(ok?'ok':'err');
  setTimeout(()=>t.className='toast',2000);
}

function badge(type,text){
  const cls={'OPEN_LONG':'long','OPEN_SHORT':'short','CLOSE':'short','HOLD':'hold','APPROVE_USDC':'pending',
    'executed':'ok','broadcasted':'ok','signed':'pending',
    'tx_constructed':'pending','pending':'pending','logged':'hold',
    'failed':'fail','reverted':'fail','broadcast_failed':'fail','tx_failed':'fail'};
  return '<span class="badge badge-'+(cls[type]||'hold')+'">'+text+'</span>';
}

function dirBadge(isLong){
  return isLong?'<span class="badge badge-long">LONG</span>':'<span class="badge badge-short">SHORT</span>';
}

function pnlClass(v){return v>0?'pnl-pos':v<0?'pnl-neg':'pnl-zero'}
function pnlFmt(v){if(v==null)return '—';v=parseFloat(v);return (v>=0?'+':'')+v.toFixed(2)}
function priceFmt(v){if(v==null||v==0)return '—';v=parseFloat(v);return v>=1?'$'+v.toFixed(2):'$'+v.toFixed(6)}

function loadStats(){
  api('rpc/stats').then(d=>{
    const s=d.data||{};
    document.getElementById('stats').innerHTML=
      '<div class="stat"><div class="val">'+s.open_positions+'</div><div class="lbl">Positions</div></div>'+
      '<div class="stat"><div class="val">$'+s.total_collateral_deployed+'</div><div class="lbl">Collateral</div></div>'+
      '<div class="stat"><div class="val">'+s.total_decisions+'</div><div class="lbl">Decisions</div></div>'+
      '<div class="stat long"><div class="val">'+s.open_longs+'</div><div class="lbl">Longs</div></div>'+
      '<div class="stat short"><div class="val">'+s.open_shorts+'</div><div class="lbl">Shorts</div></div>'+
      '<div class="stat"><div class="val">'+s.closes+'</div><div class="lbl">Closes</div></div>'+
      '<div class="stat long"><div class="val">'+s.executed+'</div><div class="lbl">Executed</div></div>'+
      '<div class="stat short"><div class="val">'+s.failed+'</div><div class="lbl">Failed</div></div>';
  });
}

function loadPnl(){
  api('rpc/pnl').then(d=>{
    const p=d.data||{};
    const el=document.getElementById('pnl-stats');
    el.innerHTML=
      '<div class="stat '+(p.total_pnl>=0?'pos':'neg')+'"><div class="val">$'+pnlFmt(p.total_pnl)+'</div><div class="lbl">Total P&L</div></div>'+
      '<div class="stat '+(p.total_realized_pnl>=0?'pos':'neg')+'"><div class="val">$'+pnlFmt(p.total_realized_pnl)+'</div><div class="lbl">Realized</div></div>'+
      '<div class="stat '+(p.total_unrealized_pnl>=0?'pos':'neg')+'"><div class="val">$'+pnlFmt(p.total_unrealized_pnl)+'</div><div class="lbl">Unrealized</div></div>'+
      '<div class="stat"><div class="val">'+(p.win_rate*100).toFixed(1)+'%</div><div class="lbl">Win Rate ('+p.win_count+'W/'+p.loss_count+'L)</div></div>'+
      (p.best_trade?'<div class="stat pos"><div class="val">$'+pnlFmt(p.best_trade.pnl)+'</div><div class="lbl">Best: '+p.best_trade.pair+'</div></div>':'')+
      (p.worst_trade?'<div class="stat neg"><div class="val">$'+pnlFmt(p.worst_trade.pnl)+'</div><div class="lbl">Worst: '+p.worst_trade.pair+'</div></div>':'');
  });
}

function loadPositions(){
  api('rpc/positions').then(d=>{
    const rows=d.data||[];
    const tb=document.getElementById('positions');
    if(!rows.length){tb.innerHTML='<tr><td colspan="9" class="empty">No open positions</td></tr>';return}
    tb.innerHTML=rows.map(r=>{
      const pnlPct=parseFloat(r.pnl_pct||0);
      const unrealized=parseFloat(r.unrealized_pnl||0);
      return '<tr>'+
        '<td>'+r.pair_name+'</td>'+
        '<td>'+dirBadge(r.is_long)+'</td>'+
        '<td>$'+parseFloat(r.collateral_usdc).toFixed(2)+'</td>'+
        '<td>'+parseFloat(r.leverage).toFixed(1)+'x</td>'+
        '<td>'+priceFmt(r.open_price)+'</td>'+
        '<td>'+priceFmt(r.current_price)+'</td>'+
        '<td>'+priceFmt(r.liquidation_price)+'</td>'+
        '<td class="'+pnlClass(pnlPct)+'">'+pnlFmt(pnlPct)+'%</td>'+
        '<td class="'+pnlClass(unrealized)+'">$'+pnlFmt(unrealized)+'</td>'+
      '</tr>'}).join('');
  });
}

function loadTradeHistory(){
  api('rpc/trade_history').then(d=>{
    const rows=d.data||[];
    const tb=document.getElementById('trade-history');
    if(!rows.length){tb.innerHTML='<tr><td colspan="9" class="empty">No trades yet</td></tr>';return}
    tb.innerHTML=rows.map(r=>{
      const rpnl=parseFloat(r.realized_pnl||0);
      const rpct=parseFloat(r.pnl_pct||0);
      return '<tr>'+
        '<td>'+r.pair_name+'</td>'+
        '<td>'+dirBadge(r.is_long)+'</td>'+
        '<td>'+(r.collateral_usdc!=null?'$'+parseFloat(r.collateral_usdc).toFixed(2):'—')+'</td>'+
        '<td>'+(r.leverage!=null?parseFloat(r.leverage).toFixed(1)+'x':'—')+'</td>'+
        '<td>'+priceFmt(r.open_price)+'</td>'+
        '<td>'+priceFmt(r.close_price)+'</td>'+
        '<td class="'+pnlClass(rpnl)+'">$'+pnlFmt(rpnl)+'</td>'+
        '<td class="'+pnlClass(rpct)+'">'+pnlFmt(rpct)+'%</td>'+
        '<td>'+(r.closed_at||'')+'</td>'+
      '</tr>'}).join('');
  });
}

function loadDecisions(){
  api('rpc/history',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({limit:30})}).then(d=>{
    const rows=d.data||[];
    const tb=document.getElementById('decisions');
    if(!rows.length){tb.innerHTML='<tr><td colspan="6" class="empty">No decisions yet</td></tr>';return}
    tb.innerHTML=rows.map(r=>'<tr>'+
      '<td>'+r.id+'</td>'+
      '<td>'+badge(r.decision,r.decision)+'</td>'+
      '<td>'+(r.pair_name||'—')+'</td>'+
      '<td style="max-width:300px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">'+(r.reason||'—')+'</td>'+
      '<td>'+badge(r.status,r.status)+'</td>'+
      '<td>'+(r.created_at||'')+'</td>'+
    '</tr>').join('');
  });
}

function loadWorker(){
  api('rpc/status').then(d=>{
    const s=d.data||{};
    const running=s.worker_running;
    const el=document.getElementById('worker-status');
    el.innerHTML='<span class="dot '+(running?'dot-on':'dot-off')+'"></span>Worker '+(running?'running':'stopped')+
      (s.last_pulse_at?' &middot; Last pulse: '+s.last_pulse_at:'')+
      (s.usdc_approved==='true'?' &middot; USDC: Approved':' &middot; <span style="color:#f85149">USDC: Not Approved</span>');
  });
}

function ctrl(action){
  api('rpc/control',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({action:action})}).then(d=>{
    toast(action+' OK',d.success!==false);
    loadWorker();
    if(action==='trigger')setTimeout(()=>{loadDecisions();loadStats();loadPositions()},3000);
  });
}

function refreshPrices(){
  api('rpc/refresh',{method:'POST'}).then(d=>{
    toast('Prices refreshed',d.success!==false);
    loadPositions();loadPnl();
  });
}

function syncPositions(){
  api('rpc/sync_positions',{method:'POST'}).then(d=>{
    const r=d.data||{};
    toast('Synced: '+r.synced+' trades (+'+(r.added||0)+' new, '+(r.closed||0)+' closed)',d.success!==false);
    loadPositions();loadPnl();loadStats();
  });
}

loadStats();loadPnl();loadPositions();loadTradeHistory();loadDecisions();loadWorker();
setInterval(()=>{loadStats();loadPnl();loadPositions();loadTradeHistory();loadDecisions();loadWorker()},15000);
</script>
</body>
</html>"""


@app.route("/")
def dashboard():
    return Response(DASHBOARD_HTML, content_type="text/html")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    logging.getLogger("werkzeug").setLevel(logging.ERROR)
    init_db()
    port = int(os.environ.get("MODULE_PORT", os.environ.get("PERPS_TRADER_PORT", "9105")))
    # Start pulse worker if enabled
    if get_config_value("enabled", "true").lower() == "true":
        start_worker()
    app.run(host="127.0.0.1", port=port)
