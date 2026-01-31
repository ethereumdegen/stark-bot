---
name: github
description: "Advanced GitHub operations with safe commits, PR creation, deployment, and quality checks."
homepage: https://cli.github.com/manual/
metadata: {"requires_auth": true}
requires_tools: [git, committer, deploy, pr_quality, github_user, api_keys_check]
tags: [github, git, pr, version-control, deployment, ci-cd]
---

# GitHub Operations Guide

You have access to specialized tools for safe and effective GitHub operations:

| Tool | Purpose |
|------|---------|
| `api_keys_check` | **Check if GITHUB_TOKEN is configured** |
| `github_user` | **Get authenticated username** - call this before operations needing your username |
| `git` | Basic git operations (status, diff, log, add, commit, branch, checkout, push, pull, fetch, clone) |
| `committer` | **Safe scoped commits** with secret detection, conventional commit enforcement |
| `deploy` | **Deployment ops** (push, PR creation, workflow monitoring, merge) |
| `pr_quality` | **Pre-PR checks** (debug code, TODOs, size validation) |

**Before GitHub operations, verify authentication:**
```tool:api_keys_check
key_name: GITHUB_TOKEN
```

If not configured, ask the user to add their GitHub Personal Access Token in Settings > API Keys.

## IMPORTANT: Use the Right Tools

**For commits:** Use `committer` instead of raw `git commit`. It provides:
- Secret detection (API keys, tokens, passwords)
- Sensitive file blocking (.env, credentials)
- Conventional commit enforcement
- Protected branch protection

**For deployment:** Use `deploy` for push/PR/CI operations. It provides:
- Safety checks before push
- Automatic PR creation with proper formatting
- CI/CD workflow monitoring
- Auto-merge capabilities

---

## Getting Your GitHub Username

When you need your authenticated GitHub username (for creating repos, setting remotes, etc.), use the `github_user` tool:

```json
{"tool": "github_user"}
```

This returns your authenticated username (e.g., "octocat"). Use it in commands like:
- `gh repo create <username>/my-repo --public`
- `git remote add origin https://github.com/<username>/repo.git`

---

## Workflow: Safe Feature Development

### 1. Clone/Setup Repository

```json
{"tool": "git", "operation": "clone", "url": "https://github.com/owner/repo"}
```

Or if working in existing workspace:
```json
{"tool": "git", "operation": "fetch"}
{"tool": "git", "operation": "pull"}
```

### 2. Create Feature Branch

```json
{"tool": "git", "operation": "checkout", "branch": "feature/my-change", "create": true}
```

### 3. Make Changes

Use `read_file`, `edit_file`, `write_file` tools to modify code.

### 4. Run Quality Checks

Before committing, check for issues:
```json
{"tool": "pr_quality", "operation": "full_check"}
```

This detects:
- Debug code (console.log, println!, dbg!)
- TODO/FIXME without issue references
- Files that are too large
- Overall PR size

### 5. Safe Commit with Committer Tool

```json
{
  "tool": "committer",
  "message": "feat(auth): add OAuth2 login support",
  "files": ["src/auth.rs", "src/config.rs"]
}
```

**Features:**
- Only stages specified files (no accidental commits)
- Scans for secrets before commit
- Validates conventional commit format
- Adds Co-Authored-By attribution

### 6. Push and Create PR

```json
{
  "tool": "deploy",
  "operation": "create_pr",
  "title": "feat(auth): Add OAuth2 login support",
  "body": "## Summary\n- Adds OAuth2 authentication\n- Updates config schema\n\n## Test Plan\n- [ ] Test login flow\n- [ ] Test token refresh"
}
```

This automatically:
- Pushes your branch
- Creates the PR with proper formatting
- Returns the PR URL

### 7. Monitor CI/CD

```json
{"tool": "deploy", "operation": "workflow_status"}
```

Or for specific PR:
```json
{"tool": "deploy", "operation": "pr_status", "pr_number": 123}
```

---

## Conventional Commit Format

