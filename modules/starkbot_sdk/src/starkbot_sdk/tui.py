"""TUI dashboard support for StarkBot modules (legacy).

Modules subclass StarkbotDashboard and override `build()` to return
a Rich renderable. The SDK handles ANSI rendering and Flask endpoint
registration.

NOTE: New modules should use the declarative dashboard system instead:
    from starkbot_sdk.dashboard import Dashboard, register_dashboard
"""

from __future__ import annotations

import logging
import os
import threading
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from flask import Flask

import httpx
from rich.console import Console, RenderableType

log = logging.getLogger(__name__)


class StarkbotDashboard:
    """Base class for module TUI dashboards.

    Subclass and override `build(width, state)` to return a Rich renderable.
    Use `self.api(endpoint, body)` to call the module's own RPC endpoints.
    """

    def __init__(self, module_url: str) -> None:
        self.module_url = module_url.rstrip("/")

    def api(self, endpoint: str, body: dict | None = None) -> dict:
        """Call an RPC endpoint on this module's own service."""
        url = f"{self.module_url}{endpoint}"
        if body is not None:
            resp = httpx.post(url, json=body, timeout=5)
        else:
            resp = httpx.get(url, timeout=5)
        resp.raise_for_status()
        return resp.json()

    def build(self, width: int, state: dict | None = None) -> RenderableType:
        """Override to return a Rich renderable for the dashboard."""
        raise NotImplementedError("Subclass must implement build()")

    def actions(self) -> dict[str, Any]:
        """Return action metadata for interactive mode.

        Override to declare navigable rows and keyboard actions.
        """
        return {"navigable": False, "actions": []}

    def handle_action(
        self, action: str, state: dict, inputs: list[str] | None = None
    ) -> dict[str, Any]:
        """Execute a mutation action. Override in subclass."""
        return {"ok": False, "error": f"Unknown action: {action}"}

    def _get_entry_count(self) -> int:
        """Return the total number of navigable rows. Override in subclass."""
        return 0


def render_ansi(
    dashboard_class: type[StarkbotDashboard],
    module_url: str,
    width: int = 120,
    height: int = 40,
    state: dict | None = None,
) -> str:
    """Instantiate a dashboard and render it to an ANSI string."""
    dashboard = dashboard_class(module_url)
    renderable = dashboard.build(width, state)
    console = Console(record=True, width=width, height=height, force_terminal=True)
    console.print(renderable)
    return console.export_text(styles=True)


def notify_tui_update(module_name: str) -> None:
    """Fire-and-forget POST to the backend to broadcast a TUI invalidation event."""
    backend_url = os.environ.get("STARKBOT_SELF_URL", "http://127.0.0.1:8080")
    token = os.environ.get("STARKBOT_INTERNAL_TOKEN", "")
    if not token:
        return

    def _fire() -> None:
        try:
            httpx.post(
                f"{backend_url}/api/internal/modules/tui-invalidate",
                json={"module": module_name},
                headers={"X-Internal-Token": token},
                timeout=2,
            )
        except Exception:
            log.debug("TUI invalidate notify failed for %s", module_name, exc_info=True)

    threading.Thread(target=_fire, daemon=True).start()


def register_tui_endpoint(
    flask_app: Flask,
    dashboard_class: type[StarkbotDashboard],
    module_url: str,
) -> None:
    """Wire up TUI dashboard routes on a Flask app.

    Routes:
      GET  /rpc/dashboard/tui         — render ANSI with optional state params
      GET  /rpc/dashboard/tui/actions  — return action metadata as JSON
      POST /rpc/dashboard/tui/action   — execute a mutation action
    """
    from flask import request, Response, jsonify

    @flask_app.route("/rpc/dashboard/tui", methods=["GET"])
    def _tui_dashboard():
        width = request.args.get("width", 120, type=int)
        height = request.args.get("height", 40, type=int)
        state: dict[str, Any] = {}
        if "selected" in request.args:
            state["selected"] = request.args.get("selected", 0, type=int)
        if "scroll" in request.args:
            state["scroll"] = request.args.get("scroll", 0, type=int)
        ansi = render_ansi(
            dashboard_class, module_url, width, height, state or None
        )
        return Response(ansi, content_type="text/plain; charset=utf-8")

    @flask_app.route("/rpc/dashboard/tui/actions", methods=["GET"])
    def _tui_actions():
        dashboard = dashboard_class(module_url)
        return jsonify(dashboard.actions())

    @flask_app.route("/rpc/dashboard/tui/action", methods=["POST"])
    def _tui_action():
        data = request.get_json(silent=True) or {}
        action = data.get("action", "")
        state = data.get("state", {})
        inputs = data.get("inputs")
        dashboard = dashboard_class(module_url)
        result = dashboard.handle_action(action, state, inputs)
        if result.get("ok"):
            notify_tui_update(flask_app.name)
        return jsonify(result)
