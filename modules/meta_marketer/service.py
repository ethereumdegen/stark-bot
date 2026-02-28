# /// script
# requires-python = ">=3.12"
# dependencies = ["requests", "starkbot-sdk"]
#
# [tool.uv.sources]
# starkbot-sdk = { path = "../starkbot_sdk" }
# ///
"""
Meta Marketer module — wraps the Meta Marketing API for campaign management,
performance insights, and automated auditing.

RPC protocol endpoints:
  GET  /rpc/status             -> service health
  POST /rpc/tools/ads          -> campaign/adset/ad CRUD (action-based)
  POST /rpc/tools/insights     -> performance insights & audit (action-based)
  POST /rpc/backup/export      -> export state for backup
  POST /rpc/backup/restore     -> restore state from backup
  GET  /                       -> HTML dashboard

Launch with:  uv run service.py
"""

from flask import request
from starkbot_sdk import create_app, success, error
import os
import json
import time
import logging
import hashlib
import hmac
import requests as http_requests

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

META_ACCESS_TOKEN = os.environ.get("META_ACCESS_TOKEN", "")
META_AD_ACCOUNT_ID = os.environ.get("META_AD_ACCOUNT_ID", "")
META_APP_SECRET = os.environ.get("META_APP_SECRET", "")
API_VERSION = "v21.0"
BASE_URL = f"https://graph.facebook.com/{API_VERSION}"

log = logging.getLogger("meta_marketer")

app = create_app("meta_marketer", status_extra_fn=lambda: {
    "has_token": bool(META_ACCESS_TOKEN),
    "has_account": bool(META_AD_ACCOUNT_ID),
    "has_secret": bool(META_APP_SECRET),
    "ad_account_id": META_AD_ACCOUNT_ID,
    "api_version": API_VERSION,
})

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _account_id():
    """Return act_XXXXX format."""
    aid = META_AD_ACCOUNT_ID.strip()
    if not aid.startswith("act_"):
        aid = f"act_{aid}"
    return aid


def _params(**extra):
    """Build base query params with access token and optional appsecret_proof."""
    p = {"access_token": META_ACCESS_TOKEN}
    if META_APP_SECRET:
        proof = hmac.new(
            META_APP_SECRET.encode(), META_ACCESS_TOKEN.encode(), hashlib.sha256
        ).hexdigest()
        p["appsecret_proof"] = proof
    p.update(extra)
    return p


def _get(path, params=None):
    """GET from Meta Graph API."""
    url = f"{BASE_URL}/{path}"
    p = _params()
    if params:
        p.update(params)
    resp = http_requests.get(url, params=p, timeout=30)
    data = resp.json()
    if "error" in data:
        raise ValueError(data["error"].get("message", str(data["error"])))
    return data


def _post(path, payload=None):
    """POST to Meta Graph API."""
    url = f"{BASE_URL}/{path}"
    p = _params()
    if payload:
        p.update(payload)
    resp = http_requests.post(url, data=p, timeout=30)
    data = resp.json()
    if "error" in data:
        raise ValueError(data["error"].get("message", str(data["error"])))
    return data


# ---------------------------------------------------------------------------
# Standard insight fields
# ---------------------------------------------------------------------------

INSIGHT_FIELDS = [
    "campaign_name", "campaign_id",
    "adset_name", "adset_id",
    "ad_name", "ad_id",
    "spend", "impressions", "clicks", "reach", "frequency",
    "cpc", "cpm", "ctr",
    "actions", "cost_per_action_type",
    "purchase_roas",
    "conversions", "cost_per_conversion",
]

CAMPAIGN_FIELDS = [
    "id", "name", "status", "effective_status",
    "objective", "daily_budget", "lifetime_budget",
    "start_time", "stop_time", "created_time", "updated_time",
    "buying_type", "bid_strategy",
]

