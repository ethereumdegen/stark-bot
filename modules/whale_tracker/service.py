# /// script
# requires-python = ">=3.12"
# dependencies = ["flask", "requests", "starkbot-sdk"]
#
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
Whale Tracker module — monitors known whale wallets on Ethereum + Base,
classifies movements (exchange deposit/withdrawal, wallet transfer, contract
interaction), tracks historical accuracy per whale, and fires scored alerts.

RPC protocol endpoints:
  GET  /rpc/status             -> service health
  POST /rpc/tools/whales       -> manage whale registry (action-based)
  POST /rpc/tools/signals      -> query signals and accuracy (action-based)
  POST /rpc/tools/control      -> worker control (action-based)
  POST /rpc/backup/export      -> export data for backup
  POST /rpc/backup/restore     -> restore data from backup
  GET  /                       -> HTML dashboard

Launch with:  uv run service.py
"""

from flask import request
from starkbot_sdk import create_app, success, error
import sqlite3
import os
import json
import time
import logging
import threading
import requests as http_requests
from datetime import datetime, timezone

DB_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "whale_tracker.db")
POLL_INTERVAL = int(os.environ.get("WHALE_TRACKER_POLL_INTERVAL", "60"))
OUTCOME_CHECK_INTERVAL = int(os.environ.get("WHALE_TRACKER_OUTCOME_INTERVAL", "900"))  # 15 min
ALCHEMY_API_KEY = os.environ.get("ALCHEMY_API_KEY", "")
ALERT_CALLBACK_URL = os.environ.get("ALERT_CALLBACK_URL")
ALERT_CONFIDENCE_THRESHOLD = int(os.environ.get("WHALE_TRACKER_ALERT_THRESHOLD", "60"))
FIRST_RUN_LOOKBACK_BLOCKS = 500
PRICE_CACHE_TTL = 60

# Module-level state
_start_time = time.time()
_last_tick_at = None
_last_outcome_check_at = None
_tick_lock = threading.Lock()
_price_cache: dict[str, tuple[float, float]] = {}  # symbol -> (price, timestamp)
_price_cache_lock = threading.Lock()
_worker_running = True
_movements_today = 0
_signals_today = 0
_stats_lock = threading.Lock()

# ---------------------------------------------------------------------------
# Known exchange addresses (pre-seeded)
# ---------------------------------------------------------------------------

KNOWN_EXCHANGES: list[dict] = [
    # Binance — Ethereum
    {"address": "0x28c6c06298d514db089934071355e5743bf21d60", "chain": "ethereum", "exchange": "binance", "label": "Binance 14"},
    {"address": "0x21a31ee1afc51d94c2efccaa2092ad1028285549", "chain": "ethereum", "exchange": "binance", "label": "Binance 7"},
    {"address": "0xdfd5293d8e347dfe59e90efd55b2956a1343963d", "chain": "ethereum", "exchange": "binance", "label": "Binance 8"},
    {"address": "0x56eddb7aa87536c09ccc2793473599fd21a8b17f", "chain": "ethereum", "exchange": "binance", "label": "Binance 16"},
    {"address": "0x9696f59e4d72e237be84ffd425dcad154bf96976", "chain": "ethereum", "exchange": "binance", "label": "Binance 18"},
    {"address": "0xf977814e90da44bfa03b6295a0616a897441acec", "chain": "ethereum", "exchange": "binance", "label": "Binance 8 (cold)"},
    {"address": "0x5a52e96bacdabb82fd05763e25335261b270efcb", "chain": "ethereum", "exchange": "binance", "label": "Binance 12"},
    {"address": "0xbe0eb53f46cd790cd13851d5eff43d12404d33e8", "chain": "ethereum", "exchange": "binance", "label": "Binance 7 (cold)"},
    # Coinbase — Ethereum
    {"address": "0x71660c4005ba85c37ccec55d0c4493e66fe775d3", "chain": "ethereum", "exchange": "coinbase", "label": "Coinbase 1"},
    {"address": "0x503828976d22510aad0201ac7ec88293211d23da", "chain": "ethereum", "exchange": "coinbase", "label": "Coinbase 2"},
    {"address": "0xddfabcdc4d8ffc6d5beaf154f18b778f892a0740", "chain": "ethereum", "exchange": "coinbase", "label": "Coinbase 3"},
    {"address": "0x3cd751e6b0078be393132286c442345e68ff0aaa", "chain": "ethereum", "exchange": "coinbase", "label": "Coinbase 4"},
    {"address": "0xb5d85cbf7cb3ee0d56b3bb207d5fc4b82f43f511", "chain": "ethereum", "exchange": "coinbase", "label": "Coinbase 5"},
    {"address": "0xeb2629a2734e272bcc07bda959863f316f4bd4cf", "chain": "ethereum", "exchange": "coinbase", "label": "Coinbase 6"},
    {"address": "0xa9d1e08c7793af67e9d92fe308d5697fb81d3e43", "chain": "ethereum", "exchange": "coinbase", "label": "Coinbase 10"},
    {"address": "0x77134cbc06cb00b66f4c7e623d5fdbf6777635ec", "chain": "ethereum", "exchange": "coinbase", "label": "Coinbase 11"},
    # Kraken — Ethereum
    {"address": "0x2910543af39aba0cd09dbb2d50200b3e800a63d2", "chain": "ethereum", "exchange": "kraken", "label": "Kraken 1"},
    {"address": "0x0a869d79a7052c7f1b55a8ebabbea3420f0d1e13", "chain": "ethereum", "exchange": "kraken", "label": "Kraken 2"},
    {"address": "0xe853c56864a2ebe4576a807d26fdc4a0ada51919", "chain": "ethereum", "exchange": "kraken", "label": "Kraken 3"},
    {"address": "0x267be1c1d684f78cb4f6a176c4911b741e4ffdc0", "chain": "ethereum", "exchange": "kraken", "label": "Kraken 4"},
    {"address": "0xfa52274dd61e1643d2205169732f29114bc240b3", "chain": "ethereum", "exchange": "kraken", "label": "Kraken 5"},
    {"address": "0x53d284357ec70ce289d6d64134dfac8e511c8a3d", "chain": "ethereum", "exchange": "kraken", "label": "Kraken 6"},
    # OKX — Ethereum
    {"address": "0x6cc5f688a315f3dc28a7781717a9a798a59fda7b", "chain": "ethereum", "exchange": "okx", "label": "OKX 1"},
    {"address": "0x236f9f97e0e62388479bf9e5ba4889e46b0273c3", "chain": "ethereum", "exchange": "okx", "label": "OKX 2"},
    {"address": "0xa7efae728d2936e78bda97dc267687568dd593f3", "chain": "ethereum", "exchange": "okx", "label": "OKX 3"},
    # Gemini — Ethereum
    {"address": "0xd24400ae8bfebb18ca49be86258a3c749cf46853", "chain": "ethereum", "exchange": "gemini", "label": "Gemini 1"},
    {"address": "0x6fc82a5fe25a5cdb58bc74600a40a69c065263f8", "chain": "ethereum", "exchange": "gemini", "label": "Gemini 2"},
    {"address": "0x61edcdf5bb737adffe5043706e7c5bb1f1a56eea", "chain": "ethereum", "exchange": "gemini", "label": "Gemini 3"},
    # Bybit — Ethereum
    {"address": "0xf89d7b9c864f589bbf53a82105107622b35eaa40", "chain": "ethereum", "exchange": "bybit", "label": "Bybit 1"},
    # Binance — Base
    {"address": "0x28c6c06298d514db089934071355e5743bf21d60", "chain": "base", "exchange": "binance", "label": "Binance 14"},
    {"address": "0xf977814e90da44bfa03b6295a0616a897441acec", "chain": "base", "exchange": "binance", "label": "Binance 8 (cold)"},
    # Coinbase — Base
    {"address": "0x71660c4005ba85c37ccec55d0c4493e66fe775d3", "chain": "base", "exchange": "coinbase", "label": "Coinbase 1"},
    {"address": "0x503828976d22510aad0201ac7ec88293211d23da", "chain": "base", "exchange": "coinbase", "label": "Coinbase 2"},
    {"address": "0xa9d1e08c7793af67e9d92fe308d5697fb81d3e43", "chain": "base", "exchange": "coinbase", "label": "Coinbase 10"},
    # Kraken — Base
    {"address": "0x2910543af39aba0cd09dbb2d50200b3e800a63d2", "chain": "base", "exchange": "kraken", "label": "Kraken 1"},
    # OKX — Base
    {"address": "0x6cc5f688a315f3dc28a7781717a9a798a59fda7b", "chain": "base", "exchange": "okx", "label": "OKX 1"},
]


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
        CREATE TABLE IF NOT EXISTS whales (
            address TEXT PRIMARY KEY,
            chain TEXT NOT NULL DEFAULT 'ethereum',
            label TEXT,
            category TEXT NOT NULL DEFAULT 'individual',
            tags TEXT DEFAULT '[]',
            notes TEXT,
            tracked_since TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            last_checked_block INTEGER
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS exchange_addresses (
            address TEXT NOT NULL,
            chain TEXT NOT NULL,
            exchange TEXT NOT NULL,
            label TEXT,
            PRIMARY KEY (address, chain)
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS movements (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            whale_address TEXT NOT NULL,
            chain TEXT NOT NULL,
            tx_hash TEXT NOT NULL,
            block_number INTEGER NOT NULL,
            timestamp TEXT,
            direction TEXT NOT NULL,
            counterparty TEXT,
            counterparty_label TEXT,
            token_symbol TEXT,
            token_address TEXT,
            amount REAL,
            usd_value REAL,
            classification TEXT,
            size_tier TEXT,
            UNIQUE(tx_hash, whale_address, direction)
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS signals (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            movement_id INTEGER,
            whale_address TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            signal_type TEXT NOT NULL,
            confidence REAL NOT NULL,
            token TEXT,
            summary TEXT,
            price_at_signal REAL,
            price_after_24h REAL,
            price_after_72h REAL,
            outcome TEXT NOT NULL DEFAULT 'pending',
            notified INTEGER NOT NULL DEFAULT 0
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS whale_accuracy (
            whale_address TEXT PRIMARY KEY,
            total_signals INTEGER NOT NULL DEFAULT 0,
            correct_signals INTEGER NOT NULL DEFAULT 0,
            accuracy_pct REAL NOT NULL DEFAULT 0.0,
            avg_price_impact_24h REAL NOT NULL DEFAULT 0.0,
            last_updated TEXT
        )
    """)
    conn.execute("CREATE INDEX IF NOT EXISTS idx_movements_whale ON movements(whale_address, block_number DESC)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_movements_chain ON movements(chain, block_number DESC)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_signals_whale ON signals(whale_address, timestamp DESC)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_signals_outcome ON signals(outcome, timestamp DESC)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_signals_confidence ON signals(confidence DESC)")
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
# Whale registry operations
# ---------------------------------------------------------------------------