The `committer` tool enforces conventional commits:

```
type(scope): description

Types:
- feat:     New feature
- fix:      Bug fix
- docs:     Documentation only
- style:    Formatting (no code change)
- refactor: Code change (not fix/feature)
- perf:     Performance improvement
- test:     Adding tests
- chore:    Maintenance
- ci:       CI/CD changes
- build:    Build system changes
- revert:   Revert previous commit

Examples:
- feat(auth): add OAuth2 login support
- fix: resolve memory leak in cache
- refactor(api): simplify error handling
- docs(readme): update installation steps
```

---

## Quick Reference

### Check Repository Status
```json
{"tool": "git", "operation": "status"}
```

### View Recent Commits
```json
{"tool": "git", "operation": "log", "count": 10}
```

### View Diff
```json
{"tool": "git", "operation": "diff"}
{"tool": "git", "operation": "diff", "staged": true}
```

### Create Branch
```json
{"tool": "git", "operation": "checkout", "branch": "feature/name", "create": true}
```

### Switch Branch
```json
{"tool": "git", "operation": "checkout", "branch": "main"}
```

### Push Changes
```json
{"tool": "deploy", "operation": "push"}
```

### Pull Latest
```json
{"tool": "git", "operation": "pull"}
```

### Fetch Updates
```json
{"tool": "git", "operation": "fetch"}
```

---

## PR Quality Checks

### Full Check (Recommended before PR)
```json
{"tool": "pr_quality", "operation": "full_check"}
```

### Debug Code Scan Only
```json
{"tool": "pr_quality", "operation": "debug_scan"}
```

### TODO/FIXME Scan
```json
{"tool": "pr_quality", "operation": "todo_scan"}
```

### Size Check
```json
{"tool": "pr_quality", "operation": "size_check"}
```

### Diff Summary
```json
{"tool": "pr_quality", "operation": "diff_summary"}
```

---

## Deployment Operations

### Push to Remote
```json
{"tool": "deploy", "operation": "push"}
{"tool": "deploy", "operation": "push", "branch": "feature/x", "set_upstream": true}
```

### Create Pull Request
```json
{
  "tool": "deploy",
  "operation": "create_pr",
  "title": "Your PR Title",
  "body": "## Summary\n...\n\n## Test Plan\n...",
  "base_branch": "main",
  "draft": false
}
```

### Check PR Status
```json
{"tool": "deploy", "operation": "pr_status", "pr_number": 123}
```

### Check Workflow Runs
```json
{"tool": "deploy", "operation": "workflow_status"}
{"tool": "deploy", "operation": "workflow_status", "workflow_name": "ci.yml"}
```

### Trigger Deployment
```json
{"tool": "deploy", "operation": "trigger_deploy", "workflow_name": "deploy.yml", "branch": "main"}
```

### Merge PR
```json
{"tool": "deploy", "operation": "merge_pr", "pr_number": 123}
{"tool": "deploy", "operation": "merge_pr", "pr_number": 123, "auto_merge": true}
```

---

## Safety Features

### Protected Branches
Force push is **forbidden** on protected branches (main, master, production, prod, release).

### Secret Detection
The committer tool scans for:
- API keys and tokens
- Passwords in config files
- Private keys (RSA, EC, SSH)
- AWS credentials
- GitHub tokens (PAT, fine-grained)
- OpenAI/Anthropic API keys
- Slack tokens

### Sensitive File Protection
Blocked by default:
- `.env`, `.env.local`, `.env.production`
- `credentials.json`, `secrets.json`
- `*.pem`, `*.key`, `id_rsa`
- `.npmrc`, `.pypirc`, `.htpasswd`

---

## Best Practices

1. **Always use `committer`** instead of raw git commit for safety
2. **Run `pr_quality`** before creating PRs
3. **Create feature branches** - never commit directly to main
4. **Use conventional commits** for clear history
5. **Keep PRs focused** on a single change
6. **Include test plan** in PR descriptions
7. **Monitor CI** after pushing
