---
name: supabase
description: "Manage Supabase projects - databases, migrations, edge functions, storage, and secrets using the Supabase CLI."
version: 1.0.0
author: starkbot
homepage: https://supabase.com
metadata: {"requires_auth": true, "clawdbot":{"emoji":"⚡"}}
requires_tools: [exec, api_keys_check, define_tasks]
tags: [development, devops, supabase, database, infrastructure, hosting]
---

# Supabase Integration

Manage your Supabase projects using the Supabase CLI. Run SQL queries, manage migrations, deploy edge functions, handle secrets, and more.

## Authentication

**First, check if SUPABASE_ACCESS_TOKEN is configured:**
```tool:api_keys_check
key_name: SUPABASE_ACCESS_TOKEN
```

If not configured, ask the user to create a Personal Access Token at https://supabase.com/dashboard/account/tokens and add it in Settings > API Keys as `SUPABASE_ACCESS_TOKEN`.

The `SUPABASE_ACCESS_TOKEN` env var is automatically injected into all `exec` commands.

---

## Prerequisites

Ensure the Supabase CLI is installed:

```tool:exec
command: supabase --version
timeout: 30
```

Supabase CLI is pre-installed in the Docker container. If missing, report to admin.

---

## Operations

### 1. List Projects

```tool:exec
command: supabase projects list
timeout: 30
```

### 2. Link to Project

Link the current workspace to an existing Supabase project. The project ref is the unique ID found in your project's dashboard URL.

```tool:exec
command: supabase link --project-ref PROJECT_REF
timeout: 30
```

### 3. Database Status

Inspect the linked database:

```tool:exec
command: supabase inspect db info
timeout: 30
```

### 4. Run SQL Query

Execute a SQL query against the linked project:

```tool:exec
command: supabase db execute "SELECT current_database(), current_user"
timeout: 60
```

For queries against a specific project (without linking):

```tool:exec
command: supabase db execute --project-ref PROJECT_REF "SELECT ..."
timeout: 60
```

### 5. Pull Schema / Create Migration

Pull the current remote schema into a local migration file:

```tool:exec
command: supabase db pull
timeout: 60
```

Generate a diff-based migration from local changes:

```tool:exec
command: supabase db diff --use-migra -f MIGRATION_NAME
timeout: 60
```

### 6. Push Migrations

**IMPORTANT: Confirm with the user before pushing migrations.**

Push local migrations to the remote database:

```tool:exec
command: supabase db push
timeout: 120
```

### 7. Generate TypeScript Types

Generate TypeScript types from the database schema:

```tool:exec
command: supabase gen types typescript --project-ref PROJECT_REF
timeout: 60
```

### 8. Inspect Queries

View long-running queries:

```tool:exec
command: supabase inspect db long-running-queries
timeout: 30
```

View most frequently called queries:

```tool:exec
command: supabase inspect db calls
timeout: 30
```

View table sizes:

```tool:exec
command: supabase inspect db table-sizes
timeout: 30
```

### 9. Deploy Edge Functions

**IMPORTANT: Confirm with the user before deploying.**

Deploy an edge function:

```tool:exec
command: supabase functions deploy FUNCTION_NAME
timeout: 120
```

List deployed functions:

```tool:exec
command: supabase functions list
timeout: 30
```

### 10. Manage Secrets

List all secrets:

```tool:exec
command: supabase secrets list
timeout: 30
```

Set a secret:

**IMPORTANT: Confirm with the user before modifying secrets.**

```tool:exec
command: supabase secrets set KEY=VALUE
timeout: 30
```

For multiple secrets:

```tool:exec
command: supabase secrets set KEY1=VALUE1 KEY2=VALUE2
timeout: 30
```

### 11. Storage Operations

List files in a storage bucket:

```tool:exec
command: supabase storage ls ss:///BUCKET_NAME
timeout: 30
```

Copy a file to storage:

```tool:exec
command: supabase storage cp LOCAL_FILE ss:///BUCKET_NAME/PATH
timeout: 60
```

### 12. Create New Project

Create a new Supabase project. This is a multi-step workflow.

**IMPORTANT: Confirm the project name, organization, region, and database password with the user before proceeding.**

#### Step 1: Define the tasks

Call `define_tasks` with all steps upfront so progress is tracked:

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — List organizations: get available orgs to create project under. See supabase skill 'Step 2'.",
  "TASK 2 — Create project: create a new Supabase project. See supabase skill 'Step 3'.",
  "TASK 3 — Link project: link the workspace to the new project. See supabase skill 'Step 4'.",
  "TASK 4 — Configure secrets: set environment variables / secrets. See supabase skill 'Step 5'.",
  "TASK 5 — Verify project: confirm project is ready and accessible. See supabase skill 'Step 6'."
]}
```

#### Step 2: List organizations

```tool:exec
command: supabase orgs list
timeout: 30
```

#### Step 3: Create the project

```tool:exec
command: supabase projects create PROJECT_NAME --org-id ORG_ID --db-password DB_PASSWORD --region REGION
timeout: 120
```

Common regions: `us-east-1`, `us-west-1`, `eu-west-1`, `ap-southeast-1`.

#### Step 4: Link to the new project

```tool:exec
command: supabase link --project-ref PROJECT_REF
timeout: 30
```

#### Step 5: Set secrets (if needed)

Use operation #10 to set any secrets the project needs.

#### Step 6: Verify project

```tool:exec
command: supabase projects list
timeout: 30
```

Then inspect the database:

```tool:exec
command: supabase inspect db info
timeout: 30
```

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| `Unauthorized` | Token is invalid/expired | Regenerate PAT at https://supabase.com/dashboard/account/tokens |
| `Cannot find linked project` | No project linked in current directory | Use `supabase link --project-ref REF` first |
| `Permission denied` | Token lacks access to the project | Verify token owner has access to the project/org |
| `Migration conflict` | Remote has changes not in local | Run `supabase db pull` first, then resolve conflicts |
| `Function deploy failed` | Build error in edge function | Check function code for syntax errors |
| `supabase: command not found` | CLI not installed | Report to admin — should be pre-installed in Docker |

---

## Typical Workflow

1. **Verify auth** — `supabase projects list`
2. **Link to project** — `supabase link --project-ref REF`
3. **Check database** — `supabase inspect db info`
4. **Take action** — run queries, push migrations, deploy functions (confirm with user first)

---

## Best Practices

1. **Always verify auth first** before running other commands
2. **Link before acting** — use `supabase link` so commands target the right project
3. **Confirm mutations** — always ask the user before pushing migrations, deploying functions, or modifying secrets
4. **Pull before push** — run `supabase db pull` before `supabase db push` to avoid conflicts
5. **Be careful with secrets** — they may contain sensitive values, don't log them unnecessarily
6. **Use `--project-ref`** for one-off commands against a specific project without linking
