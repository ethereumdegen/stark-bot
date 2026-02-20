---
name: commit
description: Create a well-formatted git commit with proper message, staged files, and following repository conventions.
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"📝"}}
tags: [development, git, commit, version-control]
requires_tools: [git, committer]
---

# Git Commit Skill

Create properly formatted git commits following repository conventions.

## Workflow

### Step 1: Check Current Status

```tool:git
operation: status
```

Review the output to understand:
- Which files are modified
- Which files are staged
- Which files are untracked

### Step 2: Review Changes

```tool:git
operation: diff
```

For staged changes:
```tool:git
operation: diff
staged: true
```

### Step 3: Check Recent Commits (for style)

```tool:git
operation: log
count: 5
```

Look at recent commit messages to match the style (e.g., conventional commits, imperative mood).

### Step 4: Stage Files

Stage specific files (preferred over `git add .`):
```tool:git
operation: add
files: ["src/main.rs", "src/lib.rs"]
```

### Step 5: Create Commit (Recommended: Use Committer Tool)

**PREFERRED: Use the `committer` tool for safe commits with secret detection:**
```tool:committer
message: "feat(auth): add user authentication"
files: ["src/auth.rs", "src/middleware.rs", "src/routes/login.rs"]
```

The committer tool provides:
- Secret detection (blocks API keys, tokens, passwords)
- Sensitive file blocking (.env, credentials.json)
- Conventional commit format validation
- Protected branch protection
- Automatic Co-Authored-By attribution

**Alternative: Direct git commit (less safe):**
```tool:git
operation: commit
message: |
  feat: add user authentication

  Implement JWT-based auth with refresh tokens.
  - Add auth middleware
  - Create login/logout endpoints
  - Add token refresh logic
```

## Commit Message Format

Follow conventional commits when appropriate:

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Types
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `style`: Formatting, no code change
- `refactor`: Code change that neither fixes nor adds
- `test`: Adding/correcting tests
- `chore`: Maintenance tasks

### Subject Line Rules
- Use imperative mood ("add" not "added")
- No period at end
- Max 50 characters
- Capitalize first letter

### Body Rules
- Wrap at 72 characters
- Explain what and why, not how
- Separate from subject with blank line

## Examples

### Simple fix:
```
fix: resolve null pointer in user lookup
```

### Feature with body:
```
feat(auth): add OAuth2 login support

Implement Google and GitHub OAuth2 providers.
- Add OAuth2 configuration
- Create callback handlers
- Store provider tokens securely
```

### Breaking change:
```
feat!: change API response format

BREAKING CHANGE: API now returns wrapped responses.
All clients must update to handle new format.
```

## Safety Rules

1. **Never use `git add .` or `git add -A`** - Stage specific files to avoid:
   - Committing `.env` or credentials
   - Including large binaries
   - Adding generated files

2. **Review diff before committing** - Ensure no debug code or secrets

3. **Don't amend pushed commits** - Creates problems for collaborators

4. **Don't commit to main/master directly** - Use feature branches

## Tools Used

| Tool | Purpose |
|------|---------|
| `git` | All git operations |
| `read_file` | Review file contents if needed |
| `grep` | Search for sensitive patterns before commit |
