# /// script
# requires-python = ">=3.12"
# dependencies = ["flask", "requests", "requests-oauthlib", "starkbot-sdk"]
#
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
Social Monitor module — monitors Twitter/X accounts for tweet activity,
topic trends, and sentiment analysis.

Background worker polls Twitter API v2 every 300s via OAuth 1.0a.
Extracts topics (hashtags, cashtags, mentions, keywords). Runs forensics
rollup (topic scores, sentiment snapshots, signal detection) every 12 ticks.

RPC protocol endpoints:
  GET  /rpc/status              -> service health
  POST /rpc/tools/watchlist     -> manage accounts & keywords (action-based)
  POST /rpc/tools/tweets        -> query captured tweets (action-based)
  POST /rpc/tools/forensics     -> topics, sentiment, reports (action-based)
  POST /rpc/tools/control       -> worker control (action-based)
  POST /rpc/backup/export       -> export data for backup
  POST /rpc/backup/restore      -> restore data from backup
  GET  /                        -> HTML dashboard

Launch with:  uv run service.py
"""

from flask import request
from starkbot_sdk import create_app, success, error
import sqlite3
import os
import re
import json
import time
import logging
import threading
import math
from datetime import datetime, timezone, timedelta
from requests_oauthlib import OAuth1
import requests as http_requests

DB_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "social_monitor.db")
POLL_INTERVAL = int(os.environ.get("SOCIAL_MONITOR_POLL_INTERVAL", "300"))
ROLLUP_INTERVAL_TICKS = 12

TWITTER_CONSUMER_KEY = os.environ.get("TWITTER_CONSUMER_KEY", "")
TWITTER_CONSUMER_SECRET = os.environ.get("TWITTER_CONSUMER_SECRET", "")
TWITTER_ACCESS_TOKEN = os.environ.get("TWITTER_ACCESS_TOKEN", "")
TWITTER_ACCESS_TOKEN_SECRET = os.environ.get("TWITTER_ACCESS_TOKEN_SECRET", "")

_start_time = time.time()
_last_tick_at = None
_last_tick_lock = threading.Lock()

# Sentiment lexicons
POSITIVE_TERMS = [
    ("bullish", 1.0), ("moon", 0.8), ("mooning", 0.9), ("pump", 0.6), ("pumping", 0.7),
    ("lfg", 0.8), ("wagmi", 0.7), ("gm", 0.3), ("based", 0.5), ("alpha", 0.6),
    ("gem", 0.7), ("huge", 0.5), ("massive", 0.5), ("amazing", 0.6), ("exciting", 0.5),
    ("bullrun", 0.9), ("breakout", 0.7), ("rally", 0.6), ("undervalued", 0.6),
    ("accumulate", 0.5), ("diamond hands", 0.7), ("hodl", 0.5), ("buy", 0.4),
    ("long", 0.4), ("support", 0.3),
]
NEGATIVE_TERMS = [
    ("bearish", -1.0), ("rug", -1.0), ("rugpull", -1.0), ("scam", -1.0),
    ("dump", -0.7), ("dumping", -0.8), ("ngmi", -0.7), ("rekt", -0.8),
    ("crash", -0.8), ("crashing", -0.9), ("sell", -0.4), ("short", -0.4),
    ("overvalued", -0.6), ("bubble", -0.6), ("ponzi", -0.9), ("fraud", -0.9),
    ("hack", -0.8), ("exploit", -0.7), ("fud", -0.5), ("dead", -0.7),
    ("bleeding", -0.6), ("pain", -0.5), ("fear", -0.5), ("warning", -0.4),
    ("careful", -0.3),
]
POSITIVE_EMOJIS = ["🚀", "💎", "🔥", "📈", "💪", "🎉", "✅", "💰", "🤝", "👀"]
NEGATIVE_EMOJIS = ["💀", "🤡", "📉", "⚠️", "❌", "😱", "🔻", "💩"]
NEGATION_PATTERNS = ["not ", "isn't ", "isn\u2019t ", "no ", "don't ", "don\u2019t ", "never "]

HASHTAG_RE = re.compile(r"#(\w+)")
CASHTAG_RE = re.compile(r"\$([A-Za-z]{1,10})")
MENTION_RE = re.compile(r"@(\w+)")


# ---------------------------------------------------------------------------
# Database helpers
# ---------------------------------------------------------------------------

def get_db():
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    return conn


def init_db():
    conn = get_db()
    conn.execute("""
        CREATE TABLE IF NOT EXISTS monitored_accounts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            twitter_user_id TEXT NOT NULL UNIQUE,
            username TEXT NOT NULL,
            display_name TEXT,
            monitor_enabled INTEGER NOT NULL DEFAULT 1,
            custom_keywords TEXT,
            notes TEXT,
            last_tweet_id TEXT,
            last_checked_at TEXT,
            total_tweets_captured INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS captured_tweets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id INTEGER NOT NULL,
            tweet_id TEXT NOT NULL UNIQUE,
            text TEXT NOT NULL,
            tweet_type TEXT NOT NULL DEFAULT 'original',
            conversation_id TEXT,
            in_reply_to_user_id TEXT,
            like_count INTEGER DEFAULT 0,
            retweet_count INTEGER DEFAULT 0,
            reply_count INTEGER DEFAULT 0,
            quote_count INTEGER DEFAULT 0,
            tweeted_at TEXT NOT NULL,
            captured_at TEXT NOT NULL DEFAULT (datetime('now')),
            processed INTEGER NOT NULL DEFAULT 0,
            raw_json TEXT,
            FOREIGN KEY (account_id) REFERENCES monitored_accounts(id) ON DELETE CASCADE
        )
    """)
    conn.execute("CREATE INDEX IF NOT EXISTS idx_tweets_account_time ON captured_tweets(account_id, tweeted_at DESC)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_tweets_processed ON captured_tweets(processed, captured_at ASC)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_tweets_time ON captured_tweets(tweeted_at DESC)")
    conn.execute("""
        CREATE TABLE IF NOT EXISTS tweet_topics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            tweet_id INTEGER NOT NULL,
            account_id INTEGER NOT NULL,
            topic TEXT NOT NULL,
            topic_type TEXT NOT NULL,
            raw_form TEXT,
            FOREIGN KEY (tweet_id) REFERENCES captured_tweets(id) ON DELETE CASCADE,
            FOREIGN KEY (account_id) REFERENCES monitored_accounts(id) ON DELETE CASCADE
        )
    """)
    conn.execute("CREATE INDEX IF NOT EXISTS idx_topics_topic_account ON tweet_topics(topic, account_id)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_topics_account ON tweet_topics(account_id, topic)")
    conn.execute("""
        CREATE TABLE IF NOT EXISTS topic_scores (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id INTEGER NOT NULL,
            topic TEXT NOT NULL,
            mention_count_7d INTEGER NOT NULL DEFAULT 0,
            mention_count_30d INTEGER NOT NULL DEFAULT 0,
            mention_count_total INTEGER NOT NULL DEFAULT 0,
            trend TEXT NOT NULL DEFAULT 'stable',
            first_seen_at TEXT NOT NULL,
            last_seen_at TEXT NOT NULL,
            avg_engagement_score REAL DEFAULT 0.0,
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (account_id) REFERENCES monitored_accounts(id) ON DELETE CASCADE,
            UNIQUE(account_id, topic)
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS sentiment_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id INTEGER NOT NULL,
            window_start TEXT NOT NULL,
            window_end TEXT NOT NULL,
            sentiment_score REAL NOT NULL DEFAULT 0.0,
            sentiment_label TEXT NOT NULL DEFAULT 'neutral',
            tweet_count INTEGER NOT NULL DEFAULT 0,
            top_topics_json TEXT,
            signals_json TEXT,
            ai_summary TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (account_id) REFERENCES monitored_accounts(id) ON DELETE CASCADE
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS tracked_keywords (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            keyword TEXT NOT NULL UNIQUE,
            category TEXT,
            aliases_json TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )
    """)
    conn.commit()
    conn.close()


def row_to_dict(row):
    if row is None:
        return None
    return dict(row)


def now_iso():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S+00:00")


# ---------------------------------------------------------------------------
# Account operations
# ---------------------------------------------------------------------------

def account_add(twitter_user_id: str, username: str, display_name: str | None, notes: str | None, custom_keywords: str | None):
    conn = get_db()
    ts = now_iso()
    try:
        conn.execute(
            "INSERT INTO monitored_accounts (twitter_user_id, username, display_name, notes, custom_keywords, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (twitter_user_id, username, display_name, notes, custom_keywords, ts, ts),
        )
        conn.commit()
        row = conn.execute("SELECT * FROM monitored_accounts WHERE twitter_user_id = ?", (twitter_user_id,)).fetchone()
        conn.close()
        return row_to_dict(row), None
    except sqlite3.IntegrityError:
        conn.close()
        return None, f"@{username} is already being monitored"


def account_remove(entry_id: int) -> bool:
    conn = get_db()
    cursor = conn.execute("DELETE FROM monitored_accounts WHERE id = ?", (entry_id,))
    conn.commit()
    conn.close()
    return cursor.rowcount > 0


def account_list():
    conn = get_db()
    rows = conn.execute("SELECT * FROM monitored_accounts ORDER BY created_at ASC").fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


def account_update(entry_id: int, monitor_enabled=None, custom_keywords=None, notes=None) -> bool:
    conn = get_db()
    ts = now_iso()
    updates = ["updated_at = ?"]
    params: list = [ts]
    if monitor_enabled is not None:
        updates.append("monitor_enabled = ?")
        params.append(1 if monitor_enabled else 0)
    if custom_keywords is not None:
        updates.append("custom_keywords = ?")
        params.append(custom_keywords)
    if notes is not None:
        updates.append("notes = ?")
        params.append(notes)
    params.append(entry_id)
    cursor = conn.execute(f"UPDATE monitored_accounts SET {', '.join(updates)} WHERE id = ?", params)
    conn.commit()
    conn.close()
    return cursor.rowcount > 0


def account_get_by_id(entry_id: int):
    conn = get_db()
    row = conn.execute("SELECT * FROM monitored_accounts WHERE id = ?", (entry_id,)).fetchone()
    conn.close()
    return row_to_dict(row)


def account_get_by_username(username: str):
    conn = get_db()
    row = conn.execute("SELECT * FROM monitored_accounts WHERE LOWER(username) = ?", (username.lower(),)).fetchone()
    conn.close()
    return row_to_dict(row)


# ---------------------------------------------------------------------------
# Keyword operations
# ---------------------------------------------------------------------------

def keyword_add(keyword: str, category: str | None, aliases: str | None):
    conn = get_db()
    ts = now_iso()
    normalized = keyword.lower()
    aliases_json = None
    if aliases:
        aliases_json = json.dumps([a.strip() for a in aliases.split(",")])
    try:
        conn.execute(
            "INSERT INTO tracked_keywords (keyword, category, aliases_json, created_at) VALUES (?, ?, ?, ?)",
            (normalized, category, aliases_json, ts),
        )
        conn.commit()
        row = conn.execute("SELECT * FROM tracked_keywords WHERE keyword = ?", (normalized,)).fetchone()
        conn.close()
        return row_to_dict(row), None
    except sqlite3.IntegrityError:
        conn.close()
        return None, f"Keyword '{keyword}' already tracked"


def keyword_remove(entry_id: int) -> bool:
    conn = get_db()
    cursor = conn.execute("DELETE FROM tracked_keywords WHERE id = ?", (entry_id,))
    conn.commit()
    conn.close()
    return cursor.rowcount > 0


def keyword_list():
    conn = get_db()
    rows = conn.execute("SELECT * FROM tracked_keywords ORDER BY keyword ASC").fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


# ---------------------------------------------------------------------------
# Tweet operations
# ---------------------------------------------------------------------------

def tweet_query(account_id=None, username=None, search_text=None, tweet_type=None, since=None, until=None, limit=50):
    conn = get_db()
    conditions = ["1=1"]
    params: list = []
    if account_id is not None:
        conditions.append("t.account_id = ?")
        params.append(account_id)
    if username:
        conditions.append("t.account_id IN (SELECT id FROM monitored_accounts WHERE LOWER(username) = ?)")
        params.append(username.lower())
    if search_text:
        conditions.append("t.text LIKE ?")
        params.append(f"%{search_text}%")
    if tweet_type:
        conditions.append("t.tweet_type = ?")
        params.append(tweet_type)
    if since:
        conditions.append("t.tweeted_at >= ?")
        params.append(since)
    if until:
        conditions.append("t.tweeted_at <= ?")
        params.append(until)
    limit = min(limit or 50, 200)
    sql = f"SELECT t.* FROM captured_tweets t WHERE {' AND '.join(conditions)} ORDER BY t.tweeted_at DESC LIMIT {limit}"
    rows = conn.execute(sql, params).fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


def tweet_stats():
    conn = get_db()
    total = conn.execute("SELECT COUNT(*) FROM captured_tweets").fetchone()[0]
    monitored = conn.execute("SELECT COUNT(*) FROM monitored_accounts").fetchone()[0]
    active = conn.execute("SELECT COUNT(*) FROM monitored_accounts WHERE monitor_enabled = 1").fetchone()[0]
    today = conn.execute("SELECT COUNT(*) FROM captured_tweets WHERE tweeted_at >= datetime('now', '-1 day')").fetchone()[0]
    week = conn.execute("SELECT COUNT(*) FROM captured_tweets WHERE tweeted_at >= datetime('now', '-7 days')").fetchone()[0]
    topics = conn.execute("SELECT COUNT(DISTINCT topic) FROM tweet_topics").fetchone()[0]
    conn.close()
    return {
        "total_tweets": total,
        "monitored_accounts": monitored,
        "active_accounts": active,
        "tweets_today": today,
        "tweets_7d": week,
        "unique_topics": topics,
    }


# ---------------------------------------------------------------------------
# Topic score operations
# ---------------------------------------------------------------------------

def topic_query(account_id=None, topic=None, trend=None, min_mentions=None, limit=50):
    conn = get_db()
    conditions = ["1=1"]
    params: list = []
    if account_id is not None:
        conditions.append("ts.account_id = ?")
        params.append(account_id)
    if topic:
        conditions.append("ts.topic LIKE ?")
        params.append(f"%{topic}%")
    if trend:
        conditions.append("ts.trend = ?")
        params.append(trend)
    if min_mentions is not None:
        conditions.append("ts.mention_count_total >= ?")
        params.append(min_mentions)
    limit = min(limit or 50, 200)
    sql = f"SELECT ts.* FROM topic_scores ts WHERE {' AND '.join(conditions)} ORDER BY ts.mention_count_total DESC LIMIT {limit}"
    rows = conn.execute(sql, params).fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


# ---------------------------------------------------------------------------
# Sentiment operations
# ---------------------------------------------------------------------------

def sentiment_query(account_id=None, since=None, until=None, limit=50):
    conn = get_db()
    conditions = ["1=1"]
    params: list = []
    if account_id is not None:
        conditions.append("s.account_id = ?")
        params.append(account_id)
    if since:
        conditions.append("s.window_end >= ?")
        params.append(since)
    if until:
        conditions.append("s.window_start <= ?")
        params.append(until)
    limit = min(limit or 50, 200)
    sql = f"SELECT s.* FROM sentiment_snapshots s WHERE {' AND '.join(conditions)} ORDER BY s.window_end DESC LIMIT {limit}"
    rows = conn.execute(sql, params).fetchall()
    conn.close()
    return [row_to_dict(r) for r in rows]


# ---------------------------------------------------------------------------
# Forensics: report generation
# ---------------------------------------------------------------------------

def generate_report(account: dict) -> dict:
    top_topics = topic_query(account_id=account["id"], limit=20)
    recent_sentiment = sentiment_query(account_id=account["id"], limit=24)

    current_sentiment = recent_sentiment[0]["sentiment_score"] if recent_sentiment else 0.0
    signals = detect_signals(account, current_sentiment)

    conn = get_db()
    date_range = conn.execute(
        "SELECT MIN(tweeted_at), MAX(tweeted_at) FROM captured_tweets WHERE account_id = ?",
        (account["id"],)
    ).fetchone()
    conn.close()
    dr = None
    if date_range and date_range[0] and date_range[1]:
        dr = [date_range[0], date_range[1]]

    return {
        "account": account,
        "top_topics": top_topics,
        "recent_sentiment": recent_sentiment,
        "signals": signals,
        "tweet_count": account["total_tweets_captured"],
        "date_range": dr,
    }


# ---------------------------------------------------------------------------
# Forensics: sentiment scoring
# ---------------------------------------------------------------------------

def score_sentiment(text: str) -> tuple[float, str]:
    lower = text.lower()
    score = 0.0
    hits = 0

    for term, weight in POSITIVE_TERMS:
        if term in lower:
            score += weight
            hits += 1
    for term, weight in NEGATIVE_TERMS:
        if term in lower:
            score += weight
            hits += 1
    for emoji in POSITIVE_EMOJIS:
        if emoji in text:
            score += 0.3
            hits += 1
    for emoji in NEGATIVE_EMOJIS:
        if emoji in text:
            score -= 0.3
            hits += 1

    for pattern in NEGATION_PATTERNS:
        if pattern in lower:
            score *= 0.5
            break

    if hits > 0:
        score = score / math.sqrt(hits)
        score = max(-1.0, min(1.0, score))

    label = "positive" if score > 0.3 else ("negative" if score < -0.3 else "neutral")
    return score, label


# ---------------------------------------------------------------------------
# Forensics: topic extraction
# ---------------------------------------------------------------------------

def extract_and_store_topics(conn, tweet_db_id: int, account_id: int, text: str, tracked_keywords: list[dict]):
    seen: set[tuple[str, str]] = set()

    for m in HASHTAG_RE.finditer(text):
        raw = m.group(0)
        topic = m.group(1).lower()
        if ("hashtag", topic) not in seen:
            seen.add(("hashtag", topic))
            conn.execute("INSERT INTO tweet_topics (tweet_id, account_id, topic, topic_type, raw_form) VALUES (?, ?, ?, ?, ?)",
                         (tweet_db_id, account_id, topic, "hashtag", raw))

    for m in CASHTAG_RE.finditer(text):
        raw = m.group(0)
        topic = m.group(1).lower()
        if ("cashtag", topic) not in seen:
            seen.add(("cashtag", topic))
            conn.execute("INSERT INTO tweet_topics (tweet_id, account_id, topic, topic_type, raw_form) VALUES (?, ?, ?, ?, ?)",
                         (tweet_db_id, account_id, topic, "cashtag", raw))

    for m in MENTION_RE.finditer(text):
        raw = m.group(0)
        topic = m.group(1).lower()
        if ("mention", topic) not in seen:
            seen.add(("mention", topic))
            conn.execute("INSERT INTO tweet_topics (tweet_id, account_id, topic, topic_type, raw_form) VALUES (?, ?, ?, ?, ?)",
                         (tweet_db_id, account_id, topic, "mention", raw))

    lower_text = text.lower()
    for kw in tracked_keywords:
        all_forms = [kw["keyword"]]
        if kw.get("aliases_json"):
            try:
                aliases = json.loads(kw["aliases_json"])
                all_forms.extend(a.lower() for a in aliases)
            except Exception:
                pass
        for form in all_forms:
            if form in lower_text:
                if ("keyword", kw["keyword"]) not in seen:
                    seen.add(("keyword", kw["keyword"]))
                    conn.execute("INSERT INTO tweet_topics (tweet_id, account_id, topic, topic_type, raw_form) VALUES (?, ?, ?, ?, ?)",
                                 (tweet_db_id, account_id, kw["keyword"], "keyword", form))
                break


# ---------------------------------------------------------------------------
# Forensics: periodic rollup
# ---------------------------------------------------------------------------

def rollup_topic_scores(conn):
    ts = now_iso()
    pairs = conn.execute("SELECT DISTINCT account_id, topic FROM tweet_topics").fetchall()
    count = 0
    for account_id, topic in pairs:
        c7 = conn.execute(
            "SELECT COUNT(*) FROM tweet_topics tt JOIN captured_tweets ct ON tt.tweet_id = ct.id WHERE tt.account_id = ? AND tt.topic = ? AND ct.tweeted_at >= datetime('now', '-7 days')",
            (account_id, topic)).fetchone()[0]
        c30 = conn.execute(
            "SELECT COUNT(*) FROM tweet_topics tt JOIN captured_tweets ct ON tt.tweet_id = ct.id WHERE tt.account_id = ? AND tt.topic = ? AND ct.tweeted_at >= datetime('now', '-30 days')",
            (account_id, topic)).fetchone()[0]
        ctotal = conn.execute(
            "SELECT COUNT(*) FROM tweet_topics WHERE account_id = ? AND topic = ?",
            (account_id, topic)).fetchone()[0]
        first = conn.execute(
            "SELECT MIN(ct.tweeted_at) FROM tweet_topics tt JOIN captured_tweets ct ON tt.tweet_id = ct.id WHERE tt.account_id = ? AND tt.topic = ?",
            (account_id, topic)).fetchone()[0] or ts
        last = conn.execute(
            "SELECT MAX(ct.tweeted_at) FROM tweet_topics tt JOIN captured_tweets ct ON tt.tweet_id = ct.id WHERE tt.account_id = ? AND tt.topic = ?",
            (account_id, topic)).fetchone()[0] or ts
        cprev7 = conn.execute(
            "SELECT COUNT(*) FROM tweet_topics tt JOIN captured_tweets ct ON tt.tweet_id = ct.id WHERE tt.account_id = ? AND tt.topic = ? AND ct.tweeted_at >= datetime('now', '-14 days') AND ct.tweeted_at < datetime('now', '-7 days')",
            (account_id, topic)).fetchone()[0]

        if c7 > 0 and cprev7 == 0:
            trend = "new"
        elif c7 == 0 and ctotal > 0:
            trend = "dormant"
        elif cprev7 > 0 and c7 > cprev7 * 1.5:
            trend = "rising"
        elif cprev7 > 0 and c7 < cprev7 * 0.5:
            trend = "falling"
        else:
            trend = "stable"

        avg_eng = conn.execute(
            "SELECT AVG(ct.like_count + 2.0 * ct.retweet_count + 1.5 * ct.reply_count) FROM tweet_topics tt JOIN captured_tweets ct ON tt.tweet_id = ct.id WHERE tt.account_id = ? AND tt.topic = ?",
            (account_id, topic)).fetchone()[0] or 0.0

        conn.execute(
            """INSERT INTO topic_scores (account_id, topic, mention_count_7d, mention_count_30d,
               mention_count_total, trend, first_seen_at, last_seen_at, avg_engagement_score, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(account_id, topic) DO UPDATE SET
               mention_count_7d=?, mention_count_30d=?, mention_count_total=?,
               trend=?, first_seen_at=?, last_seen_at=?, avg_engagement_score=?, updated_at=?""",
            (account_id, topic, c7, c30, ctotal, trend, first, last, avg_eng, ts,
             c7, c30, ctotal, trend, first, last, avg_eng, ts))
        count += 1
    conn.commit()
    return count


def detect_signals(account: dict, current_sentiment: float) -> list[dict]:
    signals = []
    conn = get_db()

    # Volume spike
    daily_avg_row = conn.execute(
        "SELECT CAST(COUNT(*) AS REAL) / MAX(1, CAST(julianday('now') - julianday(MIN(tweeted_at)) AS REAL)) FROM captured_tweets WHERE account_id = ?",
        (account["id"],)).fetchone()
    daily_avg = daily_avg_row[0] if daily_avg_row else 0.0

    day_ago = (datetime.now(timezone.utc) - timedelta(hours=24)).strftime("%Y-%m-%dT%H:%M:%S+00:00")
    today_count = conn.execute(
        "SELECT COUNT(*) FROM captured_tweets WHERE account_id = ? AND tweeted_at >= ?",
        (account["id"], day_ago)).fetchone()[0]

    if daily_avg > 0 and today_count > daily_avg * 2:
        signals.append({
            "signal_type": "volume_spike",
            "description": f"@{account['username']} tweet volume spike: {today_count} tweets today vs {daily_avg:.1f} daily avg",
            "account_id": account["id"], "username": account["username"], "severity": "medium",
        })

    # Sentiment swing
    prev = conn.execute(
        "SELECT sentiment_score FROM sentiment_snapshots WHERE account_id = ? ORDER BY window_end DESC LIMIT 1",
        (account["id"],)).fetchone()
    if prev:
        delta = abs(current_sentiment - prev[0])
        if delta > 0.4:
            direction = "positive" if current_sentiment > prev[0] else "negative"
            signals.append({
                "signal_type": "sentiment_swing",
                "description": f"@{account['username']} sentiment swing: {prev[0]:.2f} -> {current_sentiment:.2f} ({direction} shift)",
                "account_id": account["id"], "username": account["username"], "severity": "high",
            })

    # New interest
    new_topics = conn.execute(
        "SELECT topic, first_seen_at FROM topic_scores WHERE account_id = ? AND trend = 'new' LIMIT 10",
        (account["id"],)).fetchall()
    for t in new_topics:
        signals.append({
            "signal_type": "new_interest",
            "description": f"@{account['username']} started talking about '{t[0]}' (first seen: {t[1]})",
            "account_id": account["id"], "username": account["username"], "severity": "low",
        })

    # Gone quiet
    if account.get("last_checked_at"):
        try:
            last_time = datetime.fromisoformat(account["last_checked_at"].replace("Z", "+00:00"))
            hours_since = (datetime.now(timezone.utc) - last_time).total_seconds() / 3600
            if hours_since > 48 and account["total_tweets_captured"] > 10:
                signals.append({
                    "signal_type": "gone_quiet",
                    "description": f"@{account['username']} has gone quiet — no tweets in {int(hours_since)}h (previously active with {account['total_tweets_captured']} tweets)",
                    "account_id": account["id"], "username": account["username"], "severity": "medium",
                })
        except Exception:
            pass

    conn.close()
    return signals


def run_periodic_forensics(logger):
    conn = get_db()
    try:
        n = rollup_topic_scores(conn)
        if n > 0:
            logger.info(f"[SOCIAL_MONITOR] Rolled up {n} topic scores")
    except Exception as e:
        logger.error(f"[SOCIAL_MONITOR] Topic rollup error: {e}")

    accounts = conn.execute("SELECT * FROM monitored_accounts WHERE monitor_enabled = 1").fetchall()
    now = datetime.now(timezone.utc)
    window_end = now.strftime("%Y-%m-%dT%H:%M:%S+00:00")
    window_start = (now - timedelta(hours=1)).strftime("%Y-%m-%dT%H:%M:%S+00:00")

    for acct in accounts:
        acct = row_to_dict(acct)
        recent = conn.execute(
            "SELECT * FROM captured_tweets WHERE account_id = ? AND tweeted_at >= ?",
            (acct["id"], window_start)).fetchall()
        if not recent:
            continue

        total_score = sum(score_sentiment(row_to_dict(t)["text"])[0] for t in recent)
        avg_score = total_score / len(recent)
        avg_label = "positive" if avg_score > 0.3 else ("negative" if avg_score < -0.3 else "neutral")

        top = conn.execute(
            "SELECT topic, mention_count_7d FROM topic_scores WHERE account_id = ? ORDER BY mention_count_total DESC LIMIT 5",
            (acct["id"],)).fetchall()
        topic_counts = {t[0]: t[1] for t in top}
        top_topics_json = json.dumps(topic_counts)

        signals = detect_signals(acct, avg_score)
        signals_json = json.dumps(signals) if signals else None

        conn.execute(
            """INSERT INTO sentiment_snapshots (account_id, window_start, window_end,
               sentiment_score, sentiment_label, tweet_count, top_topics_json, signals_json)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
            (acct["id"], window_start, window_end, avg_score, avg_label, len(recent), top_topics_json, signals_json))
    conn.commit()
    conn.close()


# ---------------------------------------------------------------------------
# Twitter API v2 (via requests-oauthlib)
# ---------------------------------------------------------------------------

def _get_twitter_auth():
    if not all([TWITTER_CONSUMER_KEY, TWITTER_CONSUMER_SECRET, TWITTER_ACCESS_TOKEN, TWITTER_ACCESS_TOKEN_SECRET]):
        return None
    return OAuth1(TWITTER_CONSUMER_KEY, TWITTER_CONSUMER_SECRET, TWITTER_ACCESS_TOKEN, TWITTER_ACCESS_TOKEN_SECRET)


def twitter_lookup_user(username: str) -> tuple[dict | None, str | None]:
    auth = _get_twitter_auth()
    if not auth:
        return None, "Twitter credentials not configured"
    clean = username.lstrip("@")
    resp = http_requests.get(f"https://api.twitter.com/2/users/by/username/{clean}", auth=auth, timeout=15)
    if not resp.ok:
        return None, f"Twitter API error ({resp.status_code}): {resp.text[:200]}"
    data = resp.json()
    if "data" in data:
        return data["data"], None
    if "errors" in data:
        return None, f"Twitter API error: {data['errors'][0].get('detail', 'Unknown error')}"
    return None, "User not found"


def twitter_get_user_tweets(user_id: str, since_id: str | None, max_results: int = 100):
    auth = _get_twitter_auth()
    if not auth:
        return [], None, "Twitter credentials not configured"
    params = {
        "max_results": str(max_results),
        "tweet.fields": "created_at,conversation_id,in_reply_to_user_id,public_metrics,referenced_tweets",
        "exclude": "retweets",
    }
    if since_id:
        params["since_id"] = since_id
    resp = http_requests.get(f"https://api.twitter.com/2/users/{user_id}/tweets", params=params, auth=auth, timeout=30)

    remaining = resp.headers.get("x-rate-limit-remaining")
    remaining = int(remaining) if remaining else None

    if resp.status_code == 429:
        return [], remaining, "Rate limited — backing off"
    if not resp.ok:
        return [], remaining, f"Twitter API error ({resp.status_code}): {resp.text[:200]}"

    data = resp.json()
    tweets = data.get("data", [])
    return tweets, remaining, None


def _determine_tweet_type(tweet: dict) -> str:
    refs = tweet.get("referenced_tweets", [])
    if refs:
        for r in refs:
            rt = r.get("type", "")
            if rt == "replied_to":
                return "reply"
            elif rt == "quoted":
                return "quote"
            elif rt == "retweeted":
                return "retweet"
    return "original"


# ---------------------------------------------------------------------------
# Backup operations
# ---------------------------------------------------------------------------

def backup_export():
    conn = get_db()
    accounts = conn.execute(
        "SELECT username, display_name, twitter_user_id, monitor_enabled, custom_keywords, notes FROM monitored_accounts ORDER BY created_at ASC"
    ).fetchall()
    keywords = conn.execute(
        "SELECT keyword, category, aliases_json FROM tracked_keywords ORDER BY keyword ASC"
    ).fetchall()
    conn.close()
    return {
        "accounts": [row_to_dict(a) for a in accounts],
        "keywords": [row_to_dict(k) for k in keywords],
    }


def backup_restore(data: dict) -> int:
    conn = get_db()
    for table in ["sentiment_snapshots", "topic_scores", "tweet_topics", "captured_tweets", "monitored_accounts", "tracked_keywords"]:
        conn.execute(f"DELETE FROM {table}")
    ts = now_iso()
    count = 0
    for acct in data.get("accounts", []):
        conn.execute(
            "INSERT OR IGNORE INTO monitored_accounts (twitter_user_id, username, display_name, monitor_enabled, custom_keywords, notes, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            (acct.get("twitter_user_id"), acct.get("username"), acct.get("display_name"),
             acct.get("monitor_enabled", 1), acct.get("custom_keywords"), acct.get("notes"), ts, ts))
        count += 1
    for kw in data.get("keywords", []):
        conn.execute(
            "INSERT OR IGNORE INTO tracked_keywords (keyword, category, aliases_json, created_at) VALUES (?, ?, ?, ?)",
            (kw.get("keyword"), kw.get("category"), kw.get("aliases_json"), ts))
        count += 1
    conn.commit()
    conn.close()
    return count


