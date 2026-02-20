---
name: github
description: "Advanced GitHub operations with safe commits, PR creation, deployment, and quality checks."
version: 1.3.0
author: starkbot
homepage: https://cli.github.com/manual/
metadata: {"requires_auth": true, "clawdbot":{"emoji":"🐙"}}
requires_tools: [git, committer, deploy, pr_quality, github_user, api_keys_check, exec]
tags: [github, git, pr, version-control, deployment, ci-cd, development]
---

# GitHub Operations Guide

## DEFAULT BEHAVIOR: External GitHub Repos (gh CLI)

**By default, assume the user wants to interact with EXTERNAL GitHub repositories.**

Use the `gh` CLI tool via `exec` for ALL GitHub operations unless the user explicitly says:
- "local" / "locally" / "my local"
- "this repo" / "current repo" / "workspace"
- "my changes" / "my branch"

### Quick Reference: External Repo Commands

| Operation | Command |
|-----------|---------|
| View repo info | `gh repo view owner/repo` |
| View README | `gh repo view owner/repo --json readme -q .readme` |
| List commits | `gh api repos/owner/repo/commits --jq '.[].commit.message'` |
| View commit details | `gh api repos/owner/repo/commits/SHA` |
| List PRs | `gh pr list -R owner/repo` |
| View PR | `gh pr view NUMBER -R owner/repo` |
| PR diff | `gh pr diff NUMBER -R owner/repo` |
| List issues | `gh issue list -R owner/repo` |
| View issue | `gh issue view NUMBER -R owner/repo` |
| List releases | `gh release list -R owner/repo` |
| View workflows | `gh run list -R owner/repo` |
| Clone repo | `gh repo clone owner/repo` |
| Fork repo | `gh repo fork owner/repo` |
| Search code | `gh search code "query" --repo owner/repo` |
| Repo stats | `gh api repos/owner/repo --jq '{stars: .stargazers_count, forks: .forks_count, issues: .open_issues_count}'` |

### Extracting Owner/Repo from URLs

When user provides a URL like `https://github.com/anthropics/claude-code`:
- Owner: `anthropics`
- Repo: `claude-code`
- Use: `gh repo view anthropics/claude-code`

**Check the Context Bank** - GitHub URLs are auto-extracted and available there.

### Example: Inspect an External Repo

User: "What's the latest activity on https://github.com/rust-lang/rust?"

```json
{"tool": "exec", "command": "gh repo view rust-lang/rust"}
{"tool": "exec", "command": "gh pr list -R rust-lang/rust --limit 5"}
{"tool": "exec", "command": "gh api repos/rust-lang/rust/commits --jq '.[0:5] | .[].commit.message'"}
```

**NEVER use `git log`, `git status`, or `git diff` for external repos** - those only show your local workspace!

---

## LOCAL MODE: Your Workspace (git tool)

**Only use these when the user explicitly mentions "local", "this repo", or their own workspace.**

The `git` tool and other specialized tools operate on your local filesystem:

You have access to specialized tools for safe and effective GitHub operations:

| Tool | Purpose |
|------|---------|
| `api_keys_check` | **Check if GITHUB_TOKEN is configured** |
| `github_user` | **Get authenticated username** - call this before operations needing your username |
| `git` | Basic git operations (status, diff, log, add, commit, branch, checkout, push, pull, fetch, clone) |
| `committer` | **Safe scoped commits** with secret detection, conventional commit enforcement |
| `deploy` | **Deployment ops** (push, PR creation, workflow monitoring, merge) |
| `pr_quality` | **Pre-PR checks** (debug code, TODOs, size validation) |
| `exec` | **Shell commands** for GitHub Projects and other `gh` CLI operations |

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

## GitHub Projects (Kanban Boards)

Use the `exec` tool to manage GitHub Projects via the `gh project` CLI commands.

**Important:** Replace `OWNER` with the actual owner from the GitHub URL the user provided. Check the Context Bank for extracted URLs.

### List User's Projects
```json
{"tool": "exec", "command": "gh project list --owner OWNER"}
```

### View Project Details
```json
{"tool": "exec", "command": "gh project view PROJECT_NUMBER --owner OWNER"}
```

### List Project Fields (Columns/Status Options)
Get field IDs needed for moving items between columns:
```json
{"tool": "exec", "command": "gh project field-list PROJECT_NUMBER --owner OWNER --format json"}
```

### List Items in Project
```json
{"tool": "exec", "command": "gh project item-list PROJECT_NUMBER --owner OWNER --format json"}
```

### Add Existing Issue/PR to Project
```json
{"tool": "exec", "command": "gh project item-add PROJECT_NUMBER --owner OWNER --url https://github.com/OWNER/REPO/issues/123"}
```

### Create Draft Item (Task) in Project
```json
{"tool": "exec", "command": "gh project item-create PROJECT_NUMBER --owner OWNER --title \"Task title\" --body \"Task description\""}
```

### Move Item Between Columns (Update Status)
First get field IDs with `field-list`, then:
```json
{"tool": "exec", "command": "gh project item-edit --id ITEM_ID --project-id PROJECT_ID --field-id FIELD_ID --single-select-option-id OPTION_ID"}
```

### Example: Add Task to Kanban Board

To add a task to a project like `https://github.com/users/someuser/projects/9`:

1. **Extract owner from URL:** The owner is `someuser`

2. **Get project ID and field info:**
```json
{"tool": "exec", "command": "gh project view 9 --owner someuser --format json"}
{"tool": "exec", "command": "gh project field-list 9 --owner someuser --format json"}
```

3. **Create a draft item:**
```json
{"tool": "exec", "command": "gh project item-create 9 --owner someuser --title \"Implement feature X\" --body \"Description of the task\""}
```

4. **Or add an existing issue:**
```json
{"tool": "exec", "command": "gh project item-add 9 --owner someuser --url https://github.com/someuser/repo/issues/42"}
```

**Note:** The `--owner` flag uses the username for user-owned projects or org name for organization projects. Always extract the owner from the URL the user provided.

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

## GitHub Discussions

For reading and writing GitHub Discussions, use the **`github_discussions`** skill which provides:
- List/search discussions
- View discussion details and comments
- Create new discussions
- Add comments and replies
- Mark answers (Q&A categories)
- Update/delete discussions and comments

Discussions use the GraphQL API via `gh api graphql`.

---

## Best Practices

1. **Always use `committer`** instead of raw git commit for safety
2. **Run `pr_quality`** before creating PRs
3. **Create feature branches** - never commit directly to main
4. **Use conventional commits** for clear history
5. **Keep PRs focused** on a single change
6. **Include test plan** in PR descriptions
7. **Monitor CI** after pushing

---

## Mode Detection Summary

**DEFAULT = EXTERNAL (gh CLI)**

| User says... | Mode | Tools to use |
|--------------|------|--------------|
| GitHub URL (github.com/...) | External | `gh` via `exec` |
| "check this repo" + URL | External | `gh` via `exec` |
| "what's new on owner/repo" | External | `gh` via `exec` |
| "show me PRs for..." | External | `gh` via `exec` |
| (no specific context) | External | `gh` via `exec` |
| "local" / "locally" | Local | `git`, `committer`, `deploy` |
| "my changes" / "my branch" | Local | `git`, `committer`, `deploy` |
| "this workspace" | Local | `git`, `committer`, `deploy` |
| "commit" / "push my code" | Local | `committer`, `deploy` |
| "current repo" | Local | `git`, `committer`, `deploy` |

**When in doubt, use `gh` for external repos. Only switch to local tools when the user explicitly refers to their own workspace or wants to make local changes.**
