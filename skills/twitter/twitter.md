---
name: twitter
description: "Post tweets to Twitter/X. Supports replies and quote tweets."
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"𝕏"}}
tags: [twitter, social, messaging, x, social-media]
requires_tools: [twitter_post]
---

# Twitter/X Posting

Post tweets using the `twitter_post` tool. Requires OAuth 1.0a credentials configured in Settings > API Keys.

## Required Credentials

Get these from your [Twitter Developer App](https://developer.twitter.com/en/portal/projects-and-apps) under "Keys and Tokens":

- `TWITTER_CONSUMER_KEY` (API Key)
- `TWITTER_CONSUMER_SECRET` (API Secret)
- `TWITTER_ACCESS_TOKEN`
- `TWITTER_ACCESS_TOKEN_SECRET`

## Post a Tweet

```tool:twitter_post
text: "Hello from starkbot!"
```

## Reply to a Tweet

```tool:twitter_post
text: "Great point!"
reply_to: "1234567890123456789"
```

## Quote Tweet

```tool:twitter_post
text: "Adding my thoughts on this"
quote_tweet_id: "1234567890123456789"
```

## Guidelines

- Keep tweets succinct (under ~500 characters when possible)
- Concise and punchy is best
- Threads: post multiple tweets replying to each other
- The tool returns the tweet URL on success

## Ideas

- Post project updates and releases
- Share insights or hot takes
- Engage with community by replying
- Quote tweet interesting content with commentary
