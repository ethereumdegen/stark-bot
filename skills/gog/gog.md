---
name: gog
description: Google Workspace CLI — Gmail, Calendar, Drive, Contacts, Sheets, Docs.
version: 1.4.0
requires_binaries: [gog]
requires_tools: [exec]
tags: [google, gmail, calendar, drive, sheets, docs, productivity]
homepage: https://gogcli.sh
requires_api_keys:
  GOG_ACCOUNT:
    description: Google account email
    secret: false
  GOG_CLIENT_CREDENTIALS:
    description: OAuth client_secret.json content
  GOG_KEYRING_PASSWORD:
    description: Password for headless token storage
---

# gog — Google Workspace CLI

Use `gog` via bash to interact with Google Workspace. `GOG_ACCOUNT`, `GOG_KEYRING_PASSWORD`, and `GOG_KEYRING_BACKEND` are injected automatically from API keys.

**Always use `--no-input --json` flags** for non-interactive, machine-readable output.

## Required API keys
- `GOG_ACCOUNT` — Google account email (auto-injected as env var)
- `GOG_KEYRING_PASSWORD` — password for non-interactive token storage on headless servers (auto-injected as env var)
- `GOG_CLIENT_CREDENTIALS` — OAuth client_secret.json content (for initial credential setup)

## First-time setup (headless server)

Check auth status first:
```
gog --no-input auth status
```

If no account is configured, run the setup flow:

1. Write credentials and register them with gog:
```
echo "$GOG_CLIENT_CREDENTIALS" > /tmp/gog_creds.json && gog auth credentials /tmp/gog_creds.json && rm /tmp/gog_creds.json
```
2. Start the remote auth flow — this prints a URL for the user to open in their browser:
```
gog auth add $GOG_ACCOUNT --services gmail,calendar,drive,contacts,sheets,docs --remote --step 1
```
3. **Tell the user** to open the URL in their browser, authorize, and paste back the full callback URL from the address bar (starts with `http://127.0.0.1`). Then complete the flow:
```
gog auth add $GOG_ACCOUNT --remote --step 2 --auth-url '<callback-url-from-user>'
```
4. After setup, save auth tokens to cloud backup so they persist across restarts:
```
# Use the cloud_backup tool with action "backup" to persist tokens
```

## Gmail
```
gog gmail search 'newer_than:7d' --max 10 --no-input --json
gog gmail send --to user@example.com --subject "Subject" --body "Body" --no-input
```

## Calendar
```
gog calendar events <calendarId> --from <iso> --to <iso> --no-input --json
```

## Drive
```
gog drive search "query" --max 10 --no-input --json
```

## Contacts
```
gog contacts list --max 20 --no-input --json
```

## Sheets
```
gog sheets get <sheetId> "Tab!A1:D10" --no-input --json
gog sheets update <sheetId> "Tab!A1:B2" --values-json '[["A","B"],["1","2"]]' --input USER_ENTERED --no-input
gog sheets append <sheetId> "Tab!A:C" --values-json '[["x","y","z"]]' --insert INSERT_ROWS --no-input
gog sheets clear <sheetId> "Tab!A2:Z" --no-input
gog sheets metadata <sheetId> --no-input --json
```

## Docs
```
gog docs cat <docId> --no-input
gog docs export <docId> --format txt --out /tmp/doc.txt --no-input
```

## Rules
- Always confirm with the user before sending mail or creating/modifying events.
- Always use `--no-input` to prevent interactive prompts that would hang.
- Use `--json` for any read operations to get structured output.
- Prefer `--values-json` for sheets data.
- If a command fails with an auth error, check `gog --no-input auth status` and guide the user through re-authentication.
- Use `gog <service> --help` to discover subcommands.
