---
name: twitter
description: "Post tweets and interact with Twitter/X using Bearer token and curl."
version: 1.0.0
author: starkbot
homepage: https://developer.x.com/en/docs
metadata: {"requires_auth": true, "clawdbot":{"emoji":"bird","requires":{"bins":["curl","jq"]}}}
requires_binaries: [curl, jq]
tags: [twitter, x, social-media, posting, tweets]
---

# Twitter/X Integration

Post tweets and interact with Twitter using Bearer token authentication.

## Setup

**First, check if TWITTER_TOKEN is configured:**
```tool:api_keys_check
key_name: TWITTER_TOKEN
```

If not configured:
1. Ask the user to get credentials from: https://developer.x.com/en/portal/dashboard
2. They need an **OAuth 2.0 User Access Token** (for posting) or **Bearer Token** (read-only)
3. Add it in Settings > API Keys > Twitter

## Post a Tweet

```bash
curl -X POST "https://api.twitter.com/2/tweets" \
  -H "Authorization: Bearer $TWITTER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"text": "Hello from StarkBot!"}'
```

## Reply to a Tweet

```bash
curl -X POST "https://api.twitter.com/2/tweets" \
  -H "Authorization: Bearer $TWITTER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"text": "My reply!", "reply": {"in_reply_to_tweet_id": "TWEET_ID"}}'
```

## Search Tweets

```bash
curl -s "https://api.twitter.com/2/tweets/search/recent?query=SEARCH_TERM&max_results=10" \
  -H "Authorization: Bearer $TWITTER_TOKEN" | jq
```

## Get User Info

```bash
curl -s "https://api.twitter.com/2/users/by/username/USERNAME?user.fields=description,public_metrics" \
  -H "Authorization: Bearer $TWITTER_TOKEN" | jq
```

## Get User Timeline

```bash
# First get user ID
USER_ID=$(curl -s "https://api.twitter.com/2/users/by/username/USERNAME" \
  -H "Authorization: Bearer $TWITTER_TOKEN" | jq -r '.data.id')

# Then get tweets
curl -s "https://api.twitter.com/2/users/$USER_ID/tweets?max_results=10" \
  -H "Authorization: Bearer $TWITTER_TOKEN" | jq
```

## Get Tweet by ID

```bash
curl -s "https://api.twitter.com/2/tweets/TWEET_ID?tweet.fields=author_id,created_at,public_metrics" \
  -H "Authorization: Bearer $TWITTER_TOKEN" | jq
```

## Create a Thread

```bash
# Post first tweet
FIRST_ID=$(curl -s -X POST "https://api.twitter.com/2/tweets" \
  -H "Authorization: Bearer $TWITTER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"text": "Thread 1/3: First tweet"}' | jq -r '.data.id')

# Reply to create thread
SECOND_ID=$(curl -s -X POST "https://api.twitter.com/2/tweets" \
  -H "Authorization: Bearer $TWITTER_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"text\": \"Thread 2/3: Second\", \"reply\": {\"in_reply_to_tweet_id\": \"$FIRST_ID\"}}" | jq -r '.data.id')

# Continue...
curl -s -X POST "https://api.twitter.com/2/tweets" \
  -H "Authorization: Bearer $TWITTER_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"text\": \"Thread 3/3: Final\", \"reply\": {\"in_reply_to_tweet_id\": \"$SECOND_ID\"}}"
```

## Search Query Operators

- `from:username` - Tweets from user
- `to:username` - Replies to user
- `@username` - Mentions
- `#hashtag` - Hashtag
- `"exact phrase"` - Exact match
- `-filter:retweets` - No retweets
- `lang:en` - Language filter

Example: `from:elonmusk -is:retweet lang:en`

## Response Format

Success:
```json
{"data": {"id": "1234567890", "text": "Hello!"}}
```

Error:
```json
{"errors": [{"message": "Error message", "code": 403}]}
```

## Troubleshooting

- **401**: Invalid token - check api_keys
- **403**: No write permission - need OAuth 2.0 User Access Token
- **429**: Rate limited - wait 15 minutes
