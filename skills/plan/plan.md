---
name: plan
description: Create a structured implementation plan for a software task. Explores codebase, identifies files to modify, and produces step-by-step execution plan.
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"📋"}}
tags: [development, planning, git, code]
requires_tools: [read_file, glob, grep]
---

# Implementation Planning Skill

Create detailed implementation plans for software development tasks.

## Workflow

### Step 1: Understand the Request

Analyze the user's request and identify:
- What feature/fix/change is needed
- What constraints exist (language, framework, patterns)
- Success criteria

### Step 2: Explore the Codebase

Use these tools to understand the existing code:

**Find relevant files:**
```tool:glob
pattern: "**/*.rs"
limit: 50
```

**Search for related code:**
```tool:grep
pattern: function_name
glob: "*.rs"
output_mode: content
context: 3
```

**Read key files:**
```tool:read_file
path: src/main.rs
max_lines: 200
```

### Step 3: Identify Files to Modify

List all files that need to be:
- Created (new files)
- Modified (existing files)
- Possibly deleted (cleanup)

### Step 4: Create Implementation Steps

Break down the work into ordered steps:

1. **Setup/Prerequisites** - Any dependencies or config needed
2. **Core Implementation** - Main logic changes
3. **Integration** - Connecting components
4. **Tests** - Test coverage for new code
5. **Documentation** - Update docs if needed

### Step 5: Consider Edge Cases

Identify potential issues:
- Error handling scenarios
- Backwards compatibility
- Performance considerations
- Security implications

## Output Format

Provide the plan in this structure:

```markdown
## Implementation Plan: [Feature Name]

### Summary
Brief description of what will be implemented.

### Files to Modify
- `path/to/file1.rs` - [what changes]
- `path/to/file2.rs` - [what changes]

### New Files
- `path/to/new_file.rs` - [purpose]

### Steps

1. **Step Name**
   - Sub-task 1
   - Sub-task 2

2. **Step Name**
   - Sub-task 1

### Risks/Considerations
- Risk 1 and mitigation
- Risk 2 and mitigation

### Testing Strategy
- Unit tests for X
- Integration test for Y
```

## Tools Used

| Tool | Purpose |
|------|---------|
| `glob` | Find files by pattern |
| `grep` | Search file contents |
| `read_file` | Read file contents |
| `list_files` | List directory contents |

## Best Practices

1. **Start broad, then narrow** - Get overview before diving into details
2. **Follow existing patterns** - Match the codebase style
3. **Consider dependencies** - Check what will be affected
4. **Plan tests early** - Include testing in the plan
5. **Break into small steps** - Each step should be independently verifiable
