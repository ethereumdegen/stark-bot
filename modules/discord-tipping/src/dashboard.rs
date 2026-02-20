//! Dashboard HTML page handler for discord tipping service.

use crate::routes::AppState;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use std::sync::Arc;

pub async fn dashboard(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stats = state.db.get_stats().ok();
    let profiles = state.db.list_all_profiles().unwrap_or_default();
    let uptime = state.start_time.elapsed().as_secs();

    let stats_html = if let Some(s) = &stats {
        format!(
            r#"<div class="stats">
                <div class="stat"><span class="val">{}</span><span class="lbl">Total Profiles</span></div>
                <div class="stat green"><span class="val">{}</span><span class="lbl">Registered</span></div>
                <div class="stat yellow"><span class="val">{}</span><span class="lbl">Unregistered</span></div>
            </div>"#,
            s.total_profiles, s.registered_count, s.unregistered_count
        )
    } else {
        "<p>No stats available.</p>".to_string()
    };

    let mut registered_rows = String::new();
    let mut unregistered_chips = String::new();

    for p in &profiles {
        if p.registration_status == "registered" {
            let addr = p.public_address.as_deref().unwrap_or("-");
            let addr_short = if addr.len() > 14 {
                format!("{}...{}", &addr[..8], &addr[addr.len() - 6..])
            } else {
                addr.to_string()
            };
            registered_rows.push_str(&format!(
                "<tr><td>{}</td><td class=\"mono\">{}</td><td class=\"mono\">{}</td><td>{}</td></tr>\n",
                p.discord_username.as_deref().unwrap_or("-"),
                p.discord_user_id,
                addr_short,
                p.registered_at.as_deref().unwrap_or("-"),
            ));
        } else {
            let name = p.discord_username.as_deref().unwrap_or(&p.discord_user_id);
            unregistered_chips.push_str(&format!(
                "<span class=\"chip\">{}</span>\n",
                name
            ));
        }
    }
    if registered_rows.is_empty() {
        registered_rows = "<tr><td colspan=\"4\">No registered users yet.</td></tr>".to_string();
    }

    let uptime_str = format_uptime(uptime);

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Discord Tipping Dashboard</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0f1117; color: #e0e0e0; padding: 20px; }}
  h1 {{ color: #7289da; margin-bottom: 8px; }}
  .meta {{ color: #8b949e; font-size: 0.85em; margin-bottom: 20px; }}
  .stats {{ display: flex; gap: 16px; margin-bottom: 24px; flex-wrap: wrap; }}
  .stat {{ background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 16px 24px; text-align: center; min-width: 140px; }}
  .stat .val {{ display: block; font-size: 2em; font-weight: bold; color: #7289da; }}
  .stat.green .val {{ color: #3fb950; }}
  .stat.yellow .val {{ color: #d29922; }}
  .stat .lbl {{ display: block; font-size: 0.85em; color: #8b949e; margin-top: 4px; }}
  table {{ width: 100%; border-collapse: collapse; margin-bottom: 24px; }}
  th {{ background: #161b22; color: #8b949e; text-align: left; padding: 8px 12px; font-size: 0.85em; text-transform: uppercase; border-bottom: 1px solid #30363d; }}
  td {{ padding: 8px 12px; border-bottom: 1px solid #21262d; font-size: 0.9em; }}
  tr:hover {{ background: #161b22; }}
  .mono {{ font-family: 'SF Mono', 'Consolas', monospace; font-size: 0.85em; }}
  h2 {{ color: #c9d1d9; margin-bottom: 12px; font-size: 1.1em; }}
  .section {{ margin-bottom: 28px; }}
  .chip {{ display: inline-block; background: #21262d; border: 1px solid #30363d; color: #8b949e; padding: 4px 10px; border-radius: 12px; font-size: 0.8em; margin: 3px; }}
</style>
</head>
<body>
  <h1>Discord Tipping</h1>
  <p class="meta">Uptime: {uptime_str}</p>

  {stats_html}

  <div class="section">
    <h2>Registered Users</h2>
    <table>
      <thead><tr><th>Username</th><th>Discord ID</th><th>Wallet Address</th><th>Registered</th></tr></thead>
      <tbody>{registered_rows}</tbody>
    </table>
  </div>

  {unregistered_section}

  <script>setTimeout(() => location.reload(), 30000);</script>
</body>
</html>"#,
        uptime_str = uptime_str,
        stats_html = stats_html,
        registered_rows = registered_rows,
        unregistered_section = if !unregistered_chips.is_empty() {
            format!(
                r#"<div class="section"><h2>Unregistered Users</h2><div>{}</div></div>"#,
                unregistered_chips
            )
        } else {
            String::new()
        },
    );

    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html)
}

fn format_uptime(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}
