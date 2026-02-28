"""Declarative dashboard for the HyperClaw module."""

from __future__ import annotations

from starkbot_sdk.dashboard import (
    Badge,
    Button,
    Cell,
    Dashboard,
    Layout,
    Stat,
    Table,
)


def _pnl_color(v) -> str:
    if v is None:
        return "#777"
    v = float(v)
    if v > 0:
        return "#4ade80"
    if v < 0:
        return "#f87171"
    return "#777"


def _pnl_fmt(v) -> str:
    if v is None:
        return "\u2014"
    v = float(v)
    return f"{v:+.2f}"


def _price_fmt(v) -> str:
    if v is None or float(v) == 0:
        return "\u2014"
    v = float(v)
    return f"${v:.2f}" if v >= 1 else f"${v:.6f}"


def _side_badge(side: str) -> Badge:
    return Badge("LONG", "success") if side == "LONG" else Badge("SHORT", "danger")


def _decision_badge(decision: str) -> Badge:
    m = {
        "OPEN_LONG": "success", "OPEN_SHORT": "danger",
        "CLOSE": "warning", "HOLD": "default",
        "REGISTER": "warning", "ADD_KEY": "warning",
        "DEPOSIT": "warning", "APPROVE_USDC": "warning",
    }
    return Badge(decision, m.get(decision, "default"))


def _status_badge(status: str) -> Badge:
    ok = {"executed", "completed", "broadcasted"}
    pending = {"signed", "tx_constructed", "pending"}
    fail = {"failed", "reverted", "broadcast_failed"}
    if status in ok:
        return Badge(status, "success")
    if status in pending:
        return Badge(status, "warning")
    if status in fail:
        return Badge(status, "danger")
    return Badge(status, "default")


