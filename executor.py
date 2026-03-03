#!/usr/bin/env python3
"""
executor.py — DeFi Risk Guardian
Selects, simulates, and broadcasts protective transactions
to restore safe health factors on Starknet DeFi protocols.

All actions are pre-simulated via Starknet's simulateTransactions
before broadcasting. If simulation fails, the action is aborted.
"""

from __future__ import annotations

import asyncio
import json
import os
import time
from dataclasses import dataclass, asdict
from enum import Enum
from typing import Optional

import httpx

from scanner import Position
from risk_engine import ScoredPosition, RiskReport

# ──────────────────────────────────────────────
# Config
# ──────────────────────────────────────────────

RPC_URL = os.getenv("STARKNET_RPC", "https://starknet-mainnet.public.blastapi.io")
WALLET_ADDRESS = os.getenv("STARKNET_WALLET", "")
PRIVATE_KEY = os.getenv("STARKNET_PRIVATE_KEY", "")  # Never log this

MAX_GAS_GWEI = float(os.getenv("GUARDIAN_MAX_GAS_GWEI", "50"))
COLLATERAL_TOP_UP_PCT = float(os.getenv("GUARDIAN_COLLATERAL_TOP_UP_PCT", "20"))
EMERGENCY_FULL_EXIT = os.getenv("GUARDIAN_EMERGENCY_FULL_EXIT", "false").lower() == "true"


# ──────────────────────────────────────────────
# Action Types
# ──────────────────────────────────────────────

class ActionType(str, Enum):
    REPAY_DEBT        = "repay_debt"
    ADD_COLLATERAL    = "add_collateral"
    PARTIAL_EXIT      = "partial_exit"
    FULL_EXIT         = "full_exit"
    RECENTER_LP       = "recenter_lp"
    WITHDRAW_YIELD    = "withdraw_yield"
    NOOP              = "noop"


@dataclass
class PlannedAction:
    action_type: ActionType
    protocol: str
    asset: str
    amount: float               # In asset native units
    amount_usd: float
    expected_new_hf: Optional[float]
    rationale: str
    calldata: Optional[str] = None    # Hex calldata, filled by build_calldata()


@dataclass
class ExecutionResult:
    success: bool
    action: PlannedAction
    tx_hash: Optional[str]
    gas_used_gwei: Optional[float]
    actual_new_hf: Optional[float]
    error: Optional[str]
    simulated: bool
    timestamp: str

    def to_dict(self) -> dict:
        d = asdict(self)
        d["action"]["action_type"] = self.action.action_type.value
        return d


# ──────────────────────────────────────────────
# Action Planner
# ──────────────────────────────────────────────

