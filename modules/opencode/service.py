# /// script
# requires-python = ">=3.12"
# dependencies = ["flask", "requests", "starkbot-sdk"]
#
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
OpenCode module — thin RPC wrapper around `opencode serve`.

Spawns `opencode serve` as a subprocess on startup, waits for it to become
healthy, then proxies coding tasks to it via HTTP.

RPC protocol endpoints:
  GET  /rpc/status      -> service health
  POST /rpc/prompt      -> send coding task to OpenCode agent
  GET  /rpc/sessions    -> list active sessions

Launch with:  uv run service.py
"""

from flask import request
from starkbot_sdk import create_app, success, error
import os
import time
import logging
import subprocess
import threading
import requests as http_requests

OPENCODE_PORT = int(os.environ.get("OPENCODE_PORT", "4096"))
OPENCODE_HOST = os.environ.get("OPENCODE_HOST", "127.0.0.1")
OPENCODE_PROJECT_DIR = os.environ.get("OPENCODE_PROJECT_DIR", ".")
OPENCODE_URL = f"http://{OPENCODE_HOST}:{OPENCODE_PORT}"

_start_time = time.time()
_prompt_count = 0
_prompt_count_lock = threading.Lock()


# ---------------------------------------------------------------------------
# OpenCode HTTP client
# ---------------------------------------------------------------------------

def oc_health() -> bool:
    try:
        resp = http_requests.get(f"{OPENCODE_URL}/global/health", timeout=5)
        return resp.ok
    except Exception:
        return False


def oc_create_session() -> dict:
    resp = http_requests.post(f"{OPENCODE_URL}/session", timeout=15)
    if not resp.ok:
        raise RuntimeError(f"Create session HTTP {resp.status_code}: {resp.text}")
    return resp.json()


def oc_prompt(session_id: str, text: str) -> str:
    body = {"parts": [{"type": "text", "text": text}]}
    resp = http_requests.post(
        f"{OPENCODE_URL}/session/{session_id}/message",
        json=body, timeout=300,
    )
    if not resp.ok:
        raise RuntimeError(f"Prompt HTTP {resp.status_code}: {resp.text}")
    messages = resp.json()
    response_text = "\n".join(
        part.get("text", "")
        for msg in messages if msg.get("role") == "assistant"
        for part in msg.get("parts", [])
        if part.get("type") == "text" and part.get("text")
    )
    return response_text or repr(messages)


def oc_list_sessions() -> list[dict]:
    resp = http_requests.get(f"{OPENCODE_URL}/session", timeout=15)
    if not resp.ok:
        raise RuntimeError(f"List sessions error: {resp.text}")
    return resp.json()


# ---------------------------------------------------------------------------
# App
# ---------------------------------------------------------------------------

def _status_extra():
    return {
        "opencode_healthy": oc_health(),
        "opencode_port": OPENCODE_PORT,
        "total_prompts": _prompt_count,
    }


app = create_app("opencode", status_extra_fn=_status_extra)


# ---------------------------------------------------------------------------
# RPC: Prompt
# ---------------------------------------------------------------------------

@app.route("/rpc/prompt", methods=["POST"])
def rpc_prompt():
    global _prompt_count
    body = request.get_json(silent=True) or {}
    task = body.get("task", "").strip()
    if not task:
        return error("task is empty")

    try:
        session = oc_create_session()
    except Exception as e:
        return error(str(e), 502)

    full_prompt = task
    if body.get("project_path"):
        full_prompt = f"Working directory: {body['project_path']}\n\n{task}"

    try:
        response = oc_prompt(session["id"], full_prompt)
    except Exception as e:
        return error(f"OpenCode prompt failed: {e}", 502)

    with _prompt_count_lock:
        _prompt_count += 1

    return success({"session_id": session["id"], "response": response})


# ---------------------------------------------------------------------------
# RPC: Sessions
# ---------------------------------------------------------------------------

@app.route("/rpc/sessions", methods=["GET"])
def rpc_sessions():
    try:
        sessions = oc_list_sessions()
        infos = [{"id": s["id"], "title": s.get("title")} for s in sessions]
        return success(infos)
    except Exception as e:
        return error(str(e), 502)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    logging.getLogger("werkzeug").setLevel(logging.ERROR)

    # Spawn opencode serve
    logging.info(f"Spawning opencode serve on {OPENCODE_HOST}:{OPENCODE_PORT} (project: {OPENCODE_PROJECT_DIR})")
    cmd = ["opencode", "serve", "--port", str(OPENCODE_PORT), "--hostname", OPENCODE_HOST]
    env = dict(os.environ)
    if os.environ.get("OPENCODE_SERVER_PASSWORD"):
        env["OPENCODE_SERVER_PASSWORD"] = os.environ["OPENCODE_SERVER_PASSWORD"]

    try:
        proc = subprocess.Popen(cmd, cwd=OPENCODE_PROJECT_DIR, env=env)
        logging.info(f"opencode serve spawned (pid {proc.pid})")
    except FileNotFoundError:
        logging.error("Failed to spawn opencode serve — make sure `opencode` is installed and in PATH")
        raise SystemExit(1)

    # Wait for health
    deadline = time.time() + 30
    while time.time() < deadline:
        if oc_health():
            logging.info("opencode serve is healthy")
            break
        time.sleep(0.5)
    else:
        logging.error("opencode serve did not become healthy within 30s")
        raise SystemExit(1)

    port = int(os.environ.get("MODULE_PORT", os.environ.get("OPENCODE_MODULE_PORT", "9103")))
    app.run(host="127.0.0.1", port=port)
