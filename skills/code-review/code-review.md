---
name: code-review
description: Review code changes and provide feedback. Checks for bugs, style issues, security concerns, and suggests improvements.
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🔍"}}
tags: [development, review, code, git]
requires_tools: [git, read_file, grep, pr_quality]
---

# Code Review Skill

Review code changes and provide constructive feedback.

## Review Workflow

### Step 1: Get the Changes

**View unstaged changes:**
```tool:git
operation: diff
```

**View staged changes:**
```tool:git
operation: diff
staged: true
```

**View specific file:**
```tool:git
operation: diff
files: ["src/main.rs"]
```

### Step 2: Understand Context

**Read the full file:**
```tool:read_file
path: src/modified_file.rs
```

**Check related code:**
```tool:grep
pattern: function_name
glob: "*.rs"
output_mode: content
context: 5
```

**Check recent commits:**
```tool:git
operation: log
count: 5
```

### Step 3: Review Checklist

#### Correctness
- [ ] Does the code do what it's supposed to?
- [ ] Are edge cases handled?
- [ ] Are error conditions handled?
- [ ] Is the logic correct?

#### Security
- [ ] No hardcoded secrets/credentials
- [ ] Input validation present
- [ ] No SQL/command injection
- [ ] Proper authentication/authorization

#### Style & Readability
- [ ] Follows project conventions
- [ ] Clear variable/function names
- [ ] Appropriate comments
- [ ] No dead code

#### Performance
- [ ] No obvious inefficiencies
- [ ] Appropriate data structures
- [ ] No unnecessary allocations
- [ ] Database queries optimized

#### Testing
- [ ] Tests for new functionality
- [ ] Tests for edge cases
- [ ] Existing tests still pass

### Step 4: Provide Feedback

## Review Output Format

```markdown
## Code Review

### Summary
[Brief overview of the changes and overall assessment]

### Approval Status
- ✅ Approved
- ⚠️ Approved with suggestions
- ❌ Changes requested

### Issues Found

#### Critical (Must Fix)
1. **[File:Line]** - [Issue description]
   - Problem: [What's wrong]
   - Suggestion: [How to fix]

#### Suggestions (Should Consider)
1. **[File:Line]** - [Suggestion description]
   - Current: [What it does now]
   - Suggested: [What it could do better]

#### Nitpicks (Optional)
1. **[File:Line]** - [Minor suggestion]

### Positive Notes
- [What was done well]
- [Good patterns followed]

### Questions
- [Any clarifications needed]
```

## Common Issues to Look For

### Security
- Hardcoded credentials or API keys
- SQL string concatenation (injection risk)
- Missing input validation
- Insecure random number generation
- Path traversal vulnerabilities

### Bugs
- Off-by-one errors
- Null/undefined handling
- Race conditions
- Resource leaks
- Integer overflow

### Code Quality
- Magic numbers
- Deeply nested code
- Copy-pasted code
- Unused imports/variables
- Inconsistent naming

### Performance
- N+1 queries
- Blocking I/O in async code
- Excessive memory allocation
- Missing caching opportunities
- Inefficient algorithms

## Severity Levels

| Level | Description | Action |
|-------|-------------|--------|
| Critical | Bug, security issue, or crash | Must fix before merge |
| Major | Significant problem | Should fix |
| Minor | Style or small improvement | Nice to have |
| Nitpick | Trivial preference | Optional |

## Tools Used

| Tool | Purpose |
|------|---------|
| `git` | View diffs and history |
| `read_file` | Read full file context |
| `grep` | Search for patterns |
| `glob` | Find related files |
| `pr_quality` | Automated quality checks |

## Automated Quality Checks

Before manual review, run automated checks:

**Full quality check:**
```tool:pr_quality
operation: full_check
base_branch: main
```

**Check for debug code:**
```tool:pr_quality
operation: debug_scan
```

**Check PR size:**
```tool:pr_quality
operation: size_check
```

**Get diff summary:**
```tool:pr_quality
operation: diff_summary
```
