# /// script
# requires-python = ">=3.12"
# dependencies = ["requests", "starkbot-sdk", "pynacl"]
#
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
HyperClaw module — autonomous perpetual futures trader on Orderly Network
via the HyperClaw broker.

Uses the Orderly Network REST API with broker_id="hyper_claw" for leveraged
LONG/SHORT perpetual positions.  Evaluates Bankr signals, manages positions,
and handles the full lifecycle: registration → deposit → trade → close.

RPC protocol endpoints:
  GET  /rpc/status          -> service health
  POST /rpc/decision        -> agent submits OPEN_LONG/OPEN_SHORT/CLOSE/HOLD
  GET  /rpc/positions       -> open positions with P&L
  GET  /rpc/orders          -> open orders
  GET  /rpc/trade_history   -> closed trades with realized P&L
  POST /rpc/history         -> query trade decision history
  GET  /rpc/stats           -> aggregate trading statistics
  GET  /rpc/pnl             -> aggregate P&L summary
  POST /rpc/refresh         -> refresh positions & P&L from Orderly
  GET  /rpc/config          -> view trader config
  POST /rpc/config          -> update trader config
  POST /rpc/control         -> start/stop/trigger worker
  POST /rpc/register        -> initiate Orderly account registration
  POST /rpc/add_key         -> register ed25519 trading key with Orderly
  POST /rpc/sign            -> submit signed EIP-712 or vault deposit tx
  POST /rpc/deposit         -> construct USDC vault deposit tx
  GET  /rpc/symbols         -> available perpetual symbols
  GET  /rpc/account         -> Orderly account info & balances
  POST /rpc/backup/export   -> export all tables
  POST /rpc/backup/restore  -> restore from backup
  GET  /                    -> HTML dashboard

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
import hashlib
import base64
import struct
import requests as http_requests
from datetime import datetime, timezone

# ---------------------------------------------------------------------------
# Ed25519 helpers (via PyNaCl)
# ---------------------------------------------------------------------------

try:
    from nacl.signing import SigningKey, VerifyKey
    from nacl.encoding import RawEncoder
    HAS_NACL = True
except ImportError:
    HAS_NACL = False


def generate_ed25519_keypair() -> tuple[str, str]:
    """Generate an ed25519 keypair. Returns (public_key_hex, secret_key_hex)."""
    if not HAS_NACL:
        raise RuntimeError("pynacl not installed — cannot generate ed25519 keys")
    sk = SigningKey.generate()
    pk = sk.verify_key
    return pk.encode().hex(), sk.encode().hex()


def ed25519_sign(secret_key_hex: str, message: str) -> str:
    """Sign a message with ed25519, return base64url-encoded signature."""
    if not HAS_NACL:
        raise RuntimeError("pynacl not installed")
    sk = SigningKey(bytes.fromhex(secret_key_hex))
    signed = sk.sign(message.encode())
    sig_bytes = signed.signature
    return base64.urlsafe_b64encode(sig_bytes).decode().rstrip("=")


# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

DB_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "hyper_claw.db")
BACKEND_URL = os.environ.get("STARKBOT_BACKEND_URL", "http://127.0.0.1:8080")
INTERNAL_TOKEN = os.environ.get("STARKBOT_INTERNAL_TOKEN", "")
ALCHEMY_API_KEY = os.environ.get("ALCHEMY_API_KEY", "")

# Orderly Network
ORDERLY_API_URL = "https://api-evm.orderly.org"
BROKER_ID = "hyper_claw"

# Default chain: Base
DEFAULT_CHAIN_ID = 8453
BASE_RPC_URL = f"https://base-mainnet.g.alchemy.com/v2/{ALCHEMY_API_KEY}" if ALCHEMY_API_KEY else ""

# USDC on Base (for vault deposits)
USDC_ADDRESS = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
USDC_DECIMALS = 6

# Orderly vault contract on Base
ORDERLY_VAULT_ADDRESS = "0x816f722424B49Cf1275cc86DA9840Fbd5a6167e9"

# Defaults
DEFAULT_PULSE_INTERVAL = 300  # 5 minutes

