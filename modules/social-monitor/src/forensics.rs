//! Forensics processing — topic extraction, sentiment scoring, signal detection.
//!
//! This module handles the intelligence layer on top of raw captured tweets.

use crate::db::Db;
use social_monitor_types::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// =====================================================
// Topic Extraction (per-tweet, real-time)
// =====================================================

/// Extract topics from a tweet's text and insert them into the database.
pub fn extract_and_store_topics(
    db: &Db,
    tweet_db_id: i64,
    account_id: i64,
    text: &str,
    tracked_keywords: &[TrackedKeyword],
) {
    let mut seen = HashSet::new();

    // Extract hashtags: #word
    let hashtag_re = regex::Regex::new(r"#(\w+)").unwrap();
    for cap in hashtag_re.captures_iter(text) {
        let raw = cap.get(0).unwrap().as_str();
        let topic = cap.get(1).unwrap().as_str().to_lowercase();
        if seen.insert(("hashtag", topic.clone())) {
            let _ = db.insert_topic(tweet_db_id, account_id, &topic, "hashtag", Some(raw));
        }
    }

    // Extract cashtags: $SYMBOL (1-10 alpha chars)
    let cashtag_re = regex::Regex::new(r"\$([A-Za-z]{1,10})").unwrap();
    for cap in cashtag_re.captures_iter(text) {
        let raw = cap.get(0).unwrap().as_str();
        let topic = cap.get(1).unwrap().as_str().to_lowercase();
        if seen.insert(("cashtag", topic.clone())) {
            let _ = db.insert_topic(tweet_db_id, account_id, &topic, "cashtag", Some(raw));
        }
    }

    // Extract mentions: @username
    let mention_re = regex::Regex::new(r"@(\w+)").unwrap();
    for cap in mention_re.captures_iter(text) {
        let raw = cap.get(0).unwrap().as_str();
        let topic = cap.get(1).unwrap().as_str().to_lowercase();
        if seen.insert(("mention", topic.clone())) {
            let _ = db.insert_topic(tweet_db_id, account_id, &topic, "mention", Some(raw));
        }
    }

    // Match tracked keywords (case-insensitive substring matching with aliases)
    let lower_text = text.to_lowercase();
    for kw in tracked_keywords {
        let mut all_forms = vec![kw.keyword.clone()];
        if let Some(ref aliases) = kw.aliases_json {
            if let Ok(aliases_vec) = serde_json::from_str::<Vec<String>>(aliases) {
                all_forms.extend(aliases_vec.into_iter().map(|a| a.to_lowercase()));
            }
        }

        for form in &all_forms {
            if lower_text.contains(form.as_str()) {
                if seen.insert(("keyword", kw.keyword.clone())) {
                    let _ = db.insert_topic(
                        tweet_db_id,
                        account_id,
                        &kw.keyword,
                        "keyword",
                        Some(form),
                    );
                }
                break;
            }
        }
    }
}

// =====================================================
// Sentiment Scoring (rule-based, per-tweet)
// =====================================================

