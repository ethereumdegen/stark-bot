---
name: discord
description: "Control Discord: send messages, react, post stickers/emojis, run polls, manage threads/pins, fetch permissions/member/role/channel info, handle moderation."
version: 2.3.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🎮"}}
tags: [discord, social, messaging, communication, social-media]
requires_tools: [discord, discord_lookup, agent_send, discord_resolve_user]
---

# Discord Actions

## Overview

Use `discord` to manage messages, reactions, threads, polls, and moderation. You can disable groups via `discord.actions.*` (defaults to enabled, except roles/moderation). The tool uses the bot token configured for Clawdbot.

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

### React to a message

```tool:discord
action: react
channelId: "123"
messageId: "456"
emoji: "✅"
```

### List reactions + users

```tool:discord
action: reactions
channelId: "123"
messageId: "456"
limit: 100
```

### Send a sticker

```tool:discord
action: sticker
to: "channel:123"
stickerIds: ["9876543210"]
content: "Nice work!"
```

- Up to 3 sticker IDs per message.
- `to` can be `user:<id>` for DMs.

### Upload a custom emoji

```tool:discord
action: emojiUpload
guildId: "999"
name: party_blob
mediaUrl: "file:///tmp/party.png"
roleIds: ["222"]
```

- Emoji images must be PNG/JPG/GIF and <= 256KB.
- `roleIds` is optional; omit to make the emoji available to everyone.

### Upload a sticker

```tool:discord
action: stickerUpload
guildId: "999"
name: clawdbot_wave
description: "Clawdbot waving hello"
tags: "👋"
mediaUrl: "file:///tmp/wave.png"
```

- Stickers require `name`, `description`, and `tags`.
- Uploads must be PNG/APNG/Lottie JSON and <= 512KB.

### Create a poll

```tool:discord
action: poll
to: "channel:123"
question: "Lunch?"
answers: ["Pizza", "Sushi", "Salad"]
allowMultiselect: false
durationHours: 24
content: "Vote now"
```

- `durationHours` defaults to 24; max 32 days (768 hours).

### Check bot permissions for a channel

```tool:discord
action: permissions
channelId: "123"
```

## Ideas to try

- React with ✅/⚠️ to mark status updates.
- Post a quick poll for release decisions or meeting times.
- Send celebratory stickers after successful deploys.
- Upload new emojis/stickers for release moments.
- Run weekly "priority check" polls in team channels.
- DM stickers as acknowledgements when a user's request is completed.

## Tipping Discord Users

When a user says "tip @someone X TOKEN", follow these steps:

### Context Bank Integration

The **Context Bank** automatically extracts key information from user messages:
- **Addresses** (0x...) → Available for recipient lookups
- **Token symbols** (USDC, STARKBOT, etc.) → Use with `token_lookup`
- **Numbers** (1, 100, 2500000) → The user's intended amount

Check the context bank in your system context for pre-extracted values before asking the user to repeat themselves.

### Required Tool Flow

**ALWAYS follow this sequence for tipping:**

1. `discord_resolve_user` → Get recipient's wallet address
2. `token_lookup` → Get token address and decimals
3. `to_raw_amount` → Convert human amount to raw units
4. `web3_function_call` → Execute the transfer

---

### Step 1: Resolve the Discord mention to a public address

```tool:discord_resolve_user
user_mention: "<@123456789>"
```

This returns the user's registered public address (if they have one). Users register their address with `@starkbot register 0x...`.

**If the user is not registered**, inform them they need to register first.

### Step 2: Look up the token

```tool:token_lookup
symbol: "STARKBOT"
network: base
cache_as: token_address
```

This sets registers:
- `token_address` → contract address
- `token_address_decimals` → decimals (e.g., 18)

### Step 3: Convert amount to raw units

```tool:to_raw_amount
amount: "1"
cache_as: "transfer_amount"
```

This reads `token_address_decimals` automatically and sets:
- `transfer_amount` → "1000000000000000000" (for 18 decimals)

### Step 4: Transfer tokens to the resolved address

```tool:web3_function_call
abi: erc20
contract: "<TOKEN_ADDRESS from step 2>"
function: transfer
params: ["<RESOLVED_ADDRESS from step 1>", "<RAW_AMOUNT from step 3>"]
network: base
```

### Complete Example: "tip @jimmy 1 STARKBOT"