def whale_add(address: str, chain: str, label: str | None, category: str, tags: list | None, notes: str | None):
    if not is_valid_eth_address(address):
        return None, "Invalid Ethereum address"
    conn = get_db()
    addr = address.lower()
    ts = now_iso()
    try:
        conn.execute(
            "INSERT INTO whales (address, chain, label, category, tags, notes, tracked_since) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (addr, chain, label, category, json.dumps(tags or []), notes, ts),
        )
        conn.commit()
        row = conn.execute("SELECT * FROM whales WHERE address = ?", (addr,)).fetchone()
        conn.close()
        return row_to_dict(row), None
    except sqlite3.IntegrityError:
        conn.close()
        return None, f"Whale {address} is already tracked"


def whale_remove(address: str):
    conn = get_db()
    addr = address.lower()
    cursor = conn.execute("DELETE FROM whales WHERE address = ?", (addr,))
    conn.commit()
    conn.close()
    return cursor.rowcount > 0


def whale_list():
    conn = get_db()
    rows = conn.execute("""
        SELECT w.*, a.total_signals, a.correct_signals, a.accuracy_pct, a.avg_price_impact_24h
        FROM whales w
        LEFT JOIN whale_accuracy a ON w.address = a.whale_address
        ORDER BY w.tracked_since ASC
    """).fetchall()
    conn.close()
    result = []
    for r in rows:
        d = row_to_dict(r)
        try:
            d["tags"] = json.loads(d.get("tags") or "[]")
        except (json.JSONDecodeError, TypeError):
            d["tags"] = []
        result.append(d)
    return result


