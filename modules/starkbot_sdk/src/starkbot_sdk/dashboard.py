"""Declarative dashboard framework for StarkBot modules.

One ``layout()`` definition per module renders to both HTML and TUI
automatically via ``register_dashboard()``.
"""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from flask import Flask

import httpx
from markupsafe import escape
from rich.console import Console, Group, RenderableType
from rich.panel import Panel
from rich.table import Table as RichTable
from rich.text import Text

log = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Data classes
# ---------------------------------------------------------------------------


@dataclass
class Stat:
    """Stat card displayed in the dashboard header."""

    label: str
    value: str | int | float
    variant: str | None = None  # "success" / "warning" / "danger" / None


@dataclass
class Badge:
    """Status pill inside a table cell."""

    text: str
    variant: str = "default"  # "success" / "warning" / "danger" / "default"


@dataclass
class Cell:
    """Styled table cell."""

    text: str
    color: str | None = None
    mono: bool = False
    title: str | None = None  # HTML tooltip


@dataclass
class Column:
    """Column definition with optional styling."""

    header: str
    mono: bool = False


@dataclass
class Table:
    """Table specification."""

    columns: list[str | Column]
    rows: list[list[str | Badge | Cell]]
    title: str | None = None
    empty: str = "No data"


@dataclass
class Button:
    """Action button that calls an RPC endpoint."""

    label: str
    endpoint: str
    body: dict | None = None
    variant: str = "default"  # "success" / "warning" / "danger" / "default"
    confirm: str | None = None  # confirmation prompt before calling


@dataclass
class Layout:
    """Complete dashboard layout."""

    stats: list[Stat]
    tables: list[Table]
    buttons: list[Button] = field(default_factory=list)
    meta: dict[str, str] | None = None
    warnings: list[str] = field(default_factory=list)
    navigable_table: int = 0
    refresh_interval: int = 30
    extra_html: str = ""  # raw HTML injected between buttons and tables


# ---------------------------------------------------------------------------
# Dashboard base class
# ---------------------------------------------------------------------------


class Dashboard:
    """Base class for declarative module dashboards.

    Subclass and override ``layout()`` to return a :class:`Layout`.
    """

    title = "Module"

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

    def layout(self) -> Layout:
        """Override to return the dashboard layout."""
        raise NotImplementedError("Subclass must implement layout()")

    def actions(self) -> dict[str, Any]:
        """Return action metadata for interactive TUI mode."""
        return {"navigable": False, "actions": []}

    def handle_action(
        self, action: str, state: dict, inputs: list[str] | None = None
    ) -> dict[str, Any]:
        """Execute a mutation action."""
        return {"ok": False, "error": f"Unknown action: {action}"}

    def _get_entry_count(self) -> int:
        """Return the total number of navigable rows."""
        return 0


# ---------------------------------------------------------------------------
# HTML renderer — palette & constants
# ---------------------------------------------------------------------------

_VARIANT_COLORS = {
    "success": "#4ade80",
    "warning": "#facc15",
    "danger": "#f87171",
    None: "#e0e0e0",
}

_BADGE_STYLES = {
    "success": "background:#166534;color:#fff",
    "warning": "background:#854d0e;color:#fff",
    "danger": "background:#991b1b;color:#fff",
    "default": "background:#2a2a2a;color:#999",
}

