# Discord Bot Integration

This guide explains how to set up and use Discord with StarkBot.

## Quick Start

1. **Create a Discord bot** in the [Discord Developer Portal](https://discord.com/developers/applications)
2. **Configure the bot token** in StarkBot via the API or frontend
3. **Invite the bot** to your server with required permissions
4. **Start the channel** and begin chatting

## Step-by-Step Setup

### 1. Create a Discord Application & Bot

1. Go to the [Discord Developer Portal](https://discord.com/developers/applications)
2. Click "New Application" and give it a name
3. Go to the **Bot** section in the left sidebar
4. Click "Add Bot" (or "Reset Token" if already exists)
5. Copy the **Bot Token** - you'll need this for configuration

### 2. Enable Required Intents

In the Bot section of your application, enable these **Privileged Gateway Intents**:

- **Message Content Intent** (required to read message text)
- **Server Members Intent** (recommended for member lookups)

Without Message Content Intent, the bot cannot read what users type.

> **Warning**: Only install Starkbot in your own Discord server. The admin will have full control over the Agentic Loop and Tools.

### 3. Set Bot Permissions

When generating the invite URL, select these permissions:

**Minimal Required:**
- View Channels
- Send Messages
- Read Message History

**Recommended:**
- Embed Links
- Attach Files
- Add Reactions

### 4. Generate Invite URL & Add to Server

1. Go to **OAuth2 → URL Generator**
2. Select scopes: `bot`
3. Select the permissions listed above
4. Copy the generated URL
5. Open the URL in a browser and select your server

### 5. Configure in StarkBot

#### Via API:

```bash
# First, login to get an auth token
curl -X POST http://localhost:8080/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"secret_key":"YOUR_SECRET_KEY"}'

# Create Discord channel configuration
curl -X POST http://localhost:8080/api/channels \
  -H "Authorization: Bearer YOUR_AUTH_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "channel_type": "discord",
    "name": "My Discord Bot",
    "bot_token": "YOUR_DISCORD_BOT_TOKEN"
  }'

# Start the channel
curl -X POST http://localhost:8080/api/channels/{channel_id}/start \
  -H "Authorization: Bearer YOUR_AUTH_TOKEN"
```

#### Via Frontend:

1. Navigate to the Channels page
2. Click "Add Channel"
3. Select "Discord" as channel type
4. Enter a name and paste your bot token
5. Save and click "Start"

## Features

### Message Handling

- **Character Limit**: Messages are automatically chunked to Discord's 2000 character limit
- **Smart Splitting**: Long responses split on line boundaries to preserve formatting
- **Conversation History**: Each user/channel gets persistent conversation context
- **Session Commands**: Users can type `/new` or `/reset` to clear conversation history

### Supported Channels

- **Server Text Channels**: Bot responds when messaged in channels where it has access
- **Direct Messages**: Users can DM the bot directly
- **Threads**: Treated as separate conversation contexts

### Memory System

The bot can store memories from conversations:

- `[REMEMBER: text]` - Store long-term memories about users
- `[DAILY_LOG: text]` - Store daily notes
- `[REMEMBER_IMPORTANT: text]` - Store critical information with higher priority

### Tool Support

When tools are enabled, the bot can:
- Execute commands
- Search the web
- Read/write files
- Use custom skills

## Proactive Messaging (agent_send)

The bot can send messages to Discord channels proactively, not just in response to users.

### Setup for Proactive Messaging

Add your Discord bot token as an API key:

```bash
curl -X POST http://localhost:8080/api/api_keys \
  -H "Authorization: Bearer YOUR_AUTH_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "service_name": "discord_bot",
    "api_key": "YOUR_DISCORD_BOT_TOKEN"
  }'
```

### How It Works

The AI can use the `agent_send` tool to send messages:

```json
{
  "tool_name": "agent_send",
  "tool_params": {
    "channel": "1234567890123456789",
    "message": "Hello from StarkBot!",
    "platform": "discord",
    "reply_to": "9876543210987654321"
  }
}
```

**Parameters:**
- `channel` (required): Discord channel ID (18-digit snowflake)
- `message` (required): Message content to send
- `platform`: Set to "discord" (auto-detected from channel ID format)
- `reply_to`: Optional message ID to reply to

### Getting a Discord Channel ID

1. Open Discord Settings → App Settings → Advanced
2. Enable **Developer Mode**
3. Right-click any channel → "Copy Channel ID"

### Use Cases

- **Scheduled Messages**: Use cron jobs to send daily updates
- **Alerts & Notifications**: Bot notifies channels when events occur
- **Cross-Platform**: Receive on Telegram, respond on Discord
- **Heartbeat**: Periodic check-in messages to channels

## API Endpoints

### Channel Management

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/channels` | List all channels |
| POST | `/api/channels` | Create new channel |
| GET | `/api/channels/{id}` | Get channel details |
| PUT | `/api/channels/{id}` | Update channel |
| DELETE | `/api/channels/{id}` | Delete channel |
| POST | `/api/channels/{id}/start` | Start/connect channel |
| POST | `/api/channels/{id}/stop` | Stop/disconnect channel |

### Create Channel Request

```json
{
  "channel_type": "discord",
  "name": "My Bot",
  "bot_token": "YOUR_BOT_TOKEN"
}
```

### Channel Response

```json
{
  "id": 1,
  "channel_type": "discord",
  "name": "My Bot",
  "enabled": true,
  "running": true,
  "created_at": "2026-01-25T12:00:00Z",
  "updated_at": "2026-01-25T12:00:00Z"
}
```

Note: Bot tokens are never returned in API responses for security.

## Auto-Start on Boot

Channels with `enabled: true` automatically connect when StarkBot starts. To have your Discord bot auto-connect:

1. Create and configure the channel
2. Start it once (this sets `enabled: true`)
3. On next StarkBot restart, it will auto-connect

## Troubleshooting

### Bot doesn't respond to messages

1. **Check Message Content Intent**: Must be enabled in Developer Portal
2. **Check permissions**: Bot needs "View Channels" and "Read Message History"
3. **Check channel is started**: Verify via API or frontend that the channel is running
4. **Check logs**: Look for connection errors in StarkBot logs

### Bot connects but can't send messages

1. **Check Send Messages permission**: Bot needs this in the channel
2. **Check channel permissions**: Some channels may have bot restrictions
3. **Check message length**: Very long messages may fail

### "Invalid Token" error

1. **Regenerate token**: Go to Developer Portal → Bot → Reset Token
2. **Update configuration**: Use the new token in StarkBot
3. **No spaces**: Ensure no extra whitespace in the token

### Rate limiting

Discord has rate limits. If sending many messages:
- The bot handles chunking automatically
- Very rapid messages may be delayed
- Check Discord API rate limit headers in logs

## Admin Permissions

StarkBot distinguishes between admin and non-admin users:

| User Type | Capabilities |
|-----------|-------------|
| Admin | Full agent queries, no safe mode, no rate limit |
| Non-admin | Safe mode only, rate limited, basic commands |

### How Admins are Determined

By default, StarkBot uses **Discord's built-in Administrator permission** to determine who is an admin. Users who are:
- The server owner, OR
- Have a role with the Administrator permission

...are automatically treated as StarkBot admins.

### Optional: Explicit Admin User IDs

You can optionally configure specific Discord user IDs to be admins via channel settings:

```bash
curl -X PUT http://localhost:8080/api/channels/{id}/settings \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "settings": [
      {
        "key": "discord_admin_user_ids",
        "value": "123456789012345678, 987654321098765432"
      }
    ]
  }'
```

When explicit admin IDs are configured, **only those users** are treated as admins (Discord Administrator permission is ignored).

## Security Notes

- **Bot tokens are secrets**: Never commit them to version control
- **Tokens not exposed**: API responses never include bot tokens
- **Per-channel isolation**: Each Discord server gets separate conversation contexts
- **User identification**: Users are tracked by Discord user ID

## Technical Details

- **Library**: Uses [Serenity](https://github.com/serenity-rs/serenity) v0.12 for Discord connectivity
- **Connection**: WebSocket connection to Discord Gateway
- **Message limit**: 2000 characters per message (Discord's limit)
- **Graceful shutdown**: Channels can be stopped without losing state
