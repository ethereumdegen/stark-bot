---
name: create-skill
description: "Guide for building custom skills for Starkbot - explains skill format, structure, and best practices."
version: 1.0.0
author: starkbot
homepage: https://github.com/anthropics/starkbot
metadata: {"clawdbot":{"emoji":"🛠️"}}
requires_tools: [write_file, read_file]
tags: [development, skills, tutorial, guide, meta, documentation]
arguments:
  skill_name:
    description: "Name for the new skill (lowercase, underscores allowed)"
    required: false
  skill_purpose:
    description: "What the skill should do"
    required: false
---

# Building Custom Skills for Starkbot

This guide explains how to create custom skills for Starkbot. Skills are markdown files with YAML frontmatter that teach Starkbot how to perform specific tasks.

---

## Skill File Structure

Every skill is a `.md` file with two parts:

```markdown
---
# YAML Frontmatter (metadata)
name: my_skill
description: "What this skill does"
...
---

# Markdown Content (instructions for the AI)
Detailed instructions on how to perform the skill...
```

---

## Complete Frontmatter Reference

```yaml
---
# REQUIRED FIELDS
name: skill_name                    # Unique identifier (lowercase, underscores OK)
description: "Brief description"    # What the skill does (shown in skill list)

# RECOMMENDED FIELDS
version: 1.0.0                      # Semantic version (major.minor.patch)
author: your_name                   # Creator name or handle
tags: [tag1, tag2, tag3]           # Categories for search/filtering

# OPTIONAL FIELDS
homepage: https://example.com       # Documentation or reference URL
metadata: {"key": "value"}          # Custom metadata (JSON format)
requires_tools: [tool1, tool2]      # Tools the skill needs to function
requires_binaries: [git, node]      # System binaries needed (checked at runtime)

# ARGUMENTS (user-provided parameters)
arguments:
  arg_name:
    description: "What this argument is for"
    required: true                  # true = must be provided, false = optional
    default: "default_value"        # Optional default if not provided
---
```

---

## Skill Storage Locations

Skills are loaded from three locations with priority:

| Location | Priority | Purpose |
|----------|----------|---------|
| `workspace/.skills/` | Highest (3) | Project-specific skills |
| `skills/managed/` | Medium (2) | Installed from registry |
| `skills/` | Lowest (1) | Bundled with Starkbot |

**Note:** If the same skill exists in multiple locations, the higher priority version is used.

---

## Step-by-Step: Creating a New Skill

### Step 1: Plan Your Skill

Define:
1. **Purpose**: What task does this skill accomplish?
2. **Tools needed**: Which Starkbot tools will it use?
3. **Arguments**: What inputs does the user need to provide?
4. **Workflow**: What are the steps to complete the task?

### Step 2: Create the Skill File

Create a new file: `skills/{{skill_name}}.md`

```json
{
  "tool": "write_file",
  "path": "skills/my_new_skill.md",
  "content": "---\nname: my_new_skill\n..."
}
```

### Step 3: Write the Frontmatter

Start with required fields, then add optional ones:

```yaml
---
name: my_new_skill
description: "Does something useful"
version: 1.0.0
author: your_name
requires_tools: [web_fetch, write_file]
tags: [utility, automation]
arguments:
  target:
    description: "The target to process"
    required: true
---
```

### Step 4: Write the Instructions

The markdown body teaches the AI how to perform the skill:

```markdown
# My New Skill

## Overview
Brief explanation of what this skill accomplishes.

## Prerequisites
- List any setup requirements
- API keys, configurations, etc.

## Workflow

### Step 1: First Action
Explanation of what to do first.

\`\`\`json
{
  "tool": "tool_name",
  "param": "value"
}
\`\`\`

### Step 2: Second Action
Continue with next steps...

## Error Handling
How to handle common errors.

## Examples
Show example usage and expected outputs.
```

---

## Available Tools Reference

Common tools you can use in skills:

### File Operations
| Tool | Purpose |
|------|---------|
| `read_file` | Read file contents |
| `write_file` | Create/overwrite files |
| `edit_file` | Modify existing files |
| `list_files` | List directory contents |
| `glob` | Find files by pattern |
| `grep` | Search file contents |

### Web & API
| Tool | Purpose |
|------|---------|
| `web_fetch` | HTTP requests (GET, POST, etc.) |
| `x402_preset_fetch` | Paid API requests via x402 |

### Development
| Tool | Purpose |
|------|---------|
| `git` | Git operations |
| `exec` | Run shell commands |
| `committer` | Safe git commits |

### Blockchain/Web3
| Tool | Purpose |
|------|---------|
| `web3_preset_function_call` | Preset smart contract calls |
| `web3_tx` | Sign/send transactions |
| `token_lookup` | Resolve token addresses |

### Communication
| Tool | Purpose |
|------|---------|
| `twitter` | Twitter/X operations |
| `agent_send` | Send messages to other agents |

