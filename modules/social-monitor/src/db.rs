//! SQLite database operations for the social monitor service.

use rusqlite::{Connection, Result as SqliteResult};
use social_monitor_types::*;
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &str) -> SqliteResult<Self> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.create_tables()?;
        Ok(db)
    }

    fn create_tables(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "CREATE TABLE IF NOT EXISTS monitored_accounts (
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
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS captured_tweets (
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
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_tweets_account_time ON captured_tweets(account_id, tweeted_at DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_tweets_processed ON captured_tweets(processed, captured_at ASC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_tweets_time ON captured_tweets(tweeted_at DESC)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS tweet_topics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tweet_id INTEGER NOT NULL,
                account_id INTEGER NOT NULL,
                topic TEXT NOT NULL,
                topic_type TEXT NOT NULL,
                raw_form TEXT,
                FOREIGN KEY (tweet_id) REFERENCES captured_tweets(id) ON DELETE CASCADE,
                FOREIGN KEY (account_id) REFERENCES monitored_accounts(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_topics_topic_account ON tweet_topics(topic, account_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_topics_account ON tweet_topics(account_id, topic)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS topic_scores (
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
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS sentiment_snapshots (
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
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS tracked_keywords (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                keyword TEXT NOT NULL UNIQUE,
                category TEXT,
                aliases_json TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        Ok(())
    }

    // =====================================================
    // Account Operations
    // =====================================================

    pub fn add_account(
        &self,
        twitter_user_id: &str,
        username: &str,
        display_name: Option<&str>,
        notes: Option<&str>,
        custom_keywords: Option<&str>,
    ) -> SqliteResult<MonitoredAccount> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO monitored_accounts (twitter_user_id, username, display_name, notes, custom_keywords, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![twitter_user_id, username, display_name, notes, custom_keywords, now],
        )?;

        let id = conn.last_insert_rowid();
        Ok(MonitoredAccount {
            id,
            twitter_user_id: twitter_user_id.to_string(),
            username: username.to_string(),
            display_name: display_name.map(|s| s.to_string()),
            monitor_enabled: true,
            custom_keywords: custom_keywords.map(|s| s.to_string()),
            notes: notes.map(|s| s.to_string()),
            last_tweet_id: None,
            last_checked_at: None,
            total_tweets_captured: 0,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn remove_account(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute("DELETE FROM monitored_accounts WHERE id = ?1", [id])?;
        Ok(rows > 0)
    }

    pub fn list_accounts(&self) -> SqliteResult<Vec<MonitoredAccount>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, twitter_user_id, username, display_name, monitor_enabled,
                    custom_keywords, notes, last_tweet_id, last_checked_at,
                    total_tweets_captured, created_at, updated_at
             FROM monitored_accounts ORDER BY created_at ASC",
        )?;
        let entries = stmt
            .query_map([], |row| row_to_account(row))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn list_active_accounts(&self) -> SqliteResult<Vec<MonitoredAccount>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, twitter_user_id, username, display_name, monitor_enabled,
                    custom_keywords, notes, last_tweet_id, last_checked_at,
                    total_tweets_captured, created_at, updated_at
             FROM monitored_accounts
             WHERE monitor_enabled = 1
             ORDER BY last_checked_at ASC NULLS FIRST",
        )?;
        let entries = stmt
            .query_map([], |row| row_to_account(row))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn get_account_by_id(&self, id: i64) -> SqliteResult<Option<MonitoredAccount>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, twitter_user_id, username, display_name, monitor_enabled,
                    custom_keywords, notes, last_tweet_id, last_checked_at,
                    total_tweets_captured, created_at, updated_at
             FROM monitored_accounts WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], |row| row_to_account(row))?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn get_account_by_username(&self, username: &str) -> SqliteResult<Option<MonitoredAccount>> {
        let conn = self.conn.lock().unwrap();
        let lower = username.to_lowercase();
        let mut stmt = conn.prepare(
            "SELECT id, twitter_user_id, username, display_name, monitor_enabled,
                    custom_keywords, notes, last_tweet_id, last_checked_at,
                    total_tweets_captured, created_at, updated_at
             FROM monitored_accounts WHERE LOWER(username) = ?1",
        )?;
        let mut rows = stmt.query_map([lower], |row| row_to_account(row))?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn update_account(
        &self,
        id: i64,
        monitor_enabled: Option<bool>,
        custom_keywords: Option<&str>,
        notes: Option<&str>,
    ) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();

        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut param_idx = 2u32;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        if let Some(enabled) = monitor_enabled {
            updates.push(format!("monitor_enabled = ?{}", param_idx));
            params.push(Box::new(enabled));
            param_idx += 1;
        }
        if let Some(kw) = custom_keywords {
            updates.push(format!("custom_keywords = ?{}", param_idx));
            params.push(Box::new(kw.to_string()));
            param_idx += 1;
        }
        if let Some(n) = notes {
            updates.push(format!("notes = ?{}", param_idx));
            params.push(Box::new(n.to_string()));
            param_idx += 1;
        }

        let sql = format!(
            "UPDATE monitored_accounts SET {} WHERE id = ?{}",
            updates.join(", "),
            param_idx
        );
        params.push(Box::new(id));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.execute(&sql, param_refs.as_slice())?;
        Ok(rows > 0)
    }

    pub fn update_account_cursor(
        &self,
        id: i64,
        last_tweet_id: &str,
        new_count: i64,
    ) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE monitored_accounts SET last_tweet_id = ?1, last_checked_at = ?2, updated_at = ?2,
             total_tweets_captured = total_tweets_captured + ?3 WHERE id = ?4",
            rusqlite::params![last_tweet_id, now, new_count, id],
        )?;
        Ok(())
    }

    pub fn update_account_checked(&self, id: i64) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE monitored_accounts SET last_checked_at = ?1, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, id],
        )?;
        Ok(())
    }

    // =====================================================
    // Tweet Operations
    // =====================================================

    pub fn insert_tweet(
        &self,
        account_id: i64,
        tweet_id: &str,
        text: &str,
        tweet_type: &str,
        conversation_id: Option<&str>,
        in_reply_to_user_id: Option<&str>,
        like_count: i64,
        retweet_count: i64,
        reply_count: i64,
        quote_count: i64,
        tweeted_at: &str,
        raw_json: Option<&str>,
    ) -> SqliteResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO captured_tweets (
                account_id, tweet_id, text, tweet_type, conversation_id,
                in_reply_to_user_id, like_count, retweet_count, reply_count,
                quote_count, tweeted_at, raw_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                account_id,
                tweet_id,
                text,
                tweet_type,
                conversation_id,
                in_reply_to_user_id,
                like_count,
                retweet_count,
                reply_count,
                quote_count,
                tweeted_at,
                raw_json
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_unprocessed_tweets(&self, limit: usize) -> SqliteResult<Vec<CapturedTweet>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, tweet_id, text, tweet_type, conversation_id,
                    in_reply_to_user_id, like_count, retweet_count, reply_count,
                    quote_count, tweeted_at, captured_at, processed, raw_json
             FROM captured_tweets
             WHERE processed = 0
             ORDER BY captured_at ASC
             LIMIT ?1",
        )?;
        let entries = stmt
            .query_map([limit as i64], |row| row_to_tweet(row))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn mark_tweet_processed(&self, tweet_db_id: i64) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE captured_tweets SET processed = 1 WHERE id = ?1",
            [tweet_db_id],
        )?;
        Ok(())
    }

    pub fn query_tweets(&self, filter: &TweetFilter) -> SqliteResult<Vec<CapturedTweet>> {
        let conn = self.conn.lock().unwrap();
        let mut conditions = vec!["1=1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut param_idx = 1u32;

        if let Some(aid) = filter.account_id {
            conditions.push(format!("t.account_id = ?{}", param_idx));
            params.push(Box::new(aid));
            param_idx += 1;
        }
        if let Some(ref username) = filter.username {
            conditions.push(format!(
                "t.account_id IN (SELECT id FROM monitored_accounts WHERE LOWER(username) = ?{})",
                param_idx
            ));
            params.push(Box::new(username.to_lowercase()));
            param_idx += 1;
        }
        if let Some(ref text) = filter.search_text {
            conditions.push(format!("t.text LIKE ?{}", param_idx));
            params.push(Box::new(format!("%{}%", text)));
            param_idx += 1;
        }
        if let Some(ref tt) = filter.tweet_type {
            conditions.push(format!("t.tweet_type = ?{}", param_idx));
            params.push(Box::new(tt.clone()));
            param_idx += 1;
        }
        if let Some(ref since) = filter.since {
            conditions.push(format!("t.tweeted_at >= ?{}", param_idx));
            params.push(Box::new(since.clone()));
            param_idx += 1;
        }
        if let Some(ref until) = filter.until {
            conditions.push(format!("t.tweeted_at <= ?{}", param_idx));
            params.push(Box::new(until.clone()));
            param_idx += 1;
        }
        let _ = param_idx;

        let limit = filter.limit.unwrap_or(50).min(200);
        let sql = format!(
            "SELECT t.id, t.account_id, t.tweet_id, t.text, t.tweet_type, t.conversation_id,
                    t.in_reply_to_user_id, t.like_count, t.retweet_count, t.reply_count,
                    t.quote_count, t.tweeted_at, t.captured_at, t.processed, t.raw_json
             FROM captured_tweets t
             WHERE {}
             ORDER BY t.tweeted_at DESC
             LIMIT {}",
            conditions.join(" AND "),
            limit
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let entries = stmt
            .query_map(param_refs.as_slice(), |row| row_to_tweet(row))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn get_tweet_stats(&self) -> SqliteResult<TweetStats> {
        let conn = self.conn.lock().unwrap();
        let total_tweets: i64 = conn
            .query_row("SELECT COUNT(*) FROM captured_tweets", [], |row| row.get(0))
            .unwrap_or(0);
        let monitored_accounts: i64 = conn
            .query_row("SELECT COUNT(*) FROM monitored_accounts", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);
        let active_accounts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM monitored_accounts WHERE monitor_enabled = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let tweets_today: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM captured_tweets WHERE tweeted_at >= datetime('now', '-1 day')",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let tweets_7d: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM captured_tweets WHERE tweeted_at >= datetime('now', '-7 days')",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let unique_topics: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT topic) FROM tweet_topics",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(TweetStats {
            total_tweets,
            monitored_accounts,
            active_accounts,
            tweets_today,
            tweets_7d,
            unique_topics,
        })
    }

    // =====================================================
    // Topic Operations
    // =====================================================

    pub fn insert_topic(
        &self,
        tweet_db_id: i64,
        account_id: i64,
        topic: &str,
        topic_type: &str,
        raw_form: Option<&str>,
    ) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tweet_topics (tweet_id, account_id, topic, topic_type, raw_form)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![tweet_db_id, account_id, topic, topic_type, raw_form],
        )?;
        Ok(())
    }

    pub fn query_topic_scores(&self, filter: &TopicFilter) -> SqliteResult<Vec<TopicScore>> {
        let conn = self.conn.lock().unwrap();
        let mut conditions = vec!["1=1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut param_idx = 1u32;

        if let Some(aid) = filter.account_id {
            conditions.push(format!("ts.account_id = ?{}", param_idx));
            params.push(Box::new(aid));
            param_idx += 1;
        }
        if let Some(ref topic) = filter.topic {
            conditions.push(format!("ts.topic LIKE ?{}", param_idx));
            params.push(Box::new(format!("%{}%", topic)));
            param_idx += 1;
        }
        if let Some(ref trend) = filter.trend {
            conditions.push(format!("ts.trend = ?{}", param_idx));
            params.push(Box::new(trend.clone()));
            param_idx += 1;
        }
        if let Some(min) = filter.min_mentions {
            conditions.push(format!("ts.mention_count_total >= ?{}", param_idx));
            params.push(Box::new(min));
            param_idx += 1;
        }
        let _ = param_idx;

        let limit = filter.limit.unwrap_or(50).min(200);
        let sql = format!(
            "SELECT ts.id, ts.account_id, ts.topic, ts.mention_count_7d, ts.mention_count_30d,
                    ts.mention_count_total, ts.trend, ts.first_seen_at, ts.last_seen_at,
                    ts.avg_engagement_score, ts.updated_at
             FROM topic_scores ts
             WHERE {}
             ORDER BY ts.mention_count_total DESC
             LIMIT {}",
            conditions.join(" AND "),
            limit
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let entries = stmt
            .query_map(param_refs.as_slice(), |row| row_to_topic_score(row))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    /// Run the topic score rollup aggregation
    pub fn rollup_topic_scores(&self) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();

        // Get all distinct (account_id, topic) pairs
        let mut stmt = conn.prepare(
            "SELECT DISTINCT account_id, topic FROM tweet_topics",
        )?;
        let pairs: Vec<(i64, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get::<_, String>(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        let mut count = 0;
        for (account_id, topic) in &pairs {
            let count_7d: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM tweet_topics tt
                     JOIN captured_tweets ct ON tt.tweet_id = ct.id
                     WHERE tt.account_id = ?1 AND tt.topic = ?2
                     AND ct.tweeted_at >= datetime('now', '-7 days')",
                    rusqlite::params![account_id, topic],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let count_30d: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM tweet_topics tt
                     JOIN captured_tweets ct ON tt.tweet_id = ct.id
                     WHERE tt.account_id = ?1 AND tt.topic = ?2
                     AND ct.tweeted_at >= datetime('now', '-30 days')",
                    rusqlite::params![account_id, topic],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let count_total: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM tweet_topics
                     WHERE account_id = ?1 AND topic = ?2",
                    rusqlite::params![account_id, topic],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let first_seen: String = conn
                .query_row(
                    "SELECT MIN(ct.tweeted_at) FROM tweet_topics tt
                     JOIN captured_tweets ct ON tt.tweet_id = ct.id
                     WHERE tt.account_id = ?1 AND tt.topic = ?2",
                    rusqlite::params![account_id, topic],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| now.clone());

            let last_seen: String = conn
                .query_row(
                    "SELECT MAX(ct.tweeted_at) FROM tweet_topics tt
                     JOIN captured_tweets ct ON tt.tweet_id = ct.id
                     WHERE tt.account_id = ?1 AND tt.topic = ?2",
                    rusqlite::params![account_id, topic],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| now.clone());

            // Previous week count for trend detection
            let count_prev_7d: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM tweet_topics tt
                     JOIN captured_tweets ct ON tt.tweet_id = ct.id
                     WHERE tt.account_id = ?1 AND tt.topic = ?2
                     AND ct.tweeted_at >= datetime('now', '-14 days')
                     AND ct.tweeted_at < datetime('now', '-7 days')",
                    rusqlite::params![account_id, topic],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let trend = if count_7d > 0 && count_prev_7d == 0 {
                "new"
            } else if count_7d == 0 && count_total > 0 {
                "dormant"
            } else if count_prev_7d > 0 && (count_7d as f64) > (count_prev_7d as f64 * 1.5) {
                "rising"
            } else if count_prev_7d > 0 && (count_7d as f64) < (count_prev_7d as f64 * 0.5) {
                "falling"
            } else {
                "stable"
            };

            // Average engagement score for tweets containing this topic
            let avg_engagement: f64 = conn
                .query_row(
                    "SELECT AVG(ct.like_count + 2.0 * ct.retweet_count + 1.5 * ct.reply_count)
                     FROM tweet_topics tt
                     JOIN captured_tweets ct ON tt.tweet_id = ct.id
                     WHERE tt.account_id = ?1 AND tt.topic = ?2",
                    rusqlite::params![account_id, topic],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            conn.execute(
                "INSERT INTO topic_scores (account_id, topic, mention_count_7d, mention_count_30d,
                 mention_count_total, trend, first_seen_at, last_seen_at, avg_engagement_score, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(account_id, topic) DO UPDATE SET
                    mention_count_7d = ?3, mention_count_30d = ?4, mention_count_total = ?5,
                    trend = ?6, first_seen_at = ?7, last_seen_at = ?8,
                    avg_engagement_score = ?9, updated_at = ?10",
                rusqlite::params![
                    account_id, topic, count_7d, count_30d, count_total,
                    trend, first_seen, last_seen, avg_engagement, now
                ],
            )?;
            count += 1;
        }

        Ok(count)
    }

    // =====================================================
    // Sentiment Operations
    // =====================================================

    pub fn insert_sentiment_snapshot(
        &self,
        account_id: i64,
        window_start: &str,
        window_end: &str,
        sentiment_score: f64,
        sentiment_label: &str,
        tweet_count: i64,
        top_topics_json: Option<&str>,
        signals_json: Option<&str>,
        ai_summary: Option<&str>,
    ) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sentiment_snapshots (account_id, window_start, window_end,
             sentiment_score, sentiment_label, tweet_count, top_topics_json, signals_json, ai_summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                account_id,
                window_start,
                window_end,
                sentiment_score,
                sentiment_label,
                tweet_count,
                top_topics_json,
                signals_json,
                ai_summary
            ],
        )?;
        Ok(())
    }

    pub fn query_sentiment(
        &self,
        filter: &SentimentFilter,
    ) -> SqliteResult<Vec<SentimentSnapshot>> {
        let conn = self.conn.lock().unwrap();
        let mut conditions = vec!["1=1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut param_idx = 1u32;

        if let Some(aid) = filter.account_id {
            conditions.push(format!("s.account_id = ?{}", param_idx));
            params.push(Box::new(aid));
            param_idx += 1;
        }
        if let Some(ref since) = filter.since {
            conditions.push(format!("s.window_end >= ?{}", param_idx));
            params.push(Box::new(since.clone()));
            param_idx += 1;
        }
        if let Some(ref until) = filter.until {
            conditions.push(format!("s.window_start <= ?{}", param_idx));
            params.push(Box::new(until.clone()));
            param_idx += 1;
        }
        let _ = param_idx;

        let limit = filter.limit.unwrap_or(50).min(200);
        let sql = format!(
            "SELECT s.id, s.account_id, s.window_start, s.window_end,
                    s.sentiment_score, s.sentiment_label, s.tweet_count,
                    s.top_topics_json, s.signals_json, s.ai_summary, s.created_at
             FROM sentiment_snapshots s
             WHERE {}
             ORDER BY s.window_end DESC
             LIMIT {}",
            conditions.join(" AND "),
            limit
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let entries = stmt
            .query_map(param_refs.as_slice(), |row| row_to_sentiment(row))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn get_last_sentiment_for_account(
        &self,
        account_id: i64,
    ) -> SqliteResult<Option<SentimentSnapshot>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, window_start, window_end,
                    sentiment_score, sentiment_label, tweet_count,
                    top_topics_json, signals_json, ai_summary, created_at
             FROM sentiment_snapshots
             WHERE account_id = ?1
             ORDER BY window_end DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map([account_id], |row| row_to_sentiment(row))?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    // =====================================================
    // Keyword Operations
    // =====================================================

    pub fn add_keyword(
        &self,
        keyword: &str,
        category: Option<&str>,
        aliases_json: Option<&str>,
    ) -> SqliteResult<TrackedKeyword> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let normalized = keyword.to_lowercase();

        conn.execute(
            "INSERT INTO tracked_keywords (keyword, category, aliases_json, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![normalized, category, aliases_json, now],
        )?;

        let id = conn.last_insert_rowid();
        Ok(TrackedKeyword {
            id,
            keyword: normalized,
            category: category.map(|s| s.to_string()),
            aliases_json: aliases_json.map(|s| s.to_string()),
            created_at: now,
        })
    }

    pub fn remove_keyword(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute("DELETE FROM tracked_keywords WHERE id = ?1", [id])?;
        Ok(rows > 0)
    }

    pub fn list_keywords(&self) -> SqliteResult<Vec<TrackedKeyword>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, keyword, category, aliases_json, created_at
             FROM tracked_keywords ORDER BY keyword ASC",
        )?;
        let entries = stmt
            .query_map([], |row| {
                Ok(TrackedKeyword {
                    id: row.get(0)?,
                    keyword: row.get(1)?,
                    category: row.get(2)?,
                    aliases_json: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    // =====================================================
    // Account Tweets for Forensics
    // =====================================================

    pub fn get_tweets_for_account_since(
        &self,
        account_id: i64,
        since: &str,
    ) -> SqliteResult<Vec<CapturedTweet>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, tweet_id, text, tweet_type, conversation_id,
                    in_reply_to_user_id, like_count, retweet_count, reply_count,
                    quote_count, tweeted_at, captured_at, processed, raw_json
             FROM captured_tweets
             WHERE account_id = ?1 AND tweeted_at >= ?2
             ORDER BY tweeted_at DESC",
        )?;
        let entries = stmt
            .query_map(rusqlite::params![account_id, since], |row| {
                row_to_tweet(row)
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn get_account_tweet_date_range(
        &self,
        account_id: i64,
    ) -> SqliteResult<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT MIN(tweeted_at), MAX(tweeted_at) FROM captured_tweets WHERE account_id = ?1",
            [account_id],
            |row| {
                let min: Option<String> = row.get(0)?;
                let max: Option<String> = row.get(1)?;
                Ok(min.zip(max))
            },
        )?;
        Ok(result)
    }

    pub fn get_daily_tweet_avg(&self, account_id: i64) -> SqliteResult<f64> {
        let conn = self.conn.lock().unwrap();
        let result: f64 = conn
            .query_row(
                "SELECT CAST(COUNT(*) AS REAL) /
                    MAX(1, CAST(julianday('now') - julianday(MIN(tweeted_at)) AS REAL))
                 FROM captured_tweets WHERE account_id = ?1",
                [account_id],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        Ok(result)
    }

    // =====================================================
    // Backup Operations
    // =====================================================

    pub fn export_for_backup(&self) -> SqliteResult<BackupData> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT username, display_name, twitter_user_id, monitor_enabled,
                    custom_keywords, notes
             FROM monitored_accounts ORDER BY created_at ASC",
        )?;
        let accounts: Vec<BackupAccount> = stmt
            .query_map([], |row| {
                Ok(BackupAccount {
                    username: row.get(0)?,
                    display_name: row.get(1)?,
                    twitter_user_id: row.get(2)?,
                    monitor_enabled: row.get(3)?,
                    custom_keywords: row.get(4)?,
                    notes: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut stmt = conn.prepare(
            "SELECT keyword, category, aliases_json FROM tracked_keywords ORDER BY keyword ASC",
        )?;
        let keywords: Vec<BackupKeyword> = stmt
            .query_map([], |row| {
                Ok(BackupKeyword {
                    keyword: row.get(0)?,
                    category: row.get(1)?,
                    aliases_json: row.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(BackupData { accounts, keywords })
    }

    pub fn clear_and_restore(&self, data: &BackupData) -> Result<usize, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sentiment_snapshots", [])
            .map_err(|e| format!("Failed to clear sentiment: {}", e))?;
        conn.execute("DELETE FROM topic_scores", [])
            .map_err(|e| format!("Failed to clear topics: {}", e))?;
        conn.execute("DELETE FROM tweet_topics", [])
            .map_err(|e| format!("Failed to clear tweet_topics: {}", e))?;
        conn.execute("DELETE FROM captured_tweets", [])
            .map_err(|e| format!("Failed to clear tweets: {}", e))?;
        conn.execute("DELETE FROM monitored_accounts", [])
            .map_err(|e| format!("Failed to clear accounts: {}", e))?;
        conn.execute("DELETE FROM tracked_keywords", [])
            .map_err(|e| format!("Failed to clear keywords: {}", e))?;

        let now = chrono::Utc::now().to_rfc3339();
        let mut count = 0;

        for acct in &data.accounts {
            conn.execute(
                "INSERT OR IGNORE INTO monitored_accounts
                    (twitter_user_id, username, display_name, monitor_enabled,
                     custom_keywords, notes, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                rusqlite::params![
                    acct.twitter_user_id,
                    acct.username,
                    acct.display_name,
                    acct.monitor_enabled,
                    acct.custom_keywords,
                    acct.notes,
                    now
                ],
            )
            .map_err(|e| format!("Failed to insert account: {}", e))?;
            count += 1;
        }

        for kw in &data.keywords {
            conn.execute(
                "INSERT OR IGNORE INTO tracked_keywords (keyword, category, aliases_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![kw.keyword, kw.category, kw.aliases_json, now],
            )
            .map_err(|e| format!("Failed to insert keyword: {}", e))?;
            count += 1;
        }

        Ok(count)
    }
}

// =====================================================
// Row Mapping Functions
// =====================================================

fn row_to_account(row: &rusqlite::Row) -> rusqlite::Result<MonitoredAccount> {
    Ok(MonitoredAccount {
        id: row.get(0)?,
        twitter_user_id: row.get(1)?,
        username: row.get(2)?,
        display_name: row.get(3)?,
        monitor_enabled: row.get(4)?,
        custom_keywords: row.get(5)?,
        notes: row.get(6)?,
        last_tweet_id: row.get(7)?,
        last_checked_at: row.get(8)?,
        total_tweets_captured: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn row_to_tweet(row: &rusqlite::Row) -> rusqlite::Result<CapturedTweet> {
    Ok(CapturedTweet {
        id: row.get(0)?,
        account_id: row.get(1)?,
        tweet_id: row.get(2)?,
        text: row.get(3)?,
        tweet_type: row.get(4)?,
        conversation_id: row.get(5)?,
        in_reply_to_user_id: row.get(6)?,
        like_count: row.get(7)?,
        retweet_count: row.get(8)?,
        reply_count: row.get(9)?,
        quote_count: row.get(10)?,
        tweeted_at: row.get(11)?,
        captured_at: row.get(12)?,
        processed: row.get(13)?,
        raw_json: row.get(14)?,
    })
}

fn row_to_topic_score(row: &rusqlite::Row) -> rusqlite::Result<TopicScore> {
    Ok(TopicScore {
        id: row.get(0)?,
        account_id: row.get(1)?,
        topic: row.get(2)?,
        mention_count_7d: row.get(3)?,
        mention_count_30d: row.get(4)?,
        mention_count_total: row.get(5)?,
        trend: row.get(6)?,
        first_seen_at: row.get(7)?,
        last_seen_at: row.get(8)?,
        avg_engagement_score: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn row_to_sentiment(row: &rusqlite::Row) -> rusqlite::Result<SentimentSnapshot> {
    Ok(SentimentSnapshot {
        id: row.get(0)?,
        account_id: row.get(1)?,
        window_start: row.get(2)?,
        window_end: row.get(3)?,
        sentiment_score: row.get(4)?,
        sentiment_label: row.get(5)?,
        tweet_count: row.get(6)?,
        top_topics_json: row.get(7)?,
        signals_json: row.get(8)?,
        ai_summary: row.get(9)?,
        created_at: row.get(10)?,
    })
}
