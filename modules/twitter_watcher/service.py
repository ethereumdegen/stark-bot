# /// script
# requires-python = ">=3.12"
# dependencies = ["tweepy", "starkbot-sdk"]
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
Twitter Watcher module — monitor Twitter accounts for new tweets and fire
hook events so agents can react (quote-tweet, reply, post commentary).

Uses tweepy with OAuth 1.0a (app + user tokens) for Twitter API v2 access.
State is kept in-memory and survives via backup/restore endpoints.
"""

import logging
import os
import threading
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone

import tweepy
from flask import request
from starkbot_sdk import create_app, error, success
from starkbot_sdk.tui import notify_tui_update

log = logging.getLogger("twitter_watcher")

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

CONSUMER_KEY = os.environ.get("TWITTER_CONSUMER_KEY", "")
CONSUMER_SECRET = os.environ.get("TWITTER_CONSUMER_SECRET", "")
ACCESS_TOKEN = os.environ.get("TWITTER_ACCESS_TOKEN", "")
ACCESS_TOKEN_SECRET = os.environ.get("TWITTER_ACCESS_TOKEN_SECRET", "")

BACKEND_URL = os.environ.get("STARKBOT_SELF_URL", "http://127.0.0.1:8080")
INTERNAL_TOKEN = os.environ.get("STARKBOT_INTERNAL_TOKEN", "")

DEFAULT_POLL_INTERVAL = 120  # seconds
MIN_POLL_INTERVAL = 30

# ---------------------------------------------------------------------------
# In-memory state (thread-safe)
# ---------------------------------------------------------------------------


@dataclass
class WatchedUser:
    username: str  # original case
    user_id: str  # numeric Twitter user ID
    since_id: str | None = None  # last seen tweet ID
    added_at: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())


_watchlist: dict[str, WatchedUser] = {}  # key = uppercased username
_lock = threading.Lock()
_poll_interval: int = DEFAULT_POLL_INTERVAL
_worker_running: bool = True
_last_poll_at: str | None = None
_start_time = time.time()
_hook_event_log: list[dict] = []  # last N fired hook events
MAX_HOOK_LOG = 10

# ---------------------------------------------------------------------------
# Twitter client
# ---------------------------------------------------------------------------


def _get_twitter_client() -> tweepy.Client | None:
    """Build a tweepy Client with OAuth 1.0a if all 4 keys are present."""
    if not all([CONSUMER_KEY, CONSUMER_SECRET, ACCESS_TOKEN, ACCESS_TOKEN_SECRET]):
        return None
    return tweepy.Client(
        consumer_key=CONSUMER_KEY,
        consumer_secret=CONSUMER_SECRET,
        access_token=ACCESS_TOKEN,
        access_token_secret=ACCESS_TOKEN_SECRET,
        wait_on_rate_limit=True,
    )


def _resolve_user_id(client: tweepy.Client, username: str) -> str | None:
    """Resolve a Twitter username to its numeric user ID."""
    try:
        resp = client.get_user(username=username)
        if resp and resp.data:
            return str(resp.data.id)
    except Exception as e:
        log.warning("Failed to resolve user ID for @%s: %s", username, e)
    return None


def _resolve_user_id_async(username: str) -> None:
    """Background task: resolve a Twitter username to its user ID and update the watchlist."""
    client = _get_twitter_client()
    if not client:
        log.warning("Cannot resolve user ID for @%s — Twitter API keys not configured", username)
        return

    user_id = _resolve_user_id(client, username)
    if not user_id:
        log.warning("Could not resolve user ID for @%s — will retry on next poll", username)
        return

    key = username.upper()
    with _lock:
        if key in _watchlist:
            _watchlist[key].user_id = user_id
            log.info("Resolved @%s -> user_id %s", username, user_id)
    notify_tui_update("twitter_watcher")


# ---------------------------------------------------------------------------
# Hook firing
# ---------------------------------------------------------------------------


def _fire_hook(payload: dict) -> None:
    """Fire twitter_watched_tweet hook via backend internal API."""
    # Record in event log regardless of whether we can fire
    event_entry = {
        "username": payload.get("username", ""),
        "tweet_id": payload.get("tweet_id", ""),
        "tweet_text": (payload.get("tweet_text") or "")[:120],
        "tweet_url": payload.get("tweet_url", ""),
        "fired_at": datetime.now(timezone.utc).isoformat(),
        "status": "pending",
    }

    if not INTERNAL_TOKEN:
        log.warning("No STARKBOT_INTERNAL_TOKEN — cannot fire hook")
        event_entry["status"] = "no_token"
        with _lock:
            _hook_event_log.append(event_entry)
            if len(_hook_event_log) > MAX_HOOK_LOG:
                _hook_event_log.pop(0)
        return

    import httpx

    try:
        httpx.post(
            f"{BACKEND_URL}/api/internal/hooks/fire",
            json={
                "event": "twitter_watched_tweet",
                "data": payload,
            },
            headers={"X-Internal-Token": INTERNAL_TOKEN},
            timeout=10,
        )
        log.info("Fired twitter_watched_tweet hook for @%s", payload.get("username"))
        event_entry["status"] = "fired"
    except Exception as e:
        log.warning("Failed to fire hook: %s", e)
        event_entry["status"] = f"error: {e}"

    with _lock:
        _hook_event_log.append(event_entry)
        if len(_hook_event_log) > MAX_HOOK_LOG:
            _hook_event_log.pop(0)


# ---------------------------------------------------------------------------
# Background poller
# ---------------------------------------------------------------------------


def _poll_once(client: tweepy.Client) -> None:
    """Poll all watched users for new tweets."""
    global _last_poll_at

    with _lock:
        users = list(_watchlist.values())

    if not users:
        return

    for user in users:
        if not user.user_id:
            # Try to resolve missing user ID
            _resolve_user_id_async(user.username)
            continue

        try:
            kwargs: dict = {
                "max_results": 5,
                "tweet_fields": ["created_at", "text"],
            }
            if user.since_id:
                kwargs["since_id"] = user.since_id

            resp = client.get_users_tweets(int(user.user_id), **kwargs)

            if not resp or not resp.data:
                continue

            # Tweets come newest-first; process oldest-first
            tweets = sorted(resp.data, key=lambda t: int(t.id))

            for tweet in tweets:
                created_at = ""
                if tweet.created_at:
                    created_at = tweet.created_at.isoformat()

                _fire_hook({
                    "username": user.username,
                    "user_id": user.user_id,
                    "tweet_id": str(tweet.id),
                    "tweet_text": tweet.text,
                    "tweet_url": f"https://twitter.com/{user.username}/status/{tweet.id}",
                    "created_at": created_at,
                })

            # Update since_id to the newest tweet
            newest_id = str(tweets[-1].id)
            with _lock:
                key = user.username.upper()
                if key in _watchlist:
                    _watchlist[key].since_id = newest_id

        except Exception as e:
            log.warning("Error polling @%s: %s", user.username, e)

    _last_poll_at = datetime.now(timezone.utc).isoformat()
    notify_tui_update("twitter_watcher")


def _worker_loop() -> None:
    """Main polling loop — runs every _poll_interval seconds."""
    log.info("[TWITTER_WATCHER] Worker started (poll interval: %ds)", _poll_interval)

    client = _get_twitter_client()
    if not client:
        log.warning("[TWITTER_WATCHER] Twitter API keys not configured — worker disabled")
        return

    # Seed since_id for any users that don't have one yet
    _seed_since_ids(client)

    first_run = True
    while _worker_running:
        delay = 10 if first_run else _poll_interval
        first_run = False
        time.sleep(delay)
        if not _worker_running:
            break
        try:
            _poll_once(client)
        except Exception as e:
            log.error("[TWITTER_WATCHER] Poll error: %s", e)


def _seed_since_ids(client: tweepy.Client) -> None:
    """For users without a since_id, set it to the latest tweet so we only fire on NEW tweets."""
    with _lock:
        users = [u for u in _watchlist.values() if u.since_id is None]

    for user in users:
        try:
            resp = client.get_users_tweets(int(user.user_id), max_results=5)
            if resp and resp.data:
                newest_id = str(max(int(t.id) for t in resp.data))
                with _lock:
                    key = user.username.upper()
                    if key in _watchlist:
                        _watchlist[key].since_id = newest_id
                log.info("Seeded since_id for @%s: %s", user.username, newest_id)
        except Exception as e:
            log.warning("Failed to seed since_id for @%s: %s", user.username, e)


# ---------------------------------------------------------------------------
# Flask app
# ---------------------------------------------------------------------------

app = create_app("twitter_watcher")


@app.route("/rpc/twitter_watcher", methods=["POST"])
def rpc_twitter_watcher():
    """Unified tool endpoint with action routing."""
    global _poll_interval
    data = request.get_json(silent=True) or {}
    action = data.get("action", "")

    if action == "add":
        username = (data.get("username") or "").strip().lstrip("@")
        if not username:
            return error("'username' is required for 'add' action")

        key = username.upper()
        with _lock:
            if key in _watchlist:
                return success({
                    "username": _watchlist[key].username,
                    "message": "Already watching this account",
                })

        # Add immediately with empty user_id; resolve async in background
        watched = WatchedUser(username=username, user_id="")
        with _lock:
            _watchlist[key] = watched

        notify_tui_update("twitter_watcher")

        # Kick off background user ID resolution
        threading.Thread(
            target=_resolve_user_id_async,
            args=(username,),
            daemon=True,
        ).start()

        return success({
            "username": username,
            "message": f"Now watching @{username} (resolving user ID in background)",
        })

    elif action == "remove":
        username = (data.get("username") or "").strip().lstrip("@")
        if not username:
            return error("'username' is required for 'remove' action")

        key = username.upper()
        with _lock:
            existed = key in _watchlist
            _watchlist.pop(key, None)

        notify_tui_update("twitter_watcher")
        if existed:
            return success({"username": username, "message": f"Stopped watching @{username}"})
        return success({"username": username, "message": "Was not watching this account"})

    elif action == "list":
        with _lock:
            entries = [
                {
                    "username": u.username,
                    "user_id": u.user_id,
                    "since_id": u.since_id,
                    "added_at": u.added_at,
                }
                for u in _watchlist.values()
            ]
        return success({
            "count": len(entries),
            "poll_interval": _poll_interval,
            "last_poll_at": _last_poll_at,
            "entries": entries,
            "recent_hooks": list(_hook_event_log),
        })

    elif action == "set_interval":
        interval = data.get("interval")
        if interval is None:
            return error("'interval' is required for 'set_interval' action")
        try:
            interval = int(interval)
        except (ValueError, TypeError):
            return error("'interval' must be an integer")
        if interval < MIN_POLL_INTERVAL:
            return error(f"Minimum poll interval is {MIN_POLL_INTERVAL} seconds")
        _poll_interval = interval
        notify_tui_update("twitter_watcher")
        return success({"interval": _poll_interval, "message": f"Poll interval set to {interval}s"})

    elif action == "set_user_id":
        username = (data.get("username") or "").strip().lstrip("@")
        user_id = (data.get("user_id") or "").strip()
        if not username:
            return error("'username' is required for 'set_user_id' action")
        if not user_id:
            return error("'user_id' is required for 'set_user_id' action")

        key = username.upper()
        with _lock:
            if key not in _watchlist:
                return error(f"@{username} is not in the watchlist")
            _watchlist[key].user_id = user_id

        notify_tui_update("twitter_watcher")
        return success({
            "username": username,
            "user_id": user_id,
            "message": f"Set user_id for @{username} to {user_id}",
        })

    else:
        return error(f"Unknown action '{action}'. Use: add, remove, list, set_interval, set_user_id")


# ---------------------------------------------------------------------------
# Backup / Restore
# ---------------------------------------------------------------------------


@app.route("/rpc/backup/export", methods=["POST"])
def backup_export():
    """Dump watchlist for backup."""
    with _lock:
        entries = [
            {
                "username": u.username,
                "user_id": u.user_id,
                "since_id": u.since_id,
                "added_at": u.added_at,
            }
            for u in _watchlist.values()
        ]
    return success(entries)


@app.route("/rpc/backup/restore", methods=["POST"])
def backup_restore():
    """Clear watchlist + bulk load from payload."""
    data = request.get_json(silent=True)
    if data is None:
        return error("Invalid JSON payload")

    entries = data if isinstance(data, list) else data.get("data", [])

    with _lock:
        _watchlist.clear()
        for entry in entries:
            username = entry.get("username", "")
            user_id = entry.get("user_id", "")
            if username and user_id:
                key = username.upper()
                _watchlist[key] = WatchedUser(
                    username=username,
                    user_id=user_id,
                    since_id=entry.get("since_id"),
                    added_at=entry.get("added_at", datetime.now(timezone.utc).isoformat()),
                )

    notify_tui_update("twitter_watcher")
    return success({"restored": len(entries)})


# ---------------------------------------------------------------------------
# Dashboard (HTML + TUI)
# ---------------------------------------------------------------------------

from starkbot_sdk.dashboard import register_dashboard  # noqa: E402
from dashboard import TwitterWatcherDashboard  # noqa: E402

PORT = int(os.environ.get("MODULE_PORT", os.environ.get("TWITTER_WATCHER_PORT", "9108")))
register_dashboard(app, TwitterWatcherDashboard, module_url=f"http://127.0.0.1:{PORT}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    port = int(os.environ.get("MODULE_PORT", os.environ.get("TWITTER_WATCHER_PORT", "9108")))
    print(f"[twitter_watcher] Service starting on port {port}", flush=True)

    # Start background poller
    if all([CONSUMER_KEY, CONSUMER_SECRET, ACCESS_TOKEN, ACCESS_TOKEN_SECRET]):
        worker_thread = threading.Thread(target=_worker_loop, daemon=True)
        worker_thread.start()
    else:
        print("[twitter_watcher] Twitter API keys not set — background worker disabled", flush=True)

    app.run(host="127.0.0.1", port=port)
