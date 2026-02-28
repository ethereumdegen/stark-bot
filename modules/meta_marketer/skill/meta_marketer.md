---
name: meta_marketer
description: "Manage Meta (Facebook/Instagram) ad campaigns — create campaigns, monitor performance, audit spend, and optimize ROAS/CPA"
version: 1.1.0
author: starkbot
requires_tools: [local_rpc]
tags: [marketing, meta, ads, facebook, instagram]
---

# Meta Marketer — RPC Reference

The `meta_marketer` module provides two RPC endpoints: `/rpc/tools/ads` for campaign CRUD and `/rpc/tools/insights` for performance analytics.

**After reading these instructions, call `local_rpc` directly to fulfill the user's request. Do NOT call `use_skill` again.**

Use `module="meta_marketer"` — the port is resolved automatically.

## Campaign Management — `/rpc/tools/ads`

### List Campaigns

```
local_rpc(module="meta_marketer", path="/rpc/tools/ads", method="POST", body={
  "action": "list_campaigns",
  "limit": 25
})
```

### Create a Campaign

All campaigns are created in **PAUSED** state for safety.

```
local_rpc(module="meta_marketer", path="/rpc/tools/ads", method="POST", body={
  "action": "create_campaign",
  "config": "{\"name\": \"Summer Sale 2026\", \"objective\": \"OUTCOME_SALES\"}"
})
```

**Supported objectives:** `OUTCOME_AWARENESS`, `OUTCOME_ENGAGEMENT`, `OUTCOME_TRAFFIC`, `OUTCOME_LEADS`, `OUTCOME_APP_PROMOTION`, `OUTCOME_SALES`

### Create an Ad Set

Budget values are in the account's currency (e.g. cents for USD accounts — $50/day = 5000).

```
local_rpc(module="meta_marketer", path="/rpc/tools/ads", method="POST", body={
  "action": "create_adset",
  "campaign_id": "CAMPAIGN_ID",
  "config": "{\"name\": \"US Women 25-44\", \"daily_budget\": 5000, \"optimization_goal\": \"OFFSITE_CONVERSIONS\", \"billing_event\": \"IMPRESSIONS\", \"targeting\": {\"geo_locations\": {\"countries\": [\"US\"]}, \"age_min\": 25, \"age_max\": 44, \"genders\": [2]}}"
})
```

### Create an Ad Creative

```
local_rpc(module="meta_marketer", path="/rpc/tools/ads", method="POST", body={
  "action": "create_creative",
  "config": "{\"name\": \"Summer Sale Image Ad\", \"object_story_spec\": {\"page_id\": \"PAGE_ID\", \"link_data\": {\"image_hash\": \"IMAGE_HASH\", \"link\": \"https://example.com/sale\", \"message\": \"50% off everything this weekend!\", \"name\": \"Summer Sale\", \"description\": \"Shop now before it's gone\", \"call_to_action\": {\"type\": \"SHOP_NOW\"}}}}"
})
```

### Create an Ad

```
local_rpc(module="meta_marketer", path="/rpc/tools/ads", method="POST", body={
  "action": "create_ad",
  "adset_id": "ADSET_ID",
  "config": "{\"name\": \"Summer Sale - Image Variant A\", \"creative\": {\"creative_id\": \"CREATIVE_ID\"}}"
})
```

### Get / Update / Pause

```
local_rpc(module="meta_marketer", path="/rpc/tools/ads", method="POST", body={
  "action": "get_campaign", "campaign_id": "ID"
})

local_rpc(module="meta_marketer", path="/rpc/tools/ads", method="POST", body={
  "action": "update_campaign", "campaign_id": "ID", "config": "{\"daily_budget\": 7500}"
})

local_rpc(module="meta_marketer", path="/rpc/tools/ads", method="POST", body={
  "action": "pause_campaign", "campaign_id": "ID"
})

local_rpc(module="meta_marketer", path="/rpc/tools/ads", method="POST", body={
  "action": "update_adset", "adset_id": "ID", "config": "{\"daily_budget\": 3000}"
})

local_rpc(module="meta_marketer", path="/rpc/tools/ads", method="POST", body={
  "action": "update_ad", "ad_id": "ID", "config": "{\"status\": \"ACTIVE\"}"
})
```