ADSET_FIELDS = [
    "id", "name", "status", "effective_status",
    "campaign_id", "daily_budget", "lifetime_budget",
    "optimization_goal", "billing_event", "bid_amount",
    "targeting", "start_time", "end_time",
    "created_time", "updated_time",
]

AD_FIELDS = [
    "id", "name", "status", "effective_status",
    "adset_id", "campaign_id",
    "creative", "created_time", "updated_time",
]


# ===================================================================
# ADS TOOL — /rpc/tools/ads
# ===================================================================

@app.route("/rpc/tools/ads", methods=["POST"])
def rpc_ads():
    data = request.get_json(silent=True) or {}
    action = data.get("action", "")

    try:
        if not META_ACCESS_TOKEN or not META_AD_ACCOUNT_ID:
            return error("META_ACCESS_TOKEN and META_AD_ACCOUNT_ID must be set")

        # --- Campaigns ---
        if action == "list_campaigns":
            return _list_campaigns(data)
        elif action == "get_campaign":
            return _get_campaign(data)
        elif action == "create_campaign":
            return _create_campaign(data)
        elif action == "update_campaign":
            return _update_campaign(data)
        elif action == "pause_campaign":
            return _pause_campaign(data)

        # --- Ad Sets ---
        elif action == "list_adsets":
            return _list_adsets(data)
        elif action == "get_adset":
            return _get_adset(data)
        elif action == "create_adset":
            return _create_adset(data)
        elif action == "update_adset":
            return _update_adset(data)

        # --- Ads ---
        elif action == "list_ads":
            return _list_ads(data)
        elif action == "get_ad":
            return _get_ad(data)
        elif action == "create_ad":
            return _create_ad(data)
        elif action == "update_ad":
            return _update_ad(data)

        # --- Creatives ---
        elif action == "list_creatives":
            return _list_creatives(data)
        elif action == "create_creative":
            return _create_creative(data)

        else:
            return error(f"Unknown action: {action}")

    except Exception as e:
        log.exception("ads tool error")
        return error(str(e))


# --- Campaign handlers ---

def _list_campaigns(data):
    limit = min(int(data.get("limit", 25)), 100)
    result = _get(f"{_account_id()}/campaigns", {
        "fields": ",".join(CAMPAIGN_FIELDS),
        "limit": str(limit),
    })
    return success({"campaigns": result.get("data", [])})


def _get_campaign(data):
    cid = data.get("campaign_id")
    if not cid:
        return error("campaign_id required")
    result = _get(cid, {"fields": ",".join(CAMPAIGN_FIELDS)})
    return success(result)


def _create_campaign(data):
    config = _parse_config(data)
    if not config.get("name"):
        return error("config.name required")
    if not config.get("objective"):
        return error("config.objective required")
    # Force PAUSED for safety
    config["status"] = "PAUSED"
    result = _post(f"{_account_id()}/campaigns", config)
    return success({"id": result.get("id"), "status": "PAUSED", "message": "Campaign created in PAUSED state — review before activating"})


def _update_campaign(data):
    cid = data.get("campaign_id")
    if not cid:
        return error("campaign_id required")
    config = _parse_config(data)
    if not config:
        return error("config required for update")
    result = _post(cid, config)
    return success({"success": result.get("success", True)})


def _pause_campaign(data):
    cid = data.get("campaign_id")
    if not cid:
        return error("campaign_id required")
    result = _post(cid, {"status": "PAUSED"})
    return success({"success": result.get("success", True), "message": f"Campaign {cid} paused"})


# --- Ad Set handlers ---

def _list_adsets(data):
    parent = data.get("campaign_id", _account_id())
    limit = min(int(data.get("limit", 25)), 100)
    result = _get(f"{parent}/adsets", {
        "fields": ",".join(ADSET_FIELDS),
        "limit": str(limit),
    })
    return success({"adsets": result.get("data", [])})


def _get_adset(data):
    sid = data.get("adset_id")
    if not sid:
        return error("adset_id required")
    result = _get(sid, {"fields": ",".join(ADSET_FIELDS)})
    return success(result)


