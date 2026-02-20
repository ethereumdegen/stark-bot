---
name: railway
description: "Manage Railway infrastructure - deploy services, manage environment variables, and monitor deployments using the Railway CLI."
version: 3.3.0
author: starkbot
homepage: https://railway.com
metadata: {"requires_auth": true, "clawdbot":{"emoji":"🚂"}}
requires_tools: [exec, api_keys_check, define_tasks]
tags: [development, devops, railway, infrastructure, deployment, hosting]
requires_api_keys:
  RAILWAY_API_TOKEN:
    description: "Railway API Token"
    secret: true
---

# Railway Integration

Manage your Railway infrastructure using the Railway CLI. Deploy services, manage environment variables, check deployment status, and more.

## Authentication

**First, check if RAILWAY_API_TOKEN is configured:**
```tool:api_keys_check
key_name: RAILWAY_API_TOKEN
```

If not configured, ask the user to create a token at https://railway.com/account/tokens and add it in Settings > API Keys as `RAILWAY_API_TOKEN`.

The `RAILWAY_API_TOKEN` env var is automatically injected into all `exec` commands.

---

## Prerequisites

Ensure the Railway CLI is installed:

```tool:exec
command: railway --version
timeout: 30
```

Railway CLI is pre-installed in the Docker container. If missing, report to admin.

---

## Operations

### 1. Verify Authentication

Use `railway list` to verify auth (works with all token types — account, workspace, and project tokens). Do NOT use `railway whoami` as it only works with account tokens and will fail with workspace/project tokens.

```tool:exec
command: railway list
timeout: 30
```

### 2. List Projects

```tool:exec
command: railway list --json
timeout: 30
```

### 3. Get Project Status

After linking to a project (see operation #4), check its status:

```tool:exec
command: railway status --json
timeout: 30
```

### 4. Link to Existing Project

Link the current workspace to a project so subsequent commands target it. Use `--project` and `--environment` flags to avoid interactive prompts:

```tool:exec
command: railway link --project PROJECT_ID --environment ENVIRONMENT_ID
timeout: 30
```

### 5. Get Deployments / Logs

View recent deployment logs:

```tool:exec
command: railway logs -n 50
timeout: 30
```

View build logs:

```tool:exec
command: railway logs --build -n 100
timeout: 30
```

### 6. Trigger Redeploy

**IMPORTANT: Confirm with the user before triggering a redeploy.**

```tool:exec
command: railway redeploy -y
timeout: 30
```

### 7. Get Environment Variables

```tool:exec
command: railway variable list --json
timeout: 30
```

### 8. Set Environment Variables

**IMPORTANT: Confirm with the user before modifying environment variables.**

```tool:exec
command: railway variable set KEY=VALUE
timeout: 30
```

For multiple variables:

```tool:exec
command: railway variable set KEY1=VALUE1 KEY2=VALUE2
timeout: 30
```

### 9. Create Railway Domain

```tool:exec
command: railway domain
timeout: 30
```

### 10. Deploy from GitHub Repo

Deploy a GitHub repository to Railway. This is a multi-step workflow.

**IMPORTANT: Confirm the repo URL, project name, and branch with the user before proceeding.**

#### Step 1: Define the tasks

Call `define_tasks` with all steps upfront so progress is tracked:

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Create project: create a new Railway project. See railway skill 'Step 2'.",
  "TASK 2 — Add service from repo: add a service linked to the GitHub repo. See railway skill 'Step 3'.",
  "TASK 3 — Generate domain: assign a public .railway.app domain. See railway skill 'Step 4'.",
  "TASK 4 — Set env vars: configure environment variables if needed. See railway skill 'Step 5'.",
  "TASK 5 — Verify deployment: check logs and confirm service is live. See railway skill 'Step 6'."
]}
```

#### Step 2: Create a new project

```tool:exec
command: railway init --name PROJECT_NAME
timeout: 60
```

This creates a project and links the current workspace to it.

> **Tip:** If deploying into an existing project, use operation #4 (link) instead.

#### Step 3: Add service from GitHub repo

**Option A: GitHub repo link (requires Railway GitHub App)**

```tool:exec
command: railway add --repo OWNER/REPO
timeout: 60
```

Replace `OWNER/REPO` with the GitHub repository (e.g. `ethereumdegen/x402-gif-machine`). Railway will automatically trigger an initial deployment.

**If this fails with "repo not found"**, the Railway GitHub App doesn't have access. Fall back to Option B.

**Option B: Clone and deploy (no GitHub App needed)**

Clone the repo locally and deploy with `railway up`:

```tool:exec
command: git clone https://github.com/OWNER/REPO.git /tmp/railway-deploy-REPO
timeout: 120
```

Then link and deploy from the cloned directory:

```tool:exec
command: cd /tmp/railway-deploy-REPO && railway link --project PROJECT_ID --environment production && railway up --detach
timeout: 300
```

> **Note:** Option B deploys a snapshot. It won't auto-deploy on new commits like Option A does. For auto-deploy, the user needs to configure the Railway GitHub App on their repo (GitHub → Settings → Applications → Railway → Configure).

#### Step 4: Generate a public domain

```tool:exec
command: railway domain
timeout: 30
```

#### Step 5: Set environment variables (if needed)

Use operation #8 to set any env vars the service needs.

#### Step 6: Verify deployment

Check the deployment logs to confirm it built and deployed successfully:

```tool:exec
command: railway logs --build -n 100
timeout: 60
```

Then check runtime logs:

```tool:exec
command: railway logs -n 50
timeout: 30
```

---

### 11. Deploy from Local Directory

Deploy the current working directory to Railway:

```tool:exec
command: railway up --detach
timeout: 300
```

Use `--detach` to avoid blocking while the build runs. Check logs separately.

### 12. Delete Service

**IMPORTANT: Confirm with the user before deleting.**

```tool:exec
command: railway delete -y
timeout: 30
```

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` on `whoami` | `whoami` only works with Account tokens, not workspace/project tokens | Use `railway list` instead to verify auth, or create an Account Token (select "No workspace") |
| `Unauthorized` on other commands | Token is invalid/expired or wrong scope | Regenerate token at https://railway.com/account/tokens |
| `Cannot login in non-interactive mode` | CLI requires browser login | Use `RAILWAY_API_TOKEN` env var instead (already injected by exec) |
| `No project linked` | CLI doesn't know which project to target | Use `railway link --project ID --environment ID` first |
| Service creation fails | Railway GitHub app not authorized | Install at https://railway.com/account/github |

### Token Types

- **Account tokens** (`RAILWAY_API_TOKEN`) — full access, works with CLI, preferred
- **Project tokens** — scoped to one environment, limited CLI support

---

## Typical Workflow

1. **Verify auth** — `railway list` (NOT `whoami` — it only works with account tokens)
2. **List projects** — `railway list`
3. **Link to project** — `railway link --project ID --environment ID`
4. **Check status** — `railway status`
5. **Take action** — redeploy, update env vars, etc. (confirm with user first)

---

## Best Practices

1. **Always verify auth first** before running other commands
2. **Link before acting** — use `railway link` so commands target the right project
3. **Confirm mutations** — always ask the user before redeploying or changing env vars
4. **Use `--json` flag** where available for structured output
5. **Be careful with env vars** — they may contain secrets, don't log values unnecessarily
