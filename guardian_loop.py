#!/usr/bin/env python3
"""
guardian_loop.py — DeFi Risk Guardian
Main orchestrator. Runs the poll → score → alert → act loop
on a configurable interval. Supports x402 micropayments for
autonomous continuous operation.

Usage:
  python guardian_loop.py --wallet 0xABC --config guardian_config.yaml
  python guardian_loop.py --wallet 0xABC --once   # single scan
"""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import signal
import sys
import time
from pathlib import Path

import httpx
import yaml

from scanner import scan_all
from risk_engine import build_report, RiskReport
from executor import protect_report, ExecutionResult
from audit_log import write_audit_entry

# ──────────────────────────────────────────────
# Defaults
# ──────────────────────────────────────────────

DEFAULT_CONFIG = {
    "poll_interval_seconds": 60,
    "alert_threshold_health_factor": 1.4,
    "action_threshold_health_factor": 1.2,
    "max_gas_per_action_gwei": 50,
    "collateral_top_up_pct": 20,
    "emergency_full_exit": False,
    "notification_channel": "console",
    "webhook_url": "",
    "autonomous_mode": True,
    "protocols": ["nostra", "zklend", "ekubo", "strk-staking"],
}


# ──────────────────────────────────────────────
# Notification
# ──────────────────────────────────────────────

async def notify(message: str, config: dict) -> None:
    channel = config.get("notification_channel", "console")

    if channel == "console":
        print(message)
        return

    if channel == "webhook":
        url = config.get("webhook_url", "")
        if not url:
            print("[notify] Webhook URL not configured.")
            return
        try:
            async with httpx.AsyncClient() as client:
                await client.post(url, json={"text": message}, timeout=10)
        except Exception as e:
            print(f"[notify] Webhook failed: {e}")
        return

    if channel == "telegram":
        bot_token = os.getenv("TELEGRAM_BOT_TOKEN", "")
        chat_id = os.getenv("TELEGRAM_CHAT_ID", "")
        if not bot_token or not chat_id:
            print("[notify] Telegram env vars not set (TELEGRAM_BOT_TOKEN, TELEGRAM_CHAT_ID).")
            return
        try:
            async with httpx.AsyncClient() as client:
                await client.post(
                    f"https://api.telegram.org/bot{bot_token}/sendMessage",
                    json={"chat_id": chat_id, "text": message, "parse_mode": "Markdown"},
                    timeout=10,
                )
        except Exception as e:
            print(f"[notify] Telegram failed: {e}")
        return

    print(f"[notify] Unknown channel '{channel}'. Printing to console:\n{message}")


# ──────────────────────────────────────────────
# x402 Micropayment for Continuous Operation
# ──────────────────────────────────────────────

async def pay_poll_fee(wallet: str) -> bool:
    """
    Pay the x402 micropayment for one poll cycle.
    Returns True if payment succeeded (or if x402 is not configured).
    In StarkBot's runtime, this is handled by the agent's payment session.
    """
    x402_endpoint = os.getenv("X402_POLL_ENDPOINT", "")
    if not x402_endpoint:
        return True  # x402 not configured, assume free/prepaid

    try:
        async with httpx.AsyncClient() as client:
            r = await client.post(
                x402_endpoint,
                json={"action": "guardian_poll", "wallet": wallet},
                timeout=5,
            )
            if r.status_code == 200:
                return True
            print(f"[x402] Payment failed: {r.status_code} {r.text}")
            return False
    except Exception as e:
        print(f"[x402] Payment error: {e}")
        return False


# ──────────────────────────────────────────────
# Wallet Balance Fetcher
# ──────────────────────────────────────────────

async def get_wallet_balances(wallet: str) -> dict[str, float]:
    """Fetch USD value of idle token balances in wallet for action planning."""
    # In production: query the wallet's token balances via Starknet RPC
    # and price them. Stubbed here.
    starknet_rpc = os.getenv("STARKNET_RPC", "https://starknet-mainnet.public.blastapi.io")
    # Stub: return empty dict — executor will skip repay/add_col and fall back to partial exit
    return {}


# ──────────────────────────────────────────────
# Single Poll Cycle
# ──────────────────────────────────────────────

