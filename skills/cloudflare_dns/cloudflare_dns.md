---
name: cloudflare_dns
description: "Manage Cloudflare DNS and Redirect Rules — list zones, create/update/delete DNS records, and set up URL redirects."
version: 1.3.0
author: starkbot
homepage: https://cloudflare.com
metadata: {"requires_auth": true, "clawdbot":{"emoji":"🌐"}}
requires_tools: [web_fetch, api_keys_check]
tags: [development, devops, cloudflare, infrastructure, dns, domains, nameservers]
requires_api_keys:
  CLOUDFLARE_API_TOKEN:
    description: "Cloudflare API Token"
    secret: true
---

# Cloudflare DNS Management

Manage DNS records across all your Cloudflare zones via the REST API. Supports all record types with proper body shapes, pagination, and filtering.

## Authentication

**First, check if CLOUDFLARE_API_TOKEN is configured:**

```tool:api_keys_check
key_name: CLOUDFLARE_API_TOKEN
```

If not configured, ask the user to create an API token at https://dash.cloudflare.com/profile/api-tokens with **DNS:Edit** permission and add it in Settings > API Keys.

**Standard headers for all requests:**

```
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
```

The `$CLOUDFLARE_API_TOKEN` placeholder is automatically expanded from the stored API key.

---

## Zones

### List All Zones

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones?per_page=50&page=1
method: GET
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

Filter by name: `?name=example.com`
Filter by status: `?status=active`

### Get Zone Details

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID
method: GET
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

Returns zone info including nameservers, status, and plan.

---

## Listing & Searching Records

### List All DNS Records (with pagination)

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records?per_page=100&page=1
method: GET
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

**Pagination**: The API defaults to 20 records per page. Always use `per_page=100` (max) to reduce round-trips. Check `result_info.total_pages` in the response — if > 1, fetch subsequent pages with `&page=2`, `&page=3`, etc.

**Response `result_info` example:**
```json
{"page": 1, "per_page": 100, "total_count": 247, "total_pages": 3}
```

### Search / Filter Records

Combine query parameters to find specific records:

| Parameter | Example | Description |
|-----------|---------|-------------|
| `type` | `?type=A` | Filter by record type (A, AAAA, CNAME, MX, TXT, NS, SRV, CAA) |
| `name` | `?name=sub.example.com` | Exact match on record name (FQDN) |
| `content` | `?content=198.51.100.4` | Exact match on record content/value |
| `match` | `?match=any` | Use `any` to OR filters (default is `all` = AND) |
| `order` | `?order=type` | Sort by: `type`, `name`, `content`, `ttl`, `proxied` |
| `direction` | `?direction=asc` | Sort direction: `asc` or `desc` |

**Common search patterns:**

Find all A records:
`/dns_records?type=A&per_page=100`

Find a specific subdomain:
`/dns_records?name=api.example.com`

Find a specific record by type + name (most precise):
`/dns_records?type=CNAME&name=www.example.com`

Find records pointing to an IP:
`/dns_records?content=198.51.100.4`

---

## The `proxied` Flag

The `proxied` field controls whether traffic routes through Cloudflare's network or goes direct:

| `proxied` | Behavior | Use When |
|-----------|----------|----------|
| `true` (orange cloud) | Traffic routes through Cloudflare — enables CDN caching, DDoS protection, WAF, SSL termination, analytics. The actual origin IP is hidden from DNS lookups. | Web traffic (HTTP/HTTPS) you want Cloudflare to protect and accelerate. |
| `false` (grey cloud) | DNS-only — returns the actual IP/value. No Cloudflare proxy features. | Mail servers (MX targets), non-HTTP services, records that must resolve to the real IP (e.g., SSH, FTP, game servers). |

**Rules:**
- Only A, AAAA, and CNAME records can be proxied
- MX, TXT, NS, SRV, CAA records are ALWAYS `proxied: false` (the API ignores the field)
- MX records that point to a hostname should NOT have that hostname's A/AAAA record proxied (mail will break)
- Default: `false` if omitted

---

## Creating Records by Type

**IMPORTANT: Always confirm with the user before creating records.**
**IMPORTANT: Always respect the user's `proxied` setting. If they say "not proxied" or "DNS only" or "grey cloud", set `proxied: false`. NEVER default to `proxied: true` unless the user explicitly asks for it or doesn't specify and the record is for HTTP/HTTPS web traffic.**

