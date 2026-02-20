---
name: telegram
description: "Read Telegram chats: get chat info, member info, list admins, member count, and read conversation history."
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"📨"}}
tags: [telegram, social, messaging, communication, social-media]
requires_tools: [telegram_read, agent_send]
---

# Telegram Actions

## Overview

Telegram operations use the **`telegram_read`** tool for all read-only operations. This tool is safe for non-admin/safe mode users.

Available actions:
- **getChatInfo** - Get chat title, type, description, pinned message
- **getChatMember** - Get info about a specific member in a chat
- **getChatAdministrators** - List all admins in a chat
- **getChatMemberCount** - Get total member count
- **readHistory** - Read recent conversation history from local DB

## Reading Conversation History

Read recent messages from the current chat (no chatId needed):

```tool:telegram_read
action: readHistory
limit: 20
```

Read messages from a specific Telegram chat by ID:

```tool:telegram_read
action: readHistory
chatId: "-1001234567890"
limit: 20
```

- `limit`: Number of messages to fetch (default: 20, max: 100)
- If no `chatId` is provided, reads from the current chat session
- Returns messages with role, content, user name, and timestamp

**Use cases:**
- Check what people are saying in a Telegram chat
- Get recent conversation context
- Monitor group activity
- Catch up on missed messages

## Get Chat Info

```tool:telegram_read
action: getChatInfo
chatId: "-1001234567890"
```

Returns: title, type (group/supergroup/private), username, description, pinned message.

## Get Chat Member Info

```tool:telegram_read
action: getChatMember
chatId: "-1001234567890"
userId: "123456789"
```

Returns: name, username, status (creator/administrator/member/restricted/left/kicked), custom title, whether they're a bot.

## List Chat Administrators

```tool:telegram_read
action: getChatAdministrators
chatId: "-1001234567890"
```

Returns: list of all admins with their names, usernames, status, and custom titles.

## Get Member Count

```tool:telegram_read
action: getChatMemberCount
chatId: "-1001234567890"
```

Returns: total number of members in the chat.

## Sending Messages to Telegram

To send a message to a Telegram chat, use `agent_send`:

```tool:agent_send
channel: "-1001234567890"
message: "Hello from Starkbot!"
platform: telegram
```

## Important Notes

- **Chat IDs**: Telegram group chat IDs are typically negative numbers (e.g., `-1001234567890`). Private chat IDs are positive.
- **readHistory** reads from the local database, not the Telegram API. It shows messages the bot has seen/received.
- The other actions (getChatInfo, getChatMember, etc.) call the Telegram Bot API live.
- When on a Telegram channel, you can use `readHistory` without a `chatId` to read the current chat.
