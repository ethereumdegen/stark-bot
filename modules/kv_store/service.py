# /// script
# requires-python = ">=3.12"
# dependencies = ["flask", "starkbot-sdk"]
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

from flask import Response, request
from starkbot_sdk import create_app, error, success

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

    return success({"restored": len(entries)})


# ---------------------------------------------------------------------------
# Dashboard
# ---------------------------------------------------------------------------

DASHBOARD_HTML = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>KV Store</title>
<style>
  *{box-sizing:border-box;margin:0;padding:0}
  body{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;
       background:#0d1117;color:#c9d1d9;padding:24px}
  h1{font-size:1.4rem;margin-bottom:16px;color:#58a6ff}
  table{width:100%;border-collapse:collapse;margin-top:12px}
  th,td{text-align:left;padding:8px 12px;border-bottom:1px solid #21262d}
  th{color:#8b949e;font-weight:600;font-size:.85rem;text-transform:uppercase;letter-spacing:.5px}
  td{font-family:"SF Mono",Consolas,monospace;font-size:.9rem}
  .key{color:#79c0ff}.val{color:#a5d6ff;word-break:break-all}
  .empty{color:#484f58;padding:24px;text-align:center}
  .count{color:#8b949e;font-size:.85rem;margin-bottom:8px}
</style>
</head>
<body>
<h1>KV Store</h1>
<div class="count" id="count"></div>
<table>
<thead><tr><th>Key</th><th>Value</th></tr></thead>
<tbody id="tbody"><tr><td colspan="2" class="empty">Loading...</td></tr></tbody>
</table>
<script>
fetch("rpc/kv",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({action:"list"})})
  .then(r=>r.json()).then(d=>{
    const entries=(d.data||{}).entries||[];
    document.getElementById("count").textContent=entries.length+" entries";
    const tbody=document.getElementById("tbody");
    if(!entries.length){tbody.innerHTML='<tr><td colspan="2" class="empty">No entries</td></tr>';return}
    entries.sort((a,b)=>a.key.localeCompare(b.key));
    tbody.innerHTML=entries.map(e=>`<tr><td class="key">${esc(e.key)}</td><td class="val">${esc(e.value)}</td></tr>`).join("");
  }).catch(()=>{document.getElementById("tbody").innerHTML='<tr><td colspan="2" class="empty">Error loading data</td></tr>'});
function esc(s){const d=document.createElement("div");d.textContent=s;return d.innerHTML}
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
    port = int(os.environ.get("MODULE_PORT", os.environ.get("KV_STORE_PORT", "9103")))
    print(f"[kv_store] Service starting on port {port}", flush=True)
    app.run(host="127.0.0.1", port=port)
