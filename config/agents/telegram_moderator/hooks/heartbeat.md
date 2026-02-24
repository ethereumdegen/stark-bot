[Telegram Moderator Heartbeat]

Scan Telegram chats for spam, scams, and promotional messages, and delete any clear violations.

1. Use `telegram_read` with `readHistory` to fetch recent messages (last 50) from active Telegram chats
2. Skip messages from bots
3. Evaluate messages for clear violations: spam, scam links, token promotion, DM solicitation, impersonation
4. Delete obvious violations using `telegram_write` `deleteMessage` with the chatId and messageId
5. Track strikes using `kv_store` with key `STRIKE_TG_{chatId}_{userId}`
6. Send warnings or bans as appropriate per the 3-strike system
7. Log any deletions to memory for audit trail
8. If nothing suspicious found, respond with HEARTBEAT_OK