def _create_adset(data):
    cid = data.get("campaign_id")
    if not cid:
        return error("campaign_id required")
    config = _parse_config(data)
    if not config.get("name"):
        return error("config.name required")
    config["campaign_id"] = cid
    config["status"] = "PAUSED"
    # Targeting must be JSON-encoded
    if "targeting" in config and isinstance(config["targeting"], dict):
        config["targeting"] = json.dumps(config["targeting"])
    result = _post(f"{_account_id()}/adsets", config)
    return success({"id": result.get("id"), "status": "PAUSED"})


def _update_adset(data):
    sid = data.get("adset_id")
    if not sid:
        return error("adset_id required")
    config = _parse_config(data)
    if not config:
        return error("config required for update")
    if "targeting" in config and isinstance(config["targeting"], dict):
        config["targeting"] = json.dumps(config["targeting"])
    result = _post(sid, config)
    return success({"success": result.get("success", True)})


# --- Ad handlers ---

def _list_ads(data):
    parent = data.get("adset_id") or data.get("campaign_id") or _account_id()
    limit = min(int(data.get("limit", 25)), 100)
    result = _get(f"{parent}/ads", {
        "fields": ",".join(AD_FIELDS),
        "limit": str(limit),
    })
    return success({"ads": result.get("data", [])})


def _get_ad(data):
    aid = data.get("ad_id")
    if not aid:
        return error("ad_id required")
    result = _get(aid, {"fields": ",".join(AD_FIELDS)})
    return success(result)


def _create_ad(data):
    sid = data.get("adset_id")
    if not sid:
        return error("adset_id required")
    config = _parse_config(data)
    if not config.get("name"):
        return error("config.name required")
    config["adset_id"] = sid
    config["status"] = "PAUSED"
    result = _post(f"{_account_id()}/ads", config)
    return success({"id": result.get("id"), "status": "PAUSED"})


def _update_ad(data):
    aid = data.get("ad_id")
    if not aid:
        return error("ad_id required")
    config = _parse_config(data)
    if not config:
        return error("config required for update")
    result = _post(aid, config)
    return success({"success": result.get("success", True)})


# --- Creative handlers ---

def _list_creatives(data):
    limit = min(int(data.get("limit", 25)), 100)
    result = _get(f"{_account_id()}/adcreatives", {
        "fields": "id,name,title,body,image_url,thumbnail_url,object_story_spec,status",
        "limit": str(limit),
    })
    return success({"creatives": result.get("data", [])})


def _create_creative(data):
    config = _parse_config(data)
    if not config.get("name"):
        return error("config.name required")
    # object_story_spec must be JSON-encoded
    if "object_story_spec" in config and isinstance(config["object_story_spec"], dict):
        config["object_story_spec"] = json.dumps(config["object_story_spec"])
    result = _post(f"{_account_id()}/adcreatives", config)
    return success({"id": result.get("id")})


# ===================================================================
# INSIGHTS TOOL — /rpc/tools/insights
# ===================================================================

@app.route("/rpc/tools/insights", methods=["POST"])
def rpc_insights():
    data = request.get_json(silent=True) or {}
    action = data.get("action", "")

    try:
        if not META_ACCESS_TOKEN or not META_AD_ACCOUNT_ID:
            return error("META_ACCESS_TOKEN and META_AD_ACCOUNT_ID must be set")

        if action == "account_insights":
            return _account_insights(data)
        elif action == "campaign_insights":
            return _campaign_insights(data)
        elif action == "adset_insights":
            return _adset_insights(data)
        elif action == "ad_insights":
            return _ad_insights(data)
        elif action == "audit":
            return _audit(data)
        else:
            return error(f"Unknown action: {action}")

    except Exception as e:
        log.exception("insights tool error")
        return error(str(e))