_CSS = """\
*{box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:0;padding:2rem;background:#0c0c0c;color:#e0e0e0}
.header{display:flex;align-items:center;justify-content:space-between;margin-bottom:1rem}
.header h1{margin:0;font-size:1.4rem;color:#fff}
.meta{color:#ccc;font-size:0.9rem;margin-bottom:1rem}
.meta span{margin-right:1.2rem}
.meta .meta-value{color:#e0e0e0;font-weight:600}
.stats{display:flex;gap:1rem;margin-bottom:1.5rem;flex-wrap:wrap}
.stat{background:#141414;border:1px solid #252525;border-radius:8px;padding:1rem 1.5rem}
.stat .value{font-size:1.8rem;font-weight:bold}
.stat .label{font-size:0.8rem;color:#777;margin-top:2px}
.warning-banner{background:#1a1400;border:1px solid #854d0e;border-radius:8px;padding:0.75rem 1rem;margin-bottom:1rem;color:#facc15}
h2{color:#ccc;font-size:1rem;margin-top:2rem;margin-bottom:0.5rem;font-weight:600}
table{border-collapse:collapse;width:100%;background:#141414;border-radius:8px;overflow:hidden;margin-bottom:1.5rem}
th,td{padding:0.5rem 0.75rem;text-align:left;border-bottom:1px solid #1e1e1e}
th{background:#0f0f0f;color:#777;font-weight:600;font-size:0.8rem;text-transform:uppercase;letter-spacing:0.03em}
td{font-size:0.85rem}
.badge{display:inline-block;padding:2px 8px;border-radius:12px;font-size:0.75rem;font-weight:500}
.mono{font-family:'SF Mono',SFMono-Regular,Consolas,monospace}
.actions{display:flex;gap:0.5rem;margin-bottom:1.5rem;flex-wrap:wrap}
.btn{padding:0.4rem 0.9rem;border:1px solid #333;border-radius:6px;background:#1a1a1a;color:#e0e0e0;cursor:pointer;font-size:0.8rem;font-family:inherit;transition:background .15s}
.btn:hover{background:#252525}
.btn-success{background:#166534;border-color:#166534;color:#fff}
.btn-success:hover{background:#15803d}
.btn-danger{background:#991b1b;border-color:#991b1b;color:#fff}
.btn-danger:hover{background:#b91c1c}
.btn-warning{background:#854d0e;border-color:#854d0e;color:#fff}
.btn-warning:hover{background:#a16207}
.toast{position:fixed;top:1rem;right:1rem;padding:0.6rem 1.2rem;border-radius:8px;font-size:0.85rem;z-index:999;animation:fadeout 3s forwards}
.toast-ok{background:#166534;color:#fff}
.toast-err{background:#991b1b;color:#fff}
@keyframes fadeout{0%,70%{opacity:1}100%{opacity:0}}
"""

_JS = """\
function showToast(m,ok){var t=document.createElement('div');t.className='toast '+(ok?'toast-ok':'toast-err');t.textContent=m;document.body.appendChild(t);setTimeout(function(){t.remove()},3000)}
async function callRpc(url,body,msg){if(msg&&!confirm(msg))return;try{var r=await fetch(url,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});var d=await r.json();showToast(d.data&&d.data.message||d.error||'Done',d.success);if(d.success)setTimeout(function(){location.reload()},400)}catch(e){showToast('Request failed',false)}}
document.querySelectorAll('[data-endpoint]').forEach(function(b){b.addEventListener('click',function(){callRpc(b.dataset.endpoint,JSON.parse(b.dataset.body||'{}'),b.dataset.confirm||null)})});
"""


# ---------------------------------------------------------------------------
# HTML renderer — helpers
# ---------------------------------------------------------------------------


def _cell_html(value: str | Badge | Cell, col_mono: bool = False) -> str:
    """Render a cell value to HTML."""
    if isinstance(value, Badge):
        style = _BADGE_STYLES.get(value.variant, _BADGE_STYLES["default"])
        return f'<span class="badge" style="{style}">{escape(value.text)}</span>'
    if isinstance(value, Cell):
        parts: list[str] = []
        if value.mono or col_mono:
            parts.append("font-family:'SF Mono',SFMono-Regular,Consolas,monospace")
        if value.color:
            parts.append(f"color:{value.color}")
        style_attr = f' style="{";".join(parts)}"' if parts else ""
        title_attr = f' title="{escape(value.title)}"' if value.title else ""
        return f"<span{style_attr}{title_attr}>{escape(value.text)}</span>"
    # Plain string
    text = escape(str(value))
    if col_mono:
        return f'<span class="mono">{text}</span>'
    return str(text)