# Module state
_start_time = time.time()
_worker_running = False
_worker_lock = threading.Lock()
_last_pulse_at = None


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
            symbol       TEXT,
            reason       TEXT,
            status       TEXT NOT NULL DEFAULT 'pending',
            created_at   TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS trade_executions (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            decision_id  INTEGER NOT NULL REFERENCES trade_decisions(id),
            tx_type      TEXT,
            order_id     TEXT,
            raw_tx_to    TEXT,
            raw_tx_data  TEXT,
            raw_tx_value TEXT,
            raw_tx_gas   TEXT,
            signed_tx    TEXT,
            tx_hash      TEXT,
            status       TEXT NOT NULL DEFAULT 'pending',
            error_msg    TEXT,
            created_at   TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS positions (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol          TEXT NOT NULL,
            side            TEXT NOT NULL,
            qty             REAL NOT NULL,
            avg_open_price  REAL NOT NULL,
            current_price   REAL,
            mark_price      REAL,
            unrealized_pnl  REAL DEFAULT 0,
            pnl_pct         REAL DEFAULT 0,
            leverage        REAL,
            collateral      REAL,
            liquidation_price REAL,
            decision_id     INTEGER,
            opened_at       TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(symbol)
        );

        CREATE TABLE IF NOT EXISTS trade_history (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol          TEXT NOT NULL,
            side            TEXT NOT NULL,
            qty             REAL,
            avg_open_price  REAL,
            close_price     REAL,
            realized_pnl    REAL,
            pnl_pct         REAL,
            fee             REAL DEFAULT 0,
            decision_id     INTEGER,
            order_id        TEXT,
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
        ("enabled", "false"),
        ("trader_address", ""),
        ("orderly_account_id", ""),
        ("orderly_key", ""),
        ("orderly_secret", ""),
        ("registered", "false"),
        ("key_added", "false"),
        ("chain_id", str(DEFAULT_CHAIN_ID)),
        ("max_collateral_per_trade", "10"),
        ("max_leverage", "20"),
        ("default_leverage", "5"),
        ("max_positions", "3"),
        ("take_profit_pct", "50"),
        ("stop_loss_pct", "25"),
        ("bankr_min_confidence", "70"),
        ("bankr_providers", ""),
        ("allowed_symbols", "PERP_ETH_USDC,PERP_BTC_USDC,PERP_SOL_USDC"),
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


def ts_ms() -> int:
    return int(time.time() * 1000)


# ---------------------------------------------------------------------------
# Orderly API helpers
# ---------------------------------------------------------------------------

def _orderly_headers(method: str, path: str, body: str = "") -> dict:
    """Build signed Orderly API request headers using stored ed25519 key."""
    account_id = get_config_value("orderly_account_id")
    public_key = get_config_value("orderly_key")
    secret_key = get_config_value("orderly_secret")

    if not all([account_id, public_key, secret_key]):
        raise ValueError("Orderly credentials not configured (orderly_account_id, orderly_key, orderly_secret)")

    timestamp = ts_ms()
    # Normalized message: timestamp + method + path + body
    message = f"{timestamp}{method.upper()}{path}{body}"
    signature = ed25519_sign(secret_key, message)

    return {
        "Content-Type": "application/json",
        "orderly-account-id": account_id,
        "orderly-key": f"ed25519:{public_key}",
        "orderly-signature": signature,
        "orderly-timestamp": str(timestamp),
    }


def orderly_get(path: str) -> dict:
    """Make authenticated GET request to Orderly API."""
    headers = _orderly_headers("GET", path)
    resp = http_requests.get(f"{ORDERLY_API_URL}{path}", headers=headers, timeout=15)
    return resp.json()


def orderly_post(path: str, body: dict) -> dict:
    """Make authenticated POST request to Orderly API."""
    body_str = json.dumps(body, separators=(",", ":"))
    headers = _orderly_headers("POST", path, body_str)
    resp = http_requests.post(
        f"{ORDERLY_API_URL}{path}", headers=headers, data=body_str, timeout=15
    )
    return resp.json()


def orderly_delete(path: str) -> dict:
    """Make authenticated DELETE request to Orderly API."""
    headers = _orderly_headers("DELETE", path)
    resp = http_requests.delete(f"{ORDERLY_API_URL}{path}", headers=headers, timeout=15)
    return resp.json()


def orderly_public_get(path: str) -> dict:
    """Make unauthenticated GET to Orderly public API."""
    resp = http_requests.get(f"{ORDERLY_API_URL}{path}", timeout=15)
    return resp.json()


# ---------------------------------------------------------------------------
# Trading helpers
# ---------------------------------------------------------------------------

def get_available_symbols() -> list[dict]:
    """Get available perpetual symbols from Orderly."""
    try:
        data = orderly_public_get("/v1/public/info")
        rows = data.get("data", {}).get("rows", [])
        return [r for r in rows if r.get("symbol", "").startswith("PERP_")]
    except Exception as e:
        logging.error(f"[HYPER_CLAW] get_available_symbols error: {e}")
        return []


def get_current_price(symbol: str) -> float | None:
    """Get current mark price for a symbol."""
    try:
        data = orderly_public_get(f"/v1/public/futures/{symbol}")
        info = data.get("data", {})
        return float(info.get("mark_price") or info.get("index_price") or 0) or None
    except Exception as e:
        logging.error(f"[HYPER_CLAW] get_current_price error for {symbol}: {e}")
        return None


def place_order(
    symbol: str,
    side: str,
    order_type: str = "MARKET",
    order_quantity: float | None = None,
    order_price: float | None = None,
    reduce_only: bool = False,
) -> dict:
    """Place an order on Orderly via HyperClaw broker."""
    body = {
        "symbol": symbol,
        "order_type": order_type,
        "side": side,
    }
    if order_quantity is not None:
        body["order_quantity"] = order_quantity
    if order_price is not None and order_type == "LIMIT":
        body["order_price"] = order_price
    if reduce_only:
        body["reduce_only"] = True

    return orderly_post("/v1/order", body)


def cancel_order(symbol: str, order_id: str) -> dict:
    """Cancel an open order."""
    return orderly_delete(f"/v1/order?symbol={symbol}&order_id={order_id}")


def get_orderly_positions() -> list[dict]:
    """Get all open positions from Orderly."""
    try:
        data = orderly_get("/v1/positions")
        rows = data.get("data", {}).get("rows", [])
        return rows
    except Exception as e:
        logging.error(f"[HYPER_CLAW] get_orderly_positions error: {e}")
        return []


def get_orderly_orders() -> list[dict]:
    """Get open orders from Orderly."""
    try:
        data = orderly_get("/v1/orders?status=INCOMPLETE")
        rows = data.get("data", {}).get("rows", [])
        return rows
    except Exception as e:
        logging.error(f"[HYPER_CLAW] get_orderly_orders error: {e}")
        return []


def get_orderly_account() -> dict:
    """Get account info from Orderly."""
    try:
        return orderly_get("/v1/client/info")
    except Exception as e:
        logging.error(f"[HYPER_CLAW] get_orderly_account error: {e}")
        return {}


def get_orderly_balances() -> dict:
    """Get account balances/holdings from Orderly."""
    try:
        data = orderly_get("/v1/client/holding")
        return data.get("data", {})
    except Exception as e:
        logging.error(f"[HYPER_CLAW] get_orderly_balances error: {e}")
        return {}


# ---------------------------------------------------------------------------
# P&L helpers
# ---------------------------------------------------------------------------

def calc_pnl(side: str, qty: float, open_price: float, current_price: float) -> tuple[float, float]:
    """Calculate unrealized P&L and percentage."""
    if open_price <= 0 or qty <= 0:
        return 0.0, 0.0
    position_value = qty * open_price
    if side.upper() in ("LONG", "BUY"):
        pnl = qty * (current_price - open_price)
    else:
        pnl = qty * (open_price - current_price)
    pnl_pct = (pnl / position_value * 100) if position_value > 0 else 0
    return round(pnl, 4), round(pnl_pct, 2)


# ---------------------------------------------------------------------------
# Registration helpers
# ---------------------------------------------------------------------------

def build_registration_nonce() -> dict | None:
    """Get registration nonce from Orderly."""
    try:
        resp = http_requests.get(f"{ORDERLY_API_URL}/v1/registration_nonce", timeout=10)
        data = resp.json()
        return data.get("data", {})
    except Exception as e:
        logging.error(f"[HYPER_CLAW] registration_nonce error: {e}")
        return None


def build_registration_eip712(trader_address: str, nonce: str, chain_id: int) -> dict:
    """Build the EIP-712 typed data for Orderly registration."""
    timestamp = ts_ms()
    return {
        "domain": {
            "name": "Orderly",
            "version": "1",
            "chainId": chain_id,
            "verifyingContract": "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC",
        },
        "types": {
            "EIP712Domain": [
                {"name": "name", "type": "string"},
                {"name": "version", "type": "string"},
                {"name": "chainId", "type": "uint256"},
                {"name": "verifyingContract", "type": "address"},
            ],
            "Registration": [
                {"name": "brokerId", "type": "string"},
                {"name": "chainId", "type": "uint256"},
                {"name": "timestamp", "type": "uint64"},
                {"name": "registrationNonce", "type": "uint256"},
            ],
        },
        "primaryType": "Registration",
        "message": {
            "brokerId": BROKER_ID,
            "chainId": chain_id,
            "timestamp": timestamp,
            "registrationNonce": nonce,
        },
    }


def build_add_key_eip712(trader_address: str, orderly_public_key: str, chain_id: int) -> dict:
    """Build the EIP-712 typed data for adding an Orderly trading key."""
    timestamp = ts_ms()
    # Scope timestamp = valid for 365 days
    scope_ts = timestamp + (365 * 24 * 60 * 60 * 1000)
    return {
        "domain": {
            "name": "Orderly",
            "version": "1",
            "chainId": chain_id,
            "verifyingContract": "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC",
        },
        "types": {
            "EIP712Domain": [
                {"name": "name", "type": "string"},
                {"name": "version", "type": "string"},
                {"name": "chainId", "type": "uint256"},
                {"name": "verifyingContract", "type": "address"},
            ],
            "AddOrderlyKey": [
                {"name": "brokerId", "type": "string"},
                {"name": "chainId", "type": "uint256"},
                {"name": "orderlyKey", "type": "string"},
                {"name": "scope", "type": "string"},
                {"name": "timestamp", "type": "uint64"},
                {"name": "expiration", "type": "uint64"},
            ],
        },
        "primaryType": "AddOrderlyKey",
        "message": {
            "brokerId": BROKER_ID,
            "chainId": chain_id,
            "orderlyKey": f"ed25519:{orderly_public_key}",
            "scope": "read,trading",
            "timestamp": timestamp,
            "expiration": scope_ts,
        },
    }


def build_vault_deposit_tx(trader_address: str, amount_usdc: float) -> dict | None:
    """Build an unsigned USDC deposit tx to the Orderly vault on Base."""
    if not BASE_RPC_URL:
        return None
    try:
        from eth_abi import encode

        # Orderly vault deposit function:
        # deposit(address token, uint256 amount)
        # function selector for deposit(address,uint256)
        selector = bytes.fromhex("47e7ef24")
        usdc_addr = bytes.fromhex(USDC_ADDRESS[2:].lower().zfill(40))
        amount_raw = int(amount_usdc * 10**USDC_DECIMALS)

        encoded = encode(
            ["address", "uint256"],
            [USDC_ADDRESS, amount_raw],
        )
        data = "0x" + selector.hex() + encoded.hex()

        return {
            "to": ORDERLY_VAULT_ADDRESS,
            "data": data,
            "value": "0",
            "gas": "200000",
        }
    except Exception as e:
        logging.error(f"[HYPER_CLAW] build_vault_deposit_tx error: {e}")
        return None


def build_usdc_approve_tx(trader_address: str, spender: str) -> dict | None:
    """Build an unsigned USDC approval tx for a spender (vault)."""
    if not BASE_RPC_URL:
        return None
    try:
        from eth_abi import encode

        # approve(address,uint256)
        selector = bytes.fromhex("095ea7b3")
        max_approval = 2**256 - 1
        encoded = encode(["address", "uint256"], [spender, max_approval])
        data = "0x" + selector.hex() + encoded.hex()

        return {
            "to": USDC_ADDRESS,
            "data": data,
            "value": "0",
            "gas": "60000",
        }
    except Exception as e:
        logging.error(f"[HYPER_CLAW] build_usdc_approve_tx error: {e}")
        return None


# ---------------------------------------------------------------------------
# Broadcast helper (for vault deposits / approvals)
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
        logging.error(f"[HYPER_CLAW] Broadcast error: {err}")
    except Exception as e:
        logging.error(f"[HYPER_CLAW] Broadcast exception: {e}")
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
        logging.warning("[HYPER_CLAW] No STARKBOT_INTERNAL_TOKEN — cannot fire hooks")
        return
    try:
        http_requests.post(
            f"{BACKEND_URL}/api/internal/hooks/fire",
            json={"event": event, "data": data or {}},
            headers={"X-Internal-Token": INTERNAL_TOKEN},
            timeout=10,
        )
    except Exception as e:
        logging.error(f"[HYPER_CLAW] Hook fire error: {e}")


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
            logging.warning(f"[HYPER_CLAW] Bankr signals API returned {resp.status_code}")
            return []
        signals = resp.json()
        if isinstance(signals, dict):
            data = signals.get("data", signals)
            if isinstance(data, dict):
                signals = data.get("signals", [])
            elif isinstance(data, list):
                signals = data
            else:
                signals = signals.get("signals", [])
    except Exception as e:
        logging.error(f"[HYPER_CLAW] Bankr signals fetch error: {e}")
        return []

    # Map allowed_symbols to token names (PERP_ETH_USDC -> ETH)
    allowed_symbols_raw = get_config_value("allowed_symbols", "PERP_ETH_USDC,PERP_BTC_USDC,PERP_SOL_USDC")
    allowed_tokens = set()
    for sym in allowed_symbols_raw.split(","):
        parts = sym.strip().split("_")
        if len(parts) >= 2:
            allowed_tokens.add(parts[1].upper())

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
        action = (sig.get("action") or "").upper()
        if action not in ("LONG", "SHORT"):
            continue
        token = (sig.get("token") or sig.get("symbol") or "").upper()
        if allowed_tokens and token not in allowed_tokens:
            continue
        results.append(sig)

    logging.info(f"[HYPER_CLAW] Bankr signals: {len(results)} qualifying out of {len(signals)} total")
    return results


# ---------------------------------------------------------------------------
# Position sync from Orderly
# ---------------------------------------------------------------------------

def sync_positions_from_orderly() -> dict:
    """Sync positions from Orderly API into local DB."""
    orderly_positions = get_orderly_positions()

    conn = get_db()
    existing = conn.execute("SELECT * FROM positions").fetchall()
    existing_map = {r["symbol"]: dict(r) for r in existing}

    orderly_map = {}
    added = 0
    updated = 0

    for p in orderly_positions:
        symbol = p.get("symbol", "")
        qty = abs(float(p.get("position_qty", 0)))
        if qty == 0:
            continue

        orderly_map[symbol] = p
        side = "LONG" if float(p.get("position_qty", 0)) > 0 else "SHORT"
        avg_open = float(p.get("average_open_price", 0))
        mark = float(p.get("mark_price", 0))
        unrealized = float(p.get("unrealized_pnl", 0))
        liq_price = float(p.get("est_liq_price", 0))
        cost = float(p.get("cost_position", 0))

        pnl_pct = (unrealized / abs(cost) * 100) if cost else 0

        if symbol in existing_map:
            conn.execute(
                """UPDATE positions SET
                     side = ?, qty = ?, avg_open_price = ?, current_price = ?,
                     mark_price = ?, unrealized_pnl = ?, pnl_pct = ?,
                     liquidation_price = ?, updated_at = ?
                   WHERE symbol = ?""",
                (side, qty, avg_open, mark, mark, round(unrealized, 4),
                 round(pnl_pct, 2), liq_price, now_iso(), symbol),
            )
            updated += 1
        else:
            conn.execute(
                """INSERT INTO positions
                     (symbol, side, qty, avg_open_price, current_price, mark_price,
                      unrealized_pnl, pnl_pct, liquidation_price, opened_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (symbol, side, qty, avg_open, mark, mark,
                 round(unrealized, 4), round(pnl_pct, 2), liq_price,
                 now_iso(), now_iso()),
            )
            added += 1

    # Positions in DB but not on Orderly → were closed/liquidated
    closed = 0
    for symbol, pos in existing_map.items():
        if symbol not in orderly_map:
            close_price = float(pos.get("current_price") or pos.get("avg_open_price", 0))
            pnl, pnl_pct = calc_pnl(
                pos["side"], float(pos["qty"]),
                float(pos["avg_open_price"]), close_price,
            )
            conn.execute(
                """INSERT INTO trade_history
                     (symbol, side, qty, avg_open_price, close_price,
                      realized_pnl, pnl_pct, decision_id, opened_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (pos["symbol"], pos["side"], pos["qty"],
                 pos["avg_open_price"], close_price, pnl, pnl_pct,
                 pos.get("decision_id"), pos.get("opened_at")),
            )
            conn.execute("DELETE FROM positions WHERE symbol = ?", (symbol,))
            closed += 1

    conn.commit()
    conn.close()
    return {"synced": len(orderly_positions), "added": added, "updated": updated, "closed": closed}


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
        total_collateral += float(p.get("collateral") or 0)
        total_unrealized += float(p["unrealized_pnl"] or 0)
        pnl_pct_val = float(p["pnl_pct"] or 0)

        info = {
            "symbol": p["symbol"],
            "side": p["side"],
            "qty": p["qty"],
            "avg_open_price": p["avg_open_price"],
            "current_price": p["current_price"],
            "mark_price": p["mark_price"],
            "liquidation_price": p["liquidation_price"],
            "unrealized_pnl": round(float(p["unrealized_pnl"] or 0), 2),
            "pnl_pct": pnl_pct_val,
        }
        position_list.append(info)

        # Liquidation alert: within 5% of liquidation price
        liq_price = float(p.get("liquidation_price") or 0)
        cur_price = float(p.get("current_price") or 0)
        if liq_price > 0 and cur_price > 0:
            if p["side"] == "LONG" and cur_price <= liq_price * 1.05:
                liquidation_alerts.append(info)
            elif p["side"] == "SHORT" and cur_price >= liq_price * 0.95:
                liquidation_alerts.append(info)

        if pnl_pct_val <= -sl_pct:
            stop_loss_alerts.append(info)
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

    closed = conn.execute(
        "SELECT symbol, realized_pnl FROM trade_history WHERE realized_pnl IS NOT NULL"
    ).fetchall()
    total_realized = sum(float(t["realized_pnl"] or 0) for t in closed)
    win_count = sum(1 for t in closed if float(t["realized_pnl"] or 0) > 0)
    loss_count = sum(1 for t in closed if float(t["realized_pnl"] or 0) < 0)

    best_trade = max(closed, key=lambda t: float(t["realized_pnl"] or 0), default=None)
    worst_trade = min(closed, key=lambda t: float(t["realized_pnl"] or 0), default=None)

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
        "best_trade": {"symbol": best_trade["symbol"], "pnl": round(float(best_trade["realized_pnl"]), 2)} if best_trade else None,
        "worst_trade": {"symbol": worst_trade["symbol"], "pnl": round(float(worst_trade["realized_pnl"]), 2)} if worst_trade else None,
    }


# ---------------------------------------------------------------------------
# Background pulse worker
# ---------------------------------------------------------------------------

def _fire_pulse():
    """Build full pulse data with risk context and fire the hook."""
    trader_address = get_config_value("trader_address", "")
    registered = get_config_value("registered", "false").lower() == "true"
    key_added = get_config_value("key_added", "false").lower() == "true"

    # Sync positions from Orderly if registered
    if registered and key_added:
        try:
            sync_positions_from_orderly()
        except Exception as e:
            logging.warning(f"[HYPER_CLAW] Position sync failed during pulse: {e}")

    position_summary = _build_position_summary()
    pnl = compute_pnl()

    # Account status
    account_status = {
        "registered": registered,
        "key_added": key_added,
        "trader_address": trader_address,
    }

    # Get balances if possible
    balances = {}
    if registered and key_added:
        try:
            balances = get_orderly_balances()
        except Exception:
            pass

    hook_data = {
        "position_summary": position_summary,
        "pnl": pnl,
        "account_status": account_status,
        "balances": balances,
        "bankr_signals": fetch_bankr_signals(),
        "trader_address": trader_address,
    }

    logging.info(
        f"[HYPER_CLAW] Firing pulse ({len(hook_data['bankr_signals'])} signals, "
        f"{position_summary['open_positions']} positions)"
    )
    fire_hook("hyper_claw_pulse", hook_data)


def pulse_worker():
    global _last_pulse_at, _worker_running
    logger = logging.getLogger("hyper_claw.worker")
    logger.info("[HYPER_CLAW] Pulse worker started")
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
        "registered": get_config_value("registered", "false"),
        "key_added": get_config_value("key_added", "false"),
    }


app = create_app("hyper_claw", status_extra_fn=extra_status)


# ----- /rpc/register -----

@app.route("/rpc/register", methods=["POST"])
def rpc_register():
    """Initiate Orderly account registration — returns EIP-712 data to sign."""
    trader_address = get_config_value("trader_address", "")
    if not trader_address:
        return error("trader_address not set in config")

    chain_id = int(get_config_value("chain_id", str(DEFAULT_CHAIN_ID)))

    # Get registration nonce
    nonce_data = build_registration_nonce()
    if not nonce_data:
        return error("Failed to get registration nonce from Orderly")

    nonce = str(nonce_data.get("registration_nonce", ""))
    if not nonce:
        return error("Empty registration nonce returned")

    eip712 = build_registration_eip712(trader_address, nonce, chain_id)

    # Store pending registration
    conn = get_db()
    cur = conn.execute(
        "INSERT INTO trade_decisions (decision, symbol, reason, status) VALUES ('REGISTER', '', 'Orderly account registration', 'pending')"
    )
    decision_id = cur.lastrowid
    cur2 = conn.execute(
        """INSERT INTO trade_executions
             (decision_id, tx_type, status)
           VALUES (?, 'register', 'pending')""",
        (decision_id,),
    )
    tx_id = cur2.lastrowid
    conn.commit()
    conn.close()

    fire_hook("hyper_claw_sign_tx", {
        "tx_id": tx_id,
        "decision_id": decision_id,
        "action": "register",
        "eip712_data": eip712,
        "trader_address": trader_address,
        "chain_id": chain_id,
    })

    return success({
        "decision_id": decision_id,
        "tx_id": tx_id,
        "eip712_data": eip712,
        "action": "register",
    })


# ----- /rpc/add_key -----

@app.route("/rpc/add_key", methods=["POST"])
def rpc_add_key():
    """Generate ed25519 keypair and register it with Orderly."""
    trader_address = get_config_value("trader_address", "")
    if not trader_address:
        return error("trader_address not set in config")
    if get_config_value("registered", "false").lower() != "true":
        return error("Account not registered — call /rpc/register first")

    chain_id = int(get_config_value("chain_id", str(DEFAULT_CHAIN_ID)))

    # Generate ed25519 keypair
    try:
        pub_key, secret_key = generate_ed25519_keypair()
    except Exception as e:
        return error(f"Failed to generate ed25519 keypair: {e}")

    # Store keys (secret stays local)
    set_config_value("orderly_key", pub_key)
    set_config_value("orderly_secret", secret_key)

    eip712 = build_add_key_eip712(trader_address, pub_key, chain_id)

    conn = get_db()
    cur = conn.execute(
        "INSERT INTO trade_decisions (decision, symbol, reason, status) VALUES ('ADD_KEY', '', 'Register ed25519 trading key', 'pending')"
    )
    decision_id = cur.lastrowid
    cur2 = conn.execute(
        """INSERT INTO trade_executions
             (decision_id, tx_type, status)
           VALUES (?, 'add_key', 'pending')""",
        (decision_id,),
    )
    tx_id = cur2.lastrowid
    conn.commit()
    conn.close()

    fire_hook("hyper_claw_sign_tx", {
        "tx_id": tx_id,
        "decision_id": decision_id,
        "action": "add_key",
        "eip712_data": eip712,
        "orderly_public_key": pub_key,
        "trader_address": trader_address,
        "chain_id": chain_id,
    })

    return success({
        "decision_id": decision_id,
        "tx_id": tx_id,
        "eip712_data": eip712,
        "orderly_public_key": pub_key,
        "action": "add_key",
    })


# ----- /rpc/sign -----

@app.route("/rpc/sign", methods=["POST"])
def rpc_sign():
    """Handle signed data — EIP-712 signatures for registration/add_key, or signed tx for deposits."""
    body = request.get_json(silent=True) or {}
    tx_id = body.get("tx_id")
    signature = body.get("signature", "")
    signed_tx = body.get("signed_tx", "")

    if not tx_id:
        return error("tx_id is required")

    conn = get_db()
    row = conn.execute("SELECT * FROM trade_executions WHERE id = ?", (tx_id,)).fetchone()
    if not row:
        conn.close()
        return error(f"No execution found with tx_id={tx_id}", 404)

    tx_type = row["tx_type"]
    decision_id = row["decision_id"]
    conn.close()

    if tx_type == "register":
        if not signature:
            return error("signature is required for registration")
        return _handle_register_sign(tx_id, decision_id, signature, body)

    elif tx_type == "add_key":
        if not signature:
            return error("signature is required for add_key")
        return _handle_add_key_sign(tx_id, decision_id, signature, body)

    elif tx_type in ("deposit", "approve_usdc"):
        if not signed_tx or not signed_tx.startswith("0x"):
            return error("signed_tx must be a 0x-prefixed hex string")
        return _handle_onchain_sign(tx_id, decision_id, tx_type, signed_tx)

    elif tx_type in ("open_trade", "close_trade"):
        # These are Orderly API orders — already placed, just update status
        conn2 = get_db()
        conn2.execute(
            "UPDATE trade_executions SET status = 'completed', updated_at = ? WHERE id = ?",
            (now_iso(), tx_id),
        )
        conn2.commit()
        conn2.close()
        return success({"tx_id": tx_id, "status": "completed"})

    else:
        return error(f"Unknown tx_type: {tx_type}")


def _handle_register_sign(tx_id: int, decision_id: int, signature: str, body: dict):
    """Complete Orderly registration with the signed EIP-712 data."""
    trader_address = get_config_value("trader_address")
    chain_id = int(get_config_value("chain_id", str(DEFAULT_CHAIN_ID)))
    eip712 = body.get("eip712_data")

    if not eip712:
        # Reconstruct from stored data
        nonce_data = build_registration_nonce()
        nonce = str(nonce_data.get("registration_nonce", "")) if nonce_data else ""
        eip712 = build_registration_eip712(trader_address, nonce, chain_id)

    message = eip712.get("message", {}) if eip712 else {}

    try:
        resp = http_requests.post(
            f"{ORDERLY_API_URL}/v1/register_account",
            json={
                "message": message,
                "signature": signature,
                "userAddress": trader_address,
            },
            timeout=15,
        )
        data = resp.json()

        if data.get("success"):
            account_id = data.get("data", {}).get("account_id", "")
            if account_id:
                set_config_value("orderly_account_id", account_id)
            set_config_value("registered", "true")

            conn = get_db()
            conn.execute(
                "UPDATE trade_executions SET status = 'completed', updated_at = ? WHERE id = ?",
                (now_iso(), tx_id),
            )
            conn.execute(
                "UPDATE trade_decisions SET status = 'executed', updated_at = ? WHERE id = ?",
                (now_iso(), decision_id),
            )
            conn.commit()
            conn.close()

            logging.info(f"[HYPER_CLAW] Registered with Orderly: {account_id}")
            return success({"status": "registered", "account_id": account_id})
        else:
            err_msg = json.dumps(data)
            conn = get_db()
            conn.execute(
                "UPDATE trade_executions SET status = 'failed', error_msg = ?, updated_at = ? WHERE id = ?",
                (err_msg, now_iso(), tx_id),
            )
            conn.execute(
                "UPDATE trade_decisions SET status = 'failed', updated_at = ? WHERE id = ?",
                (now_iso(), decision_id),
            )
            conn.commit()
            conn.close()
            return error(f"Registration failed: {err_msg}")

    except Exception as e:
        logging.error(f"[HYPER_CLAW] Registration error: {e}")
        return error(f"Registration error: {e}")


def _handle_add_key_sign(tx_id: int, decision_id: int, signature: str, body: dict):
    """Complete Orderly key addition with the signed EIP-712 data."""
    pub_key = get_config_value("orderly_key")
    chain_id = int(get_config_value("chain_id", str(DEFAULT_CHAIN_ID)))
    eip712 = body.get("eip712_data")

    message = eip712.get("message", {}) if eip712 else {}

    try:
        resp = http_requests.post(
            f"{ORDERLY_API_URL}/v1/orderly_key",
            json={
                "message": message,
                "signature": signature,
                "userAddress": get_config_value("trader_address"),
            },
            timeout=15,
        )
        data = resp.json()

        if data.get("success"):
            set_config_value("key_added", "true")

            conn = get_db()
            conn.execute(
                "UPDATE trade_executions SET status = 'completed', updated_at = ? WHERE id = ?",
                (now_iso(), tx_id),
            )
            conn.execute(
                "UPDATE trade_decisions SET status = 'executed', updated_at = ? WHERE id = ?",
                (now_iso(), decision_id),
            )
            conn.commit()
            conn.close()

            logging.info(f"[HYPER_CLAW] Ed25519 key registered with Orderly")
            return success({"status": "key_added", "orderly_key": f"ed25519:{pub_key}"})
        else:
            err_msg = json.dumps(data)
            conn = get_db()
            conn.execute(
                "UPDATE trade_executions SET status = 'failed', error_msg = ?, updated_at = ? WHERE id = ?",
                (err_msg, now_iso(), tx_id),
            )
            conn.execute(
                "UPDATE trade_decisions SET status = 'failed', updated_at = ? WHERE id = ?",
                (now_iso(), decision_id),
            )
            conn.commit()
            conn.close()
            return error(f"Add key failed: {err_msg}")

    except Exception as e:
        logging.error(f"[HYPER_CLAW] Add key error: {e}")
        return error(f"Add key error: {e}")


def _handle_onchain_sign(tx_id: int, decision_id: int, tx_type: str, signed_tx: str):
    """Handle signed on-chain tx (vault deposit or USDC approval)."""
    conn = get_db()
    conn.execute(
        "UPDATE trade_executions SET signed_tx = ?, status = 'signed', updated_at = ? WHERE id = ?",
        (signed_tx, now_iso(), tx_id),
    )
    conn.commit()
    conn.close()

    def do_broadcast():
        tx_hash = broadcast_tx(signed_tx)
        c = get_db()
        if tx_hash:
            c.execute(
                "UPDATE trade_executions SET tx_hash = ?, status = 'broadcasted', updated_at = ? WHERE id = ?",
                (tx_hash, now_iso(), tx_id),
            )
            c.execute(
                "UPDATE trade_decisions SET status = 'broadcasted', updated_at = ? WHERE id = ?",
                (now_iso(), decision_id),
            )
            c.commit()
            logging.info(f"[HYPER_CLAW] Broadcasted tx_id={tx_id} hash={tx_hash}")

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
                    (final_status, now_iso(), decision_id),
                )
                c2.commit()
                c2.close()

                if final_status == "executed" and tx_type == "approve_usdc":
                    logging.info("[HYPER_CLAW] USDC approval confirmed")
            else:
                logging.warning(f"[HYPER_CLAW] Receipt timeout for tx_id={tx_id}")
        else:
            c.execute(
                "UPDATE trade_executions SET status = 'broadcast_failed', error_msg = 'RPC error', updated_at = ? WHERE id = ?",
                (now_iso(), tx_id),
            )
            c.execute(
                "UPDATE trade_decisions SET status = 'failed', updated_at = ? WHERE id = ?",
                (now_iso(), decision_id),
            )
            c.commit()
        c.close()

    threading.Thread(target=do_broadcast, daemon=True).start()
    return success({"tx_id": tx_id, "status": "broadcasting"})