### Memory & State
| Tool | Purpose |
|------|---------|
| `memory_store` | Save to long-term memory |
| `memory_get` | Retrieve from memory |
| `set_address` | Set validated address register |
| `to_raw_amount` | Convert human amounts to raw units |

---

## Best Practices

### 1. Clear Tool Examples
Always show tool calls with proper JSON format:

```json
{
  "tool": "web_fetch",
  "url": "https://api.example.com/data",
  "method": "GET",
  "extract_mode": "raw"
}
```

### 2. Use Argument Placeholders
Reference arguments with `{{arg_name}}` syntax:

```markdown
Fetch data for {{target}}:
\`\`\`json
{
  "tool": "web_fetch",
  "url": "https://api.example.com/{{target}}"
}
\`\`\`
```

### 3. Provide Error Handling
Document how to handle failures:

```markdown
## Error Handling

If the API returns 404:
1. Check if the resource exists
2. Verify the ID format
3. Try with a different endpoint
```

### 4. Include Examples
Show real-world usage:

```markdown
## Examples

### Example 1: Basic Usage
User: "Process the report"
Action: [describe what happens]

### Example 2: With Options
User: "Process the report with format=json"
Action: [describe what happens]
```

### 5. Organize with Sections
Use clear headings:
- Overview
- Prerequisites
- Workflow (numbered steps)
- Quick Reference
- Error Handling
- Examples

### 6. Tag Appropriately
Use relevant tags for discoverability:
- Category: `development`, `crypto`, `social`, `utility`
- Platform: `twitter`, `github`, `polymarket`
- Type: `automation`, `analysis`, `trading`

---

## Skill Template

Copy this template to create a new skill:

```markdown
---
name: skill_name
description: "Brief description of what this skill does"
version: 1.0.0
author: your_name
homepage: https://docs.example.com
metadata: {"clawdbot":{"emoji":"🔧"}}
requires_tools: [tool1, tool2]
tags: [category1, category2]
arguments:
  main_arg:
    description: "Primary argument description"
    required: true
  optional_arg:
    description: "Optional argument with default"
    required: false
    default: "default_value"
---

# Skill Title

Brief overview of the skill's purpose.

## Prerequisites

- Requirement 1
- Requirement 2

## Workflow

### Step 1: Description

Explanation of the first step.

\`\`\`json
{
  "tool": "tool_name",
  "param": "{{main_arg}}"
}
\`\`\`

### Step 2: Description

Continue with additional steps...

## Quick Reference

| Action | Tool Call |
|--------|-----------|
| Action 1 | `tool_name` with params |
| Action 2 | `other_tool` with params |

## Error Handling

Common issues and solutions.

## Examples

### Basic Example
Description and expected outcome.
```

---

## Testing Your Skill

### 1. Validate Frontmatter
Ensure YAML is valid:
- Proper indentation (2 spaces)
- Quoted strings with special characters
- Valid JSON in metadata field

### 2. Check Tool Availability
Verify required tools exist:
```json
{
  "tool": "manage_skills",
  "action": "get",
  "name": "your_skill_name"
}
```

### 3. Test with Starkbot
Restart Starkbot to load the new skill, then invoke it:
- "Use the [skill_name] skill to..."
- "Help me with [skill purpose]"

### 4. Iterate
Refine based on:
- Missing instructions
- Unclear steps
- Error cases not covered

---

## Managing Skills

### List All Skills
```json
{
  "tool": "manage_skills",
  "action": "list"
}
```

### Get Skill Details
```json
{
  "tool": "manage_skills",
  "action": "get",
  "name": "skill_name"
}
```

### Enable/Disable
```json
{
  "tool": "manage_skills",
  "action": "enable",
  "name": "skill_name"
}
```

### Delete a Skill
```json
{
  "tool": "manage_skills",
  "action": "delete",
  "name": "skill_name"
}
```

---

## Advanced: ZIP Package Format

For skills with additional scripts:

```
my-skill.zip/
├── SKILL.md          # Required: skill definition
└── scripts/          # Optional: helper scripts
    ├── helper.py
    ├── process.sh
    └── utils.js
```

Supported script languages:
- `.py` → Python
- `.sh`, `.bash` → Bash
- `.js` → JavaScript
- `.ts` → TypeScript
- `.rb` → Ruby

---

## Common Patterns

### Register Pattern (Prevent Hallucination)
For critical values, use typed tools to set registers instead of inline values:

```markdown
1. Store address in register:
\`\`\`json
{"tool": "set_address", "register": "send_to", "address": "0x1234..."}
\`\`\`

2. Convert amount safely:
\`\`\`json
{"tool": "to_raw_amount", "amount": "0.01", "decimals": 18, "cache_as": "amount_raw"}
\`\`\`
```

### Validation Pattern
Always validate before acting:

```markdown
### Pre-flight Checks
1. Verify the target exists
2. Check permissions
3. Validate input format

Only proceed if all checks pass.
```

### Confirmation Pattern
For destructive operations:

```markdown
**IMPORTANT:** Before executing:
1. Show the user what will happen
2. Ask for confirmation
3. Proceed only with explicit approval
```
