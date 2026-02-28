"""Declarative dashboard for the Wallet Monitor module."""

from __future__ import annotations

from starkbot_sdk.dashboard import (
    Badge,
    Cell,
    Dashboard,
    Layout,
    Stat,
    Table,
)

_ADD_WALLET_HTML = """\
<div style="background:#141414;border:1px solid #252525;border-radius:8px;padding:1rem 1.2rem;margin-bottom:1.5rem;">
  <h2 style="margin:0 0 0.75rem;font-size:0.95rem;color:#ccc;">Add Wallet</h2>
  <div style="display:flex;gap:0.75rem;flex-wrap:wrap;align-items:flex-end;">
    <div style="display:flex;flex-direction:column;gap:4px;">
      <label style="font-size:0.75rem;color:#777;text-transform:uppercase;">Address</label>
      <input type="text" id="addr" placeholder="0x..." spellcheck="false"
             style="background:#0c0c0c;border:1px solid #333;border-radius:6px;padding:0.5rem 0.75rem;color:#e0e0e0;font-family:monospace;font-size:0.85rem;width:380px;">
    </div>
    <div style="display:flex;flex-direction:column;gap:4px;">
      <label style="font-size:0.75rem;color:#777;text-transform:uppercase;">Label</label>
      <input type="text" id="lbl" placeholder="optional"
             style="background:#0c0c0c;border:1px solid #333;border-radius:6px;padding:0.5rem 0.75rem;color:#e0e0e0;font-size:0.85rem;width:140px;">
    </div>
    <div style="display:flex;flex-direction:column;gap:4px;">
      <label style="font-size:0.75rem;color:#777;text-transform:uppercase;">Chain</label>
      <select id="chain" style="background:#0c0c0c;border:1px solid #333;border-radius:6px;padding:0.5rem 0.75rem;color:#e0e0e0;font-size:0.85rem;">
        <option value="mainnet">Mainnet</option><option value="base">Base</option>
      </select>
    </div>
    <div style="display:flex;flex-direction:column;gap:4px;">
      <label style="font-size:0.75rem;color:#777;text-transform:uppercase;">Threshold $</label>
      <input type="number" id="thr" value="1000" min="0"
             style="background:#0c0c0c;border:1px solid #333;border-radius:6px;padding:0.5rem 0.75rem;color:#e0e0e0;font-size:0.85rem;width:100px;">
    </div>
    <div style="display:flex;flex-direction:column;gap:4px;">
      <label>&nbsp;</label>
      <button class="btn btn-success" id="btn-add" onclick="addWallet()">Add</button>
    </div>
  </div>
</div>
<script>
async function rpc(body){
  var r=await fetch('rpc/tools/watchlist',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  return r.json();
}
async function addWallet(){
  var addr=document.getElementById('addr').value.trim();
  if(!addr){showToast('Address is required',false);return;}
  var btn=document.getElementById('btn-add');btn.disabled=true;
  try{
    var res=await rpc({action:'add',address:addr,label:document.getElementById('lbl').value.trim()||null,chain:document.getElementById('chain').value,threshold_usd:parseFloat(document.getElementById('thr').value)||1000});
    if(res.success){showToast('Wallet added',true);setTimeout(function(){location.reload()},400)}
    else{showToast(res.error||'Failed',false)}
  }catch(e){showToast('Network error',false)}
  btn.disabled=false;
}
async function removeWallet(id,preview){
  if(!confirm('Remove wallet '+preview+'?'))return;
  try{
    var res=await rpc({action:'remove',id:id});
    if(res.success){showToast('Removed',true);setTimeout(function(){location.reload()},400)}
    else{showToast(res.error||'Failed',false)}
  }catch(e){showToast('Network error',false)}
}
async function toggleWallet(id,enable){
  try{
    var res=await rpc({action:'update',id:id,monitor_enabled:!!enable});
    if(res.success){showToast(enable?'Resumed':'Paused',true);setTimeout(function(){location.reload()},400)}
    else{showToast(res.error||'Failed',false)}
  }catch(e){showToast('Network error',false)}
}
document.getElementById('addr').addEventListener('keydown',function(e){if(e.key==='Enter')addWallet()});
</script>
"""


