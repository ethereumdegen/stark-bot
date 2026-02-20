---
name: vercel
description: "Manage Vercel projects — deployments, environment variables, domains, and build logs."
version: 1.0.0
author: starkbot
homepage: https://vercel.com
metadata: {"requires_auth": true, "clawdbot":{"emoji":"▲"}}
requires_tools: [web_fetch, api_keys_check]
tags: [development, devops, vercel, infrastructure, deployment, hosting, frontend]
---

# Vercel Integration

Manage Vercel projects via the REST API. Deploy projects, manage environment variables, configure domains, and monitor builds.

## Authentication

**First, check if VERCEL_TOKEN is configured:**

```tool:api_keys_check
key_name: VERCEL_TOKEN
```

If not configured, ask the user to create a token at https://vercel.com/account/tokens and add it in Settings > API Keys.

---

## How to Use This Skill

All Vercel API calls use the `web_fetch` tool:

- **Base URL**: `https://api.vercel.com`
- **Headers**: `{"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}`
- **extract_mode**: `"raw"` (returns JSON)

The `$VERCEL_TOKEN` placeholder is automatically expanded from the stored API key.

For team-scoped operations, append `?teamId=TEAM_ID` to the URL.

---

## General

### Verify Authentication

```tool:web_fetch
url: https://api.vercel.com/v2/user
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

### List Teams

```tool:web_fetch
url: https://api.vercel.com/v2/teams
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

---

## Projects

### List Projects

```tool:web_fetch
url: https://api.vercel.com/v10/projects
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

Optional: `?search=my-project&limit=20`

### Get Project Details

```tool:web_fetch
url: https://api.vercel.com/v9/projects/PROJECT_ID_OR_NAME
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

### Create Project

**IMPORTANT: Confirm with user before creating.**

```tool:web_fetch
url: https://api.vercel.com/v11/projects
method: POST
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
body: {"name": "my-project", "framework": "nextjs", "gitRepository": {"type": "github", "repo": "owner/repo-name"}}
extract_mode: raw
```

Supported frameworks: `nextjs`, `react`, `vue`, `astro`, `svelte`, `nuxtjs`, `gatsby`, `remix`, `angular`, `hugo`, `jekyll`, `eleventy`, etc.

### Update Project

```tool:web_fetch
url: https://api.vercel.com/v9/projects/PROJECT_ID_OR_NAME
method: PATCH
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
body: {"buildCommand": "npm run build", "outputDirectory": "dist"}
extract_mode: raw
```

### Delete Project

**IMPORTANT: Confirm with user before deleting. This is irreversible.**

```tool:web_fetch
url: https://api.vercel.com/v9/projects/PROJECT_ID_OR_NAME
method: DELETE
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

---

## Deployments

### List Deployments

```tool:web_fetch
url: https://api.vercel.com/v6/deployments?projectId=PROJECT_ID_OR_NAME&limit=10
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

Optional filters: `&target=production`, `&state=READY`, `&branch=main`

### Get Deployment Details

```tool:web_fetch
url: https://api.vercel.com/v13/deployments/DEPLOYMENT_ID
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

### Redeploy (from existing deployment)

**IMPORTANT: Confirm with user before redeploying.**

```tool:web_fetch
url: https://api.vercel.com/v13/deployments
method: POST
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
body: {"name": "my-project", "deploymentId": "EXISTING_DEPLOYMENT_ID", "target": "production"}
extract_mode: raw
```

### Cancel Deployment

```tool:web_fetch
url: https://api.vercel.com/v12/deployments/DEPLOYMENT_ID/cancel
method: PATCH
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

### Get Build Logs

```tool:web_fetch
url: https://api.vercel.com/v3/deployments/DEPLOYMENT_ID/events
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

Optional: `?limit=-1` to get all events.

---

## Environment Variables

### List Environment Variables

```tool:web_fetch
url: https://api.vercel.com/v10/projects/PROJECT_ID_OR_NAME/env
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

### Create Environment Variable

**IMPORTANT: Confirm with user. Be careful with secrets — don't log values unnecessarily.**

```tool:web_fetch
url: https://api.vercel.com/v10/projects/PROJECT_ID_OR_NAME/env
method: POST
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
body: {"key": "API_KEY", "value": "secret-value", "type": "encrypted", "target": ["production", "preview"]}
extract_mode: raw
```

Types: `plain`, `encrypted`, `sensitive`, `secret`.
Targets: `production`, `preview`, `development` (can combine).

To upsert (create or update if exists), add `?upsert=true` to the URL.

### Update Environment Variable

```tool:web_fetch
url: https://api.vercel.com/v9/projects/PROJECT_ID_OR_NAME/env/ENV_VAR_ID
method: PATCH
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
body: {"value": "new-value"}
extract_mode: raw
```

### Delete Environment Variable

**IMPORTANT: Confirm with user before deleting.**

```tool:web_fetch
url: https://api.vercel.com/v9/projects/PROJECT_ID_OR_NAME/env/ENV_VAR_ID
method: DELETE
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

---

## Domains

### List Account Domains

```tool:web_fetch
url: https://api.vercel.com/v5/domains
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

### List Project Domains

```tool:web_fetch
url: https://api.vercel.com/v9/projects/PROJECT_ID_OR_NAME/domains
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

### Add Domain to Project

**IMPORTANT: Confirm with user before adding.**

```tool:web_fetch
url: https://api.vercel.com/v10/projects/PROJECT_ID_OR_NAME/domains
method: POST
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
body: {"name": "custom.example.com"}
extract_mode: raw
```

### Check Domain Configuration

```tool:web_fetch
url: https://api.vercel.com/v6/domains/example.com/config
method: GET
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

Returns DNS configuration status: whether the domain is properly configured, recommended A/CNAME records, and if it's misconfigured.

### Verify Domain

```tool:web_fetch
url: https://api.vercel.com/v9/projects/PROJECT_ID_OR_NAME/domains/example.com/verify
method: POST
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

### Remove Domain from Project

**IMPORTANT: Confirm with user before removing.**

```tool:web_fetch
url: https://api.vercel.com/v9/projects/PROJECT_ID_OR_NAME/domains/example.com
method: DELETE
headers: {"Authorization": "Bearer $VERCEL_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| 401 / `forbidden` | Token invalid or expired | Regenerate at https://vercel.com/account/tokens |
| 403 / Forbidden | Token lacks scope or team access | Check token scopes; add `?teamId=` for team projects |
| 404 / Not found | Invalid project/deployment ID | List projects/deployments first to get valid IDs |
| 409 / Conflict | Env var already exists | Use `?upsert=true` on create endpoint |
| 429 / Rate limited | Too many requests | Wait and retry |

---

## Typical Workflow

1. **Verify auth** — check user endpoint to confirm token works
2. **List projects** — discover project IDs and names
3. **Check deployments** — see current deployment status
4. **Take action** — deploy, update env vars, add domains (confirm with user first)
5. **Monitor** — check build logs and deployment status

---

## Best Practices

1. **Always verify auth first** before running other queries
2. **List before acting** — get IDs from list queries, don't guess
3. **Confirm mutations** — always ask user before deploying, updating, or deleting
4. **Use encrypted type** for env vars containing secrets
5. **Check domain config** after adding a domain — DNS must be properly configured
6. **Monitor build logs** after triggering a deployment to catch failures early
7. **Be careful with env vars** — they may contain secrets, don't log values unnecessarily