# ---------------------------------------------------------------------------
# Background Worker
# ---------------------------------------------------------------------------

def worker_loop():
    global _last_tick_at
    logger = logging.getLogger("social_monitor.worker")
    logger.info(f"[SOCIAL_MONITOR] Worker started (poll interval: {POLL_INTERVAL}s)")

    auth = _get_twitter_auth()
    if not auth:
        logger.error("[SOCIAL_MONITOR] Twitter credentials not available — worker stopping")
        return

    tick_count = 0
    while True:
        time.sleep(POLL_INTERVAL)
        tick_count += 1
        try:
            poll_tick(logger)
            with _last_tick_lock:
                _last_tick_at = now_iso()
        except Exception as e:
            logger.error(f"[SOCIAL_MONITOR] Tick error: {e}")

        if tick_count % ROLLUP_INTERVAL_TICKS == 0:
            try:
                run_periodic_forensics(logger)
            except Exception as e:
                logger.error(f"[SOCIAL_MONITOR] Forensics error: {e}")


def poll_tick(logger):
    conn = get_db()
    accounts = conn.execute(
        "SELECT * FROM monitored_accounts WHERE monitor_enabled = 1 ORDER BY last_checked_at ASC NULLS FIRST"
    ).fetchall()
    conn.close()
    if not accounts:
        return

    logger.debug(f"[SOCIAL_MONITOR] Tick: checking {len(accounts)} accounts")
    tracked_keywords = keyword_list()
    total_new = 0

    for acct in accounts:
        acct = row_to_dict(acct)
        try:
            new_count = fetch_account_tweets(acct, tracked_keywords, logger)
            total_new += new_count
        except Exception as e:
            if "Rate limited" in str(e):
                logger.warning("[SOCIAL_MONITOR] Rate limited — stopping this tick early")
                break
            logger.warning(f"[SOCIAL_MONITOR] Error fetching @{acct['username']}: {e}")
        time.sleep(0.5)  # 500ms delay between accounts

    # Process unprocessed tweets
    conn = get_db()
    unprocessed = conn.execute(
        "SELECT * FROM captured_tweets WHERE processed = 0 ORDER BY captured_at ASC LIMIT 500"
    ).fetchall()
    for tweet in unprocessed:
        tweet = row_to_dict(tweet)
        extract_and_store_topics(conn, tweet["id"], tweet["account_id"], tweet["text"], tracked_keywords)
        conn.execute("UPDATE captured_tweets SET processed = 1 WHERE id = ?", (tweet["id"],))
    conn.commit()
    conn.close()

    if total_new > 0:
        logger.info(f"[SOCIAL_MONITOR] Tick complete: {total_new} new tweets captured")