# ----- /rpc/decision -----

@app.route("/rpc/decision", methods=["POST"])
def rpc_decision():
    body = request.get_json(silent=True) or {}
    decision = (body.get("decision") or "").upper()

    if decision not in ("OPEN_LONG", "OPEN_SHORT", "CLOSE", "HOLD"):
        return error("decision must be OPEN_LONG, OPEN_SHORT, CLOSE, or HOLD")

    symbol = body.get("symbol", "")
    reason = body.get("reason", "")

    conn = get_db()
    cur = conn.execute(
        "INSERT INTO trade_decisions (decision, symbol, reason, status) VALUES (?, ?, ?, ?)",
        (decision, symbol, reason, "logged" if decision == "HOLD" else "pending"),
    )
    decision_id = cur.lastrowid
    conn.commit()
    conn.close()

    result = {"decision_id": decision_id, "decision": decision, "symbol": symbol}

    if decision in ("OPEN_LONG", "OPEN_SHORT"):
        # Validate
        if get_config_value("registered", "false").lower() != "true":
            return error("Account not registered — call /rpc/register first")
        if get_config_value("key_added", "false").lower() != "true":
            return error("Trading key not added — call /rpc/add_key first")

        allowed_symbols = [s.strip() for s in get_config_value("allowed_symbols", "").split(",") if s.strip()]
        if allowed_symbols and symbol not in allowed_symbols:
            return error(f"Symbol {symbol} not in allowed_symbols: {allowed_symbols}")

        conn2 = get_db()
        open_count = conn2.execute("SELECT COUNT(*) as c FROM positions").fetchone()["c"]
        conn2.close()
        max_positions = int(get_config_value("max_positions", "3"))
        if open_count >= max_positions:
            return error(f"Max positions ({max_positions}) reached")

        # Determine order params
        collateral = float(body.get("collateral", get_config_value("max_collateral_per_trade", "10")))
        max_collateral = float(get_config_value("max_collateral_per_trade", "10"))
        if collateral > max_collateral:
            return error(f"Collateral ${collateral} exceeds max ${max_collateral}")

        leverage = float(body.get("leverage", get_config_value("default_leverage", "5")))
        max_leverage = float(get_config_value("max_leverage", "20"))
        if leverage > max_leverage:
            return error(f"Leverage {leverage}x exceeds max {max_leverage}x")

        # Get current price
        current_price = get_current_price(symbol)
        if not current_price:
            return error(f"Cannot get price for {symbol}")

        # Calculate order quantity: collateral * leverage / price
        order_qty = round(collateral * leverage / current_price, 6)
        side = "BUY" if decision == "OPEN_LONG" else "SELL"

        try:
            order_result = place_order(
                symbol=symbol,
                side=side,
                order_type="MARKET",
                order_quantity=order_qty,
            )

            if order_result.get("success"):
                order_data = order_result.get("data", {})
                order_id = str(order_data.get("order_id", ""))

                conn3 = get_db()
                conn3.execute(
                    """INSERT INTO trade_executions
                         (decision_id, tx_type, order_id, status)
                       VALUES (?, 'open_trade', ?, 'executed')""",
                    (decision_id, order_id),
                )
                conn3.execute(
                    "UPDATE trade_decisions SET status = 'executed', updated_at = ? WHERE id = ?",
                    (now_iso(), decision_id),
                )
                conn3.commit()
                conn3.close()

                result["order_id"] = order_id
                result["price"] = current_price
                result["qty"] = order_qty
                result["leverage"] = leverage
                result["collateral"] = collateral
                result["side"] = side

                # Sync positions after a brief delay
                def delayed_sync():
                    time.sleep(2)
                    try:
                        sync_positions_from_orderly()
                    except Exception:
                        pass
                threading.Thread(target=delayed_sync, daemon=True).start()

            else:
                err_msg = json.dumps(order_result)
                conn3 = get_db()
                conn3.execute(
                    """INSERT INTO trade_executions
                         (decision_id, tx_type, status, error_msg)
                       VALUES (?, 'open_trade', 'failed', ?)""",
                    (decision_id, err_msg),
                )
                conn3.execute(
                    "UPDATE trade_decisions SET status = 'failed', updated_at = ? WHERE id = ?",
                    (now_iso(), decision_id),
                )
                conn3.commit()
                conn3.close()
                result["warning"] = f"Order failed: {err_msg}"

        except Exception as e:
            logging.error(f"[HYPER_CLAW] Order placement error: {e}")
            conn3 = get_db()
            conn3.execute(
                "UPDATE trade_decisions SET status = 'failed', updated_at = ? WHERE id = ?",
                (now_iso(), decision_id),
            )
            conn3.commit()
            conn3.close()
            result["warning"] = f"Order error: {e}"

    elif decision == "CLOSE":
        if get_config_value("key_added", "false").lower() != "true":
            return error("Trading key not added")

        if not symbol:
            return error("symbol is required for CLOSE")

        # Get position
        conn2 = get_db()
        pos = conn2.execute("SELECT * FROM positions WHERE symbol = ?", (symbol,)).fetchone()
        conn2.close()

        if not pos:
            return error(f"No open position for {symbol}")

        # Close = opposite side, reduce_only
        close_side = "SELL" if pos["side"] == "LONG" else "BUY"
        close_qty = float(pos["qty"])

        try:
            order_result = place_order(
                symbol=symbol,
                side=close_side,
                order_type="MARKET",
                order_quantity=close_qty,
                reduce_only=True,
            )

            if order_result.get("success"):
                order_data = order_result.get("data", {})
                order_id = str(order_data.get("order_id", ""))

                # Move to trade history
                close_price = get_current_price(symbol) or float(pos["current_price"] or pos["avg_open_price"])
                pnl, pnl_pct = calc_pnl(
                    pos["side"], float(pos["qty"]),
                    float(pos["avg_open_price"]), close_price,
                )

                conn3 = get_db()
                conn3.execute(
                    """INSERT INTO trade_executions
                         (decision_id, tx_type, order_id, status)
                       VALUES (?, 'close_trade', ?, 'executed')""",
                    (decision_id, order_id),
                )
                conn3.execute(
                    "UPDATE trade_decisions SET status = 'executed', updated_at = ? WHERE id = ?",
                    (now_iso(), decision_id),
                )
                conn3.execute(
                    """INSERT INTO trade_history
                         (symbol, side, qty, avg_open_price, close_price,
                          realized_pnl, pnl_pct, decision_id, order_id, opened_at)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    (pos["symbol"], pos["side"], pos["qty"],
                     pos["avg_open_price"], close_price, pnl, pnl_pct,
                     decision_id, order_id, pos.get("opened_at")),
                )
                conn3.execute("DELETE FROM positions WHERE symbol = ?", (symbol,))
                conn3.commit()
                conn3.close()

                result["order_id"] = order_id
                result["close_price"] = close_price
                result["realized_pnl"] = pnl
                result["pnl_pct"] = pnl_pct

                logging.info(f"[HYPER_CLAW] Position closed: {symbol} P&L=${pnl} ({pnl_pct}%)")
            else:
                err_msg = json.dumps(order_result)
                conn3 = get_db()
                conn3.execute(
                    "UPDATE trade_decisions SET status = 'failed', updated_at = ? WHERE id = ?",
                    (now_iso(), decision_id),
                )
                conn3.commit()
                conn3.close()
                result["warning"] = f"Close order failed: {err_msg}"

        except Exception as e:
            logging.error(f"[HYPER_CLAW] Close order error: {e}")
            result["warning"] = f"Close error: {e}"

    return success(result)


# ----- /rpc/deposit -----

@app.route("/rpc/deposit", methods=["POST"])
def rpc_deposit():
    """Construct a USDC vault deposit tx for Orderly."""
    body = request.get_json(silent=True) or {}
    amount = float(body.get("amount", 0))
    if amount <= 0:
        return error("amount must be > 0")

    trader_address = get_config_value("trader_address", "")
    if not trader_address:
        return error("trader_address not set in config")

    # First, approve USDC for the vault if needed
    approve_tx = build_usdc_approve_tx(trader_address, ORDERLY_VAULT_ADDRESS)
    deposit_tx = build_vault_deposit_tx(trader_address, amount)

    if not deposit_tx:
        return error("Failed to build deposit tx")

    conn = get_db()
    # Approval tx first
    if approve_tx:
        cur_a = conn.execute(
            "INSERT INTO trade_decisions (decision, symbol, reason, status) VALUES ('APPROVE_USDC', '', 'USDC approval for Orderly vault', 'pending')"
        )
        adec_id = cur_a.lastrowid
        cur_a2 = conn.execute(
            """INSERT INTO trade_executions
                 (decision_id, tx_type, raw_tx_to, raw_tx_data, raw_tx_value, raw_tx_gas, status)
               VALUES (?, 'approve_usdc', ?, ?, ?, ?, 'unsigned')""",
            (adec_id, approve_tx["to"], approve_tx["data"], approve_tx["value"], approve_tx["gas"]),
        )
        approve_tx_id = cur_a2.lastrowid
        conn.execute(
            "UPDATE trade_decisions SET status = 'tx_constructed', updated_at = ? WHERE id = ?",
            (now_iso(), adec_id),
        )

    # Deposit tx
    cur_d = conn.execute(
        "INSERT INTO trade_decisions (decision, symbol, reason, status) VALUES ('DEPOSIT', '', ?, 'pending')",
        (f"Deposit {amount} USDC to Orderly vault",),
    )
    ddec_id = cur_d.lastrowid
    cur_d2 = conn.execute(
        """INSERT INTO trade_executions
             (decision_id, tx_type, raw_tx_to, raw_tx_data, raw_tx_value, raw_tx_gas, status)
           VALUES (?, 'deposit', ?, ?, ?, ?, 'unsigned')""",
        (ddec_id, deposit_tx["to"], deposit_tx["data"], deposit_tx["value"], deposit_tx["gas"]),
    )
    deposit_tx_id = cur_d2.lastrowid
    conn.execute(
        "UPDATE trade_decisions SET status = 'tx_constructed', updated_at = ? WHERE id = ?",
        (now_iso(), ddec_id),
    )
    conn.commit()
    conn.close()

    chain_id = int(get_config_value("chain_id", str(DEFAULT_CHAIN_ID)))

    # Fire sign hook for approval first, then deposit
    if approve_tx:
        fire_hook("hyper_claw_sign_tx", {
            "tx_id": approve_tx_id,
            "decision_id": adec_id,
            "action": "approve_usdc",
            "to": approve_tx["to"],
            "data": approve_tx["data"],
            "value": approve_tx["value"],
            "gas": approve_tx["gas"],
            "chain_id": chain_id,
        })

    fire_hook("hyper_claw_sign_tx", {
        "tx_id": deposit_tx_id,
        "decision_id": ddec_id,
        "action": "deposit",
        "to": deposit_tx["to"],
        "data": deposit_tx["data"],
        "value": deposit_tx["value"],
        "gas": deposit_tx["gas"],
        "chain_id": chain_id,
        "amount_usdc": amount,
    })

    result = {"deposit_tx_id": deposit_tx_id, "deposit_tx": deposit_tx, "amount": amount}
    if approve_tx:
        result["approve_tx_id"] = approve_tx_id
        result["approve_tx"] = approve_tx
    return success(result)


# ----- /rpc/positions -----

@app.route("/rpc/positions", methods=["GET"])
def rpc_positions():
    conn = get_db()
    rows = conn.execute("SELECT * FROM positions ORDER BY opened_at DESC").fetchall()
    conn.close()
    return success([dict(r) for r in rows])


# ----- /rpc/orders -----

@app.route("/rpc/orders", methods=["GET"])
def rpc_orders():
    try:
        orders = get_orderly_orders()
        return success(orders)
    except Exception as e:
        return error(f"Failed to get orders: {e}")


# ----- /rpc/account -----

@app.route("/rpc/account", methods=["GET"])
def rpc_account():
    try:
        account = get_orderly_account()
        balances = get_orderly_balances()
        return success({"account": account, "balances": balances})
    except Exception as e:
        return error(f"Failed to get account: {e}")


# ----- /rpc/refresh -----

@app.route("/rpc/refresh", methods=["POST"])
def rpc_refresh():
    result = sync_positions_from_orderly()
    conn = get_db()
    rows = conn.execute("SELECT * FROM positions ORDER BY opened_at DESC").fetchall()
    conn.close()
    return success({
        "sync_result": result,
        "positions": [dict(r) for r in rows],
    })


# ----- /rpc/pnl -----

@app.route("/rpc/pnl", methods=["GET"])
def rpc_pnl():
    return success(compute_pnl())


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
    failed = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE status IN ('failed','reverted','broadcast_failed')").fetchone()["c"]
    open_positions = conn.execute("SELECT COUNT(*) as c FROM positions").fetchone()["c"]
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
        # Mask the secret key
        config = {}
        for r in rows:
            k, v = r["key"], r["value"]
            if k == "orderly_secret" and v:
                config[k] = v[:8] + "..." + v[-4:]
            else:
                config[k] = v
        return success(config)

    body = request.get_json(silent=True) or {}
    key = body.get("key")
    value = body.get("value")
    if not key or value is None:
        return error("key and value are required")
    allowed_keys = {
        "pulse_interval", "enabled", "trader_address",
        "max_collateral_per_trade", "max_leverage", "default_leverage",
        "max_positions", "take_profit_pct", "stop_loss_pct",
        "bankr_min_confidence", "bankr_providers",
        "allowed_symbols", "chain_id",
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


# ----- /rpc/symbols -----

@app.route("/rpc/symbols", methods=["GET"])
def rpc_symbols():
    symbols = get_available_symbols()
    allowed_raw = get_config_value("allowed_symbols", "")
    allowed = {s.strip() for s in allowed_raw.split(",") if s.strip()} if allowed_raw else set()
    result = []
    for s in symbols:
        name = s.get("symbol", "")
        result.append({
            "symbol": name,
            "base": s.get("base", ""),
            "quote": s.get("quote", ""),
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
                     (id, decision, symbol, reason, status, created_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?)""",
                (d["id"], d["decision"], d.get("symbol"), d.get("reason"),
                 d.get("status", "pending"), d.get("created_at"), d.get("updated_at")),
            )
            restored += 1
        except Exception:
            pass

    for e in data.get("executions", []):
        try:
            conn.execute(
                """INSERT OR REPLACE INTO trade_executions
                     (id, decision_id, tx_type, order_id, raw_tx_to, raw_tx_data,
                      raw_tx_value, raw_tx_gas, signed_tx, tx_hash, status, error_msg,
                      created_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (e["id"], e["decision_id"], e.get("tx_type"), e.get("order_id"),
                 e.get("raw_tx_to"), e.get("raw_tx_data"), e.get("raw_tx_value"),
                 e.get("raw_tx_gas"), e.get("signed_tx"), e.get("tx_hash"),
                 e.get("status", "pending"), e.get("error_msg"),
                 e.get("created_at"), e.get("updated_at")),
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
                     (id, symbol, side, qty, avg_open_price, current_price,
                      mark_price, unrealized_pnl, pnl_pct, leverage, collateral,
                      liquidation_price, decision_id, opened_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (p["id"], p["symbol"], p["side"], p["qty"],
                 p["avg_open_price"], p.get("current_price"),
                 p.get("mark_price"), p.get("unrealized_pnl", 0),
                 p.get("pnl_pct", 0), p.get("leverage"), p.get("collateral"),
                 p.get("liquidation_price"), p.get("decision_id"),
                 p.get("opened_at"), p.get("updated_at")),
            )
            restored += 1
        except Exception:
            pass

    for h in data.get("trade_history", []):
        try:
            conn.execute(
                """INSERT OR REPLACE INTO trade_history
                     (id, symbol, side, qty, avg_open_price, close_price,
                      realized_pnl, pnl_pct, fee, decision_id, order_id,
                      opened_at, closed_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (h["id"], h["symbol"], h["side"], h.get("qty"),
                 h.get("avg_open_price"), h.get("close_price"),
                 h.get("realized_pnl"), h.get("pnl_pct"), h.get("fee", 0),
                 h.get("decision_id"), h.get("order_id"),
                 h.get("opened_at"), h.get("closed_at")),
            )
            restored += 1
        except Exception:
            pass

    conn.commit()
    conn.close()
    return success({"restored": restored})


# ---------------------------------------------------------------------------
# Dashboard (HTML + TUI)
# ---------------------------------------------------------------------------

from starkbot_sdk.dashboard import register_dashboard  # noqa: E402
from dashboard import HyperClawDashboard  # noqa: E402

PORT = int(os.environ.get("MODULE_PORT", os.environ.get("HYPER_CLAW_PORT", "9111")))
register_dashboard(app, HyperClawDashboard, module_url=f"http://127.0.0.1:{PORT}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    logging.getLogger("werkzeug").setLevel(logging.ERROR)
    init_db()
    port = int(os.environ.get("MODULE_PORT", os.environ.get("HYPER_CLAW_PORT", "9111")))
    if get_config_value("enabled", "true").lower() == "true":
        start_worker()
    app.run(host="127.0.0.1", port=port)
