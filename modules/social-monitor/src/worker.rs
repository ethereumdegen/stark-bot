//! Background worker for social monitoring.
//!
//! Polls Twitter API every N seconds for new tweets from monitored accounts.
//! Processes tweets through the forensics pipeline.

use crate::db::Db;
use crate::forensics;
use crate::twitter_api;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Ticks between full forensics rollups (default: 12 ticks = ~1 hour at 5min intervals)
const ROLLUP_INTERVAL_TICKS: u64 = 12;

pub async fn run_worker(
    db: Arc<Db>,
    poll_interval_secs: u64,
    last_tick_at: Arc<Mutex<Option<String>>>,
) {
    log::info!(
        "[SOCIAL_MONITOR] Worker started (poll interval: {}s)",
        poll_interval_secs
    );

    let client = reqwest::Client::new();
    let credentials = match twitter_api::TwitterCredentials::from_env() {
        Some(c) => c,
        None => {
            log::error!("[SOCIAL_MONITOR] Twitter credentials not available");
            return;
        }
    };

    let mut tick_count: u64 = 0;

    loop {
        tokio::time::sleep(Duration::from_secs(poll_interval_secs)).await;
        tick_count += 1;

        match poll_tick(&db, &client, &credentials).await {
            Ok(_) => {
                let now = chrono::Utc::now().to_rfc3339();
                *last_tick_at.lock().await = Some(now);
            }
            Err(e) => {
                log::error!("[SOCIAL_MONITOR] Tick error: {}", e);
            }
        }

        // Run periodic forensics every N ticks
        if tick_count % ROLLUP_INTERVAL_TICKS == 0 {
            let db_clone = db.clone();
            tokio::task::spawn_blocking(move || {
                forensics::run_periodic_forensics(&db_clone);
            })
            .await
            .ok();
        }
    }
}

/// One poll tick: fetch new tweets for each active account, then process them
async fn poll_tick(
    db: &Arc<Db>,
    client: &reqwest::Client,
    credentials: &twitter_api::TwitterCredentials,
) -> Result<(), String> {
    let accounts = db
        .list_active_accounts()
        .map_err(|e| format!("Failed to list accounts: {}", e))?;

    if accounts.is_empty() {
        return Ok(());
    }

    log::debug!(
        "[SOCIAL_MONITOR] Tick: checking {} accounts",
        accounts.len()
    );

    let tracked_keywords = db.list_keywords().unwrap_or_default();
    let mut total_new = 0usize;

    for account in &accounts {
        match fetch_account_tweets(db, client, credentials, account).await {
            Ok(new_count) => {
                total_new += new_count;
            }
            Err(e) => {
                if e.contains("Rate limited") {
                    log::warn!(
                        "[SOCIAL_MONITOR] Rate limited — stopping this tick early"
                    );
                    break;
                }
                log::warn!(
                    "[SOCIAL_MONITOR] Error fetching @{}: {}",
                    account.username,
                    e
                );
            }
        }

        // 500ms delay between accounts to avoid bursting
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Process any unprocessed tweets
    let unprocessed = db.get_unprocessed_tweets(500).unwrap_or_default();
    for tweet in &unprocessed {
        forensics::extract_and_store_topics(
            db,
            tweet.id,
            tweet.account_id,
            &tweet.text,
            &tracked_keywords,
        );
        let _ = db.mark_tweet_processed(tweet.id);
    }

    if total_new > 0 {
        log::info!(
            "[SOCIAL_MONITOR] Tick complete: {} new tweets captured",
            total_new
        );
    }

    Ok(())
}

/// Fetch new tweets for a single account
async fn fetch_account_tweets(
    db: &Db,
    client: &reqwest::Client,
    credentials: &twitter_api::TwitterCredentials,
    account: &social_monitor_types::MonitoredAccount,
) -> Result<usize, String> {
    let (tweets, rate_info) = twitter_api::get_user_tweets(
        client,
        credentials,
        &account.twitter_user_id,
        account.last_tweet_id.as_deref(),
        100, // max per request
    )
    .await?;

    if let Some(remaining) = rate_info.remaining {
        if remaining < 5 {
            log::warn!(
                "[SOCIAL_MONITOR] Rate limit low: {} remaining for @{}",
                remaining,
                account.username
            );
        }
    }

    if tweets.is_empty() {
        let _ = db.update_account_checked(account.id);
        return Ok(0);
    }

    let mut new_count = 0usize;
    let mut max_tweet_id: Option<String> = None;

    for tweet in &tweets {
        let metrics = tweet.public_metrics.as_ref();
        let raw_json = serde_json::to_string(tweet).ok();

        let result = db.insert_tweet(
            account.id,
            &tweet.id,
            &tweet.text,
            tweet.tweet_type(),
            tweet.conversation_id.as_deref(),
            tweet.in_reply_to_user_id.as_deref(),
            metrics.and_then(|m| m.like_count).unwrap_or(0),
            metrics.and_then(|m| m.retweet_count).unwrap_or(0),
            metrics.and_then(|m| m.reply_count).unwrap_or(0),
            metrics.and_then(|m| m.quote_count).unwrap_or(0),
            tweet.created_at.as_deref().unwrap_or(""),
            raw_json.as_deref(),
        );

        if let Ok(id) = result {
            if id > 0 {
                new_count += 1;
            }
        }

        // Track the highest tweet ID for cursor
        match (&max_tweet_id, &tweet.id) {
            (None, id) => max_tweet_id = Some(id.clone()),
            (Some(current), id) if id > current => max_tweet_id = Some(id.clone()),
            _ => {}
        }
    }

    if let Some(ref max_id) = max_tweet_id {
        let _ = db.update_account_cursor(account.id, max_id, new_count as i64);
    }

    Ok(new_count)
}
