"""Declarative dashboard for the KV Store module."""

from __future__ import annotations

from typing import Any

from starkbot_sdk.dashboard import (
    Cell,
    Column,
    Dashboard,
    Layout,
    Stat,
    Table,
)


class KVStoreDashboard(Dashboard):
    title = "KV Store"

    def _get_sorted_entries(self) -> list[dict]:
        """Fetch and sort KV entries."""
        try:
            resp = self.api("/rpc/kv", {"action": "list"})
            return sorted(
                resp.get("data", {}).get("entries", []),
                key=lambda e: e["key"],
            )
        except Exception:
            return []

    def _get_entry_count(self) -> int:
        return len(self._get_sorted_entries())

    def layout(self) -> Layout:
        entries = self._get_sorted_entries()

        rows = []
        for entry in entries:
            val = entry["value"]
            display_val = val if len(val) <= 120 else val[:117] + "..."
            rows.append([
                Cell(entry["key"], mono=True),
                Cell(display_val, title=val if len(val) > 120 else None),
            ])

        return Layout(
            stats=[Stat("Total Keys", len(entries))],
            tables=[
                Table(
                    columns=[Column("Key", mono=True), "Value"],
                    rows=rows,
                    empty="No keys stored",
                ),
            ],
        )

    def actions(self) -> dict[str, Any]:
        return {
            "navigable": True,
            "actions": [
                {
                    "key": "d",
                    "label": "Delete",
                    "action": "delete_selected",
                    "confirm": True,
                },
                {
                    "key": "e",
                    "label": "Edit value",
                    "action": "edit_selected",
                    "prompts": ["New value:"],
                },
                {
                    "key": "a",
                    "label": "Add entry",
                    "action": "add_entry",
                    "prompts": ["Key:", "Value:"],
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
        entries = self._get_sorted_entries()
        selected = state.get("selected", 0)

        if action == "refresh":
            return {"ok": True}

        if action == "delete_selected":
            if not entries or selected < 0 or selected >= len(entries):
                return {"ok": False, "error": "No entry selected"}
            key = entries[selected]["key"]
            self.api("/rpc/kv", {"action": "delete", "key": key})
            return {"ok": True, "message": f"Deleted {key}"}

        if action == "edit_selected":
            if not entries or selected < 0 or selected >= len(entries):
                return {"ok": False, "error": "No entry selected"}
            if not inputs or len(inputs) < 1:
                return {"ok": False, "error": "New value required"}
            key = entries[selected]["key"]
            self.api("/rpc/kv", {"action": "set", "key": key, "value": inputs[0]})
            return {"ok": True, "message": f"Updated {key}"}

        if action == "add_entry":
            if not inputs or len(inputs) < 2:
                return {"ok": False, "error": "Key and value required"}
            self.api("/rpc/kv", {"action": "set", "key": inputs[0], "value": inputs[1]})
            return {"ok": True, "message": f"Added {inputs[0]}"}

        return {"ok": False, "error": f"Unknown action: {action}"}
