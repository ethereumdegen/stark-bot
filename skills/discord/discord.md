---
name: discord
description: "Control Discord: send messages, react, post stickers/emojis, run polls, manage threads/pins, fetch permissions/member/role/channel info, handle moderation."
version: 2.6.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🎮"}}
tags: [discord, social, messaging, communication, social-media]
requires_tools: [discord_read, discord_write, discord_lookup, agent_send, discord_resolve_user]
---

# Discord Actions

## Overview

Discord operations are split into **read** and **write** tools for security:

- **`discord_read`** - Read-only operations (safe for non-admin/safe mode): readMessages, searchMessages, permissions, memberInfo, roleInfo, channelInfo, channelList
- **`discord_write`** - Write operations (admin only): sendMessage, react, editMessage, deleteMessage
- **`discord_lookup`** - Server/channel discovery (safe for non-admin/safe mode): list_servers, search_servers, list_channels, search_channels

You can disable groups via `discord.actions.*` (defaults to enabled, except roles/moderation). The tools use the bot token configured for Clawdbot.

## Default Channel

**If no channel is specified, default to the "bot-commands" channel first, then fall back to "general" if it doesn't exist.** Use `discord_lookup` with `action: search_channels` and `query: "bot-commands"` to find it. If no results, search for `query: "general"` instead.

## Inputs to collect

- For reactions: `channelId`, `messageId`, and an `emoji`.
- For stickers/polls/sendMessage: a `to` target (`channel:<id>` or `user:<id>`). Optional `content` text. **If no channel specified, use "bot-commands" first, then "general" as fallback.**
- Polls also need a `question` plus 2–10 `answers`.
- For media: `mediaUrl` with `file:///path` for local files or `https://...` for remote.
- For emoji uploads: `guildId`, `name`, `mediaUrl`, optional `roleIds` (limit 256KB, PNG/JPG/GIF).
- For sticker uploads: `guildId`, `name`, `description`, `tags`, `mediaUrl` (limit 512KB, PNG/APNG/Lottie JSON).

Message context lines include `discord message id` and `channel` fields you can reuse directly.

**Note:** `sendMessage` uses `to: "channel:<id>"` format, not `channelId`. Other actions like `react`, `readMessages`, `editMessage` use `channelId` directly.

## Actions

### Read recent messages from a channel (discord_read)

Read the last N messages from any channel:

```tool:discord_read
action: readMessages
channelId: "123456789"
limit: 10
```

- `limit`: Number of messages to fetch (default: 50, max: 100)
- Returns messages in reverse chronological order (newest first)

**Response includes for each message:**
- `id` - Message ID (use for replies, reactions, etc.)
- `content` - Message text
- `author` - Username and user ID
- `timestamp` - When sent
- `attachments` - Any files/images
- `embeds` - Rich embeds
- `reactions` - Existing reactions

**Use cases:**
- Check recent conversation context before responding
- Find a message ID to reply to or react to
- Monitor channel activity
- Search for specific content in recent messages

**With before/after cursor (pagination):**

```tool:discord_read
action: readMessages
channelId: "123456789"
limit: 10
before: "MESSAGE_ID"
```

## Ideas to try

- React with ✅/⚠️ to mark status updates.
- Post a quick poll for release decisions or meeting times.
- Send celebratory stickers after successful deploys.
- Upload new emojis/stickers for release moments.
- Run weekly "priority check" polls in team channels.
- DM stickers as acknowledgements when a user's request is completed.

## Tipping Discord Users

For tipping users with tokens, use the **discord_tipping** skill:

```tool:use_skill
skill_name: "discord_tipping"
input: "tip @user amount TOKEN"
```

This handles resolving Discord mentions to wallet addresses and executing ERC20 transfers.

## Finding Servers and Channels by Name

Use `discord_lookup` to find server/channel IDs when you only know the name:

### List all servers the bot is in

```tool:discord_lookup
action: list_servers
```

### Search for a server by name

```tool:discord_lookup
action: search_servers
query: "starkbot"
```

### List channels in a server

```tool:discord_lookup
action: list_channels
server_id: "123456789"
```

### Search for a channel by name

```tool:discord_lookup
action: search_channels
server_id: "123456789"
query: "bot-commands"
```

If "bot-commands" doesn't exist, fall back to "general":

```tool:discord_lookup
action: search_channels
server_id: "123456789"
query: "general"
```

### Quick send with agent_send

For simple messages without the full discord_write tool:

```tool:agent_send
channel: "123456789012345678"
message: "Hello!"
platform: discord
```



### React to a message (discord_write)

```tool:discord_write
action: react
channelId: "123"
messageId: "456"
emoji: "✅"
```


### Check bot permissions for a channel (discord_read)

```tool:discord_read
action: permissions
channelId: "123"
```




### Send/edit/delete a message (discord_write)

**If the user doesn't specify a channel, default to "bot-commands" first, then "general" as fallback.** Look up the channel ID using `discord_lookup` - search for "bot-commands" first, and if not found, search for "general".

```tool:discord_write
action: sendMessage
to: "channel:123"
content: "Hello from Clawdbot"
```

**With media attachment:**

```tool:discord_write
action: sendMessage
to: "channel:123"
content: "Check out this audio!"
mediaUrl: "file:///tmp/audio.mp3"
```

- `to` uses format `channel:<id>` or `user:<id>` for DMs (not `channelId`!)
- `mediaUrl` supports local files (`file:///path/to/file`) and remote URLs (`https://...`)
- Optional `replyTo` with a message ID to reply to a specific message

```tool:discord_write
action: editMessage
channelId: "123"
messageId: "456"
content: "Fixed typo"
```

```tool:discord_write
action: deleteMessage
channelId: "123"
messageId: "456"
```


### Search messages (discord_read)

```tool:discord_read
action: searchMessages
guildId: "999"
content: "release notes"
limit: 10
```

### Member + role info (discord_read)

```tool:discord_read
action: memberInfo
guildId: "999"
userId: "111"
```

```tool:discord_read
action: roleInfo
guildId: "999"
```

### Channel info (discord_read)

```tool:discord_read
action: channelInfo
channelId: "123"
```

```tool:discord_read
action: channelList
guildId: "999"
```


## Discord Writing Style Guide

**Keep it conversational!** Discord is a chat platform, not documentation.

### Do
- Short, punchy messages (1-3 sentences ideal)
- Multiple quick replies > one wall of text
- Use emoji for tone/emphasis 🦞
- Lowercase casual style is fine
- Break up info into digestible chunks
- Match the energy of the conversation

### Don't
- No markdown tables (Discord renders them as ugly raw `| text |`)
- No `## Headers` for casual chat (use **bold** or CAPS for emphasis)
- Avoid multi-paragraph essays
- Don't over-explain simple things
- Skip the "I'd be happy to help!" fluff

### Formatting that works
- **bold** for emphasis
- `code` for technical terms
- Lists for multiple items
- > quotes for referencing
- Wrap multiple links in `<>` to suppress embeds

### Example transformations

❌ Bad:
```
I'd be happy to help with that! Here's a comprehensive overview of the versioning strategies available:

## Semantic Versioning
Semver uses MAJOR.MINOR.PATCH format where...

## Calendar Versioning
CalVer uses date-based versions like...
```

✅ Good:
```
versioning options: semver (1.2.3), calver (2026.01.04), or yolo (`latest` forever). what fits your release cadence?
```
