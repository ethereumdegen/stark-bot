---
name: impulse_map
description: "Manage the impulse map — list nodes, add new ideas, edit or remove existing ones, and connect related concepts."
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🧠"}}
requires_tools: [impulse_map_manage]
tags: [general, impulse_map, ideas, planning, secretary, productivity]
---

# Impulse Map Management

The **impulse map** is a knowledge graph where you organize thoughts, ideas, projects, and goals. It has a root node called the **trunk** that always exists and cannot be deleted.

Use the `impulse_map_manage` tool to manage nodes and connections.

## Quick Actions

### List all nodes and connections
```tool:impulse_map_manage
action: list
```

### View a specific node and its neighbors
```tool:impulse_map_manage
action: get
node_id: <id>
```

### Add a new node connected to the trunk (or another parent)
```tool:impulse_map_manage
action: create
body: "My new idea or topic"
parent_id: 1
```

### Edit a node's content
```tool:impulse_map_manage
action: update
node_id: <id>
body: "Updated content here"
```

### Remove a node
```tool:impulse_map_manage
action: delete
node_id: <id>
```

### Connect two existing nodes
```tool:impulse_map_manage
action: connect
parent_id: <parent_node_id>
child_id: <child_node_id>
```

### Disconnect two nodes
```tool:impulse_map_manage
action: disconnect
parent_id: <parent_node_id>
child_id: <child_node_id>
```

## How the Impulse Map Works

- The **trunk** (root node, usually #1) is the center of the graph and cannot be deleted
- Nodes represent topics, projects, ideas, goals, or any concept worth tracking
- Connections are parent→child relationships forming a directed graph
- Cycles are prevented — you can't create circular connections
- Nodes with more connections are visited more often during heartbeat meandering

## Best Practices

1. **Keep the trunk as a hub** — connect major topic nodes directly to the trunk
2. **Use descriptive bodies** — the heartbeat reads node content during reflection
3. **Branch ideas** — create child nodes to break down large topics
4. **Connect related ideas** — cross-link nodes that relate to each other across branches
5. **Prune regularly** — delete nodes for completed projects or stale ideas