def whale_update(address: str, label=None, category=None, tags=None, enabled=None, notes=None):
    conn = get_db()
    addr = address.lower()
    updates = []
    params: list = []
    if label is not None:
        updates.append("label = ?")
        params.append(label)
    if category is not None:
        updates.append("category = ?")
        params.append(category)
    if tags is not None:
        updates.append("tags = ?")
        params.append(json.dumps(tags))
    if enabled is not None:
        updates.append("enabled = ?")
        params.append(1 if enabled else 0)
    if notes is not None:
        updates.append("notes = ?")
        params.append(notes)
    if not updates:
        conn.close()
        return False
    params.append(addr)
    sql = f"UPDATE whales SET {', '.join(updates)} WHERE address = ?"
    cursor = conn.execute(sql, params)
    conn.commit()
    conn.close()
    return cursor.rowcount > 0


def import_exchange_addresses():
    conn = get_db()
    count = 0
    for ex in KNOWN_EXCHANGES:
        try:
            conn.execute(
                "INSERT OR IGNORE INTO exchange_addresses (address, chain, exchange, label) VALUES (?, ?, ?, ?)",
                (ex["address"].lower(), ex["chain"], ex["exchange"], ex.get("label")),
            )
            count += 1
        except sqlite3.IntegrityError:
            pass
    conn.commit()
    total = conn.execute("SELECT COUNT(*) FROM exchange_addresses").fetchone()[0]
    conn.close()
    return {"imported": count, "total": total}


def lookup_exchange(address: str, chain: str) -> dict | None:
    conn = get_db()
    row = conn.execute(
        "SELECT * FROM exchange_addresses WHERE address = ? AND chain = ?",
        (address.lower(), chain),
    ).fetchone()
    conn.close()
    return row_to_dict(row) if row else None


# ---------------------------------------------------------------------------
# Signal & movement queries
# ---------------------------------------------------------------------------

def signals_recent(whale_address=None, token=None, signal_type=None, min_confidence=None, limit=50):
    conn = get_db()
    conditions = ["1=1"]
    params: list = []
    if whale_address:
        conditions.append("s.whale_address = ?")
        params.append(whale_address.lower())
    if token:
        conditions.append("s.token = ?")
        params.append(token.upper())
    if signal_type:
        conditions.append("s.signal_type = ?")
        params.append(signal_type)
    if min_confidence is not None:
        conditions.append("s.confidence >= ?")
        params.append(float(min_confidence))
    limit = min(limit or 50, 200)
    sql = f"""
        SELECT s.*, w.label as whale_label, w.category as whale_category
        FROM signals s
        LEFT JOIN whales w ON s.whale_address = w.address
        WHERE {' AND '.join(conditions)}
        ORDER BY s.timestamp DESC
        LIMIT {limit}
    """
    rows = conn.execute(sql, params).fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


def signals_accuracy():
    conn = get_db()
    rows = conn.execute("""
        SELECT a.*, w.label, w.category, w.chain
        FROM whale_accuracy a
        JOIN whales w ON a.whale_address = w.address
        ORDER BY a.accuracy_pct DESC, a.total_signals DESC
    """).fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


def signals_history(whale_address: str, limit=100):
    conn = get_db()
    rows = conn.execute("""
        SELECT s.*, m.tx_hash, m.classification, m.size_tier, m.usd_value as movement_usd, m.counterparty_label
        FROM signals s
        LEFT JOIN movements m ON s.movement_id = m.id
        WHERE s.whale_address = ?
        ORDER BY s.timestamp DESC
        LIMIT ?
    """, (whale_address.lower(), min(limit, 500))).fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


def get_stats():
    conn = get_db()
    total_whales = conn.execute("SELECT COUNT(*) FROM whales").fetchone()[0]
    active_whales = conn.execute("SELECT COUNT(*) FROM whales WHERE enabled = 1").fetchone()[0]
    total_movements = conn.execute("SELECT COUNT(*) FROM movements").fetchone()[0]
    total_signals = conn.execute("SELECT COUNT(*) FROM signals").fetchone()[0]
    pending_signals = conn.execute("SELECT COUNT(*) FROM signals WHERE outcome = 'pending'").fetchone()[0]
    correct_signals = conn.execute("SELECT COUNT(*) FROM signals WHERE outcome = 'correct'").fetchone()[0]
    avg_accuracy = conn.execute("SELECT AVG(accuracy_pct) FROM whale_accuracy WHERE total_signals >= 3").fetchone()[0]
    conn.close()
    return {
        "total_whales": total_whales,
        "active_whales": active_whales,
        "total_movements": total_movements,
        "total_signals": total_signals,
        "pending_signals": pending_signals,
        "correct_signals": correct_signals,
        "avg_accuracy": round(avg_accuracy, 1) if avg_accuracy else None,
    }


# ---------------------------------------------------------------------------
# Backup operations
# ---------------------------------------------------------------------------

def backup_export():
    conn = get_db()
    whales = [row_to_dict(r) for r in conn.execute("SELECT * FROM whales ORDER BY tracked_since ASC").fetchall()]
    exchanges = [row_to_dict(r) for r in conn.execute("SELECT * FROM exchange_addresses ORDER BY exchange, address").fetchall()]
    conn.close()
    for w in whales:
        try:
            w["tags"] = json.loads(w.get("tags") or "[]")
        except (json.JSONDecodeError, TypeError):
            w["tags"] = []
    return {"whales": whales, "exchange_addresses": exchanges}


def backup_restore(data: dict) -> dict:
    conn = get_db()
    whale_count = 0
    exchange_count = 0
    for w in data.get("whales", []):
        addr = w.get("address")
        if not addr:
            continue
        tags = json.dumps(w.get("tags", []))
        conn.execute(
            "INSERT OR REPLACE INTO whales (address, chain, label, category, tags, notes, tracked_since, enabled) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            (addr.lower(), w.get("chain", "ethereum"), w.get("label"), w.get("category", "individual"),
             tags, w.get("notes"), w.get("tracked_since", now_iso()), w.get("enabled", 1)),
        )
        whale_count += 1
    for ex in data.get("exchange_addresses", []):
        addr = ex.get("address")
        if not addr:
            continue
        conn.execute(
            "INSERT OR IGNORE INTO exchange_addresses (address, chain, exchange, label) VALUES (?, ?, ?, ?)",
            (addr.lower(), ex.get("chain", "ethereum"), ex.get("exchange", "unknown"), ex.get("label")),
        )
        exchange_count += 1
    conn.commit()
    conn.close()
    return {"whales_restored": whale_count, "exchanges_restored": exchange_count}


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
# USD Price Estimation (CoinGecko + fallbacks)
# ---------------------------------------------------------------------------