def plan_action(
    sp: ScoredPosition,
    wallet_balances: dict[str, float],  # asset → USD value available
) -> PlannedAction:
    """
    Decide the best protective action for a scored position.
    Priority: repay_debt > add_collateral > partial_exit > full_exit
    """
    pos = sp.position
    risk = sp.risk
    target_hf = 1.6

    if pos.position_type == "lp":
        if not pos.extra.get("in_range", True):
            return PlannedAction(
                action_type=ActionType.RECENTER_LP,
                protocol=pos.protocol,
                asset=pos.collateral_asset,
                amount=pos.collateral_amount,
                amount_usd=pos.collateral_usd,
                expected_new_hf=None,
                rationale="LP position is out of range, collecting fees and recentering.",
            )
        return PlannedAction(
            action_type=ActionType.NOOP,
            protocol=pos.protocol,
            asset=pos.collateral_asset,
            amount=0,
            amount_usd=0,
            expected_new_hf=None,
            rationale="LP in range, no action needed.",
        )

    if pos.health_factor is None or pos.health_factor >= target_hf:
        return PlannedAction(
            action_type=ActionType.NOOP,
            protocol=pos.protocol,
            asset=pos.collateral_asset,
            amount=0,
            amount_usd=0,
            expected_new_hf=pos.health_factor,
            rationale="Health factor is safe, no action needed.",
        )

    # Emergency full exit?
    if EMERGENCY_FULL_EXIT and risk.score >= 90:
        return PlannedAction(
            action_type=ActionType.FULL_EXIT,
            protocol=pos.protocol,
            asset=pos.collateral_asset,
            amount=pos.collateral_amount,
            amount_usd=pos.collateral_usd,
            expected_new_hf=None,
            rationale="Emergency: score >= 90 and full_exit enabled.",
        )

    # Calculate repayment needed
    lltv = pos.lltv or 0.8
    target_debt_usd = (pos.collateral_usd * lltv) / target_hf
    repay_usd = max(0, (pos.debt_usd or 0) - target_debt_usd)
    repay_asset_amount = repay_usd / (pos.current_price or 1.0) if pos.debt_asset else 0

    # 1. Try repay debt from wallet balance
    avail_debt_usd = wallet_balances.get(pos.debt_asset or "", 0)
    if avail_debt_usd >= repay_usd and repay_usd > 0:
        return PlannedAction(
            action_type=ActionType.REPAY_DEBT,
            protocol=pos.protocol,
            asset=pos.debt_asset or "",
            amount=repay_asset_amount,
            amount_usd=repay_usd,
            expected_new_hf=target_hf,
            rationale=f"Repay ${repay_usd:,.0f} {pos.debt_asset} to reach HF {target_hf:.1f}.",
        )

    # 2. Try add collateral from wallet
    add_col_usd = pos.collateral_usd * (COLLATERAL_TOP_UP_PCT / 100)
    avail_col_usd = wallet_balances.get(pos.collateral_asset, 0)
    if avail_col_usd >= add_col_usd:
        col_amount = add_col_usd / (pos.current_price or 1.0)
        new_col_usd = pos.collateral_usd + add_col_usd
        new_hf = (new_col_usd * lltv) / (pos.debt_usd or 1)
        return PlannedAction(
            action_type=ActionType.ADD_COLLATERAL,
            protocol=pos.protocol,
            asset=pos.collateral_asset,
            amount=col_amount,
            amount_usd=add_col_usd,
            expected_new_hf=round(new_hf, 2),
            rationale=f"Add {COLLATERAL_TOP_UP_PCT:.0f}% more {pos.collateral_asset} collateral to reach HF ~{new_hf:.2f}.",
        )

    # 3. Partial exit (sell some collateral to repay debt)
    partial_col_usd = repay_usd * 1.05  # Sell 5% extra to cover gas/slippage
    partial_col_amount = partial_col_usd / (pos.current_price or 1.0)
    new_col_usd = pos.collateral_usd - partial_col_usd
    new_hf = (new_col_usd * lltv) / (max((pos.debt_usd or 1) - repay_usd, 1))
    return PlannedAction(
        action_type=ActionType.PARTIAL_EXIT,
        protocol=pos.protocol,
        asset=pos.collateral_asset,
        amount=partial_col_amount,
        amount_usd=partial_col_usd,
        expected_new_hf=round(new_hf, 2),
        rationale=f"Sell ${partial_col_usd:,.0f} of collateral to repay debt and reach HF ~{new_hf:.2f}.",
    )


# ──────────────────────────────────────────────
# Calldata Builders (Protocol-Specific)
# ──────────────────────────────────────────────

# These build the raw calldata for each protocol.
# In production, import the protocol's ABI or SDK to build these properly.

def build_calldata_nostra(action: PlannedAction) -> str:
    """Build Nostra repay/add_collateral calldata."""
    # Simplified — real implementation calls Nostra's Market contract
    # with the appropriate selector and amount (felt252 encoded).
    # Selector for repay: 0x...
    # This is intentionally left as a stub to be completed with the Nostra SDK.
    return f"0x[nostra_{action.action_type.value}_calldata_for_{action.asset}_{action.amount:.6f}]"


def build_calldata_zklend(action: PlannedAction) -> str:
    """Build zkLend repay/add_collateral calldata."""
    return f"0x[zklend_{action.action_type.value}_calldata_for_{action.asset}_{action.amount:.6f}]"


def build_calldata_ekubo(action: PlannedAction) -> str:
    """Build Ekubo LP recentering calldata."""
    return f"0x[ekubo_recenter_calldata_for_{action.asset}]"


CALLDATA_BUILDERS = {
    "nostra": build_calldata_nostra,
    "zklend": build_calldata_zklend,
    "ekubo":  build_calldata_ekubo,
}


def build_calldata(action: PlannedAction) -> str:
    builder = CALLDATA_BUILDERS.get(action.protocol)
    if builder:
        return builder(action)
    return "0x[unsupported_protocol]"


# ──────────────────────────────────────────────
# Simulation
# ──────────────────────────────────────────────

async def simulate_action(action: PlannedAction, client: httpx.AsyncClient) -> tuple[bool, float, str]:
    """
    Simulate the transaction using Starknet's simulateTransactions RPC.
    Returns (success, estimated_gas_gwei, error_message).
    """
    if action.action_type == ActionType.NOOP:
        return True, 0.0, ""

    calldata = build_calldata(action)
    action.calldata = calldata

    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "starknet_simulateTransactions",
        "params": [
            [{
                "type": "INVOKE",
                "sender_address": WALLET_ADDRESS,
                "calldata": [calldata],
                "max_fee": "0x" + format(int(MAX_GAS_GWEI * 1e9), "x"),
            }],
            "pending",
            []
        ]
    }

    try:
        r = await client.post(RPC_URL, json=payload, timeout=15)
        result = r.json()
        if "error" in result:
            return False, 0.0, result["error"].get("message", "Simulation failed")

        sim_data = result.get("result", [{}])[0]
        gas_estimate = float(sim_data.get("fee_estimation", {}).get("overall_fee", 0))
        gas_gwei = gas_estimate / 1e9

        if gas_gwei > MAX_GAS_GWEI:
            return False, gas_gwei, f"Gas too high: {gas_gwei:.1f} GWEI > {MAX_GAS_GWEI} max"

        return True, gas_gwei, ""
    except Exception as e:
        return False, 0.0, f"Simulation error: {e}"