def _resolve_columns(raw: list[str | Column]) -> list[Column]:
    return [Column(header=c) if isinstance(c, str) else c for c in raw]


def _render_table_html(table: Table) -> str:
    """Render a Table to HTML."""
    cols = _resolve_columns(table.columns)
    title_html = f"<h2>{escape(table.title)}</h2>" if table.title else ""
    header_html = "".join(f"<th>{escape(c.header)}</th>" for c in cols)

    if table.rows:
        rows_html = ""
        for row in table.rows:
            cells = ""
            for j, val in enumerate(row):
                col_mono = cols[j].mono if j < len(cols) else False
                cells += f"<td>{_cell_html(val, col_mono)}</td>"
            rows_html += f"<tr>{cells}</tr>"
    else:
        colspan = len(cols)
        rows_html = (
            f'<tr><td colspan="{colspan}" style="text-align:center;color:#555;">'
            f"{escape(table.empty)}</td></tr>"
        )

    return (
        f"{title_html}\n<table>\n"
        f"  <thead><tr>{header_html}</tr></thead>\n"
        f"  <tbody>{rows_html}</tbody>\n"
        f"</table>"
    )


def _render_buttons_html(buttons: list[Button]) -> str:
    if not buttons:
        return ""
    items: list[str] = []
    for btn in buttons:
        cls = f"btn btn-{btn.variant}" if btn.variant != "default" else "btn"
        body_attr = ""
        if btn.body:
            body_attr = f" data-body='{escape(json.dumps(btn.body))}'"
        confirm_attr = ""
        if btn.confirm:
            confirm_attr = f' data-confirm="{escape(btn.confirm)}"'
        items.append(
            f'<button class="{cls}" data-endpoint="{escape(btn.endpoint)}"'
            f"{body_attr}{confirm_attr}>{escape(btn.label)}</button>"
        )
    return f'<div class="actions">{"".join(items)}</div>\n'


# ---------------------------------------------------------------------------
# HTML renderer — main
# ---------------------------------------------------------------------------


def render_dashboard_html(title: str, layout: Layout) -> str:
    """Generate a complete HTML page from a Layout."""
    # Stats
    stats_html = ""
    for stat in layout.stats:
        color = _VARIANT_COLORS.get(stat.variant, _VARIANT_COLORS[None])
        stats_html += (
            f'<div class="stat">'
            f'<div class="value" style="color:{color}">{escape(str(stat.value))}</div>'
            f'<div class="label">{escape(stat.label)}</div>'
            f"</div>\n"
        )

    # Meta
    meta_html = ""
    if layout.meta:
        items = "".join(
            f'<span>{escape(k)}: <span class="meta-value">{escape(str(v))}</span></span>'
            for k, v in layout.meta.items()
        )
        meta_html = f'<div class="meta">{items}</div>\n'

    # Warnings
    warnings_html = ""
    for w in layout.warnings:
        warnings_html += f'<div class="warning-banner">{escape(w)}</div>\n'

    # Buttons
    buttons_html = _render_buttons_html(layout.buttons)

    # Tables
    tables_html = "\n".join(_render_table_html(t) for t in layout.tables)

    refresh_ms = layout.refresh_interval * 1000

    return (
        f"<!DOCTYPE html>\n<html><head>\n<title>{escape(title)}</title>\n"
        f"<style>\n{_CSS}</style>\n"
        f"</head><body>\n"
        f'<div class="header"><h1>{escape(title)}</h1></div>\n'
        f"{meta_html}"
        f"{warnings_html}"
        f'<div class="stats">\n{stats_html}</div>\n'
        f"{buttons_html}"
        f"{layout.extra_html}"
        f"{tables_html}\n"
        f"<script>\n{_JS}\n"
        f"setTimeout(function(){{location.reload()}},{refresh_ms});\n"
        f"</script>\n"
        f"</body></html>"
    )


