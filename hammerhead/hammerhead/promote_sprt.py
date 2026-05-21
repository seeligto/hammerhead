"""Pure-math statistics for the promotion harness.

No subprocess, no IO, no config access — only standard-library math.

Public surface
--------------
- ``wilson_interval`` — Wilson score confidence interval.
- ``elo_to_winrate`` / ``winrate_to_elo`` — logistic Elo conversion.
- ``sprt_llr`` — Bernoulli SPRT log-likelihood ratio.
- ``sprt_thresholds`` — Wald acceptance bounds for a MatchConfig.
"""

from __future__ import annotations

import math
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .promote import MatchConfig


def wilson_interval(wins: float, n: int, z: float = 1.96) -> tuple[float, float]:
    """Wilson score interval for a Bernoulli proportion.

    ``wins`` may be fractional — draws are counted as half-wins in the
    promote harness, so we accept floats.
    """
    if n <= 0:
        return (0.0, 1.0)
    p = wins / n
    z2 = z * z
    denom = 1.0 + z2 / n
    center = (p + z2 / (2.0 * n)) / denom
    half = z * math.sqrt(p * (1.0 - p) / n + z2 / (4.0 * n * n)) / denom
    return (max(0.0, center - half), min(1.0, center + half))


def elo_to_winrate(elo: float) -> float:
    """Standard logistic Elo → expected score."""
    return 1.0 / (1.0 + math.pow(10.0, -elo / 400.0))


def winrate_to_elo(winrate: float) -> float:
    """Inverse: expected score → Elo difference."""
    if winrate <= 0.0:
        return float("-inf")
    if winrate >= 1.0:
        return float("inf")
    return -400.0 * math.log10(1.0 / winrate - 1.0)


def sprt_llr(
    wins: int,
    draws: int,
    losses: int,
    *,
    elo_low: float,
    elo_high: float,
) -> float:
    """Bernoulli SPRT log-likelihood ratio.

    Each game contributes two Bernoulli trials, with score ∈ {0, 0.5, 1}:
        win  → 2 successes out of 2
        draw → 1 success  out of 2
        loss → 0 successes out of 2
    The trial-level success probability is ``elo_to_winrate(elo)``.
    """
    p0 = elo_to_winrate(elo_low)
    p1 = elo_to_winrate(elo_high)
    # Clamp to avoid log(0) when the elo is far enough out to saturate.
    eps = 1e-12
    p0 = min(max(p0, eps), 1.0 - eps)
    p1 = min(max(p1, eps), 1.0 - eps)
    successes = 2 * wins + draws
    trials = 2 * (wins + draws + losses)
    failures = trials - successes
    return successes * math.log(p1 / p0) + failures * math.log((1.0 - p1) / (1.0 - p0))


def sprt_thresholds(cfg: "MatchConfig") -> tuple[float, float]:
    """Wald acceptance bounds ``(log_low, log_high)`` for the given config."""
    log_high = math.log((1.0 - cfg.sprt_beta) / cfg.sprt_alpha)
    log_low = math.log(cfg.sprt_beta / (1.0 - cfg.sprt_alpha))
    return log_low, log_high
