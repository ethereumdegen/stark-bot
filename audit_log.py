#!/usr/bin/env python3
"""
audit_log.py — DeFi Risk Guardian
Writes guardian actions to an on-chain audit log contract on Starknet
and maintains a local append-only JSONL log file as a secondary record.

On-chain log contract: Stores (report_id, wallet, timestamp, actions_json_hash)
as an immutable record. The full action details live in the local JSONL file,
with the on-chain hash used for verification.
"""

from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path

import httpx

from risk_engine import RiskReport

# ──────────────────────────────────────────────
# Config
# ──────────────────────────────────────────────

AUDIT_LOG_FILE = Path(os.getenv("GUARDIAN_AUDIT_LOG", "guardian_audit.jsonl"))

# On-chain audit log contract address (deploy your own or use shared one)
# This contract exposes: write_entry(report_id: felt252, content_hash: felt252)
AUDIT_CONTRACT_ADDRESS = os.getenv(
    "GUARDIAN_AUDIT_CONTRACT",
    "0x0"  # Set this to a deployed audit log contract
)

RPC_URL = os.getenv("STARKNET_RPC", "https://starknet-mainnet.public.blastapi.io")
WALLET_ADDRESS = os.getenv("STARKNET_WALLET", "")


# ──────────────────────────────────────────────
# Local JSONL Log
# ──────────────────────────────────────────────

def write_local_log(entry: dict) -> None:
    """Append an entry to the local JSONL audit log."""
    with open(AUDIT_LOG_FILE, "a") as f:
        f.write(json.dumps(entry) + "\n")


def read_local_log(wallet: str | None = None, limit: int = 100) -> list[dict]:
    """Read recent entries from the local log, optionally filtered by wallet."""
    if not AUDIT_LOG_FILE.exists():
        return []
    entries = []
    with open(AUDIT_LOG_FILE) as f:
        for line in f:
            try:
                entry = json.loads(line.strip())
                if wallet is None or entry.get("wallet") == wallet:
                    entries.append(entry)
            except json.JSONDecodeError:
                continue
    return entries[-limit:]


# ──────────────────────────────────────────────
# On-Chain Log
# ──────────────────────────────────────────────

async def write_onchain_entry(report_id: str, content_hash: str) -> str | None:
    """
    Write audit entry to the on-chain log contract.
    Returns the transaction hash or None on failure.
    
    The on-chain entry is minimal: (report_id_felt, content_hash_felt).
    Full details are in the local JSONL file and retrievable by hash.
    """
    if AUDIT_CONTRACT_ADDRESS == "0x0":
        return None  # Contract not configured, skip on-chain log

    # Build the call
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "starknet_addInvokeTransaction",
        "params": [{
            "type": "INVOKE",
            "sender_address": WALLET_ADDRESS,
            "calldata": [
                AUDIT_CONTRACT_ADDRESS,
                "0x" + hashlib.sha256(b"write_entry").hexdigest()[:8],  # selector stub
                "0x2",
                report_id[:31],      # felt252 truncated
                content_hash[:31],   # felt252 truncated
            ],
            "max_fee": "0x" + format(int(5 * 1e9), "x"),  # 5 GWEI max for a log write
        }],
    }

    try:
        async with httpx.AsyncClient() as client:
            r = await client.post(RPC_URL, json=payload, timeout=10)
            result = r.json()
            tx_hash = result.get("result", {}).get("transaction_hash")
            return tx_hash
    except Exception as e:
        print(f"[audit] On-chain write failed: {e}")
        return None


# ──────────────────────────────────────────────
# Main Entry Point
# ──────────────────────────────────────────────

async def write_audit_entry(report: RiskReport) -> None:
    """
    Write a RiskReport to both the local JSONL log and the on-chain audit contract.
    """
    entry = {
        "report_id": report.report_id,
        "timestamp": report.timestamp,
        "wallet": report.wallet,
        "portfolio_risk_score": report.portfolio_risk_score,
        "portfolio_risk_category": report.portfolio_risk_category,
        "positions_count": len(report.positions),
        "actions_taken": report.actions_taken,
        "portfolio_total_usd": report.portfolio_total_usd,
    }

    # Hash the full report for on-chain reference
    content_hash = hashlib.sha256(
        json.dumps(entry, sort_keys=True).encode()
    ).hexdigest()
    entry["content_hash"] = content_hash

    # 1. Local JSONL log (always)
    write_local_log(entry)

    # 2. On-chain log (if contract configured and actions were taken)
    if report.actions_taken:
        tx = await write_onchain_entry(report.report_id, content_hash)
        if tx:
            entry["onchain_tx"] = tx
            print(f"[audit] On-chain entry written: {tx}")
        else:
            print("[audit] On-chain entry skipped (contract not configured or failed).")


# ──────────────────────────────────────────────
# CLI — Audit Log Viewer
# ──────────────────────────────────────────────

if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="DeFi Risk Guardian — Audit Log Viewer")
    parser.add_argument("--wallet", help="Filter by wallet address")
    parser.add_argument("--limit", type=int, default=20, help="Number of entries to show")
    parser.add_argument("--json", action="store_true", help="Output raw JSON")
    args = parser.parse_args()

    entries = read_local_log(args.wallet, args.limit)
    if not entries:
        print("No audit log entries found.")
        raise SystemExit(0)

    if args.json:
        print(json.dumps(entries, indent=2))
    else:
        print(f"\n📋 DeFi Risk Guardian — Audit Log ({len(entries)} entries)\n")
        for e in entries:
            actions = e.get("actions_taken", [])
            action_str = f"{len(actions)} action(s)" if actions else "no actions"
            print(
                f"  {e['timestamp'][:16]}  [{e['portfolio_risk_category'].upper():<9}] "
                f"Score:{e['portfolio_risk_score']:>3}/100  {action_str}  "
                f"id={e['report_id']}"
            )