STABLECOINS = {"USDC", "USDT", "DAI", "BUSD", "TUSD", "FRAX"}
COINGECKO_IDS = {
    "ETH": "ethereum",
    "WETH": "ethereum",
    "BTC": "bitcoin",
    "WBTC": "bitcoin",
    "stETH": "staked-ether",
    "cbETH": "coinbase-wrapped-staked-eth",
}


def fetch_price(symbol: str) -> float | None:
    symbol_upper = symbol.upper()
    if symbol_upper in STABLECOINS:
        return 1.0

    with _price_cache_lock:
        if symbol_upper in _price_cache:
            price, ts = _price_cache[symbol_upper]
            if time.time() - ts < PRICE_CACHE_TTL:
                return price

    cg_id = COINGECKO_IDS.get(symbol_upper)
    if cg_id:
        try:
            resp = http_requests.get(
                f"https://api.coingecko.com/api/v3/simple/price?ids={cg_id}&vs_currencies=usd",
                timeout=10,
            )
            data = resp.json()
            price = data.get(cg_id, {}).get("usd")
            if price and price > 0:
                with _price_cache_lock:
                    _price_cache[symbol_upper] = (price, time.time())
                return price
        except Exception:
            pass

    # DexScreener fallback
    try:
        resp = http_requests.get(f"https://api.dexscreener.com/latest/dex/search?q={symbol_upper}", timeout=10)
        data = resp.json()
        for pair in data.get("pairs", []):
            if pair.get("baseToken", {}).get("symbol", "").upper() == symbol_upper:
                price = float(pair.get("priceUsd", 0))
                if price > 0:
                    with _price_cache_lock:
                        _price_cache[symbol_upper] = (price, time.time())
                    return price
    except Exception:
        pass

    # Hardcoded fallbacks
    fallback = {"ETH": 2500.0, "WETH": 2500.0, "BTC": 60000.0, "WBTC": 60000.0}
    return fallback.get(symbol_upper)


def estimate_usd_value(symbol: str | None, amount: float | None) -> float | None:
    if amount is None or amount == 0.0:
        return 0.0 if amount == 0.0 else None
    sym = (symbol or "ETH").upper()
    if sym in STABLECOINS:
        return amount
    price = fetch_price(sym)
    return amount * price if price else None


# ---------------------------------------------------------------------------
# Classification & scoring
# ---------------------------------------------------------------------------

def classify_size(usd_value: float | None) -> str:
    if usd_value is None:
        return "unknown"
    if usd_value >= 100_000_000:
        return "whale"
    if usd_value >= 10_000_000:
        return "massive"
    if usd_value >= 1_000_000:
        return "large"
    if usd_value >= 100_000:
        return "medium"
    return "small"


def classify_movement(counterparty: str, chain: str) -> tuple[str, str | None]:
    """Returns (classification, counterparty_label)."""
    ex = lookup_exchange(counterparty, chain)
    if ex:
        return "exchange", f"{ex['exchange']} ({ex.get('label', '')})"
    # Check if it looks like a contract (heuristic: we don't have code-check, so just return wallet_transfer)
    return "wallet_transfer", None


def score_signal(classification: str, direction: str, size_tier: str, whale_accuracy_pct: float | None) -> tuple[str, float]:
    """Returns (signal_type, confidence)."""
    base_score = 0

    if classification == "exchange":
        if direction == "outflow":
            # Whale sending to exchange = likely selling = bearish
            signal_type = "bearish"
            base_score += 40
        else:
            # Whale receiving from exchange = likely buying = bullish
            signal_type = "bullish"
            base_score += 30
    elif classification == "wallet_transfer":
        signal_type = "neutral"
        base_score += 10
    else:
        signal_type = "neutral"
        base_score += 5

    # Size multiplier
    size_scores = {"whale": 30, "massive": 20, "large": 10, "medium": 5}
    base_score += size_scores.get(size_tier, 0)

    # Historical accuracy bonus
    if whale_accuracy_pct is not None:
        if whale_accuracy_pct >= 70:
            base_score += 20
        elif whale_accuracy_pct >= 50:
            base_score += 10

    confidence = min(100, max(0, base_score))
    return signal_type, confidence


# ---------------------------------------------------------------------------
# Background Worker — main poll loop
# ---------------------------------------------------------------------------

def worker_loop():
    global _last_tick_at
    logger = logging.getLogger("whale_tracker.worker")
    logger.info(f"[WHALE_TRACKER] Worker started (poll interval: {POLL_INTERVAL}s)")
    first_run = True
    while _worker_running:
        delay = 5 if first_run else POLL_INTERVAL
        first_run = False
        time.sleep(delay)
        try:
            whale_tracker_tick(logger)
            with _tick_lock:
                _last_tick_at = now_iso()
        except Exception as e:
            logger.error(f"[WHALE_TRACKER] Tick error: {e}")


def outcome_loop():
    global _last_outcome_check_at
    logger = logging.getLogger("whale_tracker.outcomes")
    logger.info(f"[WHALE_TRACKER] Outcome checker started (interval: {OUTCOME_CHECK_INTERVAL}s)")
    while _worker_running:
        time.sleep(OUTCOME_CHECK_INTERVAL)
        try:
            check_outcomes(logger)
            rebuild_accuracy()
            _last_outcome_check_at = now_iso()
        except Exception as e:
            logger.error(f"[WHALE_TRACKER] Outcome check error: {e}")