class HyperClawDashboard(Dashboard):
    title = "HyperClaw"

    def layout(self) -> Layout:
        try:
            stats_resp = self.api("/rpc/stats")
            stats = stats_resp.get("data", {})
        except Exception:
            stats = {}

        try:
            pnl_resp = self.api("/rpc/pnl")
            pnl = pnl_resp.get("data", {})
        except Exception:
            pnl = {}

        try:
            status_resp = self.api("/rpc/status")
            status = status_resp.get("data", {})
        except Exception:
            status = {}

        try:
            pos_resp = self.api("/rpc/positions")
            positions = pos_resp.get("data", []) or []
        except Exception:
            positions = []

        try:
            hist_resp = self.api("/rpc/history", {"limit": 30})
            decisions = hist_resp.get("data", []) or []
        except Exception:
            decisions = []

        try:
            trades_resp = self.api("/rpc/trade_history")
            trades = trades_resp.get("data", []) or []
        except Exception:
            trades = []

        worker_running = status.get("worker_running", False)
        registered = status.get("registered") == "true"
        key_added = status.get("key_added") == "true"

        # Stats
        stat_list = [
            Stat("Positions", stats.get("open_positions", 0)),
            Stat("Decisions", stats.get("total_decisions", 0)),
            Stat("Longs", stats.get("open_longs", 0), "success"),
            Stat("Shorts", stats.get("open_shorts", 0), "danger"),
            Stat("Closes", stats.get("closes", 0)),
            Stat("Executed", stats.get("executed", 0), "success"),
            Stat("Failed", stats.get("failed", 0), "danger"),
        ]

        if pnl:
            total_pnl = pnl.get("total_pnl")
            realized = pnl.get("total_realized_pnl")
            unrealized = pnl.get("total_unrealized_pnl")
            total_trades = pnl.get("total_trades", 0)
            win_rate = pnl.get("win_rate")
            wr_str = f"{(win_rate or 0) * 100:.1f}%" if total_trades > 0 else "N/A"
            stat_list.extend([
                Stat("Total P&L", f"${_pnl_fmt(total_pnl)}", "success" if (total_pnl or 0) >= 0 else "danger"),
                Stat("Realized", f"${_pnl_fmt(realized)}", "success" if (realized or 0) >= 0 else "danger"),
                Stat("Unrealized", f"${_pnl_fmt(unrealized)}", "success" if (unrealized or 0) >= 0 else "danger"),
                Stat("Win Rate", f"{wr_str} ({pnl.get('win_count', 0)}W/{pnl.get('loss_count', 0)}L)"),
            ])

        # Meta — status line
        reg_label = "Registered" if registered else "Not Registered"
        worker_label = "Running" if worker_running else "Stopped"
        meta = {
            "Worker": worker_label,
            "Orderly": reg_label,
        }
        if key_added:
            meta["Key"] = "Active"
        if status.get("last_pulse_at"):
            meta["Last Pulse"] = status["last_pulse_at"]

        # Warnings
        warnings = []
        if not registered:
            warnings.append("Not registered with Orderly — run registration flow before trading")

        # Buttons
        button_list = [
            Button("Refresh Positions", "/rpc/refresh"),
            Button("Trigger Pulse", "/rpc/control", body={"action": "trigger"}, variant="success"),
            Button("Start Worker", "/rpc/control", body={"action": "start"}),
            Button("Stop Worker", "/rpc/control", body={"action": "stop"}, variant="danger"),
        ]

        # Position rows
        pos_rows = []
        for r in positions:
            pnl_pct = float(r.get("pnl_pct") or 0)
            unrealized_pnl = float(r.get("unrealized_pnl") or 0)
            pos_rows.append([
                r.get("symbol", ""),
                _side_badge(r.get("side", "")),
                f"{float(r.get('qty', 0)):.4f}",
                _price_fmt(r.get("avg_open_price")),
                _price_fmt(r.get("mark_price")),
                _price_fmt(r.get("liquidation_price")),
                Cell(f"{_pnl_fmt(pnl_pct)}%", color=_pnl_color(pnl_pct)),
                Cell(f"${_pnl_fmt(unrealized_pnl)}", color=_pnl_color(unrealized_pnl)),
            ])

        # Trade history rows
        trade_rows = []
        for r in trades:
            rpnl = float(r.get("realized_pnl") or 0)
            rpct = float(r.get("pnl_pct") or 0)
            trade_rows.append([
                r.get("symbol", ""),
                _side_badge(r.get("side", "")),
                f"{float(r.get('qty', 0)):.4f}" if r.get("qty") is not None else "\u2014",
                _price_fmt(r.get("avg_open_price")),
                _price_fmt(r.get("close_price")),
                Cell(f"${_pnl_fmt(rpnl)}", color=_pnl_color(rpnl)),
                Cell(f"{_pnl_fmt(rpct)}%", color=_pnl_color(rpct)),
                r.get("closed_at") or "",
            ])

        # Decision rows
        dec_rows = []
        for r in decisions:
            reason = r.get("reason") or "\u2014"
            if len(reason) > 80:
                reason = reason[:77] + "..."
            dec_rows.append([
                str(r.get("id", "")),
                _decision_badge(r.get("decision", "")),
                r.get("symbol") or "\u2014",
                reason,
                _status_badge(r.get("status", "")),
                r.get("created_at") or "",
            ])

        return Layout(
            stats=stat_list,
            buttons=button_list,
            meta=meta,
            warnings=warnings,
            tables=[
                Table(
                    columns=["Symbol", "Side", "Qty", "Entry", "Mark", "Liq. Price", "P&L %", "Unrealized"],
                    rows=pos_rows,
                    title="Open Positions",
                    empty="No open positions.",
                ),
                Table(
                    columns=["Symbol", "Side", "Qty", "Entry", "Exit", "Realized P&L", "P&L %", "Closed"],
                    rows=trade_rows,
                    title="Trade History",
                    empty="No trades yet.",
                ),
                Table(
                    columns=["ID", "Decision", "Symbol", "Reason", "Status", "Time"],
                    rows=dec_rows,
                    title="Recent Decisions",
                    empty="No decisions yet.",
                ),
            ],
            refresh_interval=15,
            navigable_table=-1,
        )