/// Score the sentiment of a tweet text. Returns -1.0 to +1.0.
pub fn score_sentiment(text: &str) -> (f64, &'static str) {
    let lower = text.to_lowercase();
    let mut score: f64 = 0.0;
    let mut hits = 0i32;

    // Positive terms
    let positive_terms = [
        ("bullish", 1.0),
        ("moon", 0.8),
        ("mooning", 0.9),
        ("pump", 0.6),
        ("pumping", 0.7),
        ("lfg", 0.8),
        ("wagmi", 0.7),
        ("gm", 0.3),
        ("based", 0.5),
        ("alpha", 0.6),
        ("gem", 0.7),
        ("huge", 0.5),
        ("massive", 0.5),
        ("amazing", 0.6),
        ("exciting", 0.5),
        ("bullrun", 0.9),
        ("breakout", 0.7),
        ("rally", 0.6),
        ("undervalued", 0.6),
        ("accumulate", 0.5),
        ("diamond hands", 0.7),
        ("hodl", 0.5),
        ("buy", 0.4),
        ("long", 0.4),
        ("support", 0.3),
    ];

    // Negative terms
    let negative_terms = [
        ("bearish", -1.0),
        ("rug", -1.0),
        ("rugpull", -1.0),
        ("scam", -1.0),
        ("dump", -0.7),
        ("dumping", -0.8),
        ("ngmi", -0.7),
        ("rekt", -0.8),
        ("crash", -0.8),
        ("crashing", -0.9),
        ("sell", -0.4),
        ("short", -0.4),
        ("overvalued", -0.6),
        ("bubble", -0.6),
        ("ponzi", -0.9),
        ("fraud", -0.9),
        ("hack", -0.8),
        ("exploit", -0.7),
        ("fud", -0.5),
        ("dead", -0.7),
        ("bleeding", -0.6),
        ("pain", -0.5),
        ("fear", -0.5),
        ("warning", -0.4),
        ("careful", -0.3),
    ];

    for (term, weight) in &positive_terms {
        if lower.contains(term) {
            score += weight;
            hits += 1;
        }
    }
    for (term, weight) in &negative_terms {
        if lower.contains(term) {
            score += weight;
            hits += 1;
        }
    }

    // Emoji signals
    let positive_emojis = ["🚀", "💎", "🔥", "📈", "💪", "🎉", "✅", "💰", "🤝", "👀"];
    let negative_emojis = ["💀", "🤡", "📉", "⚠️", "❌", "😱", "🔻", "💩"];

    for emoji in &positive_emojis {
        if text.contains(emoji) {
            score += 0.3;
            hits += 1;
        }
    }
    for emoji in &negative_emojis {
        if text.contains(emoji) {
            score -= 0.3;
            hits += 1;
        }
    }

    // Simple negation handling: "not bullish", "isn't good", etc.
    let negation_patterns = ["not ", "isn't ", "isn't ", "no ", "don't ", "don't ", "never "];
    for pattern in &negation_patterns {
        if lower.contains(pattern) {
            // If negation found, dampen or flip the score slightly
            score *= 0.5;
            break;
        }
    }

    // Normalize to -1.0 .. +1.0
    if hits > 0 {
        score = score / (hits as f64).sqrt();
        score = score.clamp(-1.0, 1.0);
    }

    let label = if score > 0.3 {
        "positive"
    } else if score < -0.3 {
        "negative"
    } else {
        "neutral"
    };

    (score, label)
}

// =====================================================
// Periodic Rollup & Signal Detection
// =====================================================

/// Run periodic forensics: topic rollup + sentiment snapshots + signal detection.
/// Called every N ticks by the worker.
pub fn run_periodic_forensics(db: &Arc<Db>) {
    // 1. Rollup topic scores
    match db.rollup_topic_scores() {
        Ok(n) => {
            if n > 0 {
                log::info!("[SOCIAL_MONITOR] Rolled up {} topic scores", n);
            }
        }
        Err(e) => log::error!("[SOCIAL_MONITOR] Topic rollup error: {}", e),
    }

    // 2. Generate sentiment snapshots for each active account
    let accounts = match db.list_active_accounts() {
        Ok(a) => a,
        Err(e) => {
            log::error!("[SOCIAL_MONITOR] Failed to list accounts: {}", e);
            return;
        }
    };

    let now = chrono::Utc::now();
    let window_end = now.to_rfc3339();
    let window_start = (now - chrono::Duration::hours(1)).to_rfc3339();

    for account in &accounts {
        let recent_tweets = match db.get_tweets_for_account_since(account.id, &window_start) {
            Ok(t) => t,
            Err(_) => continue,
        };

        if recent_tweets.is_empty() {
            continue;
        }

        // Score sentiment across recent tweets
        let mut total_score = 0.0f64;
        let mut topic_counts: HashMap<String, i64> = HashMap::new();

        for tweet in &recent_tweets {
            let (score, _label) = score_sentiment(&tweet.text);
            total_score += score;
        }

        let avg_score = total_score / recent_tweets.len() as f64;
        let avg_label = if avg_score > 0.3 {
            "positive"
        } else if avg_score < -0.3 {
            "negative"
        } else {
            "neutral"
        };

        // Get top topics for this window
        let topics = db
            .query_topic_scores(&TopicFilter {
                account_id: Some(account.id),
                limit: Some(5),
                ..Default::default()
            })
            .unwrap_or_default();

        for ts in &topics {
            *topic_counts.entry(ts.topic.clone()).or_default() += ts.mention_count_7d;
        }

        let top_topics_json =
            serde_json::to_string(&topic_counts).unwrap_or_else(|_| "{}".to_string());

        // Detect signals
        let signals = detect_signals(db, account, avg_score);
        let signals_json = serde_json::to_string(&signals).unwrap_or_else(|_| "[]".to_string());

        let _ = db.insert_sentiment_snapshot(
            account.id,
            &window_start,
            &window_end,
            avg_score,
            avg_label,
            recent_tweets.len() as i64,
            Some(&top_topics_json),
            if signals.is_empty() {
                None
            } else {
                Some(&signals_json)
            },
            None,
        );
    }
}

