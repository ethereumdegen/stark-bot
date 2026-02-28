"""Declarative dashboard for the Whale Tracker module."""

from __future__ import annotations

from starkbot_sdk.dashboard import (
    Badge,
    Cell,
    Dashboard,
    Layout,
    Stat,
    Table,
)

_ADD_WHALE_HTML = """\
<div style="background:#141414;border:1px solid #252525;border-radius:8px;padding:1rem 1.2rem;margin-bottom:1.5rem;">
  <h2 style="margin:0 0 0.75rem;font-size:0.95rem;color:#ccc;">Add Whale</h2>
  <div style="display:flex;gap:0.75rem;flex-wrap:wrap;align-items:flex-end;">
    <div style="display:flex;flex-direction:column;gap:4px;">
      <label style="font-size:0.75rem;color:#777;text-transform:uppercase;">Address</label>
      <input type="text" id="waddr" placeholder="0x..." spellcheck="false"
             style="background:#0c0c0c;border:1px solid #333;border-radius:6px;padding:0.5rem 0.75rem;color:#e0e0e0;font-family:monospace;font-size:0.85rem;width:380px;">
    </div>
    <div style="display:flex;flex-direction:column;gap:4px;">
      <label style="font-size:0.75rem;color:#777;text-transform:uppercase;">Label</label>
      <input type="text" id="wlbl" placeholder="optional"
             style="background:#0c0c0c;border:1px solid #333;border-radius:6px;padding:0.5rem 0.75rem;color:#e0e0e0;font-size:0.85rem;width:140px;">
    </div>
    <div style="display:flex;flex-direction:column;gap:4px;">
      <label style="font-size:0.75rem;color:#777;text-transform:uppercase;">Chain</label>
      <select id="wchain" style="background:#0c0c0c;border:1px solid #333;border-radius:6px;padding:0.5rem 0.75rem;color:#e0e0e0;font-size:0.85rem;">
        <option value="ethereum">Ethereum</option><option value="base">Base</option>
      </select>
    </div>
    <div style="display:flex;flex-direction:column;gap:4px;">
      <label style="font-size:0.75rem;color:#777;text-transform:uppercase;">Category</label>
      <select id="wcat" style="background:#0c0c0c;border:1px solid #333;border-radius:6px;padding:0.5rem 0.75rem;color:#e0e0e0;font-size:0.85rem;">
        <option value="individual">Individual</option><option value="fund">Fund</option><option value="exchange">Exchange</option><option value="other">Other</option>
      </select>
    </div>
    <div style="display:flex;flex-direction:column;gap:4px;">
      <label>&nbsp;</label>
      <button class="btn btn-success" id="btn-add-whale" onclick="addWhale()">Add</button>
    </div>
  </div>
</div>
<script>
async function whaleRpc(body){
  var r=await fetch('rpc/tools/whales',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  return r.json();
}
async function addWhale(){
  var addr=document.getElementById('waddr').value.trim();
  if(!addr){showToast('Address is required',false);return;}
  var btn=document.getElementById('btn-add-whale');btn.disabled=true;
  try{
    var res=await whaleRpc({action:'add',address:addr,label:document.getElementById('wlbl').value.trim()||null,chain:document.getElementById('wchain').value,category:document.getElementById('wcat').value});
    if(res.success){showToast('Whale added',true);setTimeout(function(){location.reload()},400)}
    else{showToast(res.error||'Failed',false)}
  }catch(e){showToast('Network error',false)}
  btn.disabled=false;
}
async function removeWhale(addr,preview){
  if(!confirm('Remove whale '+preview+'?'))return;
  try{
    var res=await whaleRpc({action:'remove',address:addr});
    if(res.success){showToast('Removed',true);setTimeout(function(){location.reload()},400)}
    else{showToast(res.error||'Failed',false)}
  }catch(e){showToast('Network error',false)}
}
async function toggleWhale(addr,enable){
  try{
    var res=await whaleRpc({action:'update',address:addr,enabled:!!enable});
    if(res.success){showToast(enable?'Resumed':'Paused',true);setTimeout(function(){location.reload()},400)}
    else{showToast(res.error||'Failed',false)}
  }catch(e){showToast('Network error',false)}
}
document.getElementById('waddr').addEventListener('keydown',function(e){if(e.key==='Enter')addWhale()});
</script>
"""


