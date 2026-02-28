# /// script
# requires-python = ">=3.12"
# dependencies = ["starkbot-sdk"]
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
KV Store module — in-memory key/value store for agent state tracking.

Uses a thread-safe Python dict (no external dependencies). Data persists
across the process lifetime and survives via backup/restore endpoints.
"""

import fnmatch
import os
import re
import signal
import sys
import threading

from flask import request
from starkbot_sdk import create_app, error, success
from starkbot_sdk.tui import notify_tui_update

# ---------------------------------------------------------------------------
# In-memory store (thread-safe)
# ---------------------------------------------------------------------------

_store: dict[str, str] = {}
_lock = threading.Lock()

# ---------------------------------------------------------------------------
# Key validation
# ---------------------------------------------------------------------------

_KEY_RE = re.compile(r"^[A-Za-z0-9_]+$")
MAX_KEY_LEN = 128


def validate_key(key: str) -> str:
    """Validate and normalize a key. Returns uppercased key or raises ValueError."""
    if not key:
        raise ValueError("key cannot be empty")
    if len(key) > MAX_KEY_LEN:
        raise ValueError(f"key must be at most {MAX_KEY_LEN} characters")
    if not _KEY_RE.match(key):
        raise ValueError("key must contain only letters, digits, and underscores (A-Za-z0-9_)")
    return key.upper()


# ---------------------------------------------------------------------------
# Flask app
# ---------------------------------------------------------------------------

app = create_app("kv_store")


@app.route("/rpc/kv", methods=["POST"])
def rpc_kv():
    """Unified tool endpoint with action routing."""
    data = request.get_json(silent=True) or {}
    action = data.get("action", "")

    if action == "get":
        raw_key = data.get("key")
        if not raw_key:
            return error("'key' is required for 'get' action")
        try:
            key = validate_key(raw_key)
        except ValueError as e:
            return error(str(e))
        with _lock:
            val = _store.get(key)
        if val is None:
            return success({"key": key, "value": None, "message": "Key not found"})
        return success({"key": key, "value": val})

    elif action == "set":
        raw_key = data.get("key")
        if not raw_key:
            return error("'key' is required for 'set' action")
        value = data.get("value")
        if value is None:
            return error("'value' is required for 'set' action")
        try:
            key = validate_key(raw_key)
        except ValueError as e:
            return error(str(e))
        with _lock:
            _store[key] = str(value)
        notify_tui_update("kv_store")
        return success({"key": key, "value": str(value), "message": "Value set successfully"})

    elif action == "delete":
        raw_key = data.get("key")
        if not raw_key:
            return error("'key' is required for 'delete' action")
        try:
            key = validate_key(raw_key)
        except ValueError as e:
            return error(str(e))
        with _lock:
            existed = key in _store
            _store.pop(key, None)
        notify_tui_update("kv_store")
        return success({"key": key, "deleted": existed})

    elif action == "increment":
        raw_key = data.get("key")
        if not raw_key:
            return error("'key' is required for 'increment' action")
        try:
            key = validate_key(raw_key)
        except ValueError as e:
            return error(str(e))
        amount = int(data.get("amount", 1))
        with _lock:
            current = int(_store.get(key, "0"))
            new_val = current + amount
            _store[key] = str(new_val)
        notify_tui_update("kv_store")
        return success({"key": key, "new_value": new_val, "incremented_by": amount})

    elif action == "list":
        prefix = (data.get("prefix") or data.get("key") or "").upper()
        pattern = f"{prefix}*" if prefix else "*"
        with _lock:
            entries = [
                {"key": k, "value": v}
                for k, v in _store.items()
                if fnmatch.fnmatch(k, pattern)
            ]
        return success({"prefix": prefix, "count": len(entries), "entries": entries})

    else:
        return error(f"Unknown action '{action}'. Use: get, set, delete, increment, list")


# ---------------------------------------------------------------------------
# Backup / Restore
# ---------------------------------------------------------------------------

@app.route("/rpc/backup/export", methods=["POST"])
def backup_export():
    """Dump all keys for backup."""
    with _lock:
        entries = [{"key": k, "value": v} for k, v in _store.items()]
    notify_tui_update("kv_store")
    return success(entries)


@app.route("/rpc/backup/restore", methods=["POST"])
def backup_restore():
    """Clear store + bulk SET from payload."""
    data = request.get_json(silent=True)
    if data is None:
        return error("Invalid JSON payload")

    # Accept both {"data": [...]} envelope and raw [...]
    entries = data if isinstance(data, list) else data.get("data", [])

    with _lock:
        _store.clear()
        for entry in entries:
            k = entry.get("key", "")
            v = entry.get("value", "")
            if k:
                _store[k] = v

    notify_tui_update("kv_store")
    return success({"restored": len(entries)})


# ---------------------------------------------------------------------------
# Dashboard (HTML + TUI)
# ---------------------------------------------------------------------------

from starkbot_sdk.dashboard import register_dashboard  # noqa: E402
from dashboard import KVStoreDashboard  # noqa: E402

PORT = int(os.environ.get("MODULE_PORT", os.environ.get("KV_STORE_PORT", "9103")))
register_dashboard(app, KVStoreDashboard, module_url=f"http://127.0.0.1:{PORT}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    port = int(os.environ.get("MODULE_PORT", os.environ.get("KV_STORE_PORT", "9103")))
    print(f"[kv_store] Service starting on port {port}", flush=True)
    app.run(host="127.0.0.1", port=port)
