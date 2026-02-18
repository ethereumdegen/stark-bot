---
name: notes
description: "Obsidian-compatible note-taking system. Create, edit, search, and organize markdown notes with YAML frontmatter, [[wikilinks]], and #tags."
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"📝"}}
requires_tools: [notes]
tags: [notes, knowledge, obsidian, productivity, memory, documentation]
arguments:
  action:
    description: "Action to perform: create, edit, read, search, list, tag, link"
    required: false
    default: "create"
  content:
    description: "The content to write (for create/edit)"
    required: false
  query:
    description: "Search query or tag name"
    required: false
---

# Notes - Obsidian-Compatible Knowledge Base

You are managing a notes system that produces Obsidian-compatible markdown files. Each note has YAML frontmatter, supports `[[wikilinks]]` for cross-referencing, and `#tags` for organization.

**All note operations use the `notes` tool.** Do not use write_file or read_file for notes.

## Note Format

```markdown
---
title: "Note Title"
date: 2026-02-18T14:32:00
updated: 2026-02-18T15:10:00
tags: [tag1, tag2]
aliases: ["alternate name"]
type: note
---

# Note Title

Body content with [[wikilinks]] and #inline-tags.
```

## Note Types

| Type | Use Case |
|------|----------|
| `note` | General knowledge, information, references |
| `idea` | New concepts, inspirations, brainstorms |
| `decision` | Decisions made with context and rationale |
| `log` | Activity logs, progress updates |
| `reflection` | Thoughts, learnings, retrospectives |
| `todo` | Task lists and action items |

## Subdirectories

Organize notes into subdirectories:
- `ideas/` — brainstorms and concepts
- `decisions/` — decision records
- `daily/` — daily logs
- `projects/` — project-specific notes

## Actions

### 1. CREATE - New Note

```json
{
  "tool": "notes",
  "action": "create",
  "title": "x402 Payment Protocol",
  "content": "Body content with [[related note]] references.\n\nKey points:\n- Point 1\n- Point 2",
  "tags": "crypto, payments, protocol",
  "note_type": "note",
  "subdir": "projects"
}
```

### 2. EDIT - Update Existing Note

```json
{
  "tool": "notes",
  "action": "edit",
  "path": "projects/x402-payment-protocol.md",
  "content": "# x402 Payment Protocol\n\nUpdated body content..."
}
```

### 3. READ - View a Note

```json
{
  "tool": "notes",
  "action": "read",
  "path": "projects/x402-payment-protocol.md"
}
```

### 4. SEARCH - Full-Text Search

```json
{
  "tool": "notes",
  "action": "search",
  "query": "payment protocol",
  "limit": 10
}
```

### 5. LIST - Show All Notes

```json
{
  "tool": "notes",
  "action": "list",
  "limit": 50
}
```

### 6. TAG - Browse by Tag

List all tags:
```json
{
  "tool": "notes",
  "action": "tag"
}
```

Search notes with a specific tag:
```json
{
  "tool": "notes",
  "action": "tag",
  "query": "crypto"
}
```

### 7. LINK - Resolve Wikilinks

Resolve a wikilink to its file:
```json
{
  "tool": "notes",
  "action": "link",
  "query": "x402 Payment Protocol"
}
```

List all wikilinks in a note:
```json
{
  "tool": "notes",
  "action": "link",
  "path": "projects/x402-payment-protocol.md"
}
```

## Best Practices

1. **Use descriptive titles** — the filename is auto-slugified from the title
2. **Add tags** — they're indexed for fast filtering
3. **Use [[wikilinks]]** — cross-reference notes to build a knowledge graph
4. **Choose the right type** — helps organize and filter later
5. **Use subdirectories** — keep related notes together

## User Interaction

When the user invokes this skill:

1. **Understand intent** — Are they creating, searching, or browsing?
2. **Use the notes tool** — All operations go through the single `notes` tool
3. **Suggest connections** — When creating a note, suggest relevant [[wikilinks]] to existing notes
4. **Confirm completion** — Tell user what was done and the file path

### Example Interactions

**User:** "Make a note about the new API design"
--> Create a note with type=note, suggest relevant tags

**User:** "What notes do I have about authentication?"
--> Search for "authentication" and show results

**User:** "Show me all my ideas"
--> Search by tag "idea" or list notes in ideas/ subdirectory

**User:** "Link my API design note to the architecture note"
--> Edit the note to add [[wikilinks]]
