---
name: debug
description: Debug errors and issues in the codebase. Analyzes error messages, traces through code, and suggests fixes.
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🐛"}}
tags: [development, debugging, errors, code]
requires_tools: [read_file, list_files, grep, exec]
---

# Debug Skill

Systematically debug errors and issues in code.

## Debug Workflow

### Step 1: Understand the Error

Parse the error message to identify:
- Error type (compile error, runtime error, logic error)
- Location (file, line number)
- Stack trace (if available)
- Error message

### Step 2: Locate the Problem

**Find the file:**
```tool:read_file
path: src/problematic_file.rs
```

**Search for the error source:**
```tool:grep
pattern: error_pattern
glob: "*.rs"
output_mode: content
context: 5
```

### Step 3: Trace the Code Path

**Find function definitions:**
```tool:grep
pattern: "fn function_name"
glob: "*.rs"
output_mode: content
context: 10
```

**Find callers:**
```tool:grep
pattern: "function_name\\("
glob: "*.rs"
output_mode: content
context: 3
```

### Step 4: Check Related Code

**Find similar patterns:**
```tool:grep
pattern: similar_code
glob: "*.rs"
output_mode: files_with_matches
```

**Read related modules:**
```tool:list_files
path: src/module/
```

### Step 5: Identify Root Cause

Common causes by error type:

#### Compile Errors
| Error | Common Cause |
|-------|--------------|
| `cannot find` | Missing import or typo |
| `type mismatch` | Wrong type conversion |
| `borrow checker` | Lifetime/ownership issue |
| `unresolved` | Missing dependency |

#### Runtime Errors
| Error | Common Cause |
|-------|--------------|
| `null/None` | Unhandled empty value |
| `index out of bounds` | Array access issue |
| `division by zero` | Missing input validation |
| `connection` | Network/config issue |

#### Logic Errors
| Symptom | Common Cause |
|---------|--------------|
| Wrong output | Algorithm bug |
| Infinite loop | Missing exit condition |
| Race condition | Concurrency issue |
| Memory leak | Resource not freed |

## Debug Commands

### Rust
```tool:exec
command: cargo check
timeout: 60000
```

### Node.js
```tool:exec
command: npm run lint
timeout: 60000
```

### Python
```tool:exec
command: python -m py_compile file.py
timeout: 30000
```

## Debugging Strategies

### 1. Binary Search
- If error location is unclear
- Comment out half the code
- Narrow down to specific section

### 2. Minimal Reproduction
- Create smallest failing case
- Remove unrelated code
- Isolate the issue

### 3. Trace Variables
- Add logging at key points
- Check values at each step
- Find where value diverges

### 4. Check Assumptions
- Verify input data format
- Check configuration values
- Validate external dependencies

## Output Format

```markdown
## Debug Analysis

### Error
```
[paste error message]
```

### Location
- File: `path/to/file.rs`
- Line: 42
- Function: `process_data()`

### Root Cause
[Explanation of why the error occurs]

### Fix
[Specific code change to fix the issue]

### Prevention
[How to prevent this type of error]
```

## Tools Used

| Tool | Purpose |
|------|---------|
| `read_file` | Read source code |
| `grep` | Search for patterns |
| `glob` | Find files |
| `exec` | Run debug commands |
| `git` | Check recent changes |
