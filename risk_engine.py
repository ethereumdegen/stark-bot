#!/usr/bin/env python3
"""
risk_engine.py — DeFi Risk Guardian
Scores positions using a multi-factor risk model and assembles
a standardized RiskReport for consumption by StarkBot and other skills.
"""

from __future__ import annotations

import hashlib
import json
import time
from dataclasses import dataclass, field, asdict
from typing import Literal, Optional

from scanner import Position

# ──────────────────────────────────────────────
# Types
# ──────────────────────────────────────────────

RiskCategory = Literal["safe", "watch", "warning", "critical", "emergency"]

CATEGORY_THRESHOLDS = [
    (90, "emergency"),
    (75, "critical"),
    (56, "warning"),
    (31, "watch"),
    (0,  "safe"),
]

ACTION_MAP: dict[RiskCategory, str] = {
    "safe":      "No action needed.",
    "watch":     "Monitor closely. Consider reducing exposure if volatility increases.",
    "warning":   "Recommend repaying some debt or adding collateral soon.",
    "critical":  "Act now — repay debt or add collateral to avoid liquidation.",
    "emergency": "IMMEDIATE ACTION REQUIRED — liquidation imminent.",
}

# Approximate 30-day realized volatility (annualized) by asset.
# In production, fetch from a volatility oracle or compute from price history.
ASSET_VOLATILITY: dict[str, float] = {
    "ETH":    0.70,
    "WBTC":   0.65,
    "BTC":    0.65,
    "STRK":   1.20,
    "wstETH": 0.55,
    "LORDS":  1.50,
    "EKUBO":  1.80,
    "USDC":   0.01,
    "USDT":   0.01,
    "DAI":    0.01,
}
DEFAULT_VOLATILITY = 1.0   # For unknown assets, assume high vol


# ──────────────────────────────────────────────
# Risk Scoring
# ──────────────────────────────────────────────

@dataclass
class RiskScore:
    score: int                        # 0–100
    category: RiskCategory
    distance_to_liq_pct: Optional[float]
    time_to_liq_estimate: Optional[str]
    recommended_actions: list[str]
    score_breakdown: dict             # sub-scores for transparency


@dataclass
class ScoredPosition:
    position: Position
    risk: RiskScore

    def to_dict(self) -> dict:
        d = self.position.to_dict()
        d["risk"] = asdict(self.risk)
        return d


def _vol_for(asset: str) -> float:
    return ASSET_VOLATILITY.get(asset, DEFAULT_VOLATILITY)


def _time_to_liq_str(hf: float, daily_vol: float) -> str:
    """Rough estimate: how many hours/days until liquidation at current drift."""
    if hf >= 2.0:
        return ">30 days"
    if daily_vol <= 0:
        return "unknown"
    # Simplified: 1 std dev daily move to reach HF=1
    gap = hf - 1.0
    days_1sigma = gap / daily_vol
    hours = days_1sigma * 24
    if hours < 1:
        return "<1 hour (CRITICAL)"
    if hours < 24:
        return f"~{hours:.0f}h at current volatility"
    return f"~{days_1sigma:.0f}d at current volatility"


