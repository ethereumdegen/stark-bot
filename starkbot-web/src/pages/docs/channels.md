---
name: Channels
---

Channels connect StarkBot to messaging platforms. All messages flow through the same AI pipeline.

## Supported Platforms

| Platform | Library | Features |
|----------|---------|----------|
| **Telegram** | Teloxide | Polling, commands, groups |
| **Slack** | slack-morphism | Socket mode, threads, mentions |
| **Discord** | Serenity | Guilds, channels, DMs |
| **Web** | Built-in | Dashboard chat interface |

---

## Telegram

### Setup

1. Message [@BotFather](https://t.me/BotFather) on Telegram
2. Send `/newbot` and follow prompts
3. Save your bot token

### Add to StarkBot

```json
{
  "channel_type": "telegram",
  "name": "My Telegram Bot",
  "bot_token": "123456789:ABCdef..."
}
```

### Group Messages

By default, bots only see:
- Messages starting with `/`
- Replies to the bot
- @mentions

To see all messages: `/setprivacy` → Disable, or make bot admin.

See [Telegram Integration](/docs/telegram) for detailed setup.

---

## Slack

### Setup

1. Create app at [api.slack.com/apps](https://api.slack.com/apps)
2. Enable **Socket Mode**
3. Add Bot Token Scopes:
   - `chat:write`
   - `channels:history`, `channels:read`
   - `app_mentions:read`
4. Install to workspace
5. Get Bot Token (`xoxb-...`) and App Token (`xapp-...`)

### Add to StarkBot

```json
{
  "channel_type": "slack",
  "name": "Slack Bot",
  "bot_token": "xoxb-...",
  "app_token": "xapp-..."
}
```

### Event Subscriptions

Enable these events:
- `message.channels`
- `message.groups`
- `message.im`
- `app_mention`

---

## Discord

### Setup

1. Create app at [Discord Developer Portal](https://discord.com/developers/applications)
2. Create Bot under application
3. Enable **Message Content Intent**

> **Warning**: Only install Starkbot in your own Discord server. The admin will have full control over the Agentic Loop and Tools.

4. Generate invite URL:
   - Scopes: `bot`, `applications.commands`
   - Permissions: Send Messages, Read Message History, View Channels
5. Invite to your server

### Add to StarkBot

```json
{
  "channel_type": "discord",
  "name": "Discord Bot",
  "bot_token": "MTIz..."
}
```

### Required Intents

- `GUILD_MESSAGES`
- `MESSAGE_CONTENT`
- `DIRECT_MESSAGES`

---

## Web Channel

Always available through the dashboard. No configuration needed.

### Features

- Persistent sessions
- Slash commands (`/help`, `/new`, `/reset`, `/export`)
- Real-time tool progress
- Conversation export

### Commands

| Command | Description |
|---------|-------------|
| `/help` | List commands |
| `/new` | Start new session |
| `/reset` | Clear history |
| `/clear` | Clear display |
| `/skills` | List skills |
| `/tools` | List tools |
| `/model` | Show AI config |
| `/export` | Download JSON |
| `/stop` | Stop execution |

---

## Channel Management

### Adding

1. Go to **Channels**
2. Click **Add Channel**
3. Select platform, enter config
4. Save

### Starting / Stopping

Each channel runs independently:

- **Start** — Begin listening
- **Stop** — Pause (config preserved)

### Status

| Status | Meaning |
|--------|---------|
| Running | Actively listening |
| Stopped | Not listening |
| Error | Connection failed |

---

## Message Flow

All platforms follow the same flow:

```
Platform Message
       ↓
Channel Handler (telegram/slack/discord)
       ↓
NormalizedMessage {
  channel_type: "telegram",
  channel_id: "chat_123",
  user_id: "user_456",
  username: "john",
  content: "Hello!",
  timestamp: "2024-01-15T10:30:00Z"
}
       ↓
Message Dispatcher
       ↓
AI + Tools + Memory
       ↓
Response to Platform
```

---

## Cross-Platform Identity

StarkBot tracks users across platforms. The same person messaging from Telegram and Discord is recognized as one identity.

View identities in the **Identities** page.

This enables:
- Shared memory across platforms
- Consistent personalization
- Unified user tracking
