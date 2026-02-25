# /// script
# requires-python = ">=3.12"
# dependencies = ["flask", "requests", "starkbot-sdk"]
#
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
Wallet Monitor module — monitors ETH wallets for on-chain activity and whale trades.

Supports Ethereum Mainnet and Base chains via Alchemy Enhanced APIs.
Background worker polls every 40s, detects swaps, estimates USD values,
and flags large trades above configurable thresholds.

RPC protocol endpoints:
  GET  /rpc/status             -> service health
  POST /rpc/tools/watchlist    -> manage watchlist (action-based)
  POST /rpc/tools/activity     -> query activity (action-based)
  POST /rpc/tools/control      -> worker control (action-based)
  POST /rpc/backup/export      -> export watchlist for backup
  POST /rpc/backup/restore     -> restore watchlist from backup
  GET  /                       -> HTML dashboard

Launch with:  uv run service.py
"""

from flask import request
from starkbot_sdk import create_app, success, error
import sqlite3
import os
import re
import json
import time
import logging
import threading
import requests as http_requests
from datetime import datetime, timezone

DB_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "wallet_monitor.db")
POLL_INTERVAL = int(os.environ.get("WALLET_MONITOR_POLL_INTERVAL", "40"))
ALCHEMY_API_KEY = os.environ.get("ALCHEMY_API_KEY", "")
ALERT_CALLBACK_URL = os.environ.get("ALERT_CALLBACK_URL")
FIRST_RUN_LOOKBACK_BLOCKS = 500
PRICE_CACHE_TTL = 60

# Module-level state for worker
_start_time = time.time()
_last_tick_at = None
_last_tick_lock = threading.Lock()
_price_cache: dict[str, tuple[float, float]] = {}  # symbol -> (price, timestamp)
_price_cache_lock = threading.Lock()


# ---------------------------------------------------------------------------
# Database helpers
# ---------------------------------------------------------------------------

def get_db():
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    return conn


def init_db():
    conn = get_db()
    conn.execute("""
        CREATE TABLE IF NOT EXISTS wallet_watchlist (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            address TEXT NOT NULL,
            label TEXT,
            chain TEXT NOT NULL DEFAULT 'mainnet',
            monitor_enabled INTEGER NOT NULL DEFAULT 1,
            large_trade_threshold_usd REAL NOT NULL DEFAULT 1000.0,
            copy_trade_enabled INTEGER NOT NULL DEFAULT 0,
            copy_trade_max_usd REAL,
            last_checked_block INTEGER,
            last_checked_at TEXT,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(address, chain)
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS wallet_activity (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            watchlist_id INTEGER NOT NULL,
            chain TEXT NOT NULL,
            tx_hash TEXT NOT NULL,
            block_number INTEGER NOT NULL,
            block_timestamp TEXT,
            from_address TEXT NOT NULL,
            to_address TEXT NOT NULL,
            activity_type TEXT NOT NULL,
            asset_symbol TEXT,
            asset_address TEXT,
            amount_raw TEXT,
            amount_formatted TEXT,
            usd_value REAL,
            is_large_trade INTEGER NOT NULL DEFAULT 0,
            swap_from_token TEXT,
            swap_from_amount TEXT,
            swap_to_token TEXT,
            swap_to_amount TEXT,
            raw_data TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (watchlist_id) REFERENCES wallet_watchlist(id) ON DELETE CASCADE,
            UNIQUE(tx_hash, watchlist_id)
        )
    """)
    conn.execute("CREATE INDEX IF NOT EXISTS idx_wallet_activity_watchlist ON wallet_activity(watchlist_id, block_number DESC)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_wallet_activity_large ON wallet_activity(is_large_trade, created_at DESC)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_wallet_activity_chain ON wallet_activity(chain, block_number DESC)")
    conn.commit()
    conn.close()


def row_to_dict(row):
    if row is None:
        return None
    return dict(row)


def now_iso():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S+00:00")


def is_valid_eth_address(addr: str) -> bool:
    return bool(addr and addr.startswith("0x") and len(addr) == 42 and all(c in "0123456789abcdefABCDEF" for c in addr[2:]))


# ---------------------------------------------------------------------------
# Watchlist operations
# ---------------------------------------------------------------------------

def watchlist_add(address: str, label: str | None, chain: str, threshold_usd: float):
    if not is_valid_eth_address(address):
        return None, "Invalid Ethereum address"
    conn = get_db()
    ts = now_iso()
    addr = address.lower()
    try:
        conn.execute(
            "INSERT INTO wallet_watchlist (address, label, chain, large_trade_threshold_usd, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
            (addr, label, chain, threshold_usd, ts, ts),
        )
        conn.commit()
        entry_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
        row = conn.execute("SELECT * FROM wallet_watchlist WHERE id = ?", (entry_id,)).fetchone()
        conn.close()
        return row_to_dict(row), None
    except sqlite3.IntegrityError:
        conn.close()
        return None, f"Wallet {address} already on watchlist for chain {chain}"


def watchlist_remove(entry_id: int):
    conn = get_db()
    cursor = conn.execute("DELETE FROM wallet_watchlist WHERE id = ?", (entry_id,))
    conn.commit()
    conn.close()
    return cursor.rowcount > 0


def watchlist_list():
    conn = get_db()
    rows = conn.execute(
        "SELECT * FROM wallet_watchlist ORDER BY created_at ASC"
    ).fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


def watchlist_update(entry_id: int, label=None, threshold_usd=None, monitor_enabled=None, notes=None):
    conn = get_db()
    ts = now_iso()
    updates = ["updated_at = ?"]
    params: list = [ts]
    if label is not None:
        updates.append("label = ?")
        params.append(label)
    if threshold_usd is not None:
        updates.append("large_trade_threshold_usd = ?")
        params.append(threshold_usd)
    if monitor_enabled is not None:
        updates.append("monitor_enabled = ?")
        params.append(1 if monitor_enabled else 0)
    if notes is not None:
        updates.append("notes = ?")
        params.append(notes)
    params.append(entry_id)
    sql = f"UPDATE wallet_watchlist SET {', '.join(updates)} WHERE id = ?"
    cursor = conn.execute(sql, params)
    conn.commit()
    conn.close()
    return cursor.rowcount > 0


# ---------------------------------------------------------------------------
# Activity operations
# ---------------------------------------------------------------------------

def activity_query(watchlist_id=None, address=None, activity_type=None, chain=None, large_only=False, limit=50):
    conn = get_db()
    conditions = ["1=1"]
    params: list = []
    if watchlist_id is not None:
        conditions.append("a.watchlist_id = ?")
        params.append(watchlist_id)
    if address:
        conditions.append("(a.from_address = ? OR a.to_address = ?)")
        params.extend([address.lower(), address.lower()])
    if activity_type:
        conditions.append("a.activity_type = ?")
        params.append(activity_type)
    if chain:
        conditions.append("a.chain = ?")
        params.append(chain)
    if large_only:
        conditions.append("a.is_large_trade = 1")
    limit = min(limit or 50, 200)
    sql = f"""
        SELECT a.* FROM wallet_activity a
        WHERE {' AND '.join(conditions)}
        ORDER BY a.block_number DESC, a.id DESC
        LIMIT {limit}
    """
    rows = conn.execute(sql, params).fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


def activity_stats():
    conn = get_db()
    total = conn.execute("SELECT COUNT(*) FROM wallet_activity").fetchone()[0]
    large = conn.execute("SELECT COUNT(*) FROM wallet_activity WHERE is_large_trade = 1").fetchone()[0]
    watched = conn.execute("SELECT COUNT(*) FROM wallet_watchlist").fetchone()[0]
    active = conn.execute("SELECT COUNT(*) FROM wallet_watchlist WHERE monitor_enabled = 1").fetchone()[0]
    conn.close()
    return {
        "total_transactions": total,
        "large_trades": large,
        "watched_wallets": watched,
        "active_wallets": active,
    }


# ---------------------------------------------------------------------------
# Backup operations
# ---------------------------------------------------------------------------

def backup_export():
    conn = get_db()
    rows = conn.execute(
        "SELECT address, label, chain, monitor_enabled, large_trade_threshold_usd, copy_trade_enabled, copy_trade_max_usd, notes FROM wallet_watchlist ORDER BY created_at ASC"
    ).fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


def backup_restore(wallets: list) -> int:
    conn = get_db()
    conn.execute("DELETE FROM wallet_activity")
    conn.execute("DELETE FROM wallet_watchlist")
    ts = now_iso()
    count = 0
    for entry in wallets:
        addr = entry.get("address")
        if not addr:
            continue
        conn.execute(
            "INSERT OR IGNORE INTO wallet_watchlist (address, label, chain, monitor_enabled, large_trade_threshold_usd, copy_trade_enabled, copy_trade_max_usd, notes, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                addr, entry.get("label"), entry.get("chain", "mainnet"),
                entry.get("monitor_enabled", 1), entry.get("large_trade_threshold_usd", 1000.0),
                entry.get("copy_trade_enabled", 0), entry.get("copy_trade_max_usd"),
                entry.get("notes"), ts, ts,
            ),
        )
        count += 1
    conn.commit()
    conn.close()
    return count


# ---------------------------------------------------------------------------
# Alchemy API
# ---------------------------------------------------------------------------

def alchemy_base_url(chain: str) -> str:
    if chain == "base":
        return f"https://base-mainnet.g.alchemy.com/v2/{ALCHEMY_API_KEY}"
    return f"https://eth-mainnet.g.alchemy.com/v2/{ALCHEMY_API_KEY}"


def alchemy_get_block_number(chain: str) -> int:
    url = alchemy_base_url(chain)
    body = {"id": 1, "jsonrpc": "2.0", "method": "eth_blockNumber", "params": []}
    resp = http_requests.post(url, json=body, timeout=15)
    data = resp.json()
    if "error" in data and data["error"]:
        raise RuntimeError(f"eth_blockNumber error: {data['error'].get('message', '')}")
    hex_str = data.get("result", "0x0").replace("0x", "")
    return int(hex_str, 16)


def alchemy_get_asset_transfers(chain: str, address: str, from_block: int | None, direction: str) -> list[dict]:
    url = alchemy_base_url(chain)
    from_block_hex = f"0x{from_block:x}" if from_block is not None else "0x0"
    categories = ["external", "erc20"] if chain == "base" else ["external", "internal", "erc20"]
    params = {
        "fromBlock": from_block_hex,
        "toBlock": "latest",
        "category": categories,
        "withMetadata": True,
        "maxCount": "0x3e8",
    }
    if direction == "from":
        params["fromAddress"] = address
    else:
        params["toAddress"] = address

    all_transfers = []
    page_key = None
    while True:
        req_params = dict(params)
        if page_key:
            req_params["pageKey"] = page_key
        body = {"id": 1, "jsonrpc": "2.0", "method": "alchemy_getAssetTransfers", "params": [req_params]}
        resp = http_requests.post(url, json=body, timeout=30)
        data = resp.json()
        if "error" in data and data["error"]:
            raise RuntimeError(f"Alchemy API error: {data['error'].get('message', '')}")
        result = data.get("result", {})
        transfers = result.get("transfers", [])
        all_transfers.extend(transfers)
        page_key = result.get("pageKey")
        if not page_key or len(all_transfers) > 5000:
            break
    return all_transfers


def parse_block_number(hex_str: str) -> int:
    return int(hex_str.replace("0x", ""), 16) if hex_str else 0


# ---------------------------------------------------------------------------
# USD Price Estimation
# ---------------------------------------------------------------------------

STABLECOINS = {"USDC", "USDT", "DAI", "BUSD", "TUSD", "FRAX"}


def estimate_usd_value(asset: str | None, value: float | None, chain: str) -> float | None:
    if value is None or value == 0.0:
        return 0.0 if value == 0.0 else None
    symbol = (asset or "ETH").upper()
    if symbol in STABLECOINS:
        return value

    with _price_cache_lock:
        if symbol in _price_cache:
            price, ts = _price_cache[symbol]
            if time.time() - ts < PRICE_CACHE_TTL:
                return value * price

    dex_chain = "base" if chain == "base" else "ethereum"
    try:
        resp = http_requests.get(f"https://api.dexscreener.com/latest/dex/search?q={symbol}", timeout=10)
        data = resp.json()
        for pair in data.get("pairs", []):
            if pair.get("chainId") == dex_chain and (pair.get("baseToken", {}).get("symbol", "").upper() == symbol):
                price = float(pair.get("priceUsd", 0))
                if price > 0:
                    with _price_cache_lock:
                        _price_cache[symbol] = (price, time.time())
                    return value * price
    except Exception:
        pass

    fallback = {"ETH": 2500.0, "WETH": 2500.0}
    if symbol in fallback:
        return value * fallback[symbol]
    return None


# ---------------------------------------------------------------------------
# Background Worker
# ---------------------------------------------------------------------------

def worker_loop():
    global _last_tick_at
    logger = logging.getLogger("wallet_monitor.worker")
    logger.info(f"[WALLET_MONITOR] Worker started (poll interval: {POLL_INTERVAL}s)")
    first_run = True
    while True:
        delay = 5 if first_run else POLL_INTERVAL
        first_run = False
        time.sleep(delay)
        try:
            wallet_monitor_tick(logger)
            with _last_tick_lock:
                _last_tick_at = now_iso()
        except Exception as e:
            logger.error(f"[WALLET_MONITOR] Tick error: {e}")


def wallet_monitor_tick(logger):
    conn = get_db()
    watchlist = conn.execute(
        "SELECT * FROM wallet_watchlist WHERE monitor_enabled = 1 ORDER BY created_at ASC"
    ).fetchall()
    conn.close()
    if not watchlist:
        return

    logger.debug(f"[WALLET_MONITOR] Tick: checking {len(watchlist)} wallets")
    total_new = 0
    alerts = []

    for entry in watchlist:
        entry = row_to_dict(entry)
        try:
            new_count, entry_alerts = process_wallet(entry, logger)
            total_new += new_count
            alerts.extend(entry_alerts)
        except Exception as e:
            logger.warning(f"[WALLET_MONITOR] Error processing wallet {entry['address']} ({entry['chain']}): {e}")

    if alerts and ALERT_CALLBACK_URL:
        for alert in alerts:
            try:
                http_requests.post(ALERT_CALLBACK_URL, json=alert, timeout=10)
            except Exception as e:
                logger.warning(f"[WALLET_MONITOR] Failed to send alert callback: {e}")
        logger.warning(f"[WALLET_MONITOR] LARGE TRADE ALERTS: {' | '.join(a['message'] for a in alerts)}")

    if total_new > 0:
        logger.info(f"[WALLET_MONITOR] Tick complete: {total_new} new transactions, {len(alerts)} large trades")


def process_wallet(entry: dict, logger) -> tuple[int, list[dict]]:
    from_block = None
    if entry["last_checked_block"] is not None:
        from_block = entry["last_checked_block"] + 1
    else:
        latest = alchemy_get_block_number(entry["chain"])
        from_block = max(0, latest - FIRST_RUN_LOOKBACK_BLOCKS)
        logger.info(f"[WALLET_MONITOR] First run for {entry['address']} on {entry['chain']}: starting from block {from_block} (latest: {latest})")

    outgoing = alchemy_get_asset_transfers(entry["chain"], entry["address"], from_block, "from")
    incoming = alchemy_get_asset_transfers(entry["chain"], entry["address"], from_block, "to")

    if not outgoing and not incoming:
        try:
            latest = alchemy_get_block_number(entry["chain"])
            conn = get_db()
            ts = now_iso()
            conn.execute("UPDATE wallet_watchlist SET last_checked_block = ?, last_checked_at = ?, updated_at = ? WHERE id = ?", (latest, ts, ts, entry["id"]))
            conn.commit()
            conn.close()
        except Exception:
            pass
        return 0, []

    # Group transfers by tx_hash for swap detection
    tx_groups: dict[str, list[tuple[dict, str]]] = {}
    for t in outgoing:
        tx_groups.setdefault(t["hash"], []).append((t, "outgoing"))
    for t in incoming:
        tx_groups.setdefault(t["hash"], []).append((t, "incoming"))

    new_count = 0
    max_block = entry["last_checked_block"] or 0
    alerts = []
    conn = get_db()

    for tx_hash, transfers in tx_groups.items():
        block_number = parse_block_number(transfers[0][0].get("blockNum", "0x0"))
        if block_number > max_block:
            max_block = block_number

        block_timestamp = None
        meta = transfers[0][0].get("metadata")
        if meta:
            block_timestamp = meta.get("blockTimestamp")

        has_outgoing_erc20 = any(t["category"] == "erc20" and d == "outgoing" for t, d in transfers)
        has_incoming_erc20 = any(t["category"] == "erc20" and d == "incoming" for t, d in transfers)
        is_swap = has_outgoing_erc20 and has_incoming_erc20

        swap_from_token = swap_from_amount = swap_to_token = swap_to_amount = None
        if is_swap:
            out_erc20 = next(((t, d) for t, d in transfers if d == "outgoing" and t["category"] == "erc20"), None)
            in_erc20 = next(((t, d) for t, d in transfers if d == "incoming" and t["category"] == "erc20"), None)
            if out_erc20:
                swap_from_token = out_erc20[0].get("asset")
                swap_from_amount = str(out_erc20[0].get("value", "")) if out_erc20[0].get("value") is not None else None
            if in_erc20:
                swap_to_token = in_erc20[0].get("asset")
                swap_to_amount = str(in_erc20[0].get("value", "")) if in_erc20[0].get("value") is not None else None

        for transfer, direction in transfers:
            if is_swap:
                a_type = "swap"
            else:
                cat = transfer.get("category", "")
                a_type = {"external": "eth_transfer", "internal": "internal", "erc20": "erc20_transfer"}.get(cat, cat)

            amount_formatted = str(transfer["value"]) if transfer.get("value") is not None else None
            usd_value = estimate_usd_value(transfer.get("asset"), transfer.get("value"), entry["chain"])
            is_large_trade = usd_value is not None and usd_value >= entry["large_trade_threshold_usd"]

            raw_contract = transfer.get("rawContract") or {}
            raw_data = json.dumps(transfer) if (is_swap or is_large_trade) else None

            try:
                conn.execute(
                    """INSERT OR IGNORE INTO wallet_activity
                       (watchlist_id, chain, tx_hash, block_number, block_timestamp,
                        from_address, to_address, activity_type, asset_symbol, asset_address,
                        amount_raw, amount_formatted, usd_value, is_large_trade,
                        swap_from_token, swap_from_amount, swap_to_token, swap_to_amount, raw_data)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    (
                        entry["id"], entry["chain"], tx_hash, block_number, block_timestamp,
                        transfer.get("from", ""), transfer.get("to", "0x0") or "0x0",
                        a_type, transfer.get("asset"), raw_contract.get("address"),
                        raw_contract.get("value"), amount_formatted, usd_value, 1 if is_large_trade else 0,
                        swap_from_token, swap_from_amount, swap_to_token, swap_to_amount, raw_data,
                    ),
                )
                if conn.execute("SELECT changes()").fetchone()[0] > 0:
                    new_count += 1
                    if is_large_trade:
                        label = entry.get("label") or entry["address"]
                        usd_str = f"${usd_value:.0f}" if usd_value else "unknown"
                        addr_short = entry["address"][:10]
                        if is_swap:
                            message = f"**{label}** ({addr_short}) swapped {swap_from_amount or '?'} {swap_from_token or '?'} -> {swap_to_amount or '?'} {swap_to_token or '?'} ({usd_str}) on {entry['chain']} [tx: {tx_hash}]"
                        else:
                            asset = transfer.get("asset") or "ETH"
                            amt = amount_formatted or "?"
                            dir_str = "sent" if direction == "outgoing" else "received"
                            message = f"**{label}** ({addr_short}) {dir_str} {amt} {asset} ({usd_str}) on {entry['chain']} [tx: {tx_hash}]"
                        alerts.append({
                            "watchlist_id": entry["id"], "address": entry["address"],
                            "label": entry.get("label"), "chain": entry["chain"],
                            "tx_hash": tx_hash, "activity_type": a_type,
                            "usd_value": usd_value, "asset_symbol": transfer.get("asset"),
                            "amount_formatted": amount_formatted,
                            "swap_from_token": swap_from_token, "swap_from_amount": swap_from_amount,
                            "swap_to_token": swap_to_token, "swap_to_amount": swap_to_amount,
                            "message": message,
                        })
            except Exception:
                pass

    conn.commit()

    if max_block > (entry["last_checked_block"] or 0):
        ts = now_iso()
        conn.execute("UPDATE wallet_watchlist SET last_checked_block = ?, last_checked_at = ?, updated_at = ? WHERE id = ?", (max_block, ts, ts, entry["id"]))
        conn.commit()

    conn.close()
    return new_count, alerts


