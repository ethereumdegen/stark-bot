# /// script
# requires-python = ">=3.12"
# dependencies = ["flask", "redis", "starkbot-sdk"]
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
KV Store module — Redis-backed key/value store for agent state tracking.

Starts a redis-server subprocess (memory-only, no persistence) and exposes
RPC endpoints for get/set/delete/increment/list operations.
"""

import json
import os
import re
import signal
import subprocess
import sys
import time

import redis
from flask import Flask, Response, jsonify, request
from starkbot_sdk import create_app, error, success

# ---------------------------------------------------------------------------
# Redis subprocess management
# ---------------------------------------------------------------------------

_redis_process = None
_redis_port = 6399  # internal port for the sidecar, not the module HTTP port


def _find_free_port() -> int:
    """Ask the OS for a free TCP port."""
    import socket
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def start_redis() -> redis.Redis:
    """Start redis-server as a child process and return a connected client."""
    global _redis_process, _redis_port

    _redis_port = _find_free_port()

    _redis_process = subprocess.Popen(
        [
            "redis-server",
            "--port", str(_redis_port),
            "--save", "",          # no RDB snapshots
            "--appendonly", "no",  # no AOF persistence
            "--loglevel", "warning",
            "--bind", "127.0.0.1",
            "--daemonize", "no",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )

    # Wait for Redis to be ready (up to 5 seconds)
    client = redis.Redis(host="127.0.0.1", port=_redis_port, decode_responses=True)
    for _ in range(50):
        try:
            if client.ping():
                print(f"[kv_store] Redis started on port {_redis_port}", flush=True)
                return client
        except redis.ConnectionError:
            time.sleep(0.1)

    raise RuntimeError("Redis did not become ready within 5 seconds")


def stop_redis():
    """Gracefully stop the redis-server subprocess."""
    global _redis_process
    if _redis_process and _redis_process.poll() is None:
        _redis_process.terminate()
        try:
            _redis_process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            _redis_process.kill()
        print("[kv_store] Redis stopped", flush=True)


# ---------------------------------------------------------------------------
# Key validation (mirrors the original Rust logic)
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

rds: redis.Redis  # set in main()

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
        val = rds.get(key)
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
        rds.set(key, str(value))
        return success({"key": key, "value": str(value), "message": "Value set successfully"})

    elif action == "delete":
        raw_key = data.get("key")
        if not raw_key:
            return error("'key' is required for 'delete' action")
        try:
            key = validate_key(raw_key)
        except ValueError as e:
            return error(str(e))
        existed = rds.delete(key) > 0
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
        new_val = rds.incrby(key, amount)
        return success({"key": key, "new_value": new_val, "incremented_by": amount})

    elif action == "list":
        prefix = (data.get("prefix") or data.get("key") or "").upper()
        pattern = f"{prefix}*" if prefix else "*"
        keys = list(rds.scan_iter(match=pattern, count=500))
        entries = []
        if keys:
            values = rds.mget(keys)
            for k, v in zip(keys, values):
                if v is not None:
                    entries.append({"key": k, "value": v})
        return success({"prefix": prefix, "count": len(entries), "entries": entries})

    else:
        return error(f"Unknown action '{action}'. Use: get, set, delete, increment, list")


# ---------------------------------------------------------------------------
# Backup / Restore
# ---------------------------------------------------------------------------

@app.route("/rpc/backup/export", methods=["POST"])
def backup_export():
    """Dump all keys for backup."""
    keys = list(rds.scan_iter(match="*", count=1000))
    entries = []
    if keys:
        values = rds.mget(keys)
        for k, v in zip(keys, values):
            if v is not None:
                entries.append({"key": k, "value": v})
    return success(entries)


@app.route("/rpc/backup/restore", methods=["POST"])
def backup_restore():
    """FLUSHDB + pipeline SET from payload."""
    data = request.get_json(silent=True)
    if data is None:
        return error("Invalid JSON payload")

    # Accept both {"data": [...]} envelope and raw [...]
    entries = data if isinstance(data, list) else data.get("data", [])

    rds.flushdb()
    if entries:
        pipe = rds.pipeline()
        for entry in entries:
            k = entry.get("key", "")
            v = entry.get("value", "")
            if k:
                pipe.set(k, v)
        pipe.execute()

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
fetch("/rpc/kv",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({action:"list"})})
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
    # Start Redis sidecar
    rds = start_redis()

    # Ensure Redis is stopped on exit
    import atexit
    atexit.register(stop_redis)
    signal.signal(signal.SIGTERM, lambda *_: (stop_redis(), sys.exit(0)))

    port = int(os.environ.get("MODULE_PORT", os.environ.get("KV_STORE_PORT", "9103")))
    print(f"[kv_store] Service starting on port {port}", flush=True)
    app.run(host="127.0.0.1", port=port)
