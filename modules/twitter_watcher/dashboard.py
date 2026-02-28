"""Declarative dashboard for the Twitter Watcher module."""

from __future__ import annotations

from typing import Any

from starkbot_sdk.dashboard import (
    Badge,
    Cell,
    Column,
    Dashboard,
    Layout,
    Stat,
    Table,
)


class TwitterWatcherDashboard(Dashboard):
    title = "Twitter Watcher"

    def _fetch_list_data(self) -> dict:
        """Fetch full list data in one call."""
        try:
            resp = self.api("/rpc/twitter_watcher", {"action": "list"})
            return resp.get("data", {})
        except Exception:
            return {}

    def _get_watched_users(self) -> list[dict]:
        data = self._fetch_list_data()
        return sorted(data.get("entries", []), key=lambda e: e["username"].lower())

    def _get_entry_count(self) -> int:
        return len(self._get_watched_users())

    def layout(self) -> Layout:
        list_data = self._fetch_list_data()
        users = sorted(
            list_data.get("entries", []), key=lambda e: e["username"].lower()
        )
        recent_hooks = list_data.get("recent_hooks", [])
        poll_interval = list_data.get("poll_interval", 120)
        last_poll_at = list_data.get("last_poll_at")
        last_poll_display = last_poll_at[:19] if last_poll_at else "never"

        # Watchlist rows
        watchlist_rows: list[list[str | Badge | Cell]] = []
        for u in users:
            username = f"@{u['username']}"
            user_id = u.get("user_id") or ""
            since_id = u.get("since_id")

            if not user_id:
                status = Badge("resolving", "warning")
                uid_cell = Cell("resolving...", color="dim")
            elif since_id:
                status = Badge("tracking", "success")
                uid_cell = Cell(user_id)
            else:
                status = Badge("seeding", "default")
                uid_cell = Cell(user_id)

            watchlist_rows.append([
                Cell(username, mono=True),
                uid_cell,
                status,
                Cell(since_id or "\u2014"),
                Cell(u.get("added_at", "\u2014")[:19]),
            ])

        # Hook event rows (newest first)
        hook_rows: list[list[str | Badge | Cell]] = []
        for event in reversed(recent_hooks):
            fired_at = event.get("fired_at", "")
            if "T" in fired_at:
                fired_at = fired_at.split("T")[1][:8]

            tweet_text = event.get("tweet_text", "")
            if len(tweet_text) > 80:
                tweet_text = tweet_text[:77] + "..."

            status_str = event.get("status", "?")
            if status_str == "fired":
                status_badge = Badge(status_str, "success")
            else:
                status_badge = Badge(status_str, "danger")

            hook_rows.append([
                Cell(fired_at),
                Cell(f"@{event.get('username', '?')}", mono=True),
                Cell(tweet_text),
                status_badge,
            ])

        return Layout(
            stats=[
                Stat("Watched Accounts", len(users)),
                Stat("Poll Interval", f"{poll_interval}s"),
                Stat("Last Poll", last_poll_display),
            ],
            tables=[
                Table(
                    columns=[
                        Column("Username", mono=True),
                        "User ID",
                        "Status",
                        "Last Tweet ID",
                        "Added",
                    ],
                    rows=watchlist_rows,
                    empty="No accounts watched",
                ),
                Table(
                    columns=["Time", Column("User", mono=True), "Tweet", "Status"],
                    rows=hook_rows,
                    title="Recent Hook Events",
                    empty="No hook events yet",
                ),
            ],
        )

    def actions(self) -> dict[str, Any]:
        return {
            "navigable": True,
            "actions": [
                {
                    "key": "a",
                    "label": "Add account",
                    "action": "add_account",
                    "prompts": ["Twitter username (without @):"],
                },
                {
                    "key": "d",
                    "label": "Delete",
                    "action": "delete_selected",
                    "confirm": True,
                },
                {
                    "key": "r",
                    "label": "Refresh",
                    "action": "refresh",
                },
            ],
        }

    def handle_action(
        self, action: str, state: dict, inputs: list[str] | None = None
    ) -> dict[str, Any]:
        users = self._get_watched_users()
        selected = state.get("selected", 0)

        if action == "refresh":
            return {"ok": True}

        if action == "add_account":
            if not inputs or len(inputs) < 1 or not inputs[0].strip():
                return {"ok": False, "error": "Username required"}
            username = inputs[0].strip().lstrip("@")
            try:
                resp = self.api(
                    "/rpc/twitter_watcher",
                    {"action": "add", "username": username},
                )
                msg = resp.get("data", {}).get("message", f"Added @{username}")
                return {"ok": True, "message": msg}
            except Exception as e:
                return {"ok": False, "error": str(e)}

        if action == "delete_selected":
            if not users or selected < 0 or selected >= len(users):
                return {"ok": False, "error": "No account selected"}
            username = users[selected]["username"]
            self.api(
                "/rpc/twitter_watcher",
                {"action": "remove", "username": username},
            )
            return {"ok": True, "message": f"Removed @{username}"}

        return {"ok": False, "error": f"Unknown action: {action}"}
