---
name: install_skill
description: "Install, manage, and configure skills. Use manage_skills tool to list, install, enable/disable, or delete skills."
version: 1.1.0
author: starkbot
metadata: {"clawdbot":{"emoji":"📦"}}
requires_tools: [manage_skills]
tags: [general, all, admin, management, meta]
arguments:
  action:
    description: "Action to perform: list, install, enable, disable, delete, search"
    required: false
    default: "list"
  source:
    description: "URL or skill name (for install, enable, disable, delete)"
    required: false
---

# Skill Management

This skill teaches you how to install and manage other skills using the `manage_skills` tool.

## Available Actions

| Action | Description | Required Params |
|--------|-------------|-----------------|
| `list` | List all skills | none |
| `get` | Get skill details | `name` |
| `install` | Install from URL or markdown | `url` OR `markdown` |
| `enable` | Enable a skill | `name` |
| `disable` | Disable a skill | `name` |
| `delete` | Remove a skill | `name` |
| `search` | Find skills by query | `query` |

## List Skills

```json
{"action": "list"}
```

List only enabled skills:
```json
{"action": "list", "filter_enabled": true}
```

## Get Skill Details

```json
{"action": "get", "name": "skill_name"}
```

## Install Skills

### From URL
```json
{"action": "install", "url": "https://example.com/skills/my_skill.md"}
```

### From Markdown Content
```json
{"action": "install", "markdown": "---\nname: my_skill\ndescription: My custom skill\nversion: 1.0.0\ntags: [general]\n---\n\n# My Skill\n\nInstructions here..."}
```

## Enable/Disable Skills

```json
{"action": "enable", "name": "skill_name"}
```

```json
{"action": "disable", "name": "skill_name"}
```

## Delete Skills

```json
{"action": "delete", "name": "skill_name"}
```

## Search Skills

```json
{"action": "search", "query": "twitter"}
```

---

## Skill File Format (SKILL.md)

Every skill is a markdown file with YAML frontmatter. Here's the complete template:

```markdown
---
name: skill_name              # Required: unique identifier (lowercase, underscores)
description: "What it does"   # Required: brief description
version: 1.0.0                # Recommended: semantic version
author: your_name             # Optional: creator name
homepage: https://...         # Optional: documentation link
metadata: {"key": "value"}    # Optional: custom metadata, emoji, etc.
tags: [tag1, tag2]            # Recommended: for search/categorization
requires_tools: [tool1]       # Optional: tools the skill needs
requires_binaries: [git]      # Optional: system binaries needed
arguments:                    # Optional: user-provided parameters
  arg_name:
    description: "What this argument is for"
    required: true            # or false
    default: "default_value"  # optional default
---

# Skill Title

Instructions for the AI on how to perform this skill...
Use {{arg_name}} to reference arguments in the prompt.
```

### Required Fields
- `name` - Unique skill identifier (lowercase, use underscores)
- `description` - What the skill does (keep it concise)

### Recommended Fields
- `version` - Use semantic versioning (1.0.0)
- `tags` - For categorization and search

### Tag Categories
| Tag | Use For |
|-----|---------|
| `general`, `all` | Available to all agents |
| `social`, `twitter` | Social media skills |
| `crypto`, `defi` | Finance/trading skills |
| `development`, `code` | Programming skills |
| `utility` | General tools |

---

## Examples

### Simple Greeting Skill
```json
{
  "action": "install",
  "markdown": "---\nname: greet\ndescription: Greet the user warmly\nversion: 1.0.0\nauthor: starkbot\ntags: [general, utility]\n---\n\n# Greet User\n\nWhen activated, greet the user with a friendly, personalized message."
}
```

### API Integration Skill
```json
{
  "action": "install",
  "markdown": "---\nname: my_api\ndescription: Interact with My API service\nversion: 1.0.0\nauthor: myname\nhomepage: https://myapi.com/docs\nmetadata: {\"api_base\": \"https://api.myapi.com/v1\"}\ntags: [general, api]\nrequires_tools: [web]\n---\n\n# My API Integration\n\nBase URL: `https://api.myapi.com/v1`\n\n## Endpoints\n\n### Get Data\n```bash\ncurl https://api.myapi.com/v1/data\n```"
}
```

### Install from Remote URL
```json
{
  "action": "install",
  "url": "https://moltx.io/skill.md"
}
```