def score_position(pos: Position, portfolio_total_usd: float) -> RiskScore:
    """
    Multi-factor risk score for a single position.

    Factors:
      base_score    (0–50):  Derived from inverse health factor.
      vol_penalty   (0–30):  Higher for volatile collateral assets.
      conc_penalty  (0–10):  Higher when position is a large % of portfolio.
      lp_range_pen  (0–10):  Extra risk if LP position is out of range.

    Total: 0–100. Higher = more risk.
    """
    breakdown: dict[str, float] = {}

    # ── Base Score (health factor based) ──────────────────────────────────────
    if pos.health_factor is not None:
        hf = max(pos.health_factor, 0.01)
        # At HF=1.0: base=50. At HF=2.0: base=25. At HF=1.1: base=45.
        base = min(50, (1.0 / hf) * 50)
    elif pos.position_type == "lp":
        # LP positions have no liquidation risk, but do have IL risk
        base = 5.0
    elif pos.position_type == "staking":
        # Staking has minimal liquidation risk but slashing risk
        base = 3.0
    else:
        base = 20.0  # Unknown

    breakdown["base_score"] = round(base, 2)

    # ── Volatility Penalty ────────────────────────────────────────────────────
    col_vol = _vol_for(pos.collateral_asset.split("/")[0])  # Handle LP pair notation
    # Scale: 0% vol → 0 penalty, 150% vol → 30 penalty
    vol_pen = min(30, col_vol * 20)
    if pos.position_type in ("lp", "staking"):
        vol_pen *= 0.4  # LP/staking less sensitive to short-term vol
    breakdown["vol_penalty"] = round(vol_pen, 2)

    # ── Concentration Penalty ─────────────────────────────────────────────────
    if portfolio_total_usd > 0:
        conc = pos.collateral_usd / portfolio_total_usd
        conc_pen = min(10, conc * 10)
    else:
        conc_pen = 5.0
    breakdown["concentration_penalty"] = round(conc_pen, 2)

    # ── LP Out-of-Range Penalty ───────────────────────────────────────────────
    lp_pen = 0.0
    if pos.position_type == "lp" and not pos.extra.get("in_range", True):
        lp_pen = 10.0  # Full penalty for OOR LP (100% IL accumulation risk)
    breakdown["lp_out_of_range_penalty"] = lp_pen

    # ── Staking Validator Health ──────────────────────────────────────────────
    staking_pen = 0.0
    if pos.position_type == "staking":
        uptime = pos.extra.get("validator_uptime_pct", 100)
        if uptime < 90:
            staking_pen = (90 - uptime) * 1.5   # up to 15 points for very low uptime
    breakdown["staking_validator_penalty"] = round(staking_pen, 2)

    raw = base + vol_pen + conc_pen + lp_pen + staking_pen
    score = int(min(100, max(0, raw)))
    breakdown["total_raw"] = round(raw, 2)

    # ── Category ──────────────────────────────────────────────────────────────
    category: RiskCategory = "safe"
    for threshold, cat in CATEGORY_THRESHOLDS:
        if score >= threshold:
            category = cat  # type: ignore
            break

    # ── Distance to Liquidation ───────────────────────────────────────────────
    dist = pos.distance_to_liquidation_pct()

    # ── Time to Liquidation ───────────────────────────────────────────────────
    daily_vol = _vol_for(pos.collateral_asset) / (365 ** 0.5)
    time_est = (
        _time_to_liq_str(pos.health_factor, daily_vol)
        if pos.health_factor is not None
        else None
    )

    # ── Recommended Actions ───────────────────────────────────────────────────
    actions = [ACTION_MAP[category]]
    if pos.health_factor and pos.health_factor < 1.5 and pos.debt_usd:
        target_hf = 1.6
        # How much to repay to reach target HF?
        # HF = collateral_usd / (debt_usd * lltv_inv)
        lltv = pos.lltv or 0.8
        target_debt = pos.collateral_usd * lltv / target_hf
        repay_usd = max(0, (pos.debt_usd or 0) - target_debt)
        if repay_usd > 0:
            actions.append(
                f"Repay ${repay_usd:,.0f} of {pos.debt_asset} to reach HF {target_hf:.1f}."
            )
    if pos.position_type == "lp" and not pos.extra.get("in_range", True):
        actions.append("LP position is out of range — consider recentering or withdrawing.")

    return RiskScore(
        score=score,
        category=category,
        distance_to_liq_pct=round(dist, 2) if dist is not None else None,
        time_to_liq_estimate=time_est,
        recommended_actions=actions,
        score_breakdown=breakdown,
    )


# ──────────────────────────────────────────────
# Report Assembly
# ──────────────────────────────────────────────