# ---------------------------------------------------------------------------
# TUI renderer
# ---------------------------------------------------------------------------

_VARIANT_RICH_BOLD = {
    "success": "bold green",
    "warning": "bold yellow",
    "danger": "bold red",
    None: "bold green",
}

_VARIANT_RICH_DIM = {
    "success": "green",
    "warning": "yellow",
    "danger": "red",
    None: "dim",
}

_BADGE_RICH_STYLE = {
    "success": "green",
    "warning": "yellow",
    "danger": "red",
    "default": "dim",
}


def _cell_tui_markup(value: str | Badge | Cell) -> str:
    """Convert a cell value to Rich markup string."""
    if isinstance(value, Badge):
        style = _BADGE_RICH_STYLE.get(value.variant, "dim")
        return f"[{style}]{value.text}[/{style}]"
    if isinstance(value, Cell):
        if value.color:
            return f"[{value.color}]{value.text}[/{value.color}]"
        return value.text
    return str(value)


def render_dashboard_tui(
    title: str,
    layout: Layout,
    width: int = 120,
    state: dict | None = None,
    actions_meta: dict | None = None,
) -> RenderableType:
    """Generate a Rich renderable from a Layout."""
    selected = state.get("selected", -1) if state else -1
    scroll = state.get("scroll", 0) if state else 0
    interactive = state is not None

    # -- Header panel with stats inline ---------------------------------
    header_text = Text()
    header_text.append(title, style="bold cyan")
    for stat in layout.stats:
        header_text.append("  |  ", style="dim")
        dim_style = _VARIANT_RICH_DIM.get(stat.variant, "dim")
        bold_style = _VARIANT_RICH_BOLD.get(stat.variant, "bold green")
        header_text.append(f"{stat.label} ", style=dim_style)
        header_text.append(str(stat.value), style=bold_style)

    if layout.meta:
        for label, value in layout.meta.items():
            header_text.append("  |  ", style="dim")
            header_text.append(f"{label} ", style="dim")
            header_text.append(str(value), style="yellow")

    header = Panel(header_text, border_style="bright_blue", padding=(0, 1))
    parts: list[RenderableType] = [header]

    # -- Tables ---------------------------------------------------------
    for i, table_spec in enumerate(layout.tables):
        is_nav = i == layout.navigable_table
        cur_selected = selected if is_nav else -1
        cur_scroll = scroll if is_nav else 0

        cols = _resolve_columns(table_spec.columns)

        kwargs: dict[str, Any] = dict(
            show_header=True,
            header_style="bold bright_blue",
            border_style="bright_black",
            expand=True,
            pad_edge=True,
        )
        if table_spec.title:
            kwargs["title"] = table_spec.title
            kwargs["title_style"] = "bold cyan"

        rich_table = RichTable(**kwargs)

        if is_nav:
            rich_table.add_column("#", style="dim", width=4)
        for col in cols:
            col_style = "cyan" if col.mono else "white"
            rich_table.add_column(col.header, style=col_style, ratio=1)

        # Visible window
        max_visible = 20
        visible_rows = table_spec.rows[cur_scroll : cur_scroll + max_visible]

        if table_spec.rows:
            for j, row in enumerate(visible_rows):
                row_idx = cur_scroll + j
                is_selected = row_idx == cur_selected
                cells: list[str] = []
                if is_nav:
                    idx_str = str(row_idx)
                    if is_selected:
                        idx_str = f"[reverse] {idx_str} [/reverse]"
                    cells.append(idx_str)
                for val in row:
                    markup = _cell_tui_markup(val)
                    if is_selected:
                        markup = f"[reverse]{markup}[/reverse]"
                    cells.append(markup)
                rich_table.add_row(*cells)
        else:
            empty_cells: list[str] = []
            if is_nav:
                empty_cells.append("")
            empty_cells.append(f"[dim]{table_spec.empty}[/dim]")
            empty_cells.extend("[dim]\u2014[/dim]" for _ in range(len(cols) - 1))
            rich_table.add_row(*empty_cells)

        parts.append(rich_table)

        # Scroll indicator for navigable table
        if is_nav and len(table_spec.rows) > max_visible:
            end = min(cur_scroll + max_visible, len(table_spec.rows))
            parts.append(
                Text(
                    f"  Showing {cur_scroll + 1}-{end} of {len(table_spec.rows)}",
                    style="dim",
                )
            )

    # -- Warnings -------------------------------------------------------
    for w in layout.warnings:
        parts.append(Text(f"  \u26a0 {w}", style="bold yellow"))

    # -- Footer ---------------------------------------------------------
    if interactive and actions_meta:
        footer = Text()
        if actions_meta.get("navigable"):
            footer.append("  \u2191\u2193", style="bold white")
            footer.append(" navigate  ", style="dim")
        for act in actions_meta.get("actions", []):
            key = act.get("key", "")
            label = act.get("label", "")
            if not key:
                continue
            act_name = act.get("action", "").lower()
            if "delete" in act_name:
                key_style = "bold red"
            elif "add" in act_name:
                key_style = "bold green"
            elif "edit" in act_name:
                key_style = "bold yellow"
            elif "refresh" in act_name:
                key_style = "bold cyan"
            else:
                key_style = "bold white"
            footer.append(key, style=key_style)
            footer.append(f" {label.lower()}  ", style="dim")
        footer.append("q", style="bold white")
        footer.append(" quit", style="dim")
        parts.append(footer)
    elif interactive:
        parts.append(Text("  q: quit  |  Ctrl+C: exit", style="dim"))

    return Group(*parts)


