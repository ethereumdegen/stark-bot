---
name: kv_store
description: "Persistent key/value store for tracking state, counters, flags, and arbitrary data across conversations"
version: 0.3.6
author: starkbot
requires_tools: [local_rpc]
---

# KV Store — Persistent Key/Value Storage

This module stores string key-value pairs that persist across conversations. Use it for counters, flags, notes, config values, or any state the user wants to remember.

**After reading these instructions, call `local_rpc` directly to fulfill the user's request. Do NOT call `use_skill` again.**

Use `module="kv_store"` — the port is resolved automatically.

## Set a value

```
local_rpc(module="kv_store", path="/rpc/kv", method="POST", body={
  "action": "set",
  "key": "TEST",
  "value": "ABCD"
})
```

## Get a value

```
local_rpc(module="kv_store", path="/rpc/kv", method="POST", body={
  "action": "get",
  "key": "TEST"
})
```

## Delete a key

```
local_rpc(module="kv_store", path="/rpc/kv", method="POST", body={
  "action": "delete",
  "key": "TEST"
})
```

## Increment a counter

```
local_rpc(module="kv_store", path="/rpc/kv", method="POST", body={
  "action": "increment",
  "key": "LOGIN_COUNT",
  "amount": 1
})
```
Amount is optional (default 1), can be negative to decrement.

## List keys

```
local_rpc(module="kv_store", path="/rpc/kv", method="POST", body={
  "action": "list"
})
```

With prefix filter:
```
local_rpc(module="kv_store", path="/rpc/kv", method="POST", body={
  "action": "list",
  "prefix": "USER_"
})
```

## Interpreting User Requests

| User says | action | key | value |
|-----------|--------|-----|-------|
| "add/save/store/set X to Y" | set | X | Y |
| "add a record X => Y" | set | X | Y |
| "remember X is Y" | set | X | Y |
| "what is X" / "get X" | get | X | — |
| "delete/remove X" | delete | X | — |
| "increment/count X" | increment | X | — |
| "show all keys" / "list" | list | — | — |

## Notes

- Keys are always auto-uppercased: `test` → `TEST`
- Keys: alphanumeric + underscores only, max 128 chars
- Values are stored as strings
- All responses: `{"success": true, "data": ...}` or `{"success": false, "error": "..."}`