### A Record (IPv4 address)

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records
method: POST
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"type": "A", "name": "sub.example.com", "content": "198.51.100.4", "ttl": 1, "proxied": true}
extract_mode: raw
```

- `ttl: 1` = automatic (Cloudflare manages it). When `proxied: true`, TTL is always automatic.
- Use `ttl: 300` (5 min) through `ttl: 86400` (1 day) for non-proxied records.

### AAAA Record (IPv6 address)

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records
method: POST
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"type": "AAAA", "name": "sub.example.com", "content": "2001:db8::1", "ttl": 1, "proxied": true}
extract_mode: raw
```

### CNAME Record (alias to another hostname)

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records
method: POST
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"type": "CNAME", "name": "www.example.com", "content": "example.com", "ttl": 1, "proxied": false}
extract_mode: raw
```

- `content` is the target hostname (no trailing dot needed).
- Cloudflare supports CNAME flattening at the zone apex.
- **Set `proxied` based on what the user requests.** Use `false` for DNS-only (grey cloud), `true` for Cloudflare proxy (orange cloud). Never override the user's explicit choice.

### MX Record (mail server)

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records
method: POST
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"type": "MX", "name": "example.com", "content": "mail.example.com", "priority": 10, "ttl": 1}
extract_mode: raw
```

- `priority` is **required** — lower number = higher priority.
- Common setup: priority 10 for primary, 20 for backup.
- `content` must be a hostname, not an IP.
- Never proxy the A/AAAA record that MX points to.

### TXT Record (text/verification)

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records
method: POST
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"type": "TXT", "name": "example.com", "content": "\"v=spf1 include:_spf.google.com ~all\"", "ttl": 1}
extract_mode: raw
```

- **IMPORTANT: The `content` value MUST be wrapped in double quotes inside the string** (e.g., `"\"v=spf1 ...\""`). This is how TXT records work in DNS — the value is a quoted string. Omitting the inner quotes will cause validation failures or incorrect records.
- Common uses: SPF, DKIM, DMARC, domain verification, site verification.
- For DKIM: name is usually `selector._domainkey.example.com`.
- For DMARC: name is `_dmarc.example.com`.

### NS Record (nameserver delegation)

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records
method: POST
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"type": "NS", "name": "subdomain.example.com", "content": "ns1.otherprovider.com", "ttl": 86400}
extract_mode: raw
```

- Used for delegating a subdomain to different nameservers.
- Cannot be set at the zone apex (those are managed by Cloudflare).

### SRV Record (service locator)

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records
method: POST
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"type": "SRV", "data": {"service": "_sip", "proto": "_tcp", "name": "example.com", "priority": 10, "weight": 60, "port": 5060, "target": "sip.example.com"}}
extract_mode: raw
```

- SRV uses a `data` object instead of `content`.
- `service`: service name with leading underscore (e.g., `_sip`, `_minecraft`, `_http`).
- `proto`: protocol with leading underscore (`_tcp`, `_udp`, `_tls`).
- `name`: the domain this service is for.
- `priority`: lower = preferred.
- `weight`: for load balancing among same-priority records; higher = more traffic.
- `port`: the TCP/UDP port the service runs on.
- `target`: hostname providing the service (use `.` to indicate service not available).

### CAA Record (certificate authority authorization)

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records
method: POST
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"type": "CAA", "name": "example.com", "data": {"flags": 0, "tag": "issue", "value": "letsencrypt.org"}}
extract_mode: raw
```

- CAA uses a `data` object instead of `content`.
- `tag` values: `issue` (allow CA to issue certs), `issuewild` (allow wildcard certs), `iodef` (violation reporting URL/email).
- `flags`: usually `0`. Set to `128` for critical (CA must understand the tag or refuse to issue).
- Multiple CAA records can coexist (e.g., one for `issue`, one for `issuewild`).

---

## Updating Records

**IMPORTANT: Confirm with user before updating records.**

Use PATCH to update specific fields without replacing the entire record:

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records/RECORD_ID
method: PATCH
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"content": "198.51.100.5"}
extract_mode: raw
```

You can PATCH any combination of fields: `content`, `name`, `ttl`, `proxied`, `priority` (MX), `data` (SRV/CAA).

Use PUT to fully replace a record (all fields required):

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records/RECORD_ID
method: PUT
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"type": "A", "name": "sub.example.com", "content": "198.51.100.5", "ttl": 1, "proxied": true}
extract_mode: raw
```

**Workflow**: Always list/search first to get the `RECORD_ID`, then update.

---

## Deleting Records

**IMPORTANT: Confirm with user before deleting.**

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records/RECORD_ID
method: DELETE
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

---

