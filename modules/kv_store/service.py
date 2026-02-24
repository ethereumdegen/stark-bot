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
  .toolbar{display:flex;align-items:center;gap:12px;margin-bottom:12px}
  .count{color:#8b949e;font-size:.85rem;flex:1}
  .btn{background:#238636;color:#fff;border:none;padding:6px 14px;border-radius:6px;cursor:pointer;font-size:.85rem;font-weight:500}
  .btn:hover{background:#2ea043}
  .btn-danger{background:#da3633}.btn-danger:hover{background:#f85149}
  .btn-secondary{background:#30363d;color:#c9d1d9}.btn-secondary:hover{background:#484f58}
  .btn:disabled{opacity:.5;cursor:default}
  table{width:100%;border-collapse:collapse;margin-top:8px}
  th,td{text-align:left;padding:8px 12px;border-bottom:1px solid #21262d}
  th{color:#8b949e;font-weight:600;font-size:.85rem;text-transform:uppercase;letter-spacing:.5px}
  td{font-family:"SF Mono",Consolas,monospace;font-size:.9rem}
  .key{color:#79c0ff}.val{color:#a5d6ff;word-break:break-all}
  .empty{color:#484f58;padding:24px;text-align:center}
  input[type=text]{background:#161b22;border:1px solid #30363d;color:#c9d1d9;padding:5px 8px;border-radius:4px;
    font-family:"SF Mono",Consolas,monospace;font-size:.85rem;width:100%}
  input[type=text]:focus{outline:none;border-color:#58a6ff}
  .edit-val{display:flex;gap:6px;align-items:center}
  .edit-val input{flex:1}
  .actions{white-space:nowrap}
  .actions button{padding:4px 8px;font-size:.8rem;margin-left:4px}
  .add-row td{padding:8px 12px}
  .toast{position:fixed;bottom:20px;right:20px;padding:10px 16px;border-radius:6px;font-size:.85rem;
    color:#fff;opacity:0;transition:opacity .3s;pointer-events:none;z-index:99}
  .toast.show{opacity:1}
  .toast.ok{background:#238636}.toast.err{background:#da3633}
</style>
</head>
<body>
<h1>KV Store</h1>
<div class="toolbar">
  <div class="count" id="count"></div>
  <button class="btn" onclick="showAdd()">+ Add Key</button>
</div>
<table>
<thead><tr><th>Key</th><th>Value</th><th class="actions">Actions</th></tr></thead>
<tbody id="tbody"><tr><td colspan="3" class="empty">Loading...</td></tr></tbody>
</table>
<div class="toast" id="toast"></div>
<script>
const RPC="rpc/kv",J="application/json";
let entries=[];

function api(body){return fetch(RPC,{method:"POST",headers:{"Content-Type":J},body:JSON.stringify(body)}).then(r=>r.json())}
function esc(s){const d=document.createElement("div");d.textContent=s;return d.innerHTML}
function toast(msg,ok){const t=document.getElementById("toast");t.textContent=msg;t.className="toast show "+(ok?"ok":"err");setTimeout(()=>t.className="toast",2000)}

function load(){
  api({action:"list"}).then(d=>{
    entries=(d.data||{}).entries||[];
    entries.sort((a,b)=>a.key.localeCompare(b.key));
    render();
  }).catch(()=>{document.getElementById("tbody").innerHTML='<tr><td colspan="3" class="empty">Error loading data</td></tr>'});
}

function render(){
  document.getElementById("count").textContent=entries.length+" entries";
  const tbody=document.getElementById("tbody");
  if(!entries.length){tbody.innerHTML='<tr><td colspan="3" class="empty">No entries</td></tr>';return}
  tbody.innerHTML=entries.map(e=>`<tr id="row-${esc(e.key)}">
    <td class="key">${esc(e.key)}</td>
    <td class="val" id="val-${esc(e.key)}">${esc(e.value)}</td>
    <td class="actions">
      <button class="btn btn-secondary" onclick="startEdit('${esc(e.key)}')">Edit</button>
      <button class="btn btn-danger" onclick="del('${esc(e.key)}')">Del</button>
    </td>
  </tr>`).join("");
}

function startEdit(key){
  const td=document.getElementById("val-"+key);
  const entry=entries.find(e=>e.key===key);
  if(!entry)return;
  const cur=entry.value;
  td.innerHTML=`<div class="edit-val"><input type="text" id="inp-${esc(key)}" value="${esc(cur)}" onkeydown="if(event.key==='Enter')saveEdit('${esc(key)}');if(event.key==='Escape')render()"><button class="btn" onclick="saveEdit('${esc(key)}')">Save</button><button class="btn btn-secondary" onclick="render()">Cancel</button></div>`;
  document.getElementById("inp-"+key).focus();
}

function saveEdit(key){
  const inp=document.getElementById("inp-"+key);
  if(!inp)return;
  const val=inp.value;
  api({action:"set",key:key,value:val}).then(d=>{
    if(d.success){toast("Saved","ok");load()}else{toast(d.error||"Failed")}
  }).catch(()=>toast("Request failed"));
}

function del(key){
  if(!confirm("Delete key: "+key+"?"))return;
  api({action:"delete",key:key}).then(d=>{
    if(d.success){toast("Deleted","ok");load()}else{toast(d.error||"Failed")}
  }).catch(()=>toast("Request failed"));
}

function showAdd(){
  const tbody=document.getElementById("tbody");
  if(document.getElementById("add-row"))return;
  const tr=document.createElement("tr");tr.id="add-row";tr.className="add-row";
  tr.innerHTML=`<td><input type="text" id="add-key" placeholder="KEY_NAME"></td>
    <td><input type="text" id="add-val" placeholder="value" onkeydown="if(event.key==='Enter')doAdd()"></td>
    <td class="actions"><button class="btn" onclick="doAdd()">Save</button><button class="btn btn-secondary" onclick="this.closest('tr').remove()">Cancel</button></td>`;
  tbody.insertBefore(tr,tbody.firstChild);
  document.getElementById("add-key").focus();
}

function doAdd(){
  const k=document.getElementById("add-key").value.trim();
  const v=document.getElementById("add-val").value;
  if(!k){toast("Key is required");return}
  api({action:"set",key:k,value:v}).then(d=>{
    if(d.success){toast("Added","ok");load()}else{toast(d.error||"Failed")}
  }).catch(()=>toast("Request failed"));
}

load();
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
