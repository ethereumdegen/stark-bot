---
name: claude_code
description: "Delegate complex coding tasks to Claude Code running on a remote machine via SSH. Use for multi-file edits, project scaffolding, debugging, and any task that benefits from Claude Code's agentic capabilities."
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🖥️"}}
tags: [development, code, workflow]
requires_tools: [claude_code_remote]
requires_api_keys:
  CLAUDE_CODE_SSH_HOST:
    description: "SSH Host"
    secret: false
  CLAUDE_CODE_SSH_USER:
    description: "SSH User"
    secret: false
  CLAUDE_CODE_SSH_KEY:
    description: "SSH Private Key"
    secret: true
  CLAUDE_CODE_SSH_PORT:
    description: "SSH Port"
    secret: false
arguments:
  prompt:
    description: "The task or prompt to send to Claude Code"
    required: true
  workdir:
    description: "Working directory on the remote machine (e.g. ~/projects/my-app)"
    required: false
  model:
    description: "Model override (e.g. claude-sonnet-4-5-20250929)"
    required: false
---

# Claude Code Remote Skill

Delegate coding tasks to a remote Claude Code instance via SSH. This is ideal for:
- Complex multi-file refactors
- Project scaffolding and setup
- Running commands and iterating on the result
- Any task where Claude Code's agentic loop excels

## Prerequisites

Configure SSH connection in **Settings > API Keys > Claude Code**:
- SSH Host, User, Key Path, and Port

The remote machine must have `claude` CLI installed and configured with an API key.

## Workflow

### Step 1: Determine the Task

Analyze the user's request and decide:
- What prompt to send to Claude Code
- Which working directory to use
- Whether to constrain tools or add system prompt context

### Step 2: Send to Claude Code

**Simple task:**
```tool:claude_code_remote
prompt: <the task description>
workdir: ~/projects/target-repo
```

**With tool constraints (for focused work):**
```tool:claude_code_remote
prompt: <the task description>
workdir: ~/projects/target-repo
allowed_tools: ["Bash", "Read", "Write", "Edit"]
```

**With extra context:**
```tool:claude_code_remote
prompt: <the task description>
workdir: ~/projects/target-repo
append_system_prompt: "This is a Rust project using actix-web. Follow existing code patterns."
```

**With model override:**
```tool:claude_code_remote
prompt: <the task description>
workdir: ~/projects/target-repo
model: claude-sonnet-4-5-20250929
```

### Step 3: Report Results

After Claude Code completes:
1. Summarize what was done
2. Report the cost (`cost_usd` from metadata) if available
3. Report number of turns taken (`num_turns`)
4. If `is_error` is true, analyze and suggest next steps

## Tips

- **Be specific** in prompts — include file paths, function names, and expected behavior
- **Set workdir** to the project root so Claude Code can find all relevant files
- **Use allowed_tools** to limit scope when you want focused edits without broad exploration
- **Set max_turns** for simple tasks to avoid runaway loops (e.g. `max_turns: 5`)
- **Increase timeout** for large tasks (default 300s, max 600s)

## Tools Used

| Tool | Purpose |
|------|---------|
| `claude_code_remote` | SSH into remote machine and run Claude Code CLI |