## Bulk Operations

### Export All Records (BIND format)

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/dns_records/export
method: GET
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

Returns a BIND zone file — useful for backups or migration.

---

## Redirect Rules (URL Redirects)

**Use Redirect Rules instead of Page Rules.** Page Rules are deprecated and don't work with account-owned API tokens (error 1011). Redirect Rules use the modern Rulesets API.

**Common use case:** Redirect a subdomain (e.g., `discord.example.com`) to an external URL (e.g., a Discord invite link). Steps:
1. Create a proxied DNS A record pointing to `192.0.2.1` (dummy IP — Cloudflare intercepts before it reaches origin)
2. Create a Redirect Rule to 301 redirect to the target URL

### Get Existing Redirect Rules

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/rulesets/phases/http_request_dynamic_redirect/entrypoint
method: GET
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
extract_mode: raw
```

If this returns 404, no redirect ruleset exists yet — the PUT below will create one.

### Create / Replace Redirect Rules

**IMPORTANT: This PUT replaces ALL redirect rules for the zone. Always GET existing rules first and include them in the PUT to avoid deleting existing redirects.**

```tool:web_fetch
url: https://api.cloudflare.com/client/v4/zones/ZONE_ID/rulesets/phases/http_request_dynamic_redirect/entrypoint
method: PUT
headers: {"Authorization": "Bearer $CLOUDFLARE_API_TOKEN", "Content-Type": "application/json"}
body: {"rules": [{"expression": "(http.host eq \"discord.example.com\")", "description": "Redirect discord.example.com to Discord invite", "action": "redirect", "action_parameters": {"from_value": {"status_code": 301, "target_url": {"value": "https://discord.gg/INVITE_CODE"}, "preserve_query_string": false}}}]}
extract_mode: raw
```

**Rule fields:**
- `expression`: Cloudflare filter expression. Common patterns:
  - Exact host: `(http.host eq "sub.example.com")`
  - Host + path: `(http.host eq "example.com" and http.request.uri.path eq "/old")`
  - Starts with: `(http.host eq "example.com" and starts_with(http.request.uri.path, "/old/"))`
- `action`: always `"redirect"`
- `action_parameters.from_value.status_code`: `301` (permanent) or `302` (temporary)
- `action_parameters.from_value.target_url.value`: the destination URL
- `preserve_query_string`: `true` to forward query params, `false` to drop them

### Example: Adding a rule without removing existing ones

1. GET the current ruleset (save the `rules` array)
2. Append your new rule to the array
3. PUT the full updated rules array back

---

## Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| 401 / Authentication error | Token invalid or expired | Regenerate token at https://dash.cloudflare.com/profile/api-tokens |
| 403 / Forbidden | Token lacks DNS:Edit permission | Check token scopes — needs at minimum DNS:Read, ideally DNS:Edit |
| 404 / Not found | Invalid zone ID or record ID | List zones/records first to get valid IDs |
| 429 / Rate limited | Too many requests | Wait and retry — Cloudflare allows 1200 requests/5 minutes |
| 1011 / Account owned tokens | Page Rules API called with account token | **Use Redirect Rules instead** (see section above) — Page Rules are deprecated |
| `success: false` | API error | Check `errors` array in response for details |
| "Record already exists" | Duplicate type+name+content | Search for existing record and update it instead |

---

## Typical Workflow

1. **Verify auth** — check API token is configured
2. **Find the zone** — list zones or filter by domain name to get the ZONE_ID
3. **List existing records** — use `per_page=100` and paginate if needed
4. **Search for specific records** — filter by type + name to check what exists
5. **Create or update** — confirm with user, then create new or PATCH existing
6. **Verify** — list/search again to confirm the change took effect

---

## Best Practices

1. **Always verify auth first** before running other queries
2. **List before acting** — get IDs from list queries, don't guess
3. **Confirm all mutations** — always ask the user before creating, updating, or deleting
4. **Use `per_page=100`** on all list queries to minimize pagination
5. **Check `result_info.total_pages`** — if > 1, you need to paginate
6. **Respect the user's `proxied` preference** — if the user specifies proxied or not proxied, use exactly what they asked for. Do NOT override their choice. Only default to `proxied: true` if the user doesn't specify and the record is web-facing HTTP/HTTPS.
7. **Never proxy MX targets** — mail servers need the real IP
8. **Use `ttl: 1` (automatic)** for proxied records, explicit TTLs for DNS-only records
9. **Search by type+name** to find the exact record before updating
10. **Export before bulk changes** — use the BIND export as a backup
