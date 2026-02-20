//! Dashboard HTML page handler.
//!
//! Serves a self-contained HTML page with inline CSS/JS showing
//! monitored accounts, tweet stats, topics, and service status.

use crate::routes::AppState;
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use std::sync::Arc;

pub async fn dashboard(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stats = state.db.get_tweet_stats().ok();
    let accounts = state.db.list_accounts().unwrap_or_default();
    let recent = state
        .db
        .query_tweets(&social_monitor_types::TweetFilter {
            limit: Some(20),
            ..Default::default()
        })
        .unwrap_or_default();
    let top_topics = state
        .db
        .query_topic_scores(&social_monitor_types::TopicFilter {
            limit: Some(20),
            ..Default::default()
        })
        .unwrap_or_default();
    let last_tick = state.last_tick_at.lock().await.clone();
    let uptime = state.start_time.elapsed().as_secs();

    let stats_html = if let Some(s) = &stats {
        format!(
            r#"<div class="stats">
                <div class="stat"><span class="val">{}</span><span class="lbl">Monitored</span></div>
                <div class="stat"><span class="val">{}</span><span class="lbl">Active</span></div>
                <div class="stat"><span class="val">{}</span><span class="lbl">Total Tweets</span></div>
                <div class="stat"><span class="val">{}</span><span class="lbl">Today</span></div>
                <div class="stat"><span class="val">{}</span><span class="lbl">7 Days</span></div>
                <div class="stat"><span class="val">{}</span><span class="lbl">Topics</span></div>
            </div>"#,
            s.monitored_accounts,
            s.active_accounts,
            s.total_tweets,
            s.tweets_today,
            s.tweets_7d,
            s.unique_topics
        )
    } else {
        "<p>No stats available.</p>".to_string()
    };

    let mut account_rows = String::new();
    for a in &accounts {
        let status = if a.monitor_enabled { "Active" } else { "Paused" };
        let last_checked = a.last_checked_at.as_deref().unwrap_or("-");
        account_rows.push_str(&format!(
            "<tr><td>{}</td><td>@{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            a.id,
            a.username,
            a.display_name.as_deref().unwrap_or("-"),
            a.total_tweets_captured,
            status,
            last_checked
        ));
    }
    if account_rows.is_empty() {
        account_rows =
            "<tr><td colspan=\"6\">No accounts being monitored.</td></tr>".to_string();
    }

    let mut tweet_rows = String::new();
    for t in &recent {
        let username = accounts
            .iter()
            .find(|a| a.id == t.account_id)
            .map(|a| a.username.as_str())
            .unwrap_or("?");
        let text_short = if t.text.len() > 100 {
            format!("{}...", &t.text[..100])
        } else {
            t.text.clone()
        };
        // Escape HTML
        let text_escaped = text_short
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        tweet_rows.push_str(&format!(
            "<tr><td>@{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            username, t.tweet_type, text_escaped, t.tweeted_at,
            t.like_count + t.retweet_count
        ));
    }
    if tweet_rows.is_empty() {
        tweet_rows =
            "<tr><td colspan=\"5\">No tweets captured yet.</td></tr>".to_string();
    }

    let mut topic_rows = String::new();
    for ts in &top_topics {
        let account_name = accounts
            .iter()
            .find(|a| a.id == ts.account_id)
            .map(|a| a.username.as_str())
            .unwrap_or("?");
        let trend_cls = match ts.trend.as_str() {
            "rising" => " class=\"rising\"",
            "falling" => " class=\"falling\"",
            "new" => " class=\"new-topic\"",
            _ => "",
        };
        topic_rows.push_str(&format!(
            "<tr{}><td>{}</td><td>@{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:.1}</td></tr>\n",
            trend_cls,
            ts.topic,
            account_name,
            ts.mention_count_7d,
            ts.mention_count_30d,
            ts.mention_count_total,
            ts.trend,
            ts.avg_engagement_score
        ));
    }
    if topic_rows.is_empty() {
        topic_rows =
            "<tr><td colspan=\"7\">No topics extracted yet.</td></tr>".to_string();
    }

    let last_tick_str = last_tick.as_deref().unwrap_or("not yet");
    let uptime_str = format_uptime(uptime);

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Social Monitor Dashboard</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0f1117; color: #e0e0e0; padding: 20px; }}
  h1 {{ color: #58a6ff; margin-bottom: 8px; }}
  .meta {{ color: #8b949e; font-size: 0.85em; margin-bottom: 20px; }}
  .stats {{ display: flex; gap: 16px; margin-bottom: 24px; flex-wrap: wrap; }}
  .stat {{ background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 16px 24px; text-align: center; min-width: 120px; }}
  .stat .val {{ display: block; font-size: 2em; font-weight: bold; color: #58a6ff; }}
  .stat .lbl {{ display: block; font-size: 0.85em; color: #8b949e; margin-top: 4px; }}
  table {{ width: 100%; border-collapse: collapse; margin-bottom: 24px; }}
  th {{ background: #161b22; color: #8b949e; text-align: left; padding: 8px 12px; font-size: 0.85em; text-transform: uppercase; border-bottom: 1px solid #30363d; }}
  td {{ padding: 8px 12px; border-bottom: 1px solid #21262d; font-size: 0.9em; }}
  tr:hover {{ background: #161b22; }}
  tr.rising {{ background: #0d2818; }}
  tr.rising:hover {{ background: #133d24; }}
  tr.falling {{ background: #2d1b00; }}
  tr.falling:hover {{ background: #3d2500; }}
  tr.new-topic {{ background: #0d1b2d; }}
  tr.new-topic:hover {{ background: #132d3d; }}
  .mono {{ font-family: 'SF Mono', 'Consolas', monospace; font-size: 0.85em; }}
  h2 {{ color: #c9d1d9; margin-bottom: 12px; font-size: 1.1em; }}
  .section {{ margin-bottom: 28px; }}
  a {{ color: #58a6ff; text-decoration: none; }}
  a:hover {{ text-decoration: underline; }}
</style>
</head>
<body>
  <h1>Social Monitor</h1>
  <p class="meta">Uptime: {uptime_str} &middot; Last tick: {last_tick_str} &middot; Poll interval: {poll_interval}s</p>

  {stats_html}

  <div class="section">
    <h2>Monitored Accounts</h2>
    <table>
      <thead><tr><th>ID</th><th>Username</th><th>Name</th><th>Tweets</th><th>Status</th><th>Last Checked</th></tr></thead>
      <tbody>{account_rows}</tbody>
    </table>
  </div>

  <div class="section">
    <h2>Top Topics</h2>
    <table>
      <thead><tr><th>Topic</th><th>Account</th><th>7d</th><th>30d</th><th>Total</th><th>Trend</th><th>Engagement</th></tr></thead>
      <tbody>{topic_rows}</tbody>
    </table>
  </div>

  <div class="section">
    <h2>Recent Tweets</h2>
    <table>
      <thead><tr><th>Account</th><th>Type</th><th>Text</th><th>Time</th><th>Engagement</th></tr></thead>
      <tbody>{tweet_rows}</tbody>
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
        stats_html = stats_html,
        account_rows = account_rows,
        topic_rows = topic_rows,
        tweet_rows = tweet_rows,
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