@dataclass
class RiskReport:
    report_id: str
    timestamp: str
    wallet: str
    portfolio_total_usd: float
    portfolio_risk_score: int
    portfolio_risk_category: RiskCategory
    positions: list[ScoredPosition]
    actions_taken: list[dict] = field(default_factory=list)
    next_poll: Optional[str] = None

    def to_dict(self) -> dict:
        d = asdict(self)
        d["positions"] = [p.to_dict() for p in self.positions]
        return d

    def to_json(self, indent: int = 2) -> str:
        return json.dumps(self.to_dict(), indent=indent)

    def critical_positions(self) -> list[ScoredPosition]:
        return [p for p in self.positions if p.risk.category in ("critical", "emergency")]

    def summary_text(self) -> str:
        """Human-readable summary for notifications."""
        emoji = {
            "safe":      "✅",
            "watch":     "👀",
            "warning":   "⚠️",
            "critical":  "🚨",
            "emergency": "💀",
        }
        lines = [
            f"🛡️ DeFi Risk Guardian — Snapshot Report",
            f"Portfolio Risk Score: {self.portfolio_risk_score}/100 ({self.portfolio_risk_category.capitalize()})",
            f"Portfolio Value: ${self.portfolio_total_usd:,.0f}",
            "",
        ]
        for sp in self.positions:
            p, r = sp.position, sp.risk
            icon = emoji.get(r.category, "•")
            hf_str = f"HF={p.health_factor:.2f}" if p.health_factor else "LP/Staking"
            liq_str = f"  Liq@${p.liquidation_price:,.0f} ({r.distance_to_liq_pct:.1f}% buffer)" if r.distance_to_liq_pct else ""
            lines.append(f"{icon} [{p.protocol.upper()}] {p.collateral_asset}  ${p.collateral_usd:,.0f}  {hf_str}  Score:{r.score}/100{liq_str}")
            for action in r.recommended_actions[1:]:  # Skip generic first action
                lines.append(f"   → {action}")
        if not self.positions:
            lines.append("No open positions found.")
        return "\n".join(lines)


def build_report(
    wallet: str,
    positions: list[Position],
    actions_taken: list[dict] | None = None,
    poll_interval_seconds: int = 60,
) -> RiskReport:
    """Build a full RiskReport from a list of scanned positions."""
    portfolio_total = sum(p.collateral_usd for p in positions)

    scored = [
        ScoredPosition(position=p, risk=score_position(p, portfolio_total))
        for p in positions
    ]

    # Portfolio-level score: weighted average by position size
    if scored:
        total_w = sum(sp.position.collateral_usd for sp in scored)
        if total_w > 0:
            portfolio_score = int(
                sum(sp.risk.score * sp.position.collateral_usd for sp in scored) / total_w
            )
        else:
            portfolio_score = 0
    else:
        portfolio_score = 0

    portfolio_cat: RiskCategory = "safe"
    for threshold, cat in CATEGORY_THRESHOLDS:
        if portfolio_score >= threshold:
            portfolio_cat = cat  # type: ignore
            break

    ts = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    next_poll = time.strftime(
        "%Y-%m-%dT%H:%M:%SZ",
        time.gmtime(time.time() + poll_interval_seconds)
    )

    report_id_src = f"{wallet}-{ts}"
    report_id = "rg-" + hashlib.md5(report_id_src.encode()).hexdigest()[:8]

    return RiskReport(
        report_id=report_id,
        timestamp=ts,
        wallet=wallet,
        portfolio_total_usd=round(portfolio_total, 2),
        portfolio_risk_score=portfolio_score,
        portfolio_risk_category=portfolio_cat,
        positions=scored,
        actions_taken=actions_taken or [],
        next_poll=next_poll,
    )


# ──────────────────────────────────────────────
# CLI
# ──────────────────────────────────────────────

if __name__ == "__main__":
    import argparse
    import asyncio
    from scanner import scan_all

    parser = argparse.ArgumentParser(description="DeFi Risk Guardian — Risk Engine")
    parser.add_argument("--wallet", required=True)
    parser.add_argument("--protocols", nargs="*")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    positions = asyncio.run(scan_all(args.wallet, args.protocols))
    report = build_report(args.wallet, positions)

    if args.json:
        print(report.to_json())
    else:
        print(report.summary_text())
