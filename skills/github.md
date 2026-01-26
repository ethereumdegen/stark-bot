---
name: github
description: "Interact with GitHub using the `gh` CLI. Clone repos, create branches, make changes, and submit PRs."
homepage: https://cli.github.com/manual/
metadata: {"requires_auth": true}
requires_binaries: [git, gh]
tags: [github, git, pr, version-control]
---

# GitHub Operations Guide

You have access to git and gh (GitHub CLI) commands via the exec tool.
Authentication is handled automatically via the stored GitHub API key.

## IMPORTANT: Workspace Management

Before cloning, check if the repo already exists in the workspace:
```bash
ls -la repo-name
```

**If the repo already exists:**
```bash
cd repo-name
git fetch --all
git checkout main || git checkout master  # Go to default branch
git reset --hard origin/main || git reset --hard origin/master  # Reset to remote state
git clean -fd  # Remove untracked files
```

**If the repo doesn't exist:** Clone it fresh (see workflows below).

---

## Workflow: Contributing to Someone Else's Repo (Fork Workflow)

Use this workflow when you DON'T have write access to the repository.

### 1. Fork and Clone the Repository
```bash
gh repo fork owner/repo --clone=true --remote=true
cd repo
```
This creates a fork under your account and clones it locally.

**If repo already exists locally:** Reset it and sync with upstream:
```bash
cd repo
git remote add upstream https://github.com/owner/repo.git 2>/dev/null || true
git fetch upstream
git checkout main || git checkout master
git reset --hard upstream/main || git reset --hard upstream/master
```

### 2. Create a Feature Branch (use unique name with timestamp)
```bash
git checkout -b feature/change-$(date +%s)
```

### 3. Make Changes
Use read_file, write_file, and list_files tools to modify the code.

### 4. Commit Changes
```bash
git add -A
git commit -m "Description of changes"
```

### 5. Push to Your Fork and Create PR
```bash
git push -u origin HEAD
gh pr create --title "PR Title" --body "Description"
```
The `gh pr create` command automatically creates a PR from your fork to the original repo.

---

## Workflow: Your Own Repos (Direct Push)

Use this workflow when you HAVE write access to the repository.

### 1. Clone the Repository
```bash
gh repo clone owner/repo
cd repo
```
**If already exists:** `cd repo && git fetch && git checkout main && git pull`

### 2. Create a Feature Branch (use unique name)
```bash
git checkout -b feature/change-$(date +%s)
```

### 3. Make Changes, Commit, Push, and Create PR
```bash
git add -A
git commit -m "Description of changes"
git push -u origin HEAD
gh pr create --title "PR Title" --body "Description"
```

## Useful Commands

### Repository Info
- `gh repo view owner/repo` - View repository info
- `gh repo clone owner/repo` - Clone a repository

### Pull Requests
- `gh pr list --repo owner/repo` - List open PRs
- `gh pr view 123 --repo owner/repo` - View PR details
- `gh pr checks 123 --repo owner/repo` - Check CI status on a PR
- `gh pr create --repo owner/repo --title "Title" --body "Body"` - Create a PR

### Issues
- `gh issue list --repo owner/repo` - List issues
- `gh issue view 123 --repo owner/repo` - View issue details
- `gh issue create --repo owner/repo --title "Title" --body "Body"` - Create an issue

### CI/Workflow Runs
- `gh run list --repo owner/repo --limit 10` - List recent workflow runs
- `gh run view <run-id> --repo owner/repo` - View a run details
- `gh run view <run-id> --repo owner/repo --log-failed` - View logs for failed steps

### Git Commands
- `git -C path/to/repo status` - Check repo status
- `git -C path/to/repo log --oneline -10` - View recent commits
- `git -C path/to/repo diff` - View uncommitted changes
- `git -C path/to/repo branch -a` - List all branches

## API for Advanced Queries

The `gh api` command is useful for accessing data not available through other subcommands.

Get PR with specific fields:
```bash
gh api repos/owner/repo/pulls/55 --jq '.title, .state, .user.login'
```

## JSON Output

Most commands support `--json` for structured output. You can use `--jq` to filter:

```bash
gh issue list --repo owner/repo --json number,title --jq '.[] | "\(.number): \(.title)"'
```

## Best Practices

1. **Always create a feature branch** - never commit directly to main
2. **Write descriptive commit messages** explaining the "why"
3. **Keep PRs focused** on a single change
4. **Include context in PR descriptions** - reference issues, explain motivation
5. **Use conventional branch names** like `fix/issue-description` or `feature/new-capability`
