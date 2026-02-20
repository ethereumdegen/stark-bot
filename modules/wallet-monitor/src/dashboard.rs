//! Dashboard HTML page handler.
//!
//! Serves a self-contained HTML page with inline CSS/JS showing
//! watchlist, activity, stats, and service status.

use crate::routes::AppState;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use std::sync::Arc;

pub async fn dashboard(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stats = state.db.get_activity_stats().ok();
    let watchlist = state.db.list_watchlist().unwrap_or_default();
    let recent = state
        .db
        .query_activity(&wallet_monitor_types::ActivityFilter {
            limit: Some(20),
            ..Default::default()
        })
        .unwrap_or_default();
    let last_tick = state.last_tick_at.lock().await.clone();
    let uptime = state.start_time.elapsed().as_secs();

    let stats_html = if let Some(s) = &stats {
        format!(
            r#"<div class="stats">
                <div class="stat"><span class="val">{}</span><span class="lbl">Watched Wallets</span></div>
                <div class="stat"><span class="val">{}</span><span class="lbl">Active</span></div>
                <div class="stat"><span class="val">{}</span><span class="lbl">Total Txs</span></div>
                <div class="stat"><span class="val">{}</span><span class="lbl">Large Trades</span></div>
            </div>"#,
            s.watched_wallets, s.active_wallets, s.total_transactions, s.large_trades
        )
    } else {
        "<p>No stats available.</p>".to_string()
    };

    let mut watchlist_rows = String::new();
    for w in &watchlist {
        let label = w.label.as_deref().unwrap_or("-");
        let status = if w.monitor_enabled { "Active" } else { "Paused" };
        let last_block = w
            .last_checked_block
            .map(|b| format!("#{}", b))
            .unwrap_or_else(|| "-".to_string());
        watchlist_rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td class=\"mono\">{}</td><td>{}</td><td>${:.0}</td><td>{}</td><td>{}</td></tr>\n",
            w.id, label, w.address, w.chain, w.large_trade_threshold_usd, status, last_block
        ));
    }
    if watchlist_rows.is_empty() {
        watchlist_rows = "<tr><td colspan=\"7\">No wallets on watchlist.</td></tr>".to_string();
    }

    let mut activity_rows = String::new();
    for a in &recent {
        let usd = a
            .usd_value
            .map(|v| format!("${:.0}", v))
            .unwrap_or_else(|| "-".to_string());
        let large_cls = if a.is_large_trade { " class=\"large\"" } else { "" };
        let asset = a.asset_symbol.as_deref().unwrap_or("ETH");
        let amount = a.amount_formatted.as_deref().unwrap_or("-");
        let tx_short = if a.tx_hash.len() > 14 {
            format!("{}...{}", &a.tx_hash[..8], &a.tx_hash[a.tx_hash.len() - 4..])
        } else {
            a.tx_hash.clone()
        };
        activity_rows.push_str(&format!(
            "<tr{}><td>{}</td><td>{}</td><td>{} {}</td><td>{}</td><td class=\"mono\">{}</td><td>{}</td></tr>\n",
            large_cls, a.activity_type, a.chain, amount, asset, usd, tx_short, a.created_at
        ));
    }
    if activity_rows.is_empty() {
        activity_rows = "<tr><td colspan=\"6\">No activity recorded yet.</td></tr>".to_string();
    }

    let last_tick_str = last_tick.as_deref().unwrap_or("not yet");
    let uptime_str = format_uptime(uptime);

    let warning_banner = if !state.worker_enabled {
        r#"<div style="background:#5a2d00;border:1px solid #b35c00;border-radius:8px;padding:12px 16px;margin-bottom:20px;display:flex;align-items:center;gap:10px;">
            <span style="font-size:1.3em;">&#9888;</span>
            <div>
                <strong style="color:#ffb347;">Background worker disabled</strong>
                <span style="color:#ccc;"> &mdash; <code style="background:#3d2200;padding:2px 6px;border-radius:4px;font-size:0.9em;">ALCHEMY_API_KEY</code> is not set. Wallet polling will not run until an Alchemy API key is configured in Settings &gt; API Keys.</span>
            </div>
        </div>"#
            .to_string()
    } else {
        String::new()
    };

    let api_keys_html = {
        let alchemy_status = match &state.alchemy_key_preview {
            Some(preview) => format!(
                r#"<span style="color:#3fb950;">&#10003;</span> <code style="background:#1a1a2e;padding:2px 6px;border-radius:4px;font-size:0.9em;">{}</code>"#,
                preview
            ),
            None => r#"<span style="color:#f85149;">&#10007; Not configured</span>"#.to_string(),
        };
        format!(
            r#"<div style="background:#161b22;border:1px solid #30363d;border-radius:8px;padding:12px 16px;margin-bottom:20px;">
                <h2 style="margin-bottom:8px;">API Keys</h2>
                <div style="display:flex;align-items:center;gap:8px;font-size:0.9em;">
                    <span style="color:#8b949e;">ALCHEMY_API_KEY:</span> {}
                </div>
            </div>"#,
            alchemy_status
        )
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Wallet Monitor Dashboard</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0f1117; color: #e0e0e0; padding: 20px; }}
  h1 {{ color: #58a6ff; margin-bottom: 8px; }}
  .meta {{ color: #8b949e; font-size: 0.85em; margin-bottom: 20px; }}
  .stats {{ display: flex; gap: 16px; margin-bottom: 24px; flex-wrap: wrap; }}
  .stat {{ background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 16px 24px; text-align: center; min-width: 140px; }}
  .stat .val {{ display: block; font-size: 2em; font-weight: bold; color: #58a6ff; }}
  .stat .lbl {{ display: block; font-size: 0.85em; color: #8b949e; margin-top: 4px; }}
  table {{ width: 100%; border-collapse: collapse; margin-bottom: 24px; }}
  th {{ background: #161b22; color: #8b949e; text-align: left; padding: 8px 12px; font-size: 0.85em; text-transform: uppercase; border-bottom: 1px solid #30363d; }}
  td {{ padding: 8px 12px; border-bottom: 1px solid #21262d; font-size: 0.9em; }}
  tr:hover {{ background: #161b22; }}
  tr.large {{ background: #2d1b00; }}
  tr.large:hover {{ background: #3d2500; }}
  .mono {{ font-family: 'SF Mono', 'Consolas', monospace; font-size: 0.85em; }}
  h2 {{ color: #c9d1d9; margin-bottom: 12px; font-size: 1.1em; }}
  .section {{ margin-bottom: 28px; }}
  a {{ color: #58a6ff; text-decoration: none; }}
  a:hover {{ text-decoration: underline; }}
</style>
</head>
<body>
  <h1>Wallet Monitor</h1>
  <p class="meta">Uptime: {uptime_str} &middot; Last tick: {last_tick_str} &middot; Poll interval: {poll_interval}s</p>

  {warning_banner}

  {api_keys_html}

  {stats_html}

  <div class="section">
    <h2>Watchlist</h2>
    <table>
      <thead><tr><th>ID</th><th>Label</th><th>Address</th><th>Chain</th><th>Threshold</th><th>Status</th><th>Last Block</th></tr></thead>
      <tbody>{watchlist_rows}</tbody>
    </table>
  </div>

  <div class="section">
    <h2>Recent Activity</h2>
    <table>
      <thead><tr><th>Type</th><th>Chain</th><th>Amount</th><th>USD</th><th>Tx</th><th>Time</th></tr></thead>
      <tbody>{activity_rows}</tbody>
    </table>
  </div>

  <script>
    // Auto-refresh every 30 seconds
    setTimeout(() => location.reload(), 30000);
  </script>
</body>
</html>"#,
        uptime_str = uptime_str,
        last_tick_str = last_tick_str,
        poll_interval = state.poll_interval_secs,
        warning_banner = warning_banner,
        api_keys_html = api_keys_html,
        stats_html = stats_html,
        watchlist_rows = watchlist_rows,
        activity_rows = activity_rows,
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
