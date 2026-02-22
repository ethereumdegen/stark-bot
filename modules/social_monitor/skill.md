---
name: social_monitor
description: "Monitor Twitter/X accounts, analyze tweet topics, track sentiment trends, and detect social signals for intelligence gathering"
version: 1.0.0
author: starkbot
tags: [social, twitter, monitoring, forensics, sentiment, intelligence]
requires_tools: [social_monitor_watchlist, social_monitor_tweets, social_monitor_forensics, social_monitor_control]
---

# Social Monitor Skill

You are helping the user manage their social media monitoring setup. This skill tracks Twitter/X accounts over time, captures their tweets, extracts topics, scores sentiment, and detects signals like volume spikes or new interests.

The social monitor runs as a separate microservice. All tool calls communicate with it via RPC.

## Available Tools

1. **social_monitor_watchlist** — Manage monitored accounts and tracked keywords
   - `add_account`: Add a Twitter/X account to monitor (requires username)
   - `remove_account`: Remove an account by ID
   - `list_accounts`: Show all monitored accounts
   - `update_account`: Modify settings (enable/disable, keywords, notes)
   - `add_keyword`: Add a keyword to the global tracking list
   - `remove_keyword`: Remove a tracked keyword
   - `list_keywords`: Show all tracked keywords

2. **social_monitor_tweets** — Query captured tweets (read-only)
   - `recent`: Show recent tweets across all monitored accounts
   - `search`: Search tweets by text content
   - `by_account`: Show tweets for a specific account
   - `stats`: Overview statistics (total tweets, accounts, topics)

3. **social_monitor_forensics** — Analysis and intelligence (read-only)
   - `topics`: View topic scores with trend data (rising/falling/stable/new/dormant)
   - `sentiment`: View sentiment history over time
   - `report`: Full forensics report for an account (topics, sentiment, signals)
   - `signals`: View detected signals for an account

4. **social_monitor_control** — Service health
   - `status`: Check if the service is running, stats, last poll time

## Workflow

1. First check status: `social_monitor_control(action="status")`
2. Add accounts to monitor: `social_monitor_watchlist(action="add_account", username="punk6529")`
3. Optionally add tracked keywords: `social_monitor_watchlist(action="add_keyword", keyword="artblocks", category="nft_collection", aliases="art blocks")`
4. The background worker automatically polls every 5 minutes
5. Check captured tweets: `social_monitor_tweets(action="stats")`
6. Analyze topics: `social_monitor_forensics(action="topics")`
7. Get full report: `social_monitor_forensics(action="report", username="punk6529")`

## Important Notes

- The social monitor runs as a standalone service (social-monitor-service)
- Dashboard available at http://127.0.0.1:9102/
- Twitter API credentials must be configured (TWITTER_CONSUMER_KEY, etc.)
- Topics are extracted from hashtags, cashtags, mentions, and tracked keywords
- Sentiment is scored using rule-based analysis with crypto/social lexicon
- Signals include: volume_spike, sentiment_swing, new_interest, gone_quiet
- Topic trends: rising (>1.5x last week), falling (<0.5x), new, dormant, stable
- Poll interval configurable via SOCIAL_MONITOR_POLL_INTERVAL (default 300s)
