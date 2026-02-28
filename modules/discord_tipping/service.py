# /// script
# requires-python = ">=3.12"
# dependencies = ["starkbot-sdk"]
#
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
Discord Tipping module — manages Discord user profiles and linked wallet addresses.

RPC protocol endpoints:
  GET  /rpc/status             → service health
  POST /rpc/profile            → unified tool endpoint (action-based)
  POST /rpc/backup/export      → export profiles for backup
  POST /rpc/backup/restore     → restore profiles from backup
  GET  /rpc/csv/export         → download all profiles as CSV
  POST /rpc/csv/import         → bulk-upsert profiles from CSV upload
  GET  /                       → HTML dashboard

Launch with:  uv run service.py
"""

from flask import request, Response
from starkbot_sdk import create_app, success, error
import sqlite3
import os
import csv
import io
from datetime import datetime, timezone

MAX_CSV_BYTES = 5 * 1024 * 1024  # 5 MB
MAX_CSV_ROWS = 50_000

DB_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "discord_tipping.db")


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
        CREATE TABLE IF NOT EXISTS discord_user_profiles (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            discord_user_id     TEXT    NOT NULL UNIQUE,
            discord_username    TEXT,
            public_address      TEXT,
            registration_status TEXT    NOT NULL DEFAULT 'unregistered',
            registered_at       TEXT,
            last_interaction_at TEXT,
            created_at          TEXT    NOT NULL DEFAULT (datetime('now')),
            updated_at          TEXT    NOT NULL DEFAULT (datetime('now'))
        )
    """)
    conn.execute("""
        CREATE INDEX IF NOT EXISTS idx_profiles_address
        ON discord_user_profiles (public_address)
    """)
    conn.commit()
    conn.close()


def row_to_dict(row):
    if row is None:
        return None
    return dict(row)


def now_iso():
    return datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S")


# ---------------------------------------------------------------------------
# Profile operations
# ---------------------------------------------------------------------------

def profile_get_or_create(discord_user_id: str, username: str | None = None):
    conn = get_db()
    ts = now_iso()
    conn.execute(
        "INSERT OR IGNORE INTO discord_user_profiles (discord_user_id, discord_username, created_at, updated_at) VALUES (?, ?, ?, ?)",
        (discord_user_id, username, ts, ts),
    )
    conn.execute(
        "UPDATE discord_user_profiles SET last_interaction_at = ?, discord_username = COALESCE(?, discord_username), updated_at = ? WHERE discord_user_id = ?",
        (ts, username, ts, discord_user_id),
    )
    conn.commit()
    row = conn.execute("SELECT * FROM discord_user_profiles WHERE discord_user_id = ?", (discord_user_id,)).fetchone()
    conn.close()
    return row_to_dict(row)


def profile_get(discord_user_id: str):
    conn = get_db()
    row = conn.execute("SELECT * FROM discord_user_profiles WHERE discord_user_id = ?", (discord_user_id,)).fetchone()
    conn.close()
    return row_to_dict(row)


def profile_get_by_address(address: str):
    conn = get_db()
    row = conn.execute(
        "SELECT * FROM discord_user_profiles WHERE LOWER(public_address) = LOWER(?)", (address,)
    ).fetchone()
    conn.close()
    return row_to_dict(row)


def profile_register(discord_user_id: str, address: str):
    conn = get_db()
    ts = now_iso()
    conn.execute(
        "UPDATE discord_user_profiles SET public_address = ?, registration_status = 'registered', registered_at = ?, updated_at = ? WHERE discord_user_id = ?",
        (address, ts, ts, discord_user_id),
    )
    conn.commit()
    conn.close()
    return True


def profile_unregister(discord_user_id: str):
    conn = get_db()
    ts = now_iso()
    conn.execute(
        "UPDATE discord_user_profiles SET public_address = NULL, registration_status = 'unregistered', updated_at = ? WHERE discord_user_id = ?",
        (ts, discord_user_id),
    )
    conn.commit()
    conn.close()
    return True


def profile_list_all():
    conn = get_db()
    rows = conn.execute("SELECT * FROM discord_user_profiles ORDER BY updated_at DESC").fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


def profile_list_registered():
    conn = get_db()
    rows = conn.execute(
        "SELECT * FROM discord_user_profiles WHERE registration_status = 'registered' AND public_address IS NOT NULL ORDER BY updated_at DESC"
    ).fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


def profile_stats():
    conn = get_db()
    total = conn.execute("SELECT COUNT(*) FROM discord_user_profiles").fetchone()[0]
    registered = conn.execute(
        "SELECT COUNT(*) FROM discord_user_profiles WHERE registration_status = 'registered'"
    ).fetchone()[0]
    conn.close()
    return {
        "total_profiles": total,
        "registered_count": registered,
        "unregistered_count": total - registered,
    }


# ---------------------------------------------------------------------------
# App
# ---------------------------------------------------------------------------

app = create_app("discord_tipping", status_extra_fn=lambda: profile_stats())


# ---------------------------------------------------------------------------
# RPC: Unified tool endpoint
# ---------------------------------------------------------------------------