def whale_tracker_tick(logger):
    global _movements_today, _signals_today
    conn = get_db()
    whales = conn.execute("SELECT * FROM whales WHERE enabled = 1 ORDER BY tracked_since ASC").fetchall()
    conn.close()
    if not whales:
        return

    logger.debug(f"[WHALE_TRACKER] Tick: checking {len(whales)} whales")
    total_new_movements = 0
    new_signals = []

    for whale in whales:
        whale = row_to_dict(whale)
        try:
            movements, signals = process_whale(whale, logger)
            total_new_movements += movements
            new_signals.extend(signals)
        except Exception as e:
            logger.warning(f"[WHALE_TRACKER] Error processing whale {whale['label'] or whale['address']}: {e}")

    # Fire alerts for high-confidence signals
    if new_signals and ALERT_CALLBACK_URL:
        alerts = [s for s in new_signals if s["confidence"] >= ALERT_CONFIDENCE_THRESHOLD]
        if alerts:
            try:
                http_requests.post(ALERT_CALLBACK_URL, json={
                    "module": "whale_tracker",
                    "signals": alerts,
                    "timestamp": now_iso(),
                }, timeout=10)
                # Mark as notified
                conn = get_db()
                for s in alerts:
                    conn.execute("UPDATE signals SET notified = 1 WHERE id = ?", (s["id"],))
                conn.commit()
                conn.close()
            except Exception as e:
                logger.warning(f"[WHALE_TRACKER] Failed to send alert callback: {e}")
            logger.info(f"[WHALE_TRACKER] Fired {len(alerts)} alerts")

    with _stats_lock:
        _movements_today += total_new_movements
        _signals_today += len(new_signals)

    if total_new_movements > 0:
        logger.info(f"[WHALE_TRACKER] Tick complete: {total_new_movements} new movements, {len(new_signals)} signals")


def process_whale(whale: dict, logger) -> tuple[int, list[dict]]:
    chain = whale["chain"]
    address = whale["address"]
    from_block = None

    if whale["last_checked_block"] is not None:
        from_block = whale["last_checked_block"] + 1
    else:
        latest = alchemy_get_block_number(chain)
        from_block = max(0, latest - FIRST_RUN_LOOKBACK_BLOCKS)
        logger.info(f"[WHALE_TRACKER] First run for {whale['label'] or address} on {chain}: block {from_block}")

    outgoing = alchemy_get_asset_transfers(chain, address, from_block, "from")
    incoming = alchemy_get_asset_transfers(chain, address, from_block, "to")

    if not outgoing and not incoming:
        # Update last_checked_block even with no activity
        try:
            latest = alchemy_get_block_number(chain)
            conn = get_db()
            conn.execute("UPDATE whales SET last_checked_block = ? WHERE address = ?", (latest, address))
            conn.commit()
            conn.close()
        except Exception:
            pass
        return 0, []

    new_count = 0
    max_block = whale["last_checked_block"] or 0
    new_signals = []
    conn = get_db()

    # Get whale accuracy for scoring
    acc_row = conn.execute("SELECT accuracy_pct FROM whale_accuracy WHERE whale_address = ?", (address,)).fetchone()
    whale_acc = acc_row["accuracy_pct"] if acc_row else None

    # Process outflows (whale sending)
    for transfer in outgoing:
        block_num = parse_block_number(transfer.get("blockNum", "0x0"))
        if block_num > max_block:
            max_block = block_num
        result = process_transfer(conn, whale, transfer, "outflow", whale_acc, logger)
        if result:
            new_count += 1
            if result.get("signal"):
                new_signals.append(result["signal"])

    # Process inflows (whale receiving)
    for transfer in incoming:
        block_num = parse_block_number(transfer.get("blockNum", "0x0"))
        if block_num > max_block:
            max_block = block_num
        result = process_transfer(conn, whale, transfer, "inflow", whale_acc, logger)
        if result:
            new_count += 1
            if result.get("signal"):
                new_signals.append(result["signal"])

    conn.commit()

    # Update last_checked_block
    if max_block > (whale["last_checked_block"] or 0):
        conn.execute("UPDATE whales SET last_checked_block = ? WHERE address = ?", (max_block, address))
        conn.commit()

    conn.close()
    return new_count, new_signals


def process_transfer(conn, whale: dict, transfer: dict, direction: str, whale_acc: float | None, logger) -> dict | None:
    tx_hash = transfer.get("hash", "")
    address = whale["address"]
    chain = whale["chain"]

    counterparty = transfer.get("to", "") if direction == "outflow" else transfer.get("from", "")
    counterparty = (counterparty or "").lower()

    token_symbol = transfer.get("asset") or "ETH"
    raw_contract = transfer.get("rawContract") or {}
    token_address = raw_contract.get("address")
    amount = transfer.get("value")
    if amount is not None:
        try:
            amount = float(amount)
        except (ValueError, TypeError):
            amount = None

    usd_value = estimate_usd_value(token_symbol, amount)
    size_tier = classify_size(usd_value)
    classification, counterparty_label = classify_movement(counterparty, chain)

    # Determine full classification string
    if classification == "exchange":
        full_classification = "exchange_deposit" if direction == "outflow" else "exchange_withdrawal"
    else:
        full_classification = classification

    timestamp = None
    meta = transfer.get("metadata")
    if meta:
        timestamp = meta.get("blockTimestamp")

    block_number = parse_block_number(transfer.get("blockNum", "0x0"))

    try:
        conn.execute(
            """INSERT OR IGNORE INTO movements
               (whale_address, chain, tx_hash, block_number, timestamp, direction,
                counterparty, counterparty_label, token_symbol, token_address,
                amount, usd_value, classification, size_tier)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (address, chain, tx_hash, block_number, timestamp, direction,
             counterparty, counterparty_label, token_symbol, token_address,
             amount, usd_value, full_classification, size_tier),
        )
        if conn.execute("SELECT changes()").fetchone()[0] == 0:
            return None  # Duplicate, skip
    except Exception:
        return None

    movement_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
    result = {"movement_id": movement_id}

    # Generate signal for significant movements
    if size_tier in ("large", "massive", "whale"):
        signal_type, confidence = score_signal(full_classification, direction, size_tier, whale_acc)
        price_at_signal = fetch_price(token_symbol)

        whale_label = whale.get("label") or address[:10]
        usd_str = f"${usd_value:,.0f}" if usd_value else "unknown"
        amount_str = f"{amount:,.2f}" if amount else "?"

        if full_classification == "exchange_deposit":
            summary = f"{whale_label} deposited {amount_str} {token_symbol} ({usd_str}) to {counterparty_label or 'exchange'} — likely selling"
        elif full_classification == "exchange_withdrawal":
            summary = f"{whale_label} withdrew {amount_str} {token_symbol} ({usd_str}) from {counterparty_label or 'exchange'} — likely accumulating"
        else:
            summary = f"{whale_label} transferred {amount_str} {token_symbol} ({usd_str}) to {counterparty[:10]}..."

        conn.execute(
            """INSERT INTO signals
               (movement_id, whale_address, timestamp, signal_type, confidence, token, summary, price_at_signal)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
            (movement_id, address, timestamp or now_iso(), signal_type, confidence, token_symbol.upper(), summary, price_at_signal),
        )
        signal_id = conn.execute("SELECT last_insert_rowid()").fetchone()[0]
        result["signal"] = {
            "id": signal_id,
            "whale_address": address,
            "whale_label": whale_label,
            "signal_type": signal_type,
            "confidence": confidence,
            "token": token_symbol.upper(),
            "summary": summary,
            "size_tier": size_tier,
            "classification": full_classification,
            "usd_value": usd_value,
            "price_at_signal": price_at_signal,
            "whale_accuracy": whale_acc,
        }

    return result


