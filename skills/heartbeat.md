---
name: heartbeat
description: "View and control the heartbeat system — check status, adjust interval and schedule, enable or disable automated reflection cycles."
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"💓"}}
requires_tools: [heartbeat_config]
tags: [general, heartbeat, automation, scheduling, secretary]
---

# Heartbeat Management

The **heartbeat** is an automation system that periodically wakes the agent to reflect on the impulse map. Use the `heartbeat_config` tool to manage it.

## Quick Actions

### Check current status
```tool:heartbeat_config
action: list
```

### Enable heartbeat
```tool:heartbeat_config
action: enable
```

### Disable heartbeat
```tool:heartbeat_config
action: disable
```

### Change interval (e.g. every 60 minutes)
First list configs to get the config_id, then:
```tool:heartbeat_config
action: update
config_id: <id>
interval_minutes: 60
```

### Set active hours (e.g. 9am-5pm weekdays)
```tool:heartbeat_config
action: update
config_id: <id>
active_hours_start: "09:00"
active_hours_end: "17:00"
active_days: "mon,tue,wed,thu,fri"
```

## Settings Reference

| Setting | Description |
|---------|-------------|
| **interval_minutes** | How often heartbeat fires (default: 30) |
| **active_hours_start/end** | Time window in HH:MM format |
| **active_days** | Comma-separated days: mon,tue,wed,thu,fri,sat,sun |
| **target** | 'last' to continue last session, or a specific session key |
| **enabled** | Whether heartbeat is active |

## How Heartbeat Works

1. A scheduler checks every 60 seconds for due heartbeat configs
2. When a heartbeat fires, the agent meanders through the impulse map — randomly hopping between connected nodes
3. At each node, the agent reflects on the content and takes action if needed
4. If nothing needs attention, the agent responds with HEARTBEAT_OK (suppressed output)
5. The heartbeat tracks which impulse node it's on and maintains session continuity across beats