class WalletMonitorDashboard(Dashboard):
    title = "Wallet Monitor"

    def layout(self) -> Layout:
        try:
            status_resp = self.api("/rpc/status")
            stats = status_resp.get("data", {})
        except Exception:
            stats = {}

        try:
            wl_resp = self.api("/rpc/tools/watchlist", {"action": "list"})
            watchlist = wl_resp.get("data", []) or []
        except Exception:
            watchlist = []

        try:
            act_resp = self.api("/rpc/tools/activity", {"action": "recent", "limit": 20})
            recent = act_resp.get("data", []) or []
        except Exception:
            recent = []

        warnings = []
        if not stats.get("worker_enabled", True):
            warnings.append("Background worker disabled \u2014 ALCHEMY_API_KEY is not set.")

        # Watchlist rows
        wl_rows = []
        for w in watchlist:
            label = w.get("label") or "-"
            enabled = w.get("monitor_enabled", False)
            status = Badge("Active", "success") if enabled else Badge("Paused", "warning")
            last_block = f'#{w["last_checked_block"]}' if w.get("last_checked_block") else "-"
            addr = w.get("address", "")
            addr_short = f"{addr[:8]}...{addr[-4:]}" if len(addr) > 14 else addr
            wid = w.get("id", "")
            toggle_val = 0 if enabled else 1
            toggle_label = "\u23f8" if enabled else "\u25b6"
            actions_html = (
                f'<button class="btn" onclick="toggleWallet({wid},{toggle_val})" '
                f'style="padding:2px 8px;font-size:0.8rem;">{toggle_label}</button> '
                f'<button class="btn btn-danger" onclick="removeWallet({wid},\'{addr_short}\')" '
                f'style="padding:2px 8px;font-size:0.8rem;">\u2715</button>'
            )
            wl_rows.append([
                str(wid),
                label,
                Cell(addr_short, mono=True, title=addr),
                w.get("chain", "-"),
                f'${w.get("large_trade_threshold_usd", 1000):.0f}',
                status,
                last_block,
                Cell(actions_html, raw=True),
            ])

        # Activity rows
        act_rows = []
        for a in recent:
            usd = f'${a["usd_value"]:.0f}' if a.get("usd_value") is not None else "-"
            asset = a.get("asset_symbol") or "ETH"
            amount = a.get("amount_formatted") or "-"
            tx = a.get("tx_hash", "")
            tx_short = f"{tx[:8]}...{tx[-4:]}" if len(tx) > 14 else (tx or "-")
            is_large = a.get("is_large_trade", False)
            type_color = "#facc15" if is_large else None
            act_rows.append([
                Cell(a.get("activity_type", ""), color=type_color),
                a.get("chain", "-"),
                f"{amount} {asset}",
                usd,
                Cell(tx_short, mono=True),
                a.get("created_at", "-"),
            ])

        return Layout(
            stats=[
                Stat("Watched Wallets", stats.get("watched_wallets", 0)),
                Stat("Active", stats.get("active_wallets", 0), "success"),
                Stat("Total Txs", stats.get("total_transactions", 0)),
                Stat("Large Trades", stats.get("large_trades", 0), "warning"),
            ],
            tables=[
                Table(
                    columns=["ID", "Label", "Address", "Chain", "Threshold", "Status", "Last Block", ""],
                    rows=wl_rows,
                    title="Watchlist",
                    empty="No wallets on watchlist. Add one below.",
                ),
                Table(
                    columns=["Type", "Chain", "Amount", "USD", "Tx", "Time"],
                    rows=act_rows,
                    title="Recent Activity",
                    empty="No activity recorded yet.",
                ),
            ],
            warnings=warnings,
            extra_html=_ADD_WALLET_HTML,
            navigable_table=-1,
        )
