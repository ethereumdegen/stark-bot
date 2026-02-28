"""Declarative dashboard for the Spot Trader module."""

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


def _decision_badge(decision: str) -> Badge:
    m = {"BUY": "success", "SELL": "danger", "HOLD": "default"}
    return Badge(decision, m.get(decision, "default"))


def _status_badge(status: str) -> Badge:
    ok = {"executed", "broadcasted"}
    pending = {"signed", "tx_constructed", "pending"}
    fail = {"failed", "reverted", "broadcast_failed", "quote_failed"}
    if status in ok:
        return Badge(status, "success")
    if status in pending:
        return Badge(status, "warning")
    if status in fail:
        return Badge(status, "danger")
    return Badge(status, "default")


class SpotTraderDashboard(Dashboard):
    title = "Spot Trader"

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
            config_resp = self.api("/rpc/config")
            config = config_resp.get("data", {})
        except Exception:
            config = {}

        try:
            hist_resp = self.api("/rpc/history", {"limit": 30})
            decisions = hist_resp.get("data", []) or []
        except Exception:
            decisions = []

        try:
            port_resp = self.api("/rpc/portfolio")
            portfolio = port_resp.get("data", []) or []
        except Exception:
            portfolio = []

        try:
            trades_resp = self.api("/rpc/trade_history")
            trades = trades_resp.get("data", []) or []
        except Exception:
            trades = []

        # Worker info
        worker_running = status.get("worker_running", False)
        sim_mode = config.get("simulation_mode") or status.get("simulation_mode", "partner")
        signal_mode = config.get("signal_mode", "dexscreener")

        # Stats
        stat_list = [
            Stat("Decisions", stats.get("total_decisions", 0)),
            Stat("Buys", stats.get("buys", 0), "success"),
            Stat("Sells", stats.get("sells", 0), "danger"),
            Stat("Holds", stats.get("holds", 0)),
            Stat("Executed", stats.get("executed", 0), "success"),
            Stat("Failed", stats.get("failed", 0), "danger"),
        ]

        # PnL stats
        if pnl:
            total_pnl = pnl.get("total_pnl")
            realized = pnl.get("total_realized_pnl")
            unrealized = pnl.get("total_unrealized_pnl")
            win_rate = pnl.get("win_rate")
            stat_list.extend([
                Stat("Total P&L", f"${_pnl_fmt(total_pnl)}", "success" if (total_pnl or 0) >= 0 else "danger"),
                Stat("Realized", f"${_pnl_fmt(realized)}", "success" if (realized or 0) >= 0 else "danger"),
                Stat("Unrealized", f"${_pnl_fmt(unrealized)}", "success" if (unrealized or 0) >= 0 else "danger"),
                Stat("Win Rate", f"{(win_rate or 0) * 100:.1f}% ({pnl.get('win_count', 0)}W/{pnl.get('loss_count', 0)}L)"),
            ])

        # Meta — status line
        trade_mode_label = "ROGUE (Live)" if sim_mode == "rogue" else "PARTNER (Paper)"
        worker_label = "Running" if worker_running else "Stopped"
        meta = {
            "Worker": worker_label,
            "Trade Mode": trade_mode_label,
            "Signal": signal_mode,
        }
        if status.get("last_pulse_at"):
            meta["Last Pulse"] = status["last_pulse_at"]

        # Buttons
        button_list = [
            Button("Refresh Prices", "/rpc/refresh"),
            Button("Trigger Pulse", "/rpc/control", body={"action": "trigger"}, variant="success"),
            Button("Start Worker", "/rpc/control", body={"action": "start"}),
            Button("Stop Worker", "/rpc/control", body={"action": "stop"}, variant="danger"),
            Button(
                "Partner (Paper)", "/rpc/config",
                body={"key": "simulation_mode", "value": "partner"},
                variant="success" if sim_mode == "partner" else "default",
            ),
            Button(
                "Rogue (Live)", "/rpc/config",
                body={"key": "simulation_mode", "value": "rogue"},
                variant="danger" if sim_mode == "rogue" else "default",
            ),
            Button(
                "DexScreener", "/rpc/config",
                body={"key": "signal_mode", "value": "dexscreener"},
                variant="success" if signal_mode == "dexscreener" else "default",
            ),
            Button(
                "Bankr Signals", "/rpc/config",
                body={"key": "signal_mode", "value": "bankr"},
                variant="success" if signal_mode == "bankr" else "default",
            ),
        ]

        # Decision rows
        dec_rows = []
        for r in decisions:
            reason = r.get("reason") or "\u2014"
            if len(reason) > 80:
                reason = reason[:77] + "..."
            dec_rows.append([
                str(r.get("id", "")),
                _decision_badge(r.get("decision", "")),
                r.get("token_symbol") or "\u2014",
                reason,
                _status_badge(r.get("status", "")),
                r.get("created_at") or "",
            ])

        # Portfolio rows
        port_rows = []
        for r in portfolio:
            cost = float(r.get("total_cost_usd") or 0)
            unrealized_pnl = float(r.get("unrealized_pnl_usd") or 0)
            pct = (unrealized_pnl / cost * 100) if cost > 0 else 0
            addr = r.get("token_address") or ""
            addr_short = f"{addr[:6]}...{addr[-4:]}" if len(addr) > 10 else (addr or "\u2014")
            cur_price = r.get("current_price_usd")
            cur_str = f"${float(cur_price):.6f}" if cur_price is not None else "\u2014"
            port_rows.append([
                r.get("token_symbol") or "?",
                Cell(addr_short, mono=True, title=addr),
                f"${cost:.2f}",
                cur_str,
                Cell(f"${_pnl_fmt(unrealized_pnl)}", color=_pnl_color(unrealized_pnl)),
                Cell(f"{pct:+.1f}%", color=_pnl_color(pct)),
                str(r.get("num_buys", 0)),
                r.get("updated_at") or "",
            ])

        # Trade history rows
        trade_rows = []
        for r in trades:
            rpnl = r.get("realized_pnl")
            tx = r.get("tx_hash") or ""
            tx_short = f"{tx[:10]}..." if len(tx) > 12 else (tx or "\u2014")
            val_usd = r.get("value_usd")
            val_str = f"${float(val_usd):.2f}" if val_usd is not None else "\u2014"
            side = r.get("side", "")
            side_badge = Badge(side, "success" if side == "BUY" else "danger") if side else Badge("-", "default")
            trade_rows.append([
                r.get("token_symbol") or "?",
                side_badge,
                val_str,
                Cell(f"${_pnl_fmt(rpnl)}", color=_pnl_color(rpnl)) if rpnl is not None else "\u2014",
                Cell(tx_short, mono=True, title=tx),
                r.get("created_at") or "",
            ])

        return Layout(
            stats=stat_list,
            buttons=button_list,
            meta=meta,
            tables=[
                Table(
                    columns=["ID", "Decision", "Token", "Reason", "Status", "Time"],
                    rows=dec_rows,
                    title="Recent Decisions",
                    empty="No decisions yet.",
                ),
                Table(
                    columns=["Token", "Address", "Cost Basis", "Current Price", "Unrealized P&L", "P&L %", "Buys", "Updated"],
                    rows=port_rows,
                    title="Portfolio",
                    empty="No positions.",
                ),
                Table(
                    columns=["Token", "Side", "Value USD", "Realized P&L", "TX", "Time"],
                    rows=trade_rows,
                    title="Trade History",
                    empty="No trades yet.",
                ),
            ],
            refresh_interval=15,
            navigable_table=-1,
        )