def _build_insight_params(data):
    """Build common insight query params from request data."""
    params = {"fields": ",".join(INSIGHT_FIELDS)}

    if data.get("time_range"):
        try:
            tr = json.loads(data["time_range"]) if isinstance(data["time_range"], str) else data["time_range"]
            params["time_range"] = json.dumps(tr)
        except (json.JSONDecodeError, TypeError):
            pass
    elif data.get("date_preset"):
        params["date_preset"] = data["date_preset"]
    else:
        params["date_preset"] = "last_7d"

    if data.get("breakdowns"):
        params["breakdowns"] = data["breakdowns"]

    return params


def _account_insights(data):
    params = _build_insight_params(data)
    result = _get(f"{_account_id()}/insights", params)
    rows = result.get("data", [])
    return success({"insights": rows})


def _campaign_insights(data):
    cid = data.get("campaign_id")
    if cid:
        # Single campaign
        params = _build_insight_params(data)
        result = _get(f"{cid}/insights", params)
        return success({"insights": result.get("data", [])})
    else:
        # All campaigns
        params = _build_insight_params(data)
        params["level"] = "campaign"
        result = _get(f"{_account_id()}/insights", params)
        return success({"insights": result.get("data", [])})


def _adset_insights(data):
    sid = data.get("adset_id")
    if sid:
        params = _build_insight_params(data)
        result = _get(f"{sid}/insights", params)
        return success({"insights": result.get("data", [])})
    else:
        cid = data.get("campaign_id", _account_id())
        params = _build_insight_params(data)
        params["level"] = "adset"
        result = _get(f"{cid}/insights", params)
        return success({"insights": result.get("data", [])})


def _ad_insights(data):
    aid = data.get("ad_id")
    if aid:
        params = _build_insight_params(data)
        result = _get(f"{aid}/insights", params)
        return success({"insights": result.get("data", [])})
    else:
        parent = data.get("adset_id") or data.get("campaign_id") or _account_id()
        params = _build_insight_params(data)
        params["level"] = "ad"
        result = _get(f"{parent}/insights", params)
        return success({"insights": result.get("data", [])})