# ---------------------------------------------------------------------------
# Outcome tracking
# ---------------------------------------------------------------------------

def check_outcomes(logger):
    conn = get_db()
    # Check signals older than 24h that are still pending
    pending = conn.execute("""
        SELECT id, signal_type, token, price_at_signal, timestamp, price_after_24h, price_after_72h
        FROM signals
        WHERE outcome = 'pending' AND price_at_signal IS NOT NULL
        ORDER BY timestamp ASC
    """).fetchall()

    if not pending:
        conn.close()
        return

    now = time.time()
    updated = 0

    for sig in pending:
        sig = row_to_dict(sig)
        try:
            sig_time = datetime.fromisoformat(sig["timestamp"].replace("+00:00", "+00:00")).timestamp()
        except Exception:
            continue

        age_hours = (now - sig_time) / 3600
        current_price = fetch_price(sig["token"])
        if not current_price or not sig["price_at_signal"]:
            continue

        pct_change = ((current_price - sig["price_at_signal"]) / sig["price_at_signal"]) * 100

        # Fill 24h price if aged enough and not yet filled
        if age_hours >= 24 and sig["price_after_24h"] is None:
            conn.execute("UPDATE signals SET price_after_24h = ? WHERE id = ?", (current_price, sig["id"]))

        # Fill 72h price if aged enough and not yet filled
        if age_hours >= 72 and sig["price_after_72h"] is None:
            conn.execute("UPDATE signals SET price_after_72h = ? WHERE id = ?", (current_price, sig["id"]))

        # Determine outcome at 24h+
        if age_hours >= 24:
            if sig["signal_type"] == "bearish" and pct_change <= -1:
                outcome = "correct"
            elif sig["signal_type"] == "bullish" and pct_change >= 1:
                outcome = "correct"
            elif sig["signal_type"] == "neutral":
                outcome = "correct" if abs(pct_change) < 3 else "incorrect"
            elif age_hours >= 72:
                outcome = "incorrect"
            else:
                continue  # Still pending between 24-72h
            conn.execute("UPDATE signals SET outcome = ? WHERE id = ?", (outcome, sig["id"]))
            updated += 1

    conn.commit()
    conn.close()
    if updated:
        logger.info(f"[WHALE_TRACKER] Updated {updated} signal outcomes")


def rebuild_accuracy():
    conn = get_db()
    conn.execute("DELETE FROM whale_accuracy")
    conn.execute("""
        INSERT INTO whale_accuracy (whale_address, total_signals, correct_signals, accuracy_pct, avg_price_impact_24h, last_updated)
        SELECT
            whale_address,
            COUNT(*) as total_signals,
            SUM(CASE WHEN outcome = 'correct' THEN 1 ELSE 0 END) as correct_signals,
            ROUND(100.0 * SUM(CASE WHEN outcome = 'correct' THEN 1 ELSE 0 END) / COUNT(*), 1) as accuracy_pct,
            COALESCE(AVG(
                CASE WHEN price_at_signal > 0 AND price_after_24h > 0
                THEN ABS((price_after_24h - price_at_signal) / price_at_signal * 100)
                ELSE NULL END
            ), 0) as avg_price_impact_24h,
            ? as last_updated
        FROM signals
        WHERE outcome IN ('correct', 'incorrect')
        GROUP BY whale_address
    """, (now_iso(),))
    conn.commit()
    conn.close()


# ---------------------------------------------------------------------------
# App
# ---------------------------------------------------------------------------

def _status_extra():
    stats = get_stats()
    with _tick_lock:
        last_tick = _last_tick_at
    stats["last_tick_at"] = last_tick
    stats["last_outcome_check_at"] = _last_outcome_check_at
    stats["poll_interval_secs"] = POLL_INTERVAL
    stats["outcome_check_interval_secs"] = OUTCOME_CHECK_INTERVAL
    stats["worker_enabled"] = bool(ALCHEMY_API_KEY)
    stats["alert_threshold"] = ALERT_CONFIDENCE_THRESHOLD
    return stats


app = create_app("whale_tracker", status_extra_fn=_status_extra)


# ---------------------------------------------------------------------------
# RPC: Whales tool
# ---------------------------------------------------------------------------

@app.route("/rpc/tools/whales", methods=["POST"])
def rpc_whales():
    body = request.get_json(silent=True) or {}
    action = body.get("action")
    try:
        if action == "add":
            address = body.get("address")
            if not address:
                return error("address is required")
            chain = body.get("chain", "ethereum")
            label = body.get("label")
            category = body.get("category", "individual")
            tags = body.get("tags")
            notes = body.get("notes")
            entry, err = whale_add(address, chain, label, category, tags, notes)
            if err:
                return error(err)
            return success(entry)

        elif action == "remove":
            address = body.get("address")
            if not address:
                return error("address is required")
            if whale_remove(address):
                return success(True)
            return error(f"Whale {address} not found", 404)

        elif action == "list":
            return success(whale_list())

        elif action == "update":
            address = body.get("address")
            if not address:
                return error("address is required")
            if whale_update(address, body.get("label"), body.get("category"), body.get("tags"), body.get("enabled"), body.get("notes")):
                return success(True)
            return error(f"Whale {address} not found", 404)

        elif action == "import_exchanges":
            result = import_exchange_addresses()
            return success(result)

        else:
            return error(f"Unknown action: {action}. Valid: add, remove, list, update, import_exchanges")
    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# RPC: Signals tool