/// Detect signals for a given account
fn detect_signals(
    db: &Db,
    account: &MonitoredAccount,
    current_sentiment: f64,
) -> Vec<Signal> {
    let mut signals = Vec::new();

    // Volume spike: tweet count in last 24h > 2x daily average
    let daily_avg = db.get_daily_tweet_avg(account.id).unwrap_or(0.0);
    let now = chrono::Utc::now();
    let day_ago = (now - chrono::Duration::hours(24)).to_rfc3339();
    let recent_tweets = db
        .get_tweets_for_account_since(account.id, &day_ago)
        .unwrap_or_default();
    let today_count = recent_tweets.len() as f64;

    if daily_avg > 0.0 && today_count > daily_avg * 2.0 {
        signals.push(Signal {
            signal_type: "volume_spike".to_string(),
            description: format!(
                "@{} tweet volume spike: {} tweets today vs {:.1} daily avg",
                account.username, today_count, daily_avg
            ),
            account_id: account.id,
            username: account.username.clone(),
            severity: "medium".to_string(),
        });
    }

    // Sentiment swing: >0.4 delta from previous snapshot
    if let Ok(Some(prev)) = db.get_last_sentiment_for_account(account.id) {
        let delta = (current_sentiment - prev.sentiment_score).abs();
        if delta > 0.4 {
            let direction = if current_sentiment > prev.sentiment_score {
                "positive"
            } else {
                "negative"
            };
            signals.push(Signal {
                signal_type: "sentiment_swing".to_string(),
                description: format!(
                    "@{} sentiment swing: {:.2} -> {:.2} ({} shift)",
                    account.username, prev.sentiment_score, current_sentiment, direction
                ),
                account_id: account.id,
                username: account.username.clone(),
                severity: "high".to_string(),
            });
        }
    }

    // New interest: topics first seen in last 48h
    let topics = db
        .query_topic_scores(&TopicFilter {
            account_id: Some(account.id),
            trend: Some("new".to_string()),
            limit: Some(10),
            ..Default::default()
        })
        .unwrap_or_default();

    for ts in &topics {
        signals.push(Signal {
            signal_type: "new_interest".to_string(),
            description: format!(
                "@{} started talking about '{}' (first seen: {})",
                account.username, ts.topic, ts.first_seen_at
            ),
            account_id: account.id,
            username: account.username.clone(),
            severity: "low".to_string(),
        });
    }

    // Gone quiet: no tweets in 48h+ for previously active account
    if let Some(ref last_checked) = account.last_checked_at {
        if let Ok(last_time) = chrono::DateTime::parse_from_rfc3339(last_checked) {
            let hours_since = (now - last_time.with_timezone(&chrono::Utc)).num_hours();
            if hours_since > 48 && account.total_tweets_captured > 10 {
                signals.push(Signal {
                    signal_type: "gone_quiet".to_string(),
                    description: format!(
                        "@{} has gone quiet — no tweets in {}h (previously active with {} tweets)",
                        account.username, hours_since, account.total_tweets_captured
                    ),
                    account_id: account.id,
                    username: account.username.clone(),
                    severity: "medium".to_string(),
                });
            }
        }
    }

    signals
}

/// Generate a full forensics report for an account
pub fn generate_report(db: &Db, account: &MonitoredAccount) -> AccountForensicsReport {
    let top_topics = db
        .query_topic_scores(&TopicFilter {
            account_id: Some(account.id),
            limit: Some(20),
            ..Default::default()
        })
        .unwrap_or_default();

    let recent_sentiment = db
        .query_sentiment(&SentimentFilter {
            account_id: Some(account.id),
            limit: Some(24),
            ..Default::default()
        })
        .unwrap_or_default();

    let tweet_count = account.total_tweets_captured;
    let date_range = db.get_account_tweet_date_range(account.id).ok().flatten();

    // Detect current signals
    let current_sentiment = recent_sentiment
        .first()
        .map(|s| s.sentiment_score)
        .unwrap_or(0.0);
    let signals = detect_signals(db, account, current_sentiment);

    AccountForensicsReport {
        account: account.clone(),
        top_topics,
        recent_sentiment,
        signals,
        tweet_count,
        date_range,
    }
}