# ---------------------------------------------------------------------------
# Registration
# ---------------------------------------------------------------------------


def register_dashboard(
    flask_app: Flask,
    dashboard_class: type[Dashboard],
    module_url: str,
) -> None:
    """Wire up both HTML and TUI dashboard routes on a Flask app.

    Routes:
      GET  /                            — HTML dashboard
      GET  /rpc/dashboard/tui           — ANSI TUI rendering
      GET  /rpc/dashboard/tui/actions   — action metadata as JSON
      POST /rpc/dashboard/tui/action    — execute a mutation action
    """
    from flask import Response, jsonify, request

    from starkbot_sdk.tui import notify_tui_update

    @flask_app.route("/")
    def _dashboard_html():
        dashboard = dashboard_class(module_url)
        lay = dashboard.layout()
        return render_dashboard_html(dashboard.title, lay)

    @flask_app.route("/rpc/dashboard/tui", methods=["GET"])
    def _dashboard_tui():
        width = request.args.get("width", 120, type=int)
        height = request.args.get("height", 40, type=int)
        state: dict[str, Any] = {}
        if "selected" in request.args:
            state["selected"] = request.args.get("selected", 0, type=int)
        if "scroll" in request.args:
            state["scroll"] = request.args.get("scroll", 0, type=int)
        dashboard = dashboard_class(module_url)
        lay = dashboard.layout()
        actions_meta = dashboard.actions()
        renderable = render_dashboard_tui(
            dashboard.title, lay, width, state or None, actions_meta
        )
        console = Console(
            record=True, width=width, height=height, force_terminal=True
        )
        console.print(renderable)
        ansi = console.export_text(styles=True)
        return Response(ansi, content_type="text/plain; charset=utf-8")

    @flask_app.route("/rpc/dashboard/tui/actions", methods=["GET"])
    def _tui_actions():
        dashboard = dashboard_class(module_url)
        return jsonify(dashboard.actions())

    @flask_app.route("/rpc/dashboard/tui/action", methods=["POST"])
    def _tui_action():
        data = request.get_json(silent=True) or {}
        action = data.get("action", "")
        st = data.get("state", {})
        inputs = data.get("inputs")
        dashboard = dashboard_class(module_url)
        result = dashboard.handle_action(action, st, inputs)
        if result.get("ok"):
            notify_tui_update(flask_app.name)
        return jsonify(result)