def _format_usd(val) -> str:
    if val is None:
        return "-"
    val = float(val)
    if val >= 1_000_000_000:
        return f"${val / 1_000_000_000:,.1f}B"
    if val >= 1_000_000:
        return f"${val / 1_000_000:,.1f}M"
    if val >= 1_000:
        return f"${val / 1_000:,.1f}K"
    return f"${val:,.0f}"


def _acc_color(pct: float) -> str:
    if pct >= 60:
        return "#4ade80"
    if pct >= 40:
        return "#facc15"
    return "#f87171"


class WhaleTrackerDashboard(Dashboard):
    title = "Whale Tracker"

    def layout(self) -> Layout:
        # Fetch all data via API
        try:
            status_resp = self.api("/rpc/status")
            stats = status_resp.get("data", {})
        except Exception:
            stats = {}

        try:
            whales_resp = self.api("/rpc/tools/whales", {"action": "list"})
            whales = whales_resp.get("data", {}).get("whales", [])
        except Exception:
            whales = []

        try:
            signals_resp = self.api("/rpc/tools/signals", {"action": "recent", "limit": 20})
            recent_signals = signals_resp.get("data", {}).get("signals", [])
        except Exception:
            recent_signals = []

        try:
            accuracy_resp = self.api("/rpc/tools/signals", {"action": "accuracy"})
            accuracy_data = accuracy_resp.get("data", {}).get("accuracy", [])
        except Exception:
            accuracy_data = []

        try:
            movements_resp = self.api("/rpc/dashboard/data")
            recent_movements = movements_resp.get("data", {}).get("recent_movements", [])
        except Exception:
            recent_movements = []

        avg_acc = stats.get("avg_accuracy")
        avg_acc_str = f"{avg_acc:.1f}%" if avg_acc is not None else "N/A"

        warnings = []
        if not stats.get("worker_enabled", True):
            warnings.append("Background worker disabled \u2014 ALCHEMY_API_KEY is not set.")

        # Leaderboard rows (top 10 by accuracy)
        leaderboard_rows = []
        for a in accuracy_data[:10]:
            label = a.get("label") or (a.get("whale_address", "")[:12] + "...")
            pct = float(a.get("accuracy_pct", 0))
            leaderboard_rows.append([
                label,
                a.get("chain", "-"),
                a.get("category", "-"),
                Cell(f"{pct:.1f}%", color=_acc_color(pct)),
                str(a.get("total_signals", 0)),
                str(a.get("correct_signals", 0)),
                f"{float(a.get('avg_price_impact_24h', 0)):.2f}%",
            ])

        # Signal rows
        signal_rows = []
        type_colors = {"bearish": "#f87171", "bullish": "#4ade80", "neutral": "#777"}
        for s in recent_signals:
            label = s.get("whale_label") or (s.get("whale_address", "")[:12] + "...")
            sig_type = s.get("signal_type", "")
            conf = float(s.get("confidence", 0))
            outcome = s.get("outcome", "pending")
            if outcome == "correct":
                outcome_badge = Badge("Correct", "success")
            elif outcome == "incorrect":
                outcome_badge = Badge("Incorrect", "danger")
            else:
                outcome_badge = Badge("Pending", "default")
            summary = s.get("summary") or "-"
            if len(summary) > 80:
                summary = summary[:77] + "..."
            signal_rows.append([
                label,
                Cell(sig_type.upper(), color=type_colors.get(sig_type, "#777")),
                s.get("token", "-"),
                Cell(f"{conf:.0f}", color=_acc_color(conf)),
                summary,
                outcome_badge,
                (s.get("timestamp") or "-")[:19],
            ])

        # Movement rows
        movement_rows = []
        for m in recent_movements:
            label = m.get("whale_label") or (m.get("whale_address", "")[:12] + "...")
            direction = m.get("direction", "")
            dir_color = "#f87171" if direction == "outflow" else "#4ade80"
            amt = m.get("amount")
            amt_str = f"{float(amt):,.2f}" if amt is not None else "-"
            tx = m.get("tx_hash") or ""
            tx_short = f"{tx[:8]}...{tx[-4:]}" if len(tx) > 14 else (tx or "-")
            cp = m.get("counterparty_label") or (
                (m.get("counterparty", "")[:12] + "...") if m.get("counterparty") else "-"
            )
            movement_rows.append([
                label,
                Cell(direction, color=dir_color),
                m.get("token_symbol", "ETH"),
                amt_str,
                _format_usd(m.get("usd_value")),
                m.get("classification", "-"),
                cp,
                m.get("size_tier", "-"),
                Cell(tx_short, mono=True),
            ])

        # Whale rows
        whale_rows = []
        for w in whales:
            label = w.get("label") or "-"
            addr = w.get("address", "")
            addr_short = f"{addr[:8]}...{addr[-4:]}" if len(addr) > 14 else addr
            tags_raw = w.get("tags")
            if isinstance(tags_raw, list):
                tags = ", ".join(tags_raw) if tags_raw else "-"
            else:
                tags = str(tags_raw) if tags_raw else "-"
            acc = w.get("accuracy_pct")
            acc_str = f"{float(acc):.1f}%" if acc is not None else "-"
            enabled = w.get("enabled", False)
            status = Badge("Active", "success") if enabled else Badge("Paused", "warning")
            toggle_val = 0 if enabled else 1
            toggle_label = "\u23f8" if enabled else "\u25b6"
            addr_js = addr.replace("'", "\\'")
            actions_html = (
                f'<button class="btn" onclick="toggleWhale(\'{addr_js}\',{toggle_val})" '
                f'style="padding:2px 8px;font-size:0.8rem;">{toggle_label}</button> '
                f'<button class="btn btn-danger" onclick="removeWhale(\'{addr_js}\',\'{addr_short}\')" '
                f'style="padding:2px 8px;font-size:0.8rem;">\u2715</button>'
            )
            whale_rows.append([
                label,
                Cell(addr_short, mono=True),
                w.get("chain", "-"),
                w.get("category", "-"),
                tags,
                acc_str,
                str(w.get("total_signals", 0)),
                status,
                Cell(actions_html, raw=True),
            ])

        return Layout(
            stats=[
                Stat("Whales Tracked", stats.get("total_whales", 0)),
                Stat("Active", stats.get("active_whales", 0), "success"),
                Stat("Movements", stats.get("total_movements", 0)),
                Stat("Signals", stats.get("total_signals", 0)),
                Stat("Pending", stats.get("pending_signals", 0)),
                Stat("Avg Accuracy", avg_acc_str),
            ],
            tables=[
                Table(
                    columns=["Whale", "Chain", "Category", "Accuracy", "Signals", "Correct", "Avg Impact 24h"],
                    rows=leaderboard_rows,
                    title="Whale Leaderboard (by Accuracy)",
                    empty="No accuracy data yet. Signals need 24h+ to resolve.",
                ),
                Table(
                    columns=["Whale", "Signal", "Token", "Confidence", "Summary", "Outcome", "Time"],
                    rows=signal_rows,
                    title="Recent Signals",
                    empty="No signals recorded yet.",
                ),
                Table(
                    columns=["Whale", "Direction", "Token", "Amount", "USD", "Classification", "Counterparty", "Size", "Tx"],
                    rows=movement_rows,
                    title="Recent Movements",
                    empty="No movements recorded yet.",
                ),
                Table(
                    columns=["Label", "Address", "Chain", "Category", "Tags", "Accuracy", "Signals", "Status", ""],
                    rows=whale_rows,
                    title="Tracked Whales",
                    empty="No whales tracked. Add one above.",
                ),
            ],
            warnings=warnings,
            extra_html=_ADD_WHALE_HTML,
            navigable_table=-1,
        )
