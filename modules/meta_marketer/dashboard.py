"""Declarative dashboard for the Meta Marketer module."""

from __future__ import annotations

from starkbot_sdk.dashboard import (
    Badge,
    Cell,
    Dashboard,
    Layout,
    Stat,
    Table,
)


class MetaMarketerDashboard(Dashboard):
    title = "Meta Marketer"

    def layout(self) -> Layout:
        try:
            resp = self.api("/rpc/status")
            data = resp.get("data", {})
        except Exception:
            data = {}

        has_token = data.get("has_token", False)
        has_account = data.get("has_account", False)
        ad_account_id = data.get("ad_account_id", "")
        has_secret = data.get("has_secret", False)
        api_version = data.get("api_version", "?")

        if has_token and has_account:
            status = Stat("Status", "ready", "success")
        else:
            status = Stat("Status", "missing credentials", "warning")

        rows = [
            [
                "Access Token",
                Badge("configured", "success") if has_token else Badge("MISSING", "danger"),
            ],
            [
                "Ad Account",
                Cell(ad_account_id, mono=True) if ad_account_id else Badge("MISSING", "danger"),
            ],
            [
                "App Secret",
                Badge("configured", "success") if has_secret else Badge("not set", "default"),
            ],
            [
                "API Version",
                Cell(api_version, mono=True),
            ],
        ]

        return Layout(
            stats=[status],
            tables=[
                Table(
                    columns=["Setting", "Value"],
                    rows=rows,
                    empty="No configuration",
                ),
            ],
            refresh_interval=60,
        )