### All Ad Management Actions

`list_campaigns`, `get_campaign`, `create_campaign`, `update_campaign`, `pause_campaign`, `list_adsets`, `get_adset`, `create_adset`, `update_adset`, `list_ads`, `get_ad`, `create_ad`, `update_ad`, `list_creatives`, `create_creative`

### Parameters

| Parameter | Description |
|-----------|-------------|
| `action` | Action to perform (required) |
| `campaign_id` | Campaign ID (for get/update/pause and adset/ad listing) |
| `adset_id` | Ad Set ID (for get/update and ad listing) |
| `ad_id` | Ad ID (for get/update) |
| `config` | JSON string for create/update actions |
| `limit` | Max results (default 25, max 100) |

## Performance Analytics — `/rpc/tools/insights`

### Account-Level Insights

```
local_rpc(module="meta_marketer", path="/rpc/tools/insights", method="POST", body={
  "action": "account_insights",
  "date_preset": "last_7d"
})
```

### Campaign-Level Insights

```
local_rpc(module="meta_marketer", path="/rpc/tools/insights", method="POST", body={
  "action": "campaign_insights",
  "date_preset": "last_7d"
})

local_rpc(module="meta_marketer", path="/rpc/tools/insights", method="POST", body={
  "action": "campaign_insights",
  "campaign_id": "ID",
  "date_preset": "last_30d"
})
```

### With Breakdowns

```
local_rpc(module="meta_marketer", path="/rpc/tools/insights", method="POST", body={
  "action": "adset_insights",
  "campaign_id": "ID",
  "date_preset": "last_7d",
  "breakdowns": "age,gender"
})
```

**Available breakdowns:** `age`, `gender`, `placement`, `device`, `country`

### Custom Date Range

```
local_rpc(module="meta_marketer", path="/rpc/tools/insights", method="POST", body={
  "action": "campaign_insights",
  "time_range": "{\"since\": \"2026-01-01\", \"until\": \"2026-01-31\"}"
})
```

### Full Account Audit

Pulls all active campaigns and flags issues against your targets.

```
local_rpc(module="meta_marketer", path="/rpc/tools/insights", method="POST", body={
  "action": "audit",
  "target_cpa": 45.00,
  "target_roas": 4.0,
  "date_preset": "last_7d"
})
```

Returns:
- **summary**: total spend, conversions, avg CPA, issue count
- **campaigns**: per-campaign metrics (spend, impressions, clicks, CTR, conversions, CPA, ROAS)
- **issues**: flagged problems ranked by severity with recommended actions

Issue types detected:
- `CPA_OVER_TARGET` — CPA exceeds your target
- `ROAS_BELOW_TARGET` — ROAS below your target
- `LOW_CTR` — CTR <0.5% with significant spend (creative fatigue)
- `ZERO_CONVERSIONS` — money spent with no conversions

### All Insight Actions

`account_insights`, `campaign_insights`, `adset_insights`, `ad_insights`, `audit`

### Parameters

| Parameter | Description |
|-----------|-------------|
| `action` | Action to perform (required) |
| `campaign_id` | Campaign ID (for campaign/adset/ad level) |
| `adset_id` | Ad Set ID (for adset/ad level) |
| `ad_id` | Ad ID (for ad level) |
| `date_preset` | Date range: today, yesterday, last_3d, last_7d, last_14d, last_30d, last_90d, this_month, last_month |
| `time_range` | Custom JSON: {"since": "YYYY-MM-DD", "until": "YYYY-MM-DD"} |
| `breakdowns` | Comma-separated: age, gender, placement, device, country |
| `target_cpa` | Target CPA for audit (flags campaigns exceeding this) |
| `target_roas` | Target ROAS for audit (flags campaigns below this) |

## Safety Model

- **Read operations** (list, get, insights, audit) execute freely
- **Write operations** (create, update, pause) should be confirmed with the user before execution
- All new campaigns and ad sets are created **PAUSED** — review before activating
- Budget increases are capped at 30% per change to protect Meta's learning phase