@app.route("/rpc/profile", methods=["POST"])
def rpc_profile():
    body = request.get_json(silent=True) or {}
    action = body.get("action")

    try:
        if action == "get_or_create":
            uid = body.get("discord_user_id")
            if not uid:
                return error("discord_user_id is required")
            data = profile_get_or_create(uid, body.get("username"))
            return success(data)

        elif action == "get":
            uid = body.get("discord_user_id")
            if not uid:
                return error("discord_user_id is required")
            data = profile_get(uid)
            return success(data)

        elif action == "get_by_address":
            addr = body.get("address")
            if not addr:
                return error("address is required")
            data = profile_get_by_address(addr)
            return success(data)

        elif action == "register":
            uid = body.get("discord_user_id")
            addr = body.get("address")
            if not uid or not addr:
                return error("discord_user_id and address are required")
            profile_register(uid, addr)
            return success(True)

        elif action == "unregister":
            uid = body.get("discord_user_id")
            if not uid:
                return error("discord_user_id is required")
            profile_unregister(uid)
            return success(True)

        elif action == "list":
            data = profile_list_all()
            return success(data)

        elif action == "list_registered":
            data = profile_list_registered()
            return success(data)

        elif action == "stats":
            data = profile_stats()
            return success(data)

        else:
            return error(f"Unknown action: {action}. Valid: get_or_create, get, get_by_address, register, unregister, list, list_registered, stats")

    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# RPC: Backup / Restore
# ---------------------------------------------------------------------------

@app.route("/rpc/backup/export", methods=["POST"])
def rpc_backup_export():
    registered = profile_list_registered()
    entries = []
    for p in registered:
        entries.append({
            "discord_user_id": p["discord_user_id"],
            "discord_username": p.get("discord_username"),
            "public_address": p["public_address"],
            "registered_at": p.get("registered_at"),
        })
    return success(entries)


@app.route("/rpc/backup/restore", methods=["POST"])
def rpc_backup_restore():
    body = request.get_json(silent=True) or {}
    profiles = body.get("profiles", [])
    if not isinstance(profiles, list):
        return error("profiles must be a list")

    conn = get_db()
    conn.execute("DELETE FROM discord_user_profiles")
    ts = now_iso()
    count = 0
    for entry in profiles:
        uid = entry.get("discord_user_id")
        if not uid:
            continue
        addr = entry.get("public_address")
        status = "registered" if addr else "unregistered"
        conn.execute(
            "INSERT OR IGNORE INTO discord_user_profiles (discord_user_id, discord_username, public_address, registration_status, registered_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (uid, entry.get("discord_username"), addr, status, entry.get("registered_at"), ts, ts),
        )
        count += 1
    conn.commit()
    conn.close()
    return success(count)


# ---------------------------------------------------------------------------
# CSV Export / Import
# ---------------------------------------------------------------------------

CSV_COLUMNS = ["discord_user_id", "discord_username", "public_address", "registration_status", "registered_at"]

@app.route("/rpc/csv/export")
def rpc_csv_export():
    profiles = profile_list_all()
    buf = io.StringIO()
    writer = csv.DictWriter(buf, fieldnames=CSV_COLUMNS, extrasaction="ignore")
    writer.writeheader()
    for p in profiles:
        writer.writerow({col: p.get(col, "") for col in CSV_COLUMNS})
    return Response(
        buf.getvalue(),
        mimetype="text/csv",
        headers={"Content-Disposition": "attachment; filename=discord_tipping_profiles.csv"},
    )


@app.route("/rpc/csv/import", methods=["POST"])
def rpc_csv_import():
    if request.content_length and request.content_length > MAX_CSV_BYTES:
        return error(f"CSV too large (max {MAX_CSV_BYTES // 1024 // 1024} MB)")

    f = request.files.get("file")
    if f is None:
        raw = request.get_data(as_text=True, cache=False)
        if not raw:
            return error("No CSV file or body provided")
    else:
        raw = f.read(MAX_CSV_BYTES + 1).decode("utf-8")
        if len(raw) > MAX_CSV_BYTES:
            return error(f"CSV too large (max {MAX_CSV_BYTES // 1024 // 1024} MB)")

    reader = csv.DictReader(io.StringIO(raw))
    conn = get_db()
    ts = now_iso()
    count = 0
    for row in reader:
        if count >= MAX_CSV_ROWS:
            break
        uid = row.get("discord_user_id", "").strip()
        if not uid:
            continue
        username = row.get("discord_username", "").strip() or None
        addr = row.get("public_address", "").strip() or None
        status = row.get("registration_status", "").strip() or ("registered" if addr else "unregistered")
        registered_at = row.get("registered_at", "").strip() or None
        conn.execute(
            """INSERT INTO discord_user_profiles
                   (discord_user_id, discord_username, public_address, registration_status, registered_at, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(discord_user_id) DO UPDATE SET
                   discord_username    = COALESCE(excluded.discord_username, discord_user_profiles.discord_username),
                   public_address      = excluded.public_address,
                   registration_status = excluded.registration_status,
                   registered_at       = COALESCE(excluded.registered_at, discord_user_profiles.registered_at),
                   updated_at          = excluded.updated_at""",
            (uid, username, addr, status, registered_at, ts, ts),
        )
        count += 1
    conn.commit()
    conn.close()
    return success({"imported": count})


# ---------------------------------------------------------------------------
# Dashboard (HTML + TUI)
# ---------------------------------------------------------------------------

from starkbot_sdk.dashboard import register_dashboard  # noqa: E402
from dashboard import DiscordTippingDashboard  # noqa: E402

PORT = int(os.environ.get("MODULE_PORT", os.environ.get("DISCORD_TIPPING_PORT", "9101")))
register_dashboard(app, DiscordTippingDashboard, module_url=f"http://127.0.0.1:{PORT}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    import logging
    logging.getLogger("werkzeug").setLevel(logging.ERROR)
    init_db()
    port = int(os.environ.get("MODULE_PORT", os.environ.get("DISCORD_TIPPING_PORT", "9101")))
    app.run(host="127.0.0.1", port=port)