def fetch_account_tweets(acct: dict, tracked_keywords: list, logger) -> int:
    tweets, remaining, err = twitter_get_user_tweets(
        acct["twitter_user_id"], acct.get("last_tweet_id"), 100
    )
    if err:
        raise RuntimeError(err)

    if remaining is not None and remaining < 5:
        logger.warning(f"[SOCIAL_MONITOR] Rate limit low: {remaining} remaining for @{acct['username']}")

    conn = get_db()
    if not tweets:
        ts = now_iso()
        conn.execute("UPDATE monitored_accounts SET last_checked_at = ?, updated_at = ? WHERE id = ?", (ts, ts, acct["id"]))
        conn.commit()
        conn.close()
        return 0

    new_count = 0
    max_tweet_id = None

    for tweet in tweets:
        metrics = tweet.get("public_metrics", {})
        raw_json = json.dumps(tweet)
        tweet_type = _determine_tweet_type(tweet)

        try:
            conn.execute(
                """INSERT OR IGNORE INTO captured_tweets
                   (account_id, tweet_id, text, tweet_type, conversation_id,
                    in_reply_to_user_id, like_count, retweet_count, reply_count,
                    quote_count, tweeted_at, raw_json)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (acct["id"], tweet["id"], tweet["text"], tweet_type,
                 tweet.get("conversation_id"), tweet.get("in_reply_to_user_id"),
                 metrics.get("like_count", 0), metrics.get("retweet_count", 0),
                 metrics.get("reply_count", 0), metrics.get("quote_count", 0),
                 tweet.get("created_at", ""), raw_json))
            if conn.execute("SELECT changes()").fetchone()[0] > 0:
                new_count += 1
        except Exception:
            pass

        if max_tweet_id is None or tweet["id"] > max_tweet_id:
            max_tweet_id = tweet["id"]

    if max_tweet_id:
        ts = now_iso()
        conn.execute(
            "UPDATE monitored_accounts SET last_tweet_id = ?, last_checked_at = ?, updated_at = ?, total_tweets_captured = total_tweets_captured + ? WHERE id = ?",
            (max_tweet_id, ts, ts, new_count, acct["id"]))

    conn.commit()
    conn.close()
    return new_count


# ---------------------------------------------------------------------------
# App
# ---------------------------------------------------------------------------

def _status_extra():
    stats = tweet_stats()
    with _last_tick_lock:
        stats["last_tick_at"] = _last_tick_at
    stats["poll_interval_secs"] = POLL_INTERVAL
    return stats


app = create_app("social_monitor", status_extra_fn=_status_extra)


# ---------------------------------------------------------------------------
# RPC: Watchlist tool (accounts + keywords)
# ---------------------------------------------------------------------------

@app.route("/rpc/tools/watchlist", methods=["POST"])
def rpc_watchlist():
    body = request.get_json(silent=True) or {}
    action = body.get("action")
    try:
        if action == "add_account":
            username = body.get("username")
            if not username:
                return error("username is required")
            user, err = twitter_lookup_user(username)
            if err:
                return error(f"Failed to look up @{username.lstrip('@')}: {err}")
            entry, err = account_add(user["id"], user["username"], user.get("name"), body.get("notes"), body.get("custom_keywords"))
            if err:
                return error(err)
            return success(entry)

        elif action == "remove_account":
            entry_id = body.get("id")
            if entry_id is None:
                return error("id is required")
            if account_remove(entry_id):
                return success(True)
            return error(f"Account #{entry_id} not found", 404)

        elif action == "list_accounts":
            return success(account_list())

        elif action == "update_account":
            entry_id = body.get("id")
            if entry_id is None:
                return error("id is required")
            if account_update(entry_id, body.get("monitor_enabled"), body.get("custom_keywords"), body.get("notes")):
                return success(True)
            return error(f"Account #{entry_id} not found", 404)

        elif action == "add_keyword":
            kw = body.get("keyword")
            if not kw:
                return error("keyword is required")
            entry, err = keyword_add(kw, body.get("category"), body.get("aliases"))
            if err:
                return error(err)
            return success(entry)

        elif action == "remove_keyword":
            entry_id = body.get("id")
            if entry_id is None:
                return error("id is required")
            if keyword_remove(entry_id):
                return success(True)
            return error(f"Keyword #{entry_id} not found", 404)

        elif action == "list_keywords":
            return success(keyword_list())

        else:
            return error(f"Unknown action: {action}. Valid: add_account, remove_account, list_accounts, update_account, add_keyword, remove_keyword, list_keywords")
    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# RPC: Tweets tool
# ---------------------------------------------------------------------------

@app.route("/rpc/tools/tweets", methods=["POST"])
def rpc_tweets():
    body = request.get_json(silent=True) or {}
    action = body.get("action")
    try:
        if action == "recent":
            return success(tweet_query(limit=body.get("limit", 25)))

        elif action == "search":
            return success(tweet_query(
                search_text=body.get("search_text"),
                tweet_type=body.get("tweet_type"),
                limit=body.get("limit", 25)))

        elif action == "by_account":
            return success(tweet_query(
                username=body.get("username"),
                limit=body.get("limit", 25)))

        elif action == "stats":
            return success(tweet_stats())

        else:
            return error(f"Unknown action: {action}. Valid: recent, search, by_account, stats")
    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# RPC: Forensics tool
# ---------------------------------------------------------------------------

@app.route("/rpc/tools/forensics", methods=["POST"])
def rpc_forensics():
    body = request.get_json(silent=True) or {}
    action = body.get("action")
    try:
        if action == "topics":
            return success(topic_query(
                account_id=body.get("account_id"),
                topic=body.get("topic"),
                trend=body.get("trend"),
                limit=body.get("limit", 25)))

        elif action == "sentiment":
            return success(sentiment_query(
                account_id=body.get("account_id"),
                limit=body.get("limit", 25)))

        elif action == "report":
            acct = None
            if body.get("account_id"):
                acct = account_get_by_id(body["account_id"])
            elif body.get("username"):
                acct = account_get_by_username(body["username"])
            else:
                return error("Either account_id or username is required")
            if not acct:
                return error("Account not found", 404)
            return success(generate_report(acct))

        elif action == "signals":
            acct = None
            if body.get("account_id"):
                acct = account_get_by_id(body["account_id"])
            elif body.get("username"):
                acct = account_get_by_username(body["username"])
            else:
                return error("Either account_id or username is required")
            if not acct:
                return error("Account not found", 404)
            recent_sent = sentiment_query(account_id=acct["id"], limit=1)
            current = recent_sent[0]["sentiment_score"] if recent_sent else 0.0
            return success(detect_signals(acct, current))

        else:
            return error(f"Unknown action: {action}. Valid: topics, sentiment, report, signals")
    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# RPC: Control tool
# ---------------------------------------------------------------------------

@app.route("/rpc/tools/control", methods=["POST"])
def rpc_control():
    body = request.get_json(silent=True) or {}
    action = body.get("action")
    try:
        if action == "status":
            return success(_status_extra())
        else:
            return error(f"Unknown action: {action}. Valid: status")
    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# RPC: Backup / Restore
# ---------------------------------------------------------------------------

@app.route("/rpc/backup/export", methods=["POST"])
def rpc_backup_export():
    try:
        return success(backup_export())
    except Exception as e:
        return error(str(e))


@app.route("/rpc/backup/restore", methods=["POST"])
def rpc_backup_restore():
    body = request.get_json(silent=True) or {}
    data = body.get("data", {})
    if not isinstance(data, dict):
        return error("data must be an object with 'accounts' and 'keywords' arrays")
    try:
        count = backup_restore(data)
        return success(count)
    except Exception as e:
        return error(str(e))


# ---------------------------------------------------------------------------
# Dashboard
# ---------------------------------------------------------------------------

def _format_uptime(secs: int) -> str:
    hours = secs // 3600
    minutes = (secs % 3600) // 60
    seconds = secs % 60
    if hours > 0:
        return f"{hours}h {minutes}m {seconds}s"
    elif minutes > 0:
        return f"{minutes}m {seconds}s"
    return f"{seconds}s"


def _html_escape(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


@app.route("/")
def dashboard():
    stats = tweet_stats()
    accounts = account_list()
    recent = tweet_query(limit=20)
    top_topics = topic_query(limit=20)
    with _last_tick_lock:
        last_tick = _last_tick_at or "not yet"
    uptime = _format_uptime(int(time.time() - _start_time))

    account_rows = ""
    for a in accounts:
        status = "Active" if a["monitor_enabled"] else "Paused"
        last_checked = a.get("last_checked_at") or "-"
        account_rows += f'<tr><td>{a["id"]}</td><td>@{a["username"]}</td><td>{a.get("display_name") or "-"}</td><td>{a["total_tweets_captured"]}</td><td>{status}</td><td>{last_checked}</td></tr>\n'
    if not account_rows:
        account_rows = '<tr><td colspan="6">No accounts being monitored.</td></tr>'

    # Build account lookup for tweets and topics
    acct_map = {a["id"]: a["username"] for a in accounts}

    tweet_rows = ""
    for t in recent:
        uname = acct_map.get(t["account_id"], "?")
        text_short = _html_escape(t["text"][:100] + "..." if len(t["text"]) > 100 else t["text"])
        engagement = t.get("like_count", 0) + t.get("retweet_count", 0)
        tweet_rows += f'<tr><td>@{uname}</td><td>{t["tweet_type"]}</td><td>{text_short}</td><td>{t["tweeted_at"]}</td><td>{engagement}</td></tr>\n'
    if not tweet_rows:
        tweet_rows = '<tr><td colspan="5">No tweets captured yet.</td></tr>'

    topic_rows = ""
    for ts in top_topics:
        uname = acct_map.get(ts["account_id"], "?")
        trend_cls = {"rising": ' class="rising"', "falling": ' class="falling"', "new": ' class="new-topic"'}.get(ts["trend"], "")
        topic_rows += f'<tr{trend_cls}><td>{ts["topic"]}</td><td>@{uname}</td><td>{ts["mention_count_7d"]}</td><td>{ts["mention_count_30d"]}</td><td>{ts["mention_count_total"]}</td><td>{ts["trend"]}</td><td>{ts["avg_engagement_score"]:.1f}</td></tr>\n'
    if not topic_rows:
        topic_rows = '<tr><td colspan="7">No topics extracted yet.</td></tr>'

    html = f"""<!DOCTYPE html>
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
  h2 {{ color: #c9d1d9; margin-bottom: 12px; font-size: 1.1em; }}
  .section {{ margin-bottom: 28px; }}
</style>
</head>
<body>
  <h1>Social Monitor</h1>
  <p class="meta">Uptime: {uptime} &middot; Last tick: {last_tick} &middot; Poll interval: {POLL_INTERVAL}s</p>

  <div class="stats">
    <div class="stat"><span class="val">{stats['monitored_accounts']}</span><span class="lbl">Monitored</span></div>
    <div class="stat"><span class="val">{stats['active_accounts']}</span><span class="lbl">Active</span></div>
    <div class="stat"><span class="val">{stats['total_tweets']}</span><span class="lbl">Total Tweets</span></div>
    <div class="stat"><span class="val">{stats['tweets_today']}</span><span class="lbl">Today</span></div>
    <div class="stat"><span class="val">{stats['tweets_7d']}</span><span class="lbl">7 Days</span></div>
    <div class="stat"><span class="val">{stats['unique_topics']}</span><span class="lbl">Topics</span></div>
  </div>

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

  <script>setTimeout(() => location.reload(), 30000);</script>
</body>
</html>"""
    return html


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    logging.getLogger("werkzeug").setLevel(logging.ERROR)
    init_db()

    has_creds = all([TWITTER_CONSUMER_KEY, TWITTER_CONSUMER_SECRET, TWITTER_ACCESS_TOKEN, TWITTER_ACCESS_TOKEN_SECRET])
    if has_creds:
        worker_thread = threading.Thread(target=worker_loop, daemon=True)
        worker_thread.start()
    else:
        logging.warning("[SOCIAL_MONITOR] Twitter credentials not set — background worker disabled")

    port = int(os.environ.get("MODULE_PORT", os.environ.get("SOCIAL_MONITOR_PORT", "9102")))
    app.run(host="127.0.0.1", port=port)