# ---------------------------------------------------------------------------
# App
# ---------------------------------------------------------------------------

def _status_extra():
    stats = activity_stats()
    with _last_tick_lock:
        last_tick = _last_tick_at
    stats["last_tick_at"] = last_tick
    stats["poll_interval_secs"] = POLL_INTERVAL
    stats["worker_enabled"] = bool(ALCHEMY_API_KEY)
    return stats


app = create_app("wallet_monitor", status_extra_fn=_status_extra)


# ---------------------------------------------------------------------------
# RPC: Watchlist tool
# ---------------------------------------------------------------------------

@app.route("/rpc/tools/watchlist", methods=["POST"])
def rpc_watchlist():
    body = request.get_json(silent=True) or {}
    action = body.get("action")
    try:
        if action == "add":
            address = body.get("address")
            if not address:
                return error("address is required")
            chain = body.get("chain", "mainnet")
            threshold = body.get("threshold_usd", 1000.0)
            entry, err = watchlist_add(address, body.get("label"), chain, threshold)
            if err:
                return error(err)
            return success(entry)

        elif action == "remove":
            entry_id = body.get("id")
            if entry_id is None:
                return error("id is required")
            if watchlist_remove(entry_id):
                return success(True)
            return error(f"Entry #{entry_id} not found", 404)

        elif action == "list":
            return success(watchlist_list())

        elif action == "update":
            entry_id = body.get("id")
            if entry_id is None:
                return error("id is required")
            if watchlist_update(entry_id, body.get("label"), body.get("threshold_usd"), body.get("monitor_enabled"), body.get("notes")):
                return success(True)
            return error(f"Entry #{entry_id} not found", 404)

        else:
            return error(f"Unknown action: {action}. Valid: add, remove, list, update")
    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# RPC: Activity tool
