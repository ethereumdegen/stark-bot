---
name: conway_domains
description: "Search, register, and manage domains via Conway Domains. Handles domain availability, registration with USDC payment, renewals, and full DNS management."
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🌐"}}
tags: [domains, dns, web3, x402, registration]
sets_agent_subtype: finance
requires_tools: [siwa_auth, web_fetch, define_tasks]
---

# Conway Domains

Search for available domains, register them with USDC payments, and manage DNS records — all through the Conway Domains API.

**Base URL:** `https://api.conway.domains`
**Supported TLDs:** .com, .io, .ai, .xyz, .net, .org, .dev
**Payment:** USDC on Base via x402 protocol (auto-handled by `web_fetch`)

---

## Public Endpoints (no auth needed)

These use `web_fetch` with `extract_mode: "raw"` and no authentication.

### Domain Search

```json
{"tool": "web_fetch", "url": "https://api.conway.domains/domains/search?q=<keyword>&tlds=com,io,ai", "extract_mode": "raw"}
```

### Check Availability

```json
{"tool": "web_fetch", "url": "https://api.conway.domains/domains/check?domains=example.com,example.io,example.ai", "extract_mode": "raw"}
```

### Get TLD Pricing

```json
{"tool": "web_fetch", "url": "https://api.conway.domains/domains/pricing", "extract_mode": "raw"}
```

Or filter by TLD:
```json
{"tool": "web_fetch", "url": "https://api.conway.domains/domains/pricing?tlds=com,ai", "extract_mode": "raw"}
```

---

## Authentication

All non-public endpoints require a Conway JWT. Use `siwa_auth` to authenticate via SIWE:

```json
{
  "tool": "siwa_auth",
  "server_url": "https://api.conway.domains",
  "nonce_path": "/auth/nonce",
  "verify_path": "/auth/verify",
  "domain": "api.conway.domains",
  "uri": "https://api.conway.domains",
  "statement": "Sign in to Conway Domains",
  "cache_as": "conway_receipt"
}
```

The response will contain `access_token` and `refresh_token`. **Extract the `access_token` value** from the server response — you will pass it as `bearer_auth_token` to all subsequent `web_fetch` calls.

---

## Authenticated Endpoints

For all authenticated calls, pass the access_token from the `siwa_auth` response:

### List My Domains

```json
{"tool": "web_fetch", "url": "https://api.conway.domains/domains", "extract_mode": "raw", "bearer_auth_token": "<access_token>"}
```

### Get Domain Info

```json
{"tool": "web_fetch", "url": "https://api.conway.domains/domains/<domain>", "extract_mode": "raw", "bearer_auth_token": "<access_token>"}
```

### Register a Domain (x402 USDC payment)

`web_fetch` automatically handles the x402 payment flow (402 → sign USDC payment → retry):

```json
{
  "tool": "web_fetch",
  "url": "https://api.conway.domains/domains/register",
  "method": "POST",
  "body": {"domain": "example.com", "years": 1, "privacy": true},
  "extract_mode": "raw",
  "bearer_auth_token": "<access_token>"
}
```

### Renew a Domain (x402 USDC payment)

```json
{
  "tool": "web_fetch",
  "url": "https://api.conway.domains/domains/<domain>/renew",
  "method": "POST",
  "body": {"years": 1},
  "extract_mode": "raw",
  "bearer_auth_token": "<access_token>"
}
```

---

## DNS Management

### List DNS Records

```json
{"tool": "web_fetch", "url": "https://api.conway.domains/domains/<domain>/dns", "extract_mode": "raw", "bearer_auth_token": "<access_token>"}
```

### Add DNS Record

```json
{
  "tool": "web_fetch",
  "url": "https://api.conway.domains/domains/<domain>/dns",
  "method": "POST",
  "body": {"type": "A", "host": "@", "value": "1.2.3.4", "ttl": 3600},
  "extract_mode": "raw",
  "bearer_auth_token": "<access_token>"
}
```

Supported types: A, AAAA, CNAME, MX, TXT, SRV, CAA, NS. For MX records, add `"distance": 10` for priority.

### Update DNS Record

```json
{
  "tool": "web_fetch",
  "url": "https://api.conway.domains/domains/<domain>/dns/<record_id>",
  "method": "PUT",
  "body": {"value": "5.6.7.8", "ttl": 3600},
  "extract_mode": "raw",
  "bearer_auth_token": "<access_token>"
}
```

### Delete DNS Record

```json
{
  "tool": "web_fetch",
  "url": "https://api.conway.domains/domains/<domain>/dns/<record_id>",
  "method": "DELETE",
  "extract_mode": "raw",
  "bearer_auth_token": "<access_token>"
}
```

---

## Task Flows

### Search / Check / Pricing (simple flow)

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Search/check domains and report results to the user."
]}
```

Use the appropriate public endpoint above, then report results with `say_to_user`.

### Domain Registration (full flow)

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Check domain availability. See conway_domains skill.",
  "TASK 2 — Authenticate with Conway via SIWE. See conway_domains skill.",
  "TASK 3 — Register the domain (x402 USDC payment). See conway_domains skill.",
  "TASK 4 — Confirm registration and report to user. See conway_domains skill."
]}
```

**Task 1:** Use the check endpoint. If unavailable, suggest alternatives and stop.

**Task 2:** Call `siwa_auth` with the Conway params above. Extract `access_token` from the response.

**Task 3:** POST to `/domains/register` with the access_token. `web_fetch` handles x402 payment automatically.

**Task 4:** Report domain, expiration, payment amount to the user.

### DNS Management Flow

```json
{"tool": "define_tasks", "tasks": [
  "TASK 1 — Authenticate with Conway via SIWE. See conway_domains skill.",
  "TASK 2 — List current DNS records. See conway_domains skill.",
  "TASK 3 — Add/update/delete DNS records as requested. See conway_domains skill.",
  "TASK 4 — Report changes to user. See conway_domains skill."
]}
```

## CRITICAL RULES

1. **EXECUTE IMMEDIATELY.** Do NOT ask for confirmation unless the domain name is ambiguous. Call `define_tasks` as your VERY FIRST action.
2. **ONE TASK AT A TIME.** Only do the work described in the CURRENT task.
3. **Do NOT call `say_to_user` with `finished_task: true` until the current task is truly done.**
4. **Always use `extract_mode: "raw"`** for all Conway API calls — responses are JSON, not HTML.
5. **Remember the access_token** from the siwa_auth step and reuse it for all subsequent calls.

## Common DNS Setups

- **Website:** A record, host `@`, value = server IP
- **Subdomain:** A or CNAME record, host = subdomain name
- **Email (MX):** MX record with distance (priority)
- **Domain verification:** TXT record with verification value
- **CNAME alias:** CNAME record pointing to another domain
