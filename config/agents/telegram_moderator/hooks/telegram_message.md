[TELEGRAM HOOK — New message detected]
Chat: {chatName} ({chatId}) | Type: {chatType}
Author: {userName} (ID: {userId}) | Bot: {userBot}
Message ID: {messageId}
Content:
{content}

Evaluate this message for violations per your rules. Then:
- If CLEAN: call task_fully_completed with summary "HEARTBEAT_OK". Do nothing else.
- If VIOLATION: immediately take action using your tools (telegram_write deleteMessage, kv_store for strikes, telegram_write sendMessage for warnings/bans). After completing all actions, call task_fully_completed with a brief summary of what you did.
