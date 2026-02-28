"""Declarative dashboard for the Discord Tipping module."""

from __future__ import annotations

from starkbot_sdk.dashboard import (
    Badge,
    Cell,
    Dashboard,
    Layout,
    Stat,
    Table,
)


_CSV_EXTRA_HTML = """\
<div class="actions">
  <a class="btn" href="rpc/csv/export" download>Export CSV</a>
  <button class="btn" id="importBtn">Import CSV</button>
  <input type="file" id="csvFile" accept=".csv" hidden>
</div>
<script>
document.getElementById('importBtn').addEventListener('click',function(){document.getElementById('csvFile').click()});
document.getElementById('csvFile').addEventListener('change',function(e){
  var f=e.target.files[0];if(!f)return;
  var fd=new FormData();fd.append('file',f);
  fetch('rpc/csv/import',{method:'POST',body:fd})
    .then(function(r){return r.json()})
    .then(function(d){if(d.success){showToast('Imported '+(d.data&&d.data.imported||0)+' profile(s)',true);setTimeout(function(){location.reload()},400)}else{showToast(d.error||'Import failed',false)}})
    .catch(function(err){showToast('Import error',false)});
});
</script>
"""


class DiscordTippingDashboard(Dashboard):
    title = "Discord Tipping"

    def layout(self) -> Layout:
        try:
            stats_resp = self.api("/rpc/profile", {"action": "stats"})
            stats = stats_resp.get("data", {})
        except Exception:
            stats = {}

        try:
            list_resp = self.api("/rpc/profile", {"action": "list"})
            profiles = list_resp.get("data", []) or []
        except Exception:
            profiles = []

        total = stats.get("total_profiles", 0)
        registered = stats.get("registered_count", 0)
        unregistered = stats.get("unregistered_count", 0)

        rows = []
        for p in profiles:
            addr = p.get("public_address") or ""
            addr_display = f"{addr[:6]}...{addr[-4:]}" if len(addr) > 10 else (addr or "\u2014")
            status = p.get("registration_status", "unregistered")
            if status == "registered":
                badge = Badge("registered", "success")
            else:
                badge = Badge("unregistered", "default")

            rows.append([
                Cell(p.get("discord_user_id", ""), mono=True),
                p.get("discord_username") or "\u2014",
                Cell(addr_display, mono=True, title=addr if len(addr) > 10 else None),
                badge,
                p.get("updated_at", "\u2014"),
            ])

        return Layout(
            stats=[
                Stat("Total Profiles", total),
                Stat("Registered", registered, "success"),
                Stat("Unregistered", unregistered),
            ],
            tables=[
                Table(
                    columns=["Discord ID", "Username", "Address", "Status", "Updated"],
                    rows=rows,
                    empty="No profiles yet",
                ),
            ],
            extra_html=_CSV_EXTRA_HTML,
        )