1. **Context bank provides:** number=1, token=STARKBOT

2. Resolve @jimmy:
```tool:discord_resolve_user
user_mention: "<@jimmy's_user_id>"
```
Response: `{"public_address": "0x04abc...", "registered": true}`

3. Look up STARKBOT:
```tool:token_lookup
symbol: "STARKBOT"
network: base
cache_as: token_address
```
Response: `STARKBOT on base, Address: 0x1234..., Decimals: 18`

4. Convert amount:
```tool:to_raw_amount
amount: "1"
cache_as: "transfer_amount"
```
Response: `1 × 10^18 = 1000000000000000000` (cached as `transfer_amount`)

5. Transfer:
```tool:web3_function_call
abi: erc20
contract: "0x1234..."
function: transfer
params: ["0x04abc...", "1000000000000000000"]
network: base
```

6. Confirm: "Sent 1 STARKBOT to @jimmy (0x04abc...)"

### Common Token Addresses (Base)

| Token | Address | Decimals |
|-------|---------|----------|
| USDC | `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913` | 6 |
| WETH | `0x4200000000000000000000000000000000000006` | 18 |
| BNKR | `0x22aF33FE49fD1Fa80c7149773dDe5890D3c76F3b` | 18 |
| STARKBOT | Use `token_lookup` | 18 |

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

For simple messages without the full discord tool:

```tool:agent_send
channel: "123456789012345678"
message: "Hello!"
platform: discord
```

## Action gating

Use `discord.actions.*` to disable action groups:
- `reactions` (react + reactions list + emojiList)
- `stickers`, `polls`, `permissions`, `messages`, `threads`, `pins`, `search`
- `emojiUploads`, `stickerUploads`
- `memberInfo`, `roleInfo`, `channelInfo`, `voiceStatus`, `events`
- `roles` (role add/remove, default `false`)
- `moderation` (timeout/kick/ban, default `false`)

### Read recent messages

```tool:discord
action: readMessages
channelId: "123"
limit: 20
```

### Send/edit/delete a message

**If the user doesn't specify a channel, default to "bot-commands" first, then "general" as fallback.** Look up the channel ID using `discord_lookup` - search for "bot-commands" first, and if not found, search for "general".

```tool:discord
action: sendMessage
to: "channel:123"
content: "Hello from Clawdbot"
```

**With media attachment:**

```tool:discord
action: sendMessage
to: "channel:123"
content: "Check out this audio!"
mediaUrl: "file:///tmp/audio.mp3"
```

- `to` uses format `channel:<id>` or `user:<id>` for DMs (not `channelId`!)
- `mediaUrl` supports local files (`file:///path/to/file`) and remote URLs (`https://...`)
- Optional `replyTo` with a message ID to reply to a specific message

```tool:discord
action: editMessage
channelId: "123"
messageId: "456"
content: "Fixed typo"
```

```tool:discord
action: deleteMessage
channelId: "123"
messageId: "456"
```

### Threads

```tool:discord
action: threadCreate
channelId: "123"
name: "Bug triage"
messageId: "456"
```

```tool:discord
action: threadList
guildId: "999"
```

```tool:discord
action: threadReply
channelId: "777"
content: "Replying in thread"
```

### Pins

```tool:discord
action: pinMessage
channelId: "123"
messageId: "456"
```

```tool:discord
action: listPins
channelId: "123"
```

### Search messages

```tool:discord
action: searchMessages
guildId: "999"
content: "release notes"
channelIds: ["123", "456"]
limit: 10
```

### Member + role info

```tool:discord
action: memberInfo
guildId: "999"
userId: "111"
```

```tool:discord
action: roleInfo
guildId: "999"
```

### List available custom emojis

```tool:discord
action: emojiList
guildId: "999"
```

### Role changes (disabled by default)

```tool:discord
action: roleAdd
guildId: "999"
userId: "111"
roleId: "222"
```

### Channel info

```tool:discord
action: channelInfo
channelId: "123"
```

```tool:discord
action: channelList
guildId: "999"
```

### Voice status

```tool:discord
action: voiceStatus
guildId: "999"
userId: "111"
```

### Scheduled events

```tool:discord
action: eventList
guildId: "999"
```

### Moderation (disabled by default)

```tool:discord
action: timeout
guildId: "999"
userId: "111"
durationMinutes: 10
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
