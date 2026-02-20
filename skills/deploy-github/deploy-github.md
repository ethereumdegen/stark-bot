---
name: deploy-github
description: Deploy code to GitHub. Push changes, create PRs, monitor CI/CD, and merge when ready.
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🚢"}}
tags: [development, git, github, deployment, ci-cd]
requires_tools: [git, committer, deploy, pr_quality, exec]
---

# Deploy to GitHub Skill

Complete workflow for deploying code to GitHub with PR creation and CI/CD monitoring.

## Pre-Deployment Checklist

Before deploying, always run quality checks:

### 1. Check for Debug Code
```tool:pr_quality
operation: debug_scan
```

### 2. Check for TODOs Without Issues
```tool:pr_quality
operation: todo_scan
```

### 3. Full Quality Check
```tool:pr_quality
operation: full_check
base_branch: main
```

### 4. Review Changes
```tool:git
operation: diff
```

```tool:git
operation: status
```

---

## Deployment Workflow

### Step 1: Ensure Clean Working State

Check status:
```tool:git
operation: status
```

If there are uncommitted changes, commit them first:
```tool:committer
message: "feat(component): description of changes"
files: ["src/file1.ts", "src/file2.ts"]
```

### Step 2: Fetch Latest and Rebase

```tool:git
operation: fetch
```

```tool:git
operation: pull
```

### Step 3: Push to Remote

```tool:deploy
operation: push
set_upstream: true
```

### Step 4: Create Pull Request

```tool:deploy
operation: create_pr
title: "feat(component): Add new feature"
body: |
  ## Summary
  - What this PR does
  - Why it's needed

  ## Changes
  - File 1: Description
  - File 2: Description

  ## Test Plan
  - [ ] Manual testing done
  - [ ] Unit tests pass
  - [ ] Integration tests pass

  ## Screenshots (if applicable)
  N/A
base_branch: main
draft: false
```

### Step 5: Monitor CI/CD

Check workflow status:
```tool:deploy
operation: workflow_status
```

Check specific PR status:
```tool:deploy
operation: pr_status
pr_number: 123
```

### Step 6: Merge PR (When Ready)

When CI passes and reviews are approved:
```tool:deploy
operation: merge_pr
pr_number: 123
```

Or enable auto-merge (waits for checks):
```tool:deploy
operation: merge_pr
pr_number: 123
auto_merge: true
```

---

## CI/CD Monitoring

### View Recent Workflow Runs
```tool:deploy
operation: workflow_status
```

### View Specific Workflow
```tool:deploy
operation: workflow_status
workflow_name: ci.yml
```

### Trigger a Deployment Workflow
```tool:deploy
operation: trigger_deploy
workflow_name: deploy.yml
branch: main
```

---

## Common PR Templates

### Feature PR
```markdown
## Summary
Brief description of the new feature.

## Changes
- Added X component
- Updated Y service
- Created Z utility

## Test Plan
- [ ] Unit tests added
- [ ] Manual testing completed
- [ ] Edge cases handled

## Breaking Changes
None / List any breaking changes
```

### Bug Fix PR
```markdown
## Problem
Description of the bug.

## Root Cause
What was causing the issue.

## Solution
How this PR fixes it.

## Test Plan
- [ ] Reproduced the bug
- [ ] Verified fix works
- [ ] Added regression test
```

### Refactor PR
```markdown
## Summary
What was refactored and why.

## Changes
- Refactored X to use pattern Y
- Simplified Z logic
- Removed deprecated code

## Behavior Changes
None - this is a pure refactor.

## Test Plan
- [ ] All existing tests pass
- [ ] No behavior changes verified
```

---

## Troubleshooting

### Push Rejected
If push is rejected due to remote changes:
```tool:git
operation: pull
```
Then push again.

### PR Conflicts
If PR has conflicts:
1. Pull latest main
2. Rebase your branch
3. Resolve conflicts
4. Force push (with lease)

```tool:git
operation: pull
branch: main
```

```tool:exec
command: git rebase main
timeout: 60000
```

```tool:deploy
operation: push
force: true
```

### CI Failed
1. Check workflow status
2. Read error logs
3. Fix issues locally
4. Push fix

---

## Tools Used

| Tool | Purpose |
|------|---------|
| `pr_quality` | Pre-deployment quality checks |
| `git` | Git operations (fetch, pull, status) |
| `committer` | Safe commits before push |
| `deploy` | Push, PR creation, CI monitoring, merge |

---

## Best Practices

1. **Always run quality checks** before creating a PR
2. **Write descriptive PR titles** following conventional commits
3. **Include test plan** in PR description
4. **Monitor CI** after pushing
5. **Don't merge with failing checks**
6. **Squash commits** when merging for clean history
7. **Delete branch** after merging