# ---------------------------------------------------------------------------

@app.route("/rpc/tools/activity", methods=["POST"])
def rpc_activity():
    body = request.get_json(silent=True) or {}
    action = body.get("action")
    try:
        if action == "recent":
            data = activity_query(limit=body.get("limit", 25))
            return success(data)

        elif action == "large_trades":
            data = activity_query(large_only=True, limit=body.get("limit", 25))
            return success(data)

        elif action == "search":
            data = activity_query(
                address=body.get("address"),
                activity_type=body.get("activity_type"),
                chain=body.get("chain"),
                large_only=body.get("large_only", False),
                limit=body.get("limit", 25),
            )
            return success(data)

        elif action == "stats":
            return success(activity_stats())

        else:
            return error(f"Unknown action: {action}. Valid: recent, large_trades, search, stats")
    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# RPC: Control tool
# ---------------------------------------------------------------------------

@app.route("/rpc/tools/control", methods=["POST"])
def rpc_control():
    body = request.get_json(silent=True) or {}
    action = body.get("action")
    try:
        if action == "status":
            return success(_status_extra())
        elif action == "trigger":
            # Run a tick in a thread so we don't block
            logger = logging.getLogger("wallet_monitor.worker")
            threading.Thread(target=wallet_monitor_tick, args=(logger,), daemon=True).start()
            return success("Poll triggered")
        else:
            return error(f"Unknown action: {action}. Valid: status, trigger")
    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# RPC: Backup / Restore
