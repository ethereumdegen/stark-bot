[Discord Moderator Heartbeat]

Scan all Discord servers for spam, scams, and promotional messages, and delete any clear violations.

1. Use `discord_lookup` to list servers and get channel IDs for each server
2. For each server, read recent messages (last 50) from active text channels using `discord_read`
3. Skip messages from bots (author has `bot: true`)
4. Evaluate messages for clear violations: spam, scam links, token promotion, DM solicitation, impersonation
5. Delete obvious violations using `discord_write` `deleteMessage` with the channelId and messageId
6. Log any deletions to memory for audit trail
7. If nothing suspicious found, respond with HEARTBEAT_OK
