# /// script
# requires-python = ">=3.12"
# dependencies = ["requests", "starkbot-sdk", "web3"]
#
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
Spot Trader module — autonomous DeFi trader on Base.

Scans DexScreener for trending/new tokens, makes trade decisions via
the spot_trader agent persona, constructs swap transactions using the
0x Swap API, and broadcasts signed transactions on-chain.

RPC protocol endpoints:
  GET  /rpc/status         -> service health
  POST /rpc/decision       -> agent submits BUY/SELL/HOLD decision
  POST /rpc/sign           -> agent submits signed tx hex for broadcast
  POST /rpc/history        -> query trade decision history
  GET  /rpc/stats          -> aggregate trading statistics
  GET  /rpc/pnl            -> aggregate P&L summary
  POST /rpc/refresh        -> refresh portfolio prices from DexScreener
  GET  /rpc/trade_history  -> closed/open trade records with P&L
  GET  /rpc/config         -> view trader config
  POST /rpc/config         -> update trader config
  POST /rpc/control        -> start/stop/trigger trading loop
  GET  /rpc/portfolio      -> current token holdings with P&L
  POST /rpc/backup/export  -> export data for backup
  POST /rpc/backup/restore -> restore data from backup
  GET  /                   -> HTML dashboard

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

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

DB_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "spot_trader.db")
BACKEND_URL = os.environ.get("STARKBOT_BACKEND_URL", "http://127.0.0.1:8080")
INTERNAL_TOKEN = os.environ.get("STARKBOT_INTERNAL_TOKEN", "")
ALCHEMY_API_KEY = os.environ.get("ALCHEMY_API_KEY", "")
ZEROX_API_KEY = os.environ.get("ZEROX_API_KEY", "")

BASE_RPC_URL = f"https://base-mainnet.g.alchemy.com/v2/{ALCHEMY_API_KEY}" if ALCHEMY_API_KEY else ""
BASE_CHAIN_ID = 8453
WETH_BASE = "0x4200000000000000000000000000000000000006"

# 0x Swap API v2 (Permit2)
ZEROX_SWAP_URL = "https://api.0x.org/swap/permit2/quote"