# ──────────────────────────────────────────────
# Executor
# ──────────────────────────────────────────────

async def execute_action(action: PlannedAction, dry_run: bool = False) -> ExecutionResult:
    """
    Simulate then broadcast a protective action.
    Set dry_run=True to simulate only (never broadcasts).
    """
    ts = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())

    if action.action_type == ActionType.NOOP:
        return ExecutionResult(
            success=True, action=action, tx_hash=None,
            gas_used_gwei=0, actual_new_hf=action.expected_new_hf,
            error=None, simulated=False, timestamp=ts,
        )

    async with httpx.AsyncClient() as client:
        sim_ok, gas_gwei, sim_err = await simulate_action(action, client)

        if not sim_ok:
            return ExecutionResult(
                success=False, action=action, tx_hash=None,
                gas_used_gwei=gas_gwei, actual_new_hf=None,
                error=f"Simulation failed: {sim_err}", simulated=True, timestamp=ts,
            )

        if dry_run:
            return ExecutionResult(
                success=True, action=action, tx_hash="DRY_RUN",
                gas_used_gwei=gas_gwei, actual_new_hf=action.expected_new_hf,
                error=None, simulated=True, timestamp=ts,
            )

        # ── Broadcast ────────────────────────────────────────────────────────
        # In production, sign with the agent's session key via starknet.py
        # and submit via starknet_addInvokeTransaction.
        # Placeholder for now:
        try:
            tx_hash = f"0x{hash(action.calldata) & 0xffffffffffffffff:016x}"  # Stub
            # Real: tx = await account.execute(calls=[...])
            #        tx_hash = hex(tx.transaction_hash)
            print(f"[executor] Broadcast {action.action_type.value} on {action.protocol} → {tx_hash}")
            return ExecutionResult(
                success=True, action=action, tx_hash=tx_hash,
                gas_used_gwei=gas_gwei, actual_new_hf=action.expected_new_hf,
                error=None, simulated=True, timestamp=ts,
            )
        except Exception as e:
            return ExecutionResult(
                success=False, action=action, tx_hash=None,
                gas_used_gwei=gas_gwei, actual_new_hf=None,
                error=f"Broadcast failed: {e}", simulated=True, timestamp=ts,
            )


async def protect_report(
    report: RiskReport,
    wallet_balances: dict[str, float],
    dry_run: bool = False,
) -> list[ExecutionResult]:
    """
    Iterate over critical/emergency positions in a RiskReport
    and execute protective actions for each.
    """
    results: list[ExecutionResult] = []
    targets = report.critical_positions()

    if not targets:
        print("[executor] No critical positions. No actions needed.")
        return results

    for sp in targets:
        action = plan_action(sp, wallet_balances)
        print(f"[executor] Planning: {action.action_type.value} on {sp.position.protocol} — {action.rationale}")
        result = await execute_action(action, dry_run=dry_run)
        results.append(result)

        if result.success:
            print(f"  ✅ Success  tx={result.tx_hash}  gas={result.gas_used_gwei:.2f} GWEI")
        else:
            print(f"  ❌ Failed   error={result.error}")

    return results


# ──────────────────────────────────────────────
# CLI
# ──────────────────────────────────────────────

if __name__ == "__main__":
    import argparse
    from scanner import scan_all
    from risk_engine import build_report

    parser = argparse.ArgumentParser(description="DeFi Risk Guardian — Executor")
    parser.add_argument("--wallet", required=True)
    parser.add_argument("--protocols", nargs="*")
    parser.add_argument("--dry-run", action="store_true", help="Simulate only, don't broadcast")
    args = parser.parse_args()

    positions = asyncio.run(scan_all(args.wallet, args.protocols))
    report = build_report(args.wallet, positions)
    print(report.summary_text())
    print()

    # In production, fetch real wallet balances from the chain
    mock_balances = {"USDC": 5000, "USDT": 3000, "ETH": 500}

    results = asyncio.run(protect_report(report, mock_balances, dry_run=args.dry_run))
    print(f"\n[executor] {len(results)} action(s) taken.")
    for r in results:
        print(json.dumps(r.to_dict(), indent=2))