# ---------------------------------------------------------------------------

@app.route("/rpc/backup/export", methods=["POST"])
def rpc_backup_export():
    try:
        return success(backup_export())
    except Exception as e:
        return error(str(e))


@app.route("/rpc/backup/restore", methods=["POST"])
def rpc_backup_restore():
    body = request.get_json(silent=True) or {}
    wallets = body.get("wallets", [])
    if not isinstance(wallets, list):
        return error("wallets must be a list")
    try:
        count = backup_restore(wallets)
        return success(count)
    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# Dashboard
# ---------------------------------------------------------------------------

def _format_uptime(secs: int) -> str:
    hours = secs // 3600
    minutes = (secs % 3600) // 60
    seconds = secs % 60
    if hours > 0:
        return f"{hours}h {minutes}m {seconds}s"
    elif minutes > 0:
        return f"{minutes}m {seconds}s"
    return f"{seconds}s"


@app.route("/")
def dashboard():
    stats = activity_stats()
    wl = watchlist_list()
    recent = activity_query(limit=20)
    with _last_tick_lock:
        last_tick = _last_tick_at or "not yet"
    uptime = _format_uptime(int(time.time() - _start_time))
    worker_enabled = bool(ALCHEMY_API_KEY)

    alchemy_preview = None
    if ALCHEMY_API_KEY:
        k = ALCHEMY_API_KEY
        alchemy_preview = f"{k[:3]}...{k[-3:]}" if len(k) > 6 else k

    warning_banner = ""
    if not worker_enabled:
        warning_banner = """<div style="background:#5a2d00;border:1px solid #b35c00;border-radius:8px;padding:12px 16px;margin-bottom:20px;display:flex;align-items:center;gap:10px;">
            <span style="font-size:1.3em;">&#9888;</span>
            <div>
                <strong style="color:#ffb347;">Background worker disabled</strong>
                <span style="color:#ccc;"> &mdash; <code style="background:#3d2200;padding:2px 6px;border-radius:4px;font-size:0.9em;">ALCHEMY_API_KEY</code> is not set.</span>
            </div>
        </div>"""

    alchemy_status = f'<span style="color:#3fb950;">&#10003;</span> <code style="background:#1a1a2e;padding:2px 6px;border-radius:4px;font-size:0.9em;">{alchemy_preview}</code>' if alchemy_preview else '<span style="color:#f85149;">&#10007; Not configured</span>'

    watchlist_rows = ""
    for w in wl:
        label = w.get("label") or "-"
        status_cls = "active" if w["monitor_enabled"] else "paused"
        status_label = "Active" if w["monitor_enabled"] else "Paused"
        last_block = f"#{w['last_checked_block']}" if w.get("last_checked_block") else "-"
        toggle_icon = "&#9646;&#9646;" if w["monitor_enabled"] else "&#9654;"
        toggle_title = "Pause monitoring" if w["monitor_enabled"] else "Resume monitoring"
        watchlist_rows += f'''<tr data-id="{w["id"]}"><td>{w["id"]}</td><td>{label}</td><td class="mono">{w["address"]}</td><td>{w["chain"]}</td><td>${w["large_trade_threshold_usd"]:.0f}</td><td><span class="status-badge {status_cls}">{status_label}</span></td><td>{last_block}</td><td class="actions"><button class="btn-icon btn-toggle" onclick="toggleWallet({w["id"]}, {0 if w["monitor_enabled"] else 1})" title="{toggle_title}">{toggle_icon}</button><button class="btn-icon btn-remove" onclick="removeWallet({w["id"]}, '{w["address"][:10]}...')" title="Remove wallet">&#10005;</button></td></tr>\n'''
    if not watchlist_rows:
        watchlist_rows = '<tr><td colspan="8" style="text-align:center;color:#8b949e;padding:20px;">No wallets on watchlist. Add one below.</td></tr>'

    activity_rows = ""
    for a in recent:
        usd = f"${a['usd_value']:.0f}" if a.get("usd_value") is not None else "-"
        large_cls = ' class="large"' if a["is_large_trade"] else ""
        asset = a.get("asset_symbol") or "ETH"
        amount = a.get("amount_formatted") or "-"
        tx = a["tx_hash"]
        tx_short = f"{tx[:8]}...{tx[-4:]}" if len(tx) > 14 else tx
        activity_rows += f'<tr{large_cls}><td>{a["activity_type"]}</td><td>{a["chain"]}</td><td>{amount} {asset}</td><td>{usd}</td><td class="mono">{tx_short}</td><td>{a["created_at"]}</td></tr>\n'
    if not activity_rows:
        activity_rows = '<tr><td colspan="6">No activity recorded yet.</td></tr>'

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Wallet Monitor Dashboard</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0f1117; color: #e0e0e0; padding: 20px; }}
  h1 {{ color: #58a6ff; margin-bottom: 8px; }}
  .meta {{ color: #8b949e; font-size: 0.85em; margin-bottom: 20px; }}
  .stats {{ display: flex; gap: 16px; margin-bottom: 24px; flex-wrap: wrap; }}
  .stat {{ background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 16px 24px; text-align: center; min-width: 140px; }}
  .stat .val {{ display: block; font-size: 2em; font-weight: bold; color: #58a6ff; }}
  .stat .lbl {{ display: block; font-size: 0.85em; color: #8b949e; margin-top: 4px; }}
  table {{ width: 100%; border-collapse: collapse; margin-bottom: 24px; }}
  th {{ background: #161b22; color: #8b949e; text-align: left; padding: 8px 12px; font-size: 0.85em; text-transform: uppercase; border-bottom: 1px solid #30363d; }}
  td {{ padding: 8px 12px; border-bottom: 1px solid #21262d; font-size: 0.9em; }}
  tr:hover {{ background: #161b22; }}
  tr.large {{ background: #2d1b00; }}
  tr.large:hover {{ background: #3d2500; }}
  .mono {{ font-family: 'SF Mono', 'Consolas', monospace; font-size: 0.85em; }}
  h2 {{ color: #c9d1d9; margin-bottom: 12px; font-size: 1.1em; }}
  .section {{ margin-bottom: 28px; }}
  .actions {{ white-space: nowrap; }}
  .btn-icon {{ background: none; border: 1px solid #30363d; color: #8b949e; border-radius: 6px; padding: 4px 8px; cursor: pointer; font-size: 0.8em; margin-left: 4px; transition: all 0.15s; }}
  .btn-icon:hover {{ border-color: #58a6ff; color: #58a6ff; }}
  .btn-remove:hover {{ border-color: #f85149; color: #f85149; }}
  .status-badge {{ padding: 2px 8px; border-radius: 12px; font-size: 0.8em; font-weight: 500; }}
  .status-badge.active {{ background: #0d2818; color: #3fb950; }}
  .status-badge.paused {{ background: #2d1b00; color: #d29922; }}
  .add-form {{ background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 16px 20px; margin-bottom: 24px; }}
  .add-form .form-row {{ display: flex; gap: 12px; flex-wrap: wrap; align-items: flex-end; }}
  .add-form .field {{ display: flex; flex-direction: column; gap: 4px; }}
  .add-form label {{ font-size: 0.8em; color: #8b949e; text-transform: uppercase; }}
  .add-form input, .add-form select {{ background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 8px 12px; color: #e0e0e0; font-size: 0.9em; outline: none; }}
  .add-form input:focus, .add-form select:focus {{ border-color: #58a6ff; }}
  .add-form input.addr {{ width: 380px; font-family: 'SF Mono', 'Consolas', monospace; font-size: 0.85em; }}
  .add-form input.lbl {{ width: 140px; }}
  .add-form input.thr {{ width: 100px; }}
  .btn-add {{ background: #238636; border: 1px solid #2ea043; color: #fff; border-radius: 6px; padding: 8px 20px; cursor: pointer; font-size: 0.9em; font-weight: 500; transition: background 0.15s; }}
  .btn-add:hover {{ background: #2ea043; }}
  .btn-add:disabled {{ opacity: 0.5; cursor: not-allowed; }}
  .toast {{ position: fixed; bottom: 24px; right: 24px; padding: 12px 20px; border-radius: 8px; font-size: 0.9em; color: #fff; z-index: 999; opacity: 0; transition: opacity 0.3s; pointer-events: none; }}
  .toast.show {{ opacity: 1; }}
  .toast.ok {{ background: #238636; }}
  .toast.err {{ background: #da3633; }}
</style>
</head>
<body>
  <h1>Wallet Monitor</h1>
  <p class="meta">Uptime: {uptime} &middot; Last tick: {last_tick} &middot; Poll interval: {POLL_INTERVAL}s</p>

  {warning_banner}

  <div style="background:#161b22;border:1px solid #30363d;border-radius:8px;padding:12px 16px;margin-bottom:20px;">
    <h2 style="margin-bottom:8px;">API Keys</h2>
    <div style="display:flex;align-items:center;gap:8px;font-size:0.9em;">
      <span style="color:#8b949e;">ALCHEMY_API_KEY:</span> {alchemy_status}
    </div>
  </div>

  <div class="stats">
    <div class="stat"><span class="val">{stats['watched_wallets']}</span><span class="lbl">Watched Wallets</span></div>
    <div class="stat"><span class="val">{stats['active_wallets']}</span><span class="lbl">Active</span></div>
    <div class="stat"><span class="val">{stats['total_transactions']}</span><span class="lbl">Total Txs</span></div>
    <div class="stat"><span class="val">{stats['large_trades']}</span><span class="lbl">Large Trades</span></div>
  </div>

  <div class="section">
    <h2>Watchlist</h2>
    <table id="watchlist-table">
      <thead><tr><th>ID</th><th>Label</th><th>Address</th><th>Chain</th><th>Threshold</th><th>Status</th><th>Last Block</th><th></th></tr></thead>
      <tbody>{watchlist_rows}</tbody>
    </table>

    <div class="add-form">
      <h2 style="margin-bottom:12px;">Add Wallet</h2>
      <div class="form-row">
        <div class="field">
          <label for="addr">Address</label>
          <input type="text" id="addr" class="addr" placeholder="0x..." spellcheck="false">
        </div>
        <div class="field">
          <label for="lbl">Label</label>
          <input type="text" id="lbl" class="lbl" placeholder="optional">
        </div>
        <div class="field">
          <label for="chain">Chain</label>
          <select id="chain"><option value="mainnet">Mainnet</option><option value="base">Base</option></select>
        </div>
        <div class="field">
          <label for="thr">Threshold $</label>
          <input type="number" id="thr" class="thr" value="1000" min="0">
        </div>
        <div class="field">
          <label>&nbsp;</label>
          <button class="btn-add" id="btn-add" onclick="addWallet()">Add</button>
        </div>
      </div>
    </div>
  </div>

  <div class="section">
    <h2>Recent Activity</h2>
    <table>
      <thead><tr><th>Type</th><th>Chain</th><th>Amount</th><th>USD</th><th>Tx</th><th>Time</th></tr></thead>
      <tbody>{activity_rows}</tbody>
    </table>
  </div>

  <div id="toast" class="toast"></div>
  <script>
  function toast(msg, ok) {{
    const t = document.getElementById('toast');
    t.textContent = msg;
    t.className = 'toast show ' + (ok ? 'ok' : 'err');
    setTimeout(() => t.className = 'toast', 3000);
  }}

  async function rpc(body) {{
    const r = await fetch('/rpc/tools/watchlist', {{
      method: 'POST',
      headers: {{'Content-Type': 'application/json'}},
      body: JSON.stringify(body)
    }});
    return r.json();
  }}

  async function addWallet() {{
    const addr = document.getElementById('addr').value.trim();
    const label = document.getElementById('lbl').value.trim() || null;
    const chain = document.getElementById('chain').value;
    const thr = parseFloat(document.getElementById('thr').value) || 1000;
    if (!addr) {{ toast('Address is required', false); return; }}
    const btn = document.getElementById('btn-add');
    btn.disabled = true;
    try {{
      const res = await rpc({{action: 'add', address: addr, label: label, chain: chain, threshold_usd: thr}});
      if (res.ok) {{
        toast('Wallet added', true);
        document.getElementById('addr').value = '';
        document.getElementById('lbl').value = '';
        setTimeout(() => location.reload(), 500);
      }} else {{
        toast(res.error || 'Failed to add wallet', false);
      }}
    }} catch(e) {{ toast('Network error', false); }}
    btn.disabled = false;
  }}

  async function removeWallet(id, preview) {{
    if (!confirm('Remove wallet ' + preview + '?')) return;
    try {{
      const res = await rpc({{action: 'remove', id: id}});
      if (res.ok) {{
        toast('Wallet removed', true);
        const row = document.querySelector('tr[data-id="' + id + '"]');
        if (row) row.remove();
      }} else {{
        toast(res.error || 'Failed to remove', false);
      }}
    }} catch(e) {{ toast('Network error', false); }}
  }}

  async function toggleWallet(id, enable) {{
    try {{
      const res = await rpc({{action: 'update', id: id, monitor_enabled: !!enable}});
      if (res.ok) {{
        toast(enable ? 'Monitoring resumed' : 'Monitoring paused', true);
        setTimeout(() => location.reload(), 500);
      }} else {{
        toast(res.error || 'Failed to update', false);
      }}
    }} catch(e) {{ toast('Network error', false); }}
  }}

  // auto-refresh every 30s only if user hasn't interacted recently
  let _lastInteract = 0;
  document.addEventListener('keydown', () => _lastInteract = Date.now());
  document.addEventListener('click', () => _lastInteract = Date.now());
  setInterval(() => {{ if (Date.now() - _lastInteract > 5000) location.reload(); }}, 30000);

  // submit on Enter in address field
  document.getElementById('addr').addEventListener('keydown', e => {{ if (e.key === 'Enter') addWallet(); }});
  </script>
</body>
</html>"""
    return html


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    logging.getLogger("werkzeug").setLevel(logging.ERROR)
    init_db()

    if ALCHEMY_API_KEY:
        worker_thread = threading.Thread(target=worker_loop, daemon=True)
        worker_thread.start()
    else:
        logging.warning("[WALLET_MONITOR] ALCHEMY_API_KEY not set — background worker disabled")

    port = int(os.environ.get("MODULE_PORT", os.environ.get("WALLET_MONITOR_PORT", "9100")))
    app.run(host="127.0.0.1", port=port)