# Defaults
DEFAULT_PULSE_INTERVAL = 240  # 4 minutes
DEFAULT_MAX_TRADE_USD = "20"

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
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            decision        TEXT    NOT NULL,
            token_address   TEXT,
            token_symbol    TEXT,
            reason          TEXT,
            status          TEXT    NOT NULL DEFAULT 'pending',
            created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
            updated_at      TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS trade_executions (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            decision_id     INTEGER NOT NULL REFERENCES trade_decisions(id),
            raw_tx_to       TEXT,
            raw_tx_data     TEXT,
            raw_tx_value    TEXT,
            raw_tx_gas      TEXT,
            signed_tx       TEXT,
            tx_hash         TEXT,
            status          TEXT    NOT NULL DEFAULT 'unsigned',
            error_msg       TEXT,
            created_at      TEXT    NOT NULL DEFAULT (datetime('now')),
            updated_at      TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS trader_config (
            key     TEXT PRIMARY KEY,
            value   TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS portfolio (
            token_address   TEXT PRIMARY KEY,
            token_symbol    TEXT,
            amount_raw      TEXT    NOT NULL DEFAULT '0',
            avg_buy_price   REAL,
            last_tx_hash    TEXT,
            updated_at      TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS trade_history (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            token_address   TEXT NOT NULL,
            token_symbol    TEXT,
            side            TEXT NOT NULL,
            amount          TEXT,
            price_usd       REAL,
            value_usd       REAL,
            realized_pnl    REAL,
            decision_id     INTEGER,
            tx_hash         TEXT,
            created_at      TEXT NOT NULL DEFAULT (datetime('now'))
        );
    """)

    # Migrate: add new columns to existing tables (safe to re-run)
    _migrate_columns = [
        ("trade_executions", "sell_token", "TEXT"),
        ("trade_executions", "buy_token", "TEXT"),
        ("trade_executions", "sell_amount", "TEXT"),
        ("trade_executions", "buy_amount", "TEXT"),
        ("trade_executions", "price", "REAL"),
        ("trade_executions", "gas_price", "TEXT"),
        ("portfolio", "decimals", "INTEGER DEFAULT 18"),
        ("portfolio", "total_cost_usd", "REAL DEFAULT 0"),
        ("portfolio", "current_price_usd", "REAL"),
        ("portfolio", "unrealized_pnl_usd", "REAL DEFAULT 0"),
        ("portfolio", "entry_timestamp", "TEXT"),
        ("portfolio", "num_buys", "INTEGER DEFAULT 0"),
    ]
    for table, col, col_type in _migrate_columns:
        try:
            conn.execute(f"ALTER TABLE {table} ADD COLUMN {col} {col_type}")
        except Exception:
            pass  # column already exists

    # Seed defaults if not present
    for k, v in [
        ("pulse_interval", str(DEFAULT_PULSE_INTERVAL)),
        ("max_trade_usd", DEFAULT_MAX_TRADE_USD),
        ("chain", "base"),
        ("enabled", "true"),
        ("weth_address", WETH_BASE),
        ("signal_mode", "dexscreener"),
        ("bankr_min_confidence", "70"),
        ("bankr_providers", ""),
        ("eth_price_usd", "0"),
        ("simulation_mode", "partner"),
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
# 0x Swap API helpers
# ---------------------------------------------------------------------------

def get_swap_quote(sell_token: str, buy_token: str, sell_amount: str) -> dict | None:
    """Get a swap quote from 0x API for Base chain."""
    if not ZEROX_API_KEY:
        return None
    headers = {"0x-api-key": ZEROX_API_KEY, "0x-chain-id": str(BASE_CHAIN_ID)}
    params = {
        "chainId": BASE_CHAIN_ID,
        "sellToken": sell_token,
        "buyToken": buy_token,
        "sellAmount": sell_amount,
    }
    try:
        resp = http_requests.get(ZEROX_SWAP_URL, params=params, headers=headers, timeout=15)
        if resp.status_code == 200:
            return resp.json()
        logging.warning(f"[SPOT_TRADER] 0x quote failed ({resp.status_code}): {resp.text[:200]}")
    except Exception as e:
        logging.error(f"[SPOT_TRADER] 0x quote error: {e}")
    return None


def construct_swap_tx(decision: str, token_address: str, trade_amount_wei: str) -> dict | None:
    """Construct an unsigned swap tx via 0x API.

    BUY:  WETH -> token  (sell WETH, buy token)
    SELL: token -> WETH   (sell token, buy WETH)
    """
    if decision == "BUY":
        quote = get_swap_quote(WETH_BASE, token_address, trade_amount_wei)
    elif decision == "SELL":
        quote = get_swap_quote(token_address, WETH_BASE, trade_amount_wei)
    else:
        return None

    if not quote:
        return None

    tx = quote.get("transaction") or quote.get("tx")
    if not tx:
        return None

    return {
        "to": tx.get("to", ""),
        "data": tx.get("data", "0x"),
        "value": tx.get("value", "0"),
        "gas": tx.get("gas") or tx.get("gasLimit") or "350000",
        "sell_token": quote.get("sellToken", ""),
        "buy_token": quote.get("buyToken", ""),
        "sell_amount": quote.get("sellAmount", ""),
        "buy_amount": quote.get("buyAmount", ""),
        "price": quote.get("price", ""),
        "gas_price": quote.get("gasPrice", ""),
    }


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
        logging.error(f"[SPOT_TRADER] Broadcast error: {err}")
    except Exception as e:
        logging.error(f"[SPOT_TRADER] Broadcast exception: {e}")
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
        logging.warning("[SPOT_TRADER] No STARKBOT_INTERNAL_TOKEN — cannot fire hooks")
        return
    try:
        http_requests.post(
            f"{BACKEND_URL}/api/internal/hooks/fire",
            json={"event": event, "data": data or {}},
            headers={"X-Internal-Token": INTERNAL_TOKEN},
            timeout=10,
        )
    except Exception as e:
        logging.error(f"[SPOT_TRADER] Hook fire error: {e}")


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
            logging.warning(f"[SPOT_TRADER] Bankr signals API returned {resp.status_code}")
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
        logging.error(f"[SPOT_TRADER] Bankr signals fetch error: {e}")
        return []

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
        results.append(sig)

    logging.info(f"[SPOT_TRADER] Bankr signals: {len(results)} qualifying out of {len(signals)} total")
    return results


# ---------------------------------------------------------------------------
# Background pulse worker
# ---------------------------------------------------------------------------

def _build_portfolio_summary() -> dict:
    """Build portfolio summary with risk alerts for the pulse hook."""
    conn = get_db()
    positions = conn.execute(
        "SELECT token_address, token_symbol, total_cost_usd, current_price_usd, unrealized_pnl_usd, amount_raw FROM portfolio"
    ).fetchall()
    conn.close()

    max_positions = 3  # matches agent.md strategy
    open_count = len(positions)
    total_unrealized = 0
    stop_loss_alerts = []
    take_profit_alerts = []

    for p in positions:
        cost = float(p["total_cost_usd"] or 0)
        unrealized = float(p["unrealized_pnl_usd"] or 0)
        total_unrealized += unrealized

        if cost > 0:
            pnl_pct = round((unrealized / cost) * 100, 1)
        else:
            pnl_pct = 0

        info = {"token": p["token_symbol"], "address": p["token_address"], "pnl_pct": pnl_pct, "unrealized_usd": round(unrealized, 2)}

        # Stop-loss: -40%
        if pnl_pct <= -40:
            stop_loss_alerts.append(info)
        # Take-profit: +100% (2x)
        if pnl_pct >= 100:
            take_profit_alerts.append(info)

    return {
        "open_positions": open_count,
        "max_positions": max_positions,
        "can_buy": open_count < max_positions,
        "total_unrealized_pnl": round(total_unrealized, 2),
        "stop_loss_alerts": stop_loss_alerts,
        "take_profit_alerts": take_profit_alerts,
    }


def _fire_pulse(mode: str):
    """Build full pulse data with risk context and fire the hook."""
    # Refresh prices before computing risk
    try:
        refresh_portfolio_prices()
    except Exception as e:
        logging.warning(f"[SPOT_TRADER] Price refresh failed during pulse: {e}")

    portfolio_summary = _build_portfolio_summary()
    pnl = compute_pnl()

    simulation_mode = get_config_value("simulation_mode", "partner")
    hook_data = {
        "signal_mode": mode,
        "simulation_mode": simulation_mode,
        "portfolio_summary": portfolio_summary,
        "pnl": pnl,
    }

    if mode == "bankr":
        hook_data["bankr_signals"] = fetch_bankr_signals()
        logging.info(f"[SPOT_TRADER] Firing pulse (bankr mode, {len(hook_data['bankr_signals'])} signals, {portfolio_summary['open_positions']} positions)")
    else:
        logging.info(f"[SPOT_TRADER] Firing pulse (dexscreener mode, {portfolio_summary['open_positions']} positions)")

    fire_hook("spot_trader_pulse", hook_data)


def pulse_worker():
    global _last_pulse_at, _worker_running
    logger = logging.getLogger("spot_trader.worker")
    logger.info("[SPOT_TRADER] Pulse worker started")
    # Short initial delay
    time.sleep(10)
    while _worker_running:
        interval = int(get_config_value("pulse_interval", str(DEFAULT_PULSE_INTERVAL)))
        enabled = get_config_value("enabled", "true").lower() == "true"
        if enabled:
            mode = get_config_value("signal_mode", "dexscreener")
            _fire_pulse(mode)
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
        "simulation_mode": get_config_value("simulation_mode", "partner"),
    }


app = create_app("spot_trader", status_extra_fn=extra_status)


# ----- /rpc/decision -----

@app.route("/rpc/decision", methods=["POST"])
def rpc_decision():
    body = request.get_json(silent=True) or {}
    decision = (body.get("decision") or "").upper()
    if decision not in ("BUY", "SELL", "HOLD"):
        return error("decision must be BUY, SELL, or HOLD")

    token_address = body.get("token_address", "")
    token_symbol = body.get("token_symbol", "")
    reason = body.get("reason", "")

    conn = get_db()
    cur = conn.execute(
        "INSERT INTO trade_decisions (decision, token_address, token_symbol, reason, status) VALUES (?, ?, ?, ?, ?)",
        (decision, token_address, token_symbol, reason, "logged" if decision == "HOLD" else "pending"),
    )
    decision_id = cur.lastrowid
    conn.commit()

    result = {"decision_id": decision_id, "decision": decision, "token_symbol": token_symbol}

    if decision in ("BUY", "SELL"):
        # Construct swap tx via 0x API
        max_trade_usd = float(get_config_value("max_trade_usd", DEFAULT_MAX_TRADE_USD))
        # Approximate: $20 ≈ 0.006 ETH ≈ 6e15 wei at ~$3300/ETH (rough default)
        trade_amount_wei = str(int(max_trade_usd / 3300 * 1e18))

        tx = construct_swap_tx(decision, token_address, trade_amount_wei)
        if tx:
            conn2 = get_db()
            cur2 = conn2.execute(
                "INSERT INTO trade_executions (decision_id, raw_tx_to, raw_tx_data, raw_tx_value, raw_tx_gas, sell_token, buy_token, sell_amount, buy_amount, price, gas_price, status) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'unsigned')",
                (decision_id, tx["to"], tx["data"], tx["value"], tx["gas"],
                 tx.get("sell_token"), tx.get("buy_token"), tx.get("sell_amount"),
                 tx.get("buy_amount"), tx.get("price"), tx.get("gas_price")),
            )
            tx_id = cur2.lastrowid
            conn2.execute(
                "UPDATE trade_decisions SET status = 'tx_constructed' WHERE id = ?",
                (decision_id,),
            )
            conn2.commit()
            conn2.close()

            result["tx_id"] = tx_id
            result["tx"] = tx

            # Fire sign hook so the agent signs the tx
            fire_hook("spot_trader_sign_tx", {
                "tx_id": tx_id,
                "decision_id": decision_id,
                "decision": decision,
                "token_symbol": token_symbol,
                "to": tx["to"],
                "data": tx["data"],
                "value": tx["value"],
                "gas": tx["gas"],
                "chain_id": BASE_CHAIN_ID,
            })
        else:
            conn3 = get_db()
            conn3.execute(
                "UPDATE trade_decisions SET status = 'quote_failed' WHERE id = ?",
                (decision_id,),
            )
            conn3.commit()
            conn3.close()
            result["warning"] = "Failed to get swap quote from 0x API"

    conn.close()
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

    # Store signed tx
    conn.execute(
        "UPDATE trade_executions SET signed_tx = ?, status = 'signed', updated_at = ? WHERE id = ?",
        (signed_tx, now_iso(), tx_id),
    )
    conn.commit()
    conn.close()

    simulation_mode = get_config_value("simulation_mode", "partner")

    if simulation_mode == "partner":
        # Paper trading — simulate execution without broadcasting
        def do_simulate():
            paper_hash = f"0xPAPER{tx_id:060d}"
            c = get_db()
            c.execute(
                "UPDATE trade_executions SET tx_hash = ?, status = 'executed', updated_at = ? WHERE id = ?",
                (paper_hash, now_iso(), tx_id),
            )
            c.execute(
                "UPDATE trade_decisions SET status = 'executed', updated_at = ? WHERE id = (SELECT decision_id FROM trade_executions WHERE id = ?)",
                (now_iso(), tx_id),
            )
            c.commit()
            c.close()
            _update_portfolio_after_trade(tx_id, paper_hash)
            logging.info(f"[SPOT_TRADER] Paper trade executed tx_id={tx_id} hash={paper_hash}")

        threading.Thread(target=do_simulate, daemon=True).start()
        return success({"tx_id": tx_id, "status": "simulated (partner mode)"})

    # Rogue mode — real broadcast
    def do_broadcast():
        tx_hash = broadcast_tx(signed_tx)
        c = get_db()
        if tx_hash:
            c.execute(
                "UPDATE trade_executions SET tx_hash = ?, status = 'broadcasted', updated_at = ? WHERE id = ?",
                (tx_hash, now_iso(), tx_id),
            )
            c.execute(
                "UPDATE trade_decisions SET status = 'broadcasted', updated_at = ? WHERE id = (SELECT decision_id FROM trade_executions WHERE id = ?)",
                (now_iso(), tx_id),
            )
            c.commit()
            logging.info(f"[SPOT_TRADER] Broadcasted tx_id={tx_id} hash={tx_hash}")

            # Poll for receipt
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
                    "UPDATE trade_decisions SET status = ?, updated_at = ? WHERE id = (SELECT decision_id FROM trade_executions WHERE id = ?)",
                    (final_status, now_iso(), tx_id),
                )
                c2.commit()
                c2.close()

                # Update portfolio on successful BUY
                if final_status == "executed":
                    _update_portfolio_after_trade(tx_id, tx_hash)
            else:
                logging.warning(f"[SPOT_TRADER] Receipt timeout for tx_id={tx_id}")
        else:
            c.execute(
                "UPDATE trade_executions SET status = 'broadcast_failed', error_msg = 'RPC error', updated_at = ? WHERE id = ?",
                (now_iso(), tx_id),
            )
            c.execute(
                "UPDATE trade_decisions SET status = 'failed', updated_at = ? WHERE id = (SELECT decision_id FROM trade_executions WHERE id = ?)",
                (now_iso(), tx_id),
            )
            c.commit()
        c.close()

    threading.Thread(target=do_broadcast, daemon=True).start()
    return success({"tx_id": tx_id, "status": "broadcasting"})


def _update_portfolio_after_trade(tx_id: int, tx_hash: str):
    """Update portfolio and trade_history after a confirmed trade."""
    conn = get_db()
    row = conn.execute(
        """SELECT d.decision, d.token_address, d.token_symbol,
                  e.sell_amount, e.buy_amount, e.price
           FROM trade_decisions d
           JOIN trade_executions e ON e.decision_id = d.id
           WHERE e.id = ?""",
        (tx_id,),
    ).fetchone()
    if not row:
        conn.close()
        return

    decision = row["decision"]
    token_address = row["token_address"]
    token_symbol = row["token_symbol"]
    sell_amount = row["sell_amount"] or "0"
    buy_amount = row["buy_amount"] or "0"
    price_raw = row["price"]

    # Estimate USD value using cached ETH price
    eth_price = float(get_config_value("eth_price_usd", "0") or "0")
    decision_id = conn.execute(
        "SELECT decision_id FROM trade_executions WHERE id = ?", (tx_id,)
    ).fetchone()["decision_id"]

    if decision == "BUY":
        # sell_amount is WETH in wei — convert to ETH then USD
        sell_eth = int(sell_amount) / 1e18 if sell_amount.isdigit() else 0
        value_usd = sell_eth * eth_price if eth_price > 0 else 0
        # price from 0x = buy_token per sell_token
        token_price_usd = (eth_price / float(price_raw)) if price_raw and float(price_raw) > 0 else 0

        existing = conn.execute(
            "SELECT amount_raw, avg_buy_price, total_cost_usd, num_buys FROM portfolio WHERE token_address = ?",
            (token_address,),
        ).fetchone()

        if existing:
            old_amount = int(existing["amount_raw"] or "0")
            old_cost = float(existing["total_cost_usd"] or 0)
            old_buys = int(existing["num_buys"] or 0)
            new_amount = old_amount + int(buy_amount) if buy_amount.isdigit() else old_amount + 1
            new_cost = old_cost + value_usd
            # Weighted average buy price
            new_avg = new_cost / new_amount * 1e18 if new_amount > 0 else token_price_usd
            conn.execute(
                """UPDATE portfolio SET
                     amount_raw = ?, avg_buy_price = ?, total_cost_usd = ?,
                     num_buys = ?, last_tx_hash = ?, updated_at = ?
                   WHERE token_address = ?""",
                (str(new_amount), new_avg, new_cost, old_buys + 1, tx_hash, now_iso(), token_address),
            )
        else:
            amount = buy_amount if buy_amount.isdigit() else "1"
            conn.execute(
                """INSERT INTO portfolio
                     (token_address, token_symbol, amount_raw, avg_buy_price,
                      total_cost_usd, num_buys, entry_timestamp, last_tx_hash, updated_at)
                   VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?)""",
                (token_address, token_symbol, amount, token_price_usd,
                 value_usd, now_iso(), tx_hash, now_iso()),
            )

        # Record in trade_history
        conn.execute(
            """INSERT INTO trade_history
                 (token_address, token_symbol, side, amount, price_usd, value_usd, decision_id, tx_hash)
               VALUES (?, ?, 'BUY', ?, ?, ?, ?, ?)""",
            (token_address, token_symbol, buy_amount, token_price_usd, value_usd, decision_id, tx_hash),
        )

    elif decision == "SELL":
        # buy_amount is WETH received in wei
        buy_eth = int(buy_amount) / 1e18 if buy_amount.isdigit() else 0
        sell_value_usd = buy_eth * eth_price if eth_price > 0 else 0

        existing = conn.execute(
            "SELECT avg_buy_price, total_cost_usd, amount_raw FROM portfolio WHERE token_address = ?",
            (token_address,),
        ).fetchone()

        realized_pnl = None
        cost_basis = 0
        if existing:
            cost_basis = float(existing["total_cost_usd"] or 0)
            realized_pnl = sell_value_usd - cost_basis

        # Record in trade_history
        conn.execute(
            """INSERT INTO trade_history
                 (token_address, token_symbol, side, amount, price_usd, value_usd, realized_pnl, decision_id, tx_hash)
               VALUES (?, ?, 'SELL', ?, ?, ?, ?, ?, ?)""",
            (token_address, token_symbol, sell_amount,
             eth_price if eth_price > 0 else None, sell_value_usd, realized_pnl, decision_id, tx_hash),
        )

        # Remove position
        conn.execute("DELETE FROM portfolio WHERE token_address = ?", (token_address,))

    conn.commit()
    conn.close()


# ----- Price refresh helpers -----

DEXSCREENER_TOKEN_URL = "https://api.dexscreener.com/latest/dex/tokens"


def _fetch_token_price_usd(token_address: str) -> float | None:
    """Fetch current USD price for a token via DexScreener."""
    try:
        resp = http_requests.get(f"{DEXSCREENER_TOKEN_URL}/{token_address}", timeout=10)
        if resp.status_code != 200:
            return None
        data = resp.json()
        pairs = data.get("pairs") or []
        if not pairs:
            return None
        # Use the pair with highest liquidity
        best = max(pairs, key=lambda p: float(p.get("liquidity", {}).get("usd", 0) or 0))
        return float(best.get("priceUsd", 0))
    except Exception as e:
        logging.warning(f"[SPOT_TRADER] DexScreener price fetch error for {token_address}: {e}")
        return None


def refresh_eth_price():
    """Update cached ETH price from DexScreener WETH pair."""
    price = _fetch_token_price_usd(WETH_BASE)
    if price and price > 0:
        set_config_value("eth_price_usd", str(price))
        return price
    return float(get_config_value("eth_price_usd", "0") or "0")


def refresh_portfolio_prices():
    """Refresh current_price_usd and unrealized_pnl_usd for all open positions."""
    eth_price = refresh_eth_price()
    conn = get_db()
    rows = conn.execute("SELECT token_address, total_cost_usd, amount_raw FROM portfolio").fetchall()
    for row in rows:
        addr = row["token_address"]
        price = _fetch_token_price_usd(addr)
        if price is not None:
            amount_raw = int(row["amount_raw"] or "0")
            # Estimate current value: price * amount (normalized from raw wei)
            # For tokens with 18 decimals: value = price * amount_raw / 1e18
            current_value = price * amount_raw / 1e18
            cost = float(row["total_cost_usd"] or 0)
            unrealized = current_value - cost
            conn.execute(
                "UPDATE portfolio SET current_price_usd = ?, unrealized_pnl_usd = ?, updated_at = ? WHERE token_address = ?",
                (price, unrealized, now_iso(), addr),
            )
    conn.commit()
    conn.close()
    return eth_price


def compute_pnl() -> dict:
    """Compute aggregate P&L from trade_history and portfolio."""
    conn = get_db()

    # Realized P&L from closed trades
    sells = conn.execute(
        "SELECT token_symbol, realized_pnl FROM trade_history WHERE side = 'SELL' AND realized_pnl IS NOT NULL"
    ).fetchall()
    total_realized = sum(float(s["realized_pnl"] or 0) for s in sells)
    win_count = sum(1 for s in sells if float(s["realized_pnl"] or 0) > 0)
    loss_count = sum(1 for s in sells if float(s["realized_pnl"] or 0) < 0)

    best_trade = max(sells, key=lambda s: float(s["realized_pnl"] or 0), default=None)
    worst_trade = min(sells, key=lambda s: float(s["realized_pnl"] or 0), default=None)

    # Unrealized P&L from open positions
    positions = conn.execute("SELECT unrealized_pnl_usd FROM portfolio").fetchall()
    total_unrealized = sum(float(p["unrealized_pnl_usd"] or 0) for p in positions)

    total_trades = len(sells)
    conn.close()

    return {
        "total_realized_pnl": round(total_realized, 2),
        "total_unrealized_pnl": round(total_unrealized, 2),
        "total_pnl": round(total_realized + total_unrealized, 2),
        "win_count": win_count,
        "loss_count": loss_count,
        "win_rate": round(win_count / total_trades, 3) if total_trades > 0 else 0,
        "total_trades": total_trades,
        "best_trade": {"token": best_trade["token_symbol"], "pnl": round(float(best_trade["realized_pnl"]), 2)} if best_trade else None,
        "worst_trade": {"token": worst_trade["token_symbol"], "pnl": round(float(worst_trade["realized_pnl"]), 2)} if worst_trade else None,
    }


# ----- /rpc/pnl -----

@app.route("/rpc/pnl", methods=["GET"])
def rpc_pnl():
    return success(compute_pnl())


# ----- /rpc/refresh -----

@app.route("/rpc/refresh", methods=["POST"])
def rpc_refresh():
    eth_price = refresh_portfolio_prices()
    conn = get_db()
    rows = conn.execute("SELECT * FROM portfolio ORDER BY updated_at DESC").fetchall()
    conn.close()
    return success({
        "eth_price_usd": eth_price,
        "positions_refreshed": len(rows),
        "portfolio": [dict(r) for r in rows],
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
    buys = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE decision='BUY'").fetchone()["c"]
    sells = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE decision='SELL'").fetchone()["c"]
    holds = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE decision='HOLD'").fetchone()["c"]
    executed = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE status='executed'").fetchone()["c"]
    failed = conn.execute("SELECT COUNT(*) as c FROM trade_decisions WHERE status IN ('failed','reverted','broadcast_failed','quote_failed')").fetchone()["c"]
    conn.close()
    return success({
        "total_decisions": total,
        "buys": buys,
        "sells": sells,
        "holds": holds,
        "executed": executed,
        "failed": failed,
    })


# ----- /rpc/trade_history -----

@app.route("/rpc/trade_history", methods=["GET"])
def rpc_trade_history():
    limit = int(request.args.get("limit", 50))
    conn = get_db()
    rows = conn.execute(
        "SELECT * FROM trade_history ORDER BY created_at DESC LIMIT ?", (limit,)
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
    allowed_keys = {"pulse_interval", "max_trade_usd", "chain", "enabled", "weth_address",
                     "signal_mode", "bankr_min_confidence", "bankr_providers", "eth_price_usd",
                     "simulation_mode"}
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
        mode = get_config_value("signal_mode", "dexscreener")
        _fire_pulse(mode)
        return success({"action": "trigger", "fired": True, "signal_mode": mode})
    else:
        return error("action must be 'start', 'stop', or 'trigger'")


# ----- /rpc/portfolio -----

@app.route("/rpc/portfolio", methods=["GET"])
def rpc_portfolio():
    conn = get_db()
    rows = conn.execute("SELECT * FROM portfolio ORDER BY updated_at DESC").fetchall()
    conn.close()
    return success([dict(r) for r in rows])


# ----- /rpc/backup -----

@app.route("/rpc/backup/export", methods=["POST"])
def rpc_backup_export():
    conn = get_db()
    decisions = conn.execute("SELECT * FROM trade_decisions ORDER BY id").fetchall()
    executions = conn.execute("SELECT * FROM trade_executions ORDER BY id").fetchall()
    config = conn.execute("SELECT * FROM trader_config").fetchall()
    portfolio = conn.execute("SELECT * FROM portfolio").fetchall()
    history = conn.execute("SELECT * FROM trade_history ORDER BY id").fetchall()
    conn.close()
    return success({
        "decisions": [dict(r) for r in decisions],
        "executions": [dict(r) for r in executions],
        "config": {r["key"]: r["value"] for r in config},
        "portfolio": [dict(r) for r in portfolio],
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
                "INSERT OR REPLACE INTO trade_decisions (id, decision, token_address, token_symbol, reason, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (d["id"], d["decision"], d.get("token_address"), d.get("token_symbol"), d.get("reason"), d.get("status", "pending"), d.get("created_at"), d.get("updated_at")),
            )
            restored += 1
        except Exception:
            pass

    for e in data.get("executions", []):
        try:
            conn.execute(
                "INSERT OR REPLACE INTO trade_executions (id, decision_id, raw_tx_to, raw_tx_data, raw_tx_value, raw_tx_gas, signed_tx, tx_hash, status, error_msg, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (e["id"], e["decision_id"], e.get("raw_tx_to"), e.get("raw_tx_data"), e.get("raw_tx_value"), e.get("raw_tx_gas"), e.get("signed_tx"), e.get("tx_hash"), e.get("status", "unsigned"), e.get("error_msg"), e.get("created_at"), e.get("updated_at")),
            )
            restored += 1
        except Exception:
            pass

    for k, v in data.get("config", {}).items():
        set_config_value(k, v)

    for p in data.get("portfolio", []):
        try:
            conn.execute(
                """INSERT OR REPLACE INTO portfolio
                     (token_address, token_symbol, amount_raw, avg_buy_price, last_tx_hash, updated_at,
                      decimals, total_cost_usd, current_price_usd, unrealized_pnl_usd, entry_timestamp, num_buys)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (p["token_address"], p.get("token_symbol"), p.get("amount_raw", "0"), p.get("avg_buy_price"),
                 p.get("last_tx_hash"), p.get("updated_at"),
                 p.get("decimals", 18), p.get("total_cost_usd", 0), p.get("current_price_usd"),
                 p.get("unrealized_pnl_usd", 0), p.get("entry_timestamp"), p.get("num_buys", 0)),
            )
            restored += 1
        except Exception:
            pass

    for h in data.get("trade_history", []):
        try:
            conn.execute(
                """INSERT OR REPLACE INTO trade_history
                     (id, token_address, token_symbol, side, amount, price_usd, value_usd,
                      realized_pnl, decision_id, tx_hash, created_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (h["id"], h["token_address"], h.get("token_symbol"), h["side"],
                 h.get("amount"), h.get("price_usd"), h.get("value_usd"),
                 h.get("realized_pnl"), h.get("decision_id"), h.get("tx_hash"), h.get("created_at")),
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
from dashboard import SpotTraderDashboard  # noqa: E402

PORT = int(os.environ.get("MODULE_PORT", os.environ.get("SPOT_TRADER_PORT", "9104")))
register_dashboard(app, SpotTraderDashboard, module_url=f"http://127.0.0.1:{PORT}")



# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    logging.getLogger("werkzeug").setLevel(logging.ERROR)
    init_db()
    port = int(os.environ.get("MODULE_PORT", os.environ.get("SPOT_TRADER_PORT", "9104")))
    # Start pulse worker if enabled
    if get_config_value("enabled", "true").lower() == "true":
        start_worker()
    app.run(host="127.0.0.1", port=port)