# ---------------------------------------------------------------------------

@app.route("/rpc/tools/signals", methods=["POST"])
def rpc_signals():
    body = request.get_json(silent=True) or {}
    action = body.get("action")
    try:
        if action == "recent":
            data = signals_recent(
                whale_address=body.get("whale_address"),
                token=body.get("token"),
                signal_type=body.get("signal_type"),
                min_confidence=body.get("min_confidence"),
                limit=body.get("limit", 50),
            )
            return success(data)

        elif action == "accuracy":
            return success(signals_accuracy())

        elif action == "history":
            address = body.get("whale_address")
            if not address:
                return error("whale_address is required")
            return success(signals_history(address, body.get("limit", 100)))

        else:
            return error(f"Unknown action: {action}. Valid: recent, accuracy, history")
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
            logger = logging.getLogger("whale_tracker.worker")
            threading.Thread(target=whale_tracker_tick, args=(logger,), daemon=True).start()
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
    try:
        result = backup_restore(body)
        return success(result)
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


def _format_usd(val) -> str:
    if val is None:
        return "-"
    if val >= 1_000_000_000:
        return f"${val / 1_000_000_000:,.1f}B"
    if val >= 1_000_000:
        return f"${val / 1_000_000:,.1f}M"
    if val >= 1_000:
        return f"${val / 1_000:,.1f}K"
    return f"${val:,.0f}"