def _audit(data):
    """Full account audit — pulls all active campaigns and flags issues."""
    target_cpa = data.get("target_cpa")
    target_roas = data.get("target_roas")

    # Pull all campaigns
    campaigns = _get(f"{_account_id()}/campaigns", {
        "fields": ",".join(CAMPAIGN_FIELDS),
        "filtering": json.dumps([{"field": "effective_status", "operator": "IN", "value": ["ACTIVE"]}]),
        "limit": "100",
    }).get("data", [])

    if not campaigns:
        return success({"summary": "No active campaigns found", "campaigns": [], "issues": []})

    # Pull insights for each
    campaign_ids = [c["id"] for c in campaigns]
    params = _build_insight_params(data)
    params["level"] = "campaign"
    params["filtering"] = json.dumps([{"field": "campaign.id", "operator": "IN", "value": campaign_ids}])
    insights_resp = _get(f"{_account_id()}/insights", params)
    insights = insights_resp.get("data", [])

    # Build lookup
    insight_map = {i.get("campaign_id"): i for i in insights}

    issues = []
    audit_rows = []

    for c in campaigns:
        cid = c["id"]
        ins = insight_map.get(cid, {})
        spend = float(ins.get("spend", 0))
        impressions = int(ins.get("impressions", 0))
        clicks = int(ins.get("clicks", 0))
        ctr = float(ins.get("ctr", 0))

        # Extract conversions and CPA
        actions = ins.get("actions", [])
        cost_per_action = ins.get("cost_per_action_type", [])
        conversions = 0
        cpa = None
        for a in actions:
            if a.get("action_type") in ("purchase", "lead", "complete_registration", "offsite_conversion.fb_pixel_purchase"):
                conversions += int(a.get("value", 0))
        for cp in cost_per_action:
            if cp.get("action_type") in ("purchase", "lead", "complete_registration", "offsite_conversion.fb_pixel_purchase"):
                cpa = float(cp.get("value", 0))
                break

        # Extract ROAS
        roas_list = ins.get("purchase_roas", [])
        roas = float(roas_list[0]["value"]) if roas_list else None

        row = {
            "campaign_id": cid,
            "campaign_name": c.get("name"),
            "status": c.get("effective_status"),
            "objective": c.get("objective"),
            "spend": spend,
            "impressions": impressions,
            "clicks": clicks,
            "ctr": round(ctr, 2),
            "conversions": conversions,
            "cpa": round(cpa, 2) if cpa else None,
            "roas": round(roas, 2) if roas else None,
        }
        audit_rows.append(row)

        # Flag issues
        if target_cpa and cpa and cpa > target_cpa:
            issues.append({
                "campaign": c.get("name"),
                "issue": "CPA_OVER_TARGET",
                "detail": f"CPA ${cpa:.2f} exceeds target ${target_cpa:.2f} ({((cpa - target_cpa) / target_cpa * 100):.0f}% over)",
                "severity": "high" if cpa > target_cpa * 1.5 else "medium",
                "action": "Consider pausing or adjusting targeting/creative",
            })

        if target_roas and roas is not None and roas < target_roas:
            issues.append({
                "campaign": c.get("name"),
                "issue": "ROAS_BELOW_TARGET",
                "detail": f"ROAS {roas:.2f}x below target {target_roas:.2f}x",
                "severity": "high" if roas < target_roas * 0.5 else "medium",
                "action": "Review audience and creative performance",
            })

        if spend > 50 and impressions > 1000 and ctr < 0.5:
            issues.append({
                "campaign": c.get("name"),
                "issue": "LOW_CTR",
                "detail": f"CTR {ctr:.2f}% is very low (>$50 spend, >1k impressions)",
                "severity": "medium",
                "action": "Creative fatigue likely — test new ad variants",
            })

        if spend > 100 and conversions == 0:
            issues.append({
                "campaign": c.get("name"),
                "issue": "ZERO_CONVERSIONS",
                "detail": f"${spend:.2f} spent with zero conversions",
                "severity": "high",
                "action": "Check pixel/event setup, landing page, and audience fit",
            })

    total_spend = sum(r["spend"] for r in audit_rows)
    total_conversions = sum(r["conversions"] for r in audit_rows)
    avg_cpa = round(total_spend / total_conversions, 2) if total_conversions > 0 else None

    summary = {
        "total_campaigns": len(campaigns),
        "total_spend": round(total_spend, 2),
        "total_conversions": total_conversions,
        "avg_cpa": avg_cpa,
        "issues_found": len(issues),
    }

    return success({"summary": summary, "campaigns": audit_rows, "issues": issues})


# ===================================================================
# Backup / Restore (stateless — mainly for token validation cache)
# ===================================================================

@app.route("/rpc/backup/export", methods=["POST"])
def backup_export():
    return success({"version": "1.0.0", "note": "meta_marketer is stateless — state lives in Meta"})


@app.route("/rpc/backup/restore", methods=["POST"])
def backup_restore():
    return success({"restored": True})


# ===================================================================
# Dashboard (HTML + TUI)
# ===================================================================

from starkbot_sdk.dashboard import register_dashboard  # noqa: E402
from dashboard import MetaMarketerDashboard  # noqa: E402

PORT = int(os.environ.get("MODULE_PORT", os.environ.get("META_MARKETER_PORT", "9110")))
register_dashboard(app, MetaMarketerDashboard, module_url=f"http://127.0.0.1:{PORT}")


# ===================================================================
# Helpers
# ===================================================================

def _parse_config(data):
    """Extract config dict from request — handles both dict and JSON string."""
    config = data.get("config", {})
    if isinstance(config, str):
        try:
            config = json.loads(config)
        except (json.JSONDecodeError, TypeError):
            config = {}
    return config


# ===================================================================
# Main
# ===================================================================

if __name__ == "__main__":
    port = int(os.environ.get("META_MARKETER_PORT", "9110"))
    app.run(host="127.0.0.1", port=port)