async def run_poll(wallet: str, config: dict) -> RiskReport:
    """
    One complete guardian cycle:
    1. Scan positions
    2. Score + build report
    3. Alert if above alert threshold
    4. Act autonomously if above action threshold (and autonomous_mode=True)
    5. Log to audit trail
    """
    protocols = config.get("protocols", list(DEFAULT_CONFIG["protocols"]))
    alert_hf = config.get("alert_threshold_health_factor", 1.4)
    action_hf = config.get("action_threshold_health_factor", 1.2)
    autonomous = config.get("autonomous_mode", True)
    poll_interval = config.get("poll_interval_seconds", 60)

    # Step 1: Scan
    positions = await scan_all(wallet, protocols)
    if not positions:
        print(f"[guardian] No positions found for {wallet[:12]}...")

    # Step 2: Score
    report = build_report(wallet, positions, poll_interval_seconds=poll_interval)

    # Step 3: Alert
    needs_alert = any(
        p.position.health_factor and p.position.health_factor < alert_hf
        for p in report.positions
    ) or report.portfolio_risk_score >= 56

    if needs_alert:
        await notify(report.summary_text(), config)
    else:
        # Quiet: just log to console
        print(f"[guardian] Poll complete. Portfolio risk: {report.portfolio_risk_score}/100 ({report.portfolio_risk_category}). All safe.")

    # Step 4: Act
    results: list[ExecutionResult] = []
    needs_action = (
        autonomous and
        any(
            p.position.health_factor and p.position.health_factor < action_hf
            for p in report.positions
            if p.position.health_factor
        )
    ) or any(
        sp.risk.category in ("critical", "emergency")
        for sp in report.positions
    )

    if needs_action:
        balances = await get_wallet_balances(wallet)
        results = await protect_report(report, balances, dry_run=not autonomous)

        # Notify about actions taken
        if results:
            action_lines = []
            for r in results:
                if r.success:
                    action_lines.append(
                        f"✅ {r.action.action_type.value} on {r.action.protocol}: "
                        f"${r.action.amount_usd:,.0f} {r.action.asset} "
                        f"→ New HF: {r.action.expected_new_hf}  Tx: {r.tx_hash}"
                    )
                else:
                    action_lines.append(
                        f"❌ {r.action.action_type.value} on {r.action.protocol} FAILED: {r.error}"
                    )
            await notify(
                "🛡️ Guardian Actions Taken:\n" + "\n".join(action_lines),
                config,
            )

    # Step 5: Audit log
    report.actions_taken = [r.to_dict() for r in results if r.success]
    await write_audit_entry(report)

    return report


# ──────────────────────────────────────────────
# Main Loop
# ──────────────────────────────────────────────

_running = True

def _handle_signal(sig, frame):
    global _running
    print(f"\n[guardian] Shutting down (signal {sig})...")
    _running = False


async def guardian_loop(wallet: str, config: dict, once: bool = False) -> None:
    global _running
    signal.signal(signal.SIGINT, _handle_signal)
    signal.signal(signal.SIGTERM, _handle_signal)

    poll_interval = config.get("poll_interval_seconds", 60)
    print(f"[guardian] Starting guardian for {wallet[:12]}...")
    print(f"[guardian] Poll interval: {poll_interval}s | Autonomous: {config.get('autonomous_mode')} | Channel: {config.get('notification_channel')}")

    iteration = 0
    while _running:
        iteration += 1
        print(f"\n[guardian] Poll #{iteration} at {time.strftime('%H:%M:%S')}")

        # Pay x402 fee for this poll
        paid = await pay_poll_fee(wallet)
        if not paid:
            print("[guardian] x402 payment failed — pausing for 60s before retry.")
            await asyncio.sleep(60)
            continue

        try:
            await run_poll(wallet, config)
        except Exception as e:
            print(f"[guardian] Poll error: {e}")
            await notify(f"⚠️ Guardian poll error: {e}", config)

        if once:
            break

        await asyncio.sleep(poll_interval)

    print("[guardian] Guardian stopped.")


# ──────────────────────────────────────────────
# CLI
# ──────────────────────────────────────────────

def load_config(path: str | None) -> dict:
    config = dict(DEFAULT_CONFIG)
    if path and Path(path).exists():
        with open(path) as f:
            user_cfg = yaml.safe_load(f) or {}
            config.update(user_cfg.get("guardian", user_cfg))
    return config


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="DeFi Risk Guardian — Main Loop")
    parser.add_argument("--wallet", required=True, help="Starknet wallet address")
    parser.add_argument("--config", help="Path to guardian_config.yaml")
    parser.add_argument("--once", action="store_true", help="Run one poll then exit")
    parser.add_argument("--dry-run", action="store_true", help="Alert only, no txs")
    args = parser.parse_args()

    config = load_config(args.config)
    if args.dry_run:
        config["autonomous_mode"] = False

    asyncio.run(guardian_loop(args.wallet, config, once=args.once))