@app.route("/")
def dashboard():
    stats = get_stats()
    whales = whale_list()
    recent_signals = signals_recent(limit=20)
    accuracy_data = signals_accuracy()

    conn = get_db()
    recent_movements = [row_to_dict(r) for r in conn.execute("""
        SELECT m.*, w.label as whale_label
        FROM movements m
        LEFT JOIN whales w ON m.whale_address = w.address
        ORDER BY m.block_number DESC, m.id DESC
        LIMIT 20
    """).fetchall()]
    conn.close()

    with _tick_lock:
        last_tick = _last_tick_at or "not yet"
    uptime = _format_uptime(int(time.time() - _start_time))
    worker_enabled = bool(ALCHEMY_API_KEY)

    warning_banner = ""
    if not worker_enabled:
        warning_banner = """<div style="background:#5a2d00;border:1px solid #b35c00;border-radius:8px;padding:12px 16px;margin-bottom:20px;display:flex;align-items:center;gap:10px;">
            <span style="font-size:1.3em;">&#9888;</span>
            <div>
                <strong style="color:#ffb347;">Background worker disabled</strong>
                <span style="color:#ccc;"> &mdash; <code style="background:#3d2200;padding:2px 6px;border-radius:4px;font-size:0.9em;">ALCHEMY_API_KEY</code> is not set.</span>
            </div>
        </div>"""

    # Whale leaderboard rows
    leaderboard_rows = ""
    for a in accuracy_data[:10]:
        label = a.get("label") or a["whale_address"][:12] + "..."
        acc_color = "#3fb950" if a["accuracy_pct"] >= 60 else "#d29922" if a["accuracy_pct"] >= 40 else "#f85149"
        leaderboard_rows += f'<tr><td>{label}</td><td>{a.get("chain", "-")}</td><td>{a.get("category", "-")}</td><td style="color:{acc_color};font-weight:bold;">{a["accuracy_pct"]:.1f}%</td><td>{a["total_signals"]}</td><td>{a["correct_signals"]}</td><td>{a["avg_price_impact_24h"]:.2f}%</td></tr>\n'
    if not leaderboard_rows:
        leaderboard_rows = '<tr><td colspan="7" style="text-align:center;color:#8b949e;padding:20px;">No accuracy data yet. Signals need 24h+ to resolve.</td></tr>'

    # Recent signals rows
    signal_rows = ""
    for s in recent_signals:
        label = s.get("whale_label") or s["whale_address"][:12] + "..."
        type_color = {"bearish": "#f85149", "bullish": "#3fb950", "neutral": "#8b949e"}.get(s["signal_type"], "#8b949e")
        outcome_badge = {"correct": '<span class="status-badge correct">Correct</span>', "incorrect": '<span class="status-badge incorrect">Incorrect</span>', "pending": '<span class="status-badge pending">Pending</span>'}.get(s["outcome"], s["outcome"])
        conf_color = "#3fb950" if s["confidence"] >= 70 else "#d29922" if s["confidence"] >= 50 else "#8b949e"
        signal_rows += f'<tr><td>{label}</td><td style="color:{type_color};font-weight:bold;">{s["signal_type"].upper()}</td><td>{s.get("token", "-")}</td><td style="color:{conf_color};font-weight:bold;">{s["confidence"]:.0f}</td><td style="max-width:400px;">{s.get("summary", "-")}</td><td>{outcome_badge}</td><td>{s["timestamp"][:19] if s.get("timestamp") else "-"}</td></tr>\n'
    if not signal_rows:
        signal_rows = '<tr><td colspan="7" style="text-align:center;color:#8b949e;padding:20px;">No signals recorded yet.</td></tr>'

    # Recent movements rows
    movement_rows = ""
    for m in recent_movements:
        label = m.get("whale_label") or m["whale_address"][:12] + "..."
        dir_color = "#f85149" if m["direction"] == "outflow" else "#3fb950"
        usd = _format_usd(m.get("usd_value"))
        cp_label = m.get("counterparty_label") or (m["counterparty"][:12] + "..." if m.get("counterparty") else "-")
        tx_short = f'{m["tx_hash"][:8]}...{m["tx_hash"][-4:]}' if m.get("tx_hash") and len(m["tx_hash"]) > 14 else m.get("tx_hash", "-")
        amt = m.get("amount")
        amt_str = f"{amt:,.2f}" if isinstance(amt, (int, float)) else "-"
        movement_rows += f'<tr><td>{label}</td><td style="color:{dir_color};">{m["direction"]}</td><td>{m.get("token_symbol", "ETH")}</td><td>{amt_str}</td><td>{usd}</td><td>{m.get("classification", "-")}</td><td>{cp_label}</td><td>{m.get("size_tier", "-")}</td><td class="mono">{tx_short}</td></tr>\n'
    if not movement_rows:
        movement_rows = '<tr><td colspan="9" style="text-align:center;color:#8b949e;padding:20px;">No movements recorded yet.</td></tr>'

    # Whale table rows
    whale_rows = ""
    for w in whales:
        label = w.get("label") or "-"
        status_cls = "active" if w["enabled"] else "paused"
        status_label = "Active" if w["enabled"] else "Paused"
        tags = ", ".join(w.get("tags", [])) if isinstance(w.get("tags"), list) and w["tags"] else "-"
        acc = f'{w["accuracy_pct"]:.1f}%' if w.get("accuracy_pct") is not None else "-"
        total_sigs = w.get("total_signals") or 0
        addr_short = f'{w["address"][:8]}...{w["address"][-4:]}'
        whale_rows += f'<tr><td>{label}</td><td class="mono">{addr_short}</td><td>{w["chain"]}</td><td>{w["category"]}</td><td>{tags}</td><td>{acc}</td><td>{total_sigs}</td><td><span class="status-badge {status_cls}">{status_label}</span></td></tr>\n'
    if not whale_rows:
        whale_rows = '<tr><td colspan="8" style="text-align:center;color:#8b949e;padding:20px;">No whales tracked. Add one via the whales tool.</td></tr>'

    avg_acc = f'{stats["avg_accuracy"]:.1f}%' if stats.get("avg_accuracy") is not None else "N/A"

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Whale Tracker Dashboard</title>
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
  .mono {{ font-family: 'SF Mono', 'Consolas', monospace; font-size: 0.85em; }}
  h2 {{ color: #c9d1d9; margin-bottom: 12px; font-size: 1.1em; }}
  .section {{ margin-bottom: 28px; }}
  .status-badge {{ padding: 2px 8px; border-radius: 12px; font-size: 0.8em; font-weight: 500; }}
  .status-badge.active {{ background: #0d2818; color: #3fb950; }}
  .status-badge.paused {{ background: #2d1b00; color: #d29922; }}
  .status-badge.correct {{ background: #0d2818; color: #3fb950; }}
  .status-badge.incorrect {{ background: #3d0a0a; color: #f85149; }}
  .status-badge.pending {{ background: #1a1a2e; color: #8b949e; }}
</style>
</head>
<body>
  <h1>Whale Tracker</h1>
  <p class="meta">Uptime: {uptime} &middot; Last tick: {last_tick} &middot; Poll interval: {POLL_INTERVAL}s &middot; Alert threshold: {ALERT_CONFIDENCE_THRESHOLD}</p>

  {warning_banner}

  <div class="stats">
    <div class="stat"><span class="val">{stats['total_whales']}</span><span class="lbl">Whales Tracked</span></div>
    <div class="stat"><span class="val">{stats['active_whales']}</span><span class="lbl">Active</span></div>
    <div class="stat"><span class="val">{stats['total_movements']}</span><span class="lbl">Total Movements</span></div>
    <div class="stat"><span class="val">{stats['total_signals']}</span><span class="lbl">Total Signals</span></div>
    <div class="stat"><span class="val">{stats['pending_signals']}</span><span class="lbl">Pending</span></div>
    <div class="stat"><span class="val">{avg_acc}</span><span class="lbl">Avg Accuracy</span></div>
  </div>

  <div class="section">
    <h2>Whale Leaderboard (by Accuracy)</h2>
    <table>
      <thead><tr><th>Whale</th><th>Chain</th><th>Category</th><th>Accuracy</th><th>Signals</th><th>Correct</th><th>Avg Impact 24h</th></tr></thead>
      <tbody>{leaderboard_rows}</tbody>
    </table>
  </div>

  <div class="section">
    <h2>Recent Signals</h2>
    <table>
      <thead><tr><th>Whale</th><th>Signal</th><th>Token</th><th>Confidence</th><th>Summary</th><th>Outcome</th><th>Time</th></tr></thead>
      <tbody>{signal_rows}</tbody>
    </table>
  </div>

  <div class="section">
    <h2>Recent Movements</h2>
    <table>
      <thead><tr><th>Whale</th><th>Direction</th><th>Token</th><th>Amount</th><th>USD</th><th>Classification</th><th>Counterparty</th><th>Size</th><th>Tx</th></tr></thead>
      <tbody>{movement_rows}</tbody>
    </table>
  </div>

  <div class="section">
    <h2>Tracked Whales</h2>
    <table>
      <thead><tr><th>Label</th><th>Address</th><th>Chain</th><th>Category</th><th>Tags</th><th>Accuracy</th><th>Signals</th><th>Status</th></tr></thead>
      <tbody>{whale_rows}</tbody>
    </table>
  </div>

  <script>
  // auto-refresh every 30s only if user hasn't interacted recently
  let _lastInteract = 0;
  document.addEventListener('keydown', () => _lastInteract = Date.now());
  document.addEventListener('click', () => _lastInteract = Date.now());
  setInterval(() => {{ if (Date.now() - _lastInteract > 5000) location.reload(); }}, 30000);
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

    # Auto-seed exchange addresses on first run
    conn = get_db()
    ex_count = conn.execute("SELECT COUNT(*) FROM exchange_addresses").fetchone()[0]
    conn.close()
    if ex_count == 0:
        import_exchange_addresses()
        logging.info("[WHALE_TRACKER] Seeded exchange addresses")

    if ALCHEMY_API_KEY:
        worker_thread = threading.Thread(target=worker_loop, daemon=True)
        worker_thread.start()
        outcome_thread = threading.Thread(target=outcome_loop, daemon=True)
        outcome_thread.start()
    else:
        logging.warning("[WHALE_TRACKER] ALCHEMY_API_KEY not set — background worker disabled")

    port = int(os.environ.get("MODULE_PORT", os.environ.get("WHALE_TRACKER_PORT", "9106")))
    app.run(host="127.0.0.1", port=port)
