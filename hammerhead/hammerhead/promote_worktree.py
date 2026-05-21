"""Worktree / promotion-bookkeeping glue for the match harness.

Handles environment-variable / config queries for TT sizing, worker
count resolution, and command-line decoration.  No game logic, no
subprocess-bot protocol.

Public surface
--------------
- ``max_tt_mb_per_worker`` — per-engine TT cap from env or config.
- ``with_tt_bound`` — append ``--tt-size-mb`` to a bot command.
- ``resolve_worker_count`` — resolve 0-auto worker count, cap at n_games.
"""

from __future__ import annotations

import os

from .config import CONFIG


def max_tt_mb_per_worker() -> int:
    """Per-engine TT cap in MB: ``MAX_TT_MB_PER_WORKER`` env var, else the
    ``[bench.vs]`` config default. Bounds resident memory under a wide
    process pool (2 engines/game × N workers) — see SPEC_BENCHMARKS
    § Parallel match harness."""
    env = os.environ.get("MAX_TT_MB_PER_WORKER")
    if env:
        try:
            return max(1, int(env))
        except ValueError:
            pass
    return CONFIG.bench.vs.max_tt_mb_per_worker


def with_tt_bound(cmd: list[str], max_mb: int) -> list[str]:
    """Append ``--tt-size-mb max_mb`` to a ``hammerhead bot`` command
    unless the caller already pinned the TT size."""
    if "--tt-size-mb" in cmd:
        return list(cmd)
    return [*cmd, "--tt-size-mb", str(max_mb)]


def resolve_worker_count(n_workers: int, n_games: int) -> int:
    """Resolve ``n_workers`` (0 = auto: ``cpu_count() - 2``), capped at
    ``n_games`` — more workers than games is wasted process startup."""
    if n_workers <= 0:
        n_workers = max(1, (os.cpu_count() or 2) - 2)
    return max(1, min(n_workers, n_games))
