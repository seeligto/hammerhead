"""Phase 18 — S1/S2 eval-weight tuning sweep.

Coordinate-descent + local pairwise A/B. Each *cell* A/Bs a candidate
S1/S2 shape-weight vector against a fixed baseline (Phase 17 weights,
all zero) over ``games`` self-play games, colour-balanced, and reports
a Wilson 95% interval on the candidate's score.

Weights are applied at runtime via ``Engine.set_eval_shape_weights`` —
no rebuild per cell. See ``subagents/scans/phase18-tuning-methodology.md``.

The sweep is **deterministic** (per-cell seed derived from
``(seed_base, stage, shape, alpha, cell_index)``) and **resumable**
(cells are written to the output JSON as they finish; relaunching with
the same ``--out`` skips cells already recorded).
"""

from __future__ import annotations

import hashlib
import json
import multiprocessing
import os
import random
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path

# Shape order — identical to `ThreatCounts` / `ShapeWeights` / hexo.toml.
SHAPE_NAMES: tuple[str, ...] = (
    "open_3",
    "rhombus",
    "arch",
    "bone",
    "trapezoid",
    "open_2",
    "closed_3",
    "triangle",
)
N_SHAPES = len(SHAPE_NAMES)
ZERO_WEIGHTS: tuple[int, ...] = (0,) * N_SHAPES

# Default seed base — overridable so an independent re-run can be
# requested without colliding openings.
DEFAULT_SEED_BASE = 0x9E37_79B9_7F4A_7C15


# ─────────────────────────────────────────────────────────────────────────────
# Seed derivation
# ─────────────────────────────────────────────────────────────────────────────


def cell_seed(seed_base: int, stage: str, shape: str, alpha: float, cell_index: int) -> int:
    """Deterministic 64-bit seed for one sweep cell.

    Re-running a cell with the same ``(seed_base, stage, shape, alpha,
    cell_index)`` yields the same per-game openings and hence the same
    ``(W, L, D)``.
    """
    key = f"{seed_base:#x}|{stage}|{shape}|{alpha:.6f}|{cell_index}"
    digest = hashlib.sha256(key.encode("utf-8")).digest()
    return int.from_bytes(digest[:8], "big")


# ─────────────────────────────────────────────────────────────────────────────
# Cell / result records
# ─────────────────────────────────────────────────────────────────────────────


@dataclass(frozen=True, slots=True)
class TuneCell:
    """One sweep cell — a candidate weight vector to A/B vs baseline."""

    label: str
    shape: str
    alpha: float
    weights: tuple[int, ...]
    seed: int

    def __post_init__(self) -> None:
        if len(self.weights) != N_SHAPES:
            raise ValueError(f"weights must have {N_SHAPES} entries, got {len(self.weights)}")


@dataclass(frozen=True, slots=True)
class CellResult:
    """A/B outcome for one cell — candidate's point of view."""

    label: str
    shape: str
    alpha: float
    weights: list[int]
    games: int
    time_ms: int
    wins: int
    losses: int
    draws: int
    score: float
    winrate: float
    wilson_lb: float
    wilson_ub: float
    seed: int


# ─────────────────────────────────────────────────────────────────────────────
# Cell construction
# ─────────────────────────────────────────────────────────────────────────────


def coordinate_descent_cells(
    shapes: list[str],
    alphas: list[float],
    anchors: dict[str, int],
    *,
    stage: str = "B",
    seed_base: int = DEFAULT_SEED_BASE,
) -> list[TuneCell]:
    """Stage B grid: one cell per ``(shape, alpha)``.

    All other shapes are held at 0. ``weight = round(alpha * anchor)``.
    """
    unknown = [s for s in shapes if s not in SHAPE_NAMES]
    if unknown:
        raise ValueError(f"unknown shapes: {unknown}")
    missing = [s for s in shapes if s not in anchors]
    if missing:
        raise ValueError(f"no Layer 1 anchor supplied for: {missing}")

    cells: list[TuneCell] = []
    idx = 0
    for shape in shapes:
        pos = SHAPE_NAMES.index(shape)
        for alpha in alphas:
            weights = list(ZERO_WEIGHTS)
            weights[pos] = round(alpha * anchors[shape])
            cells.append(
                TuneCell(
                    label=f"{shape}@{alpha:g}",
                    shape=shape,
                    alpha=alpha,
                    weights=tuple(weights),
                    seed=cell_seed(seed_base, stage, shape, alpha, idx),
                )
            )
            idx += 1
    return cells


def vector_cell(
    label: str,
    weights: tuple[int, ...],
    *,
    stage: str = "C",
    seed_base: int = DEFAULT_SEED_BASE,
    cell_index: int = 0,
) -> TuneCell:
    """An explicit-vector cell (Stage C / D — combined weight vectors)."""
    if len(weights) != N_SHAPES:
        raise ValueError(f"weights must have {N_SHAPES} entries, got {len(weights)}")
    return TuneCell(
        label=label,
        shape="combined",
        alpha=-1.0,
        weights=tuple(weights),
        seed=cell_seed(seed_base, stage, label, -1.0, cell_index),
    )


# ─────────────────────────────────────────────────────────────────────────────
# Game worker
# ─────────────────────────────────────────────────────────────────────────────


@dataclass(frozen=True, slots=True)
class _TuneGameConfig:
    """One A/B game's deterministic assignment."""

    cand_is_x: bool
    weights: tuple[int, ...]
    baseline_weights: tuple[int, ...]
    time_per_stone_ms: int
    max_plies: int
    opening_plies: int
    seed: int


def _run_tune_game(gc: _TuneGameConfig) -> str:
    """Worker entry — one A/B game. Returns ``"win"`` / ``"loss"`` /
    ``"draw"`` from the *candidate* engine's point of view."""
    # Imported here so the spawn-context worker picks them up cleanly.
    from hammerhead.benchmark import _new_engine, _play_stone, _random_opening

    ex = _new_engine()
    eo = _new_engine()
    if not hasattr(ex, "set_eval_shape_weights"):
        raise RuntimeError(
            "engine built without the eval_s1s2 feature — rebuild with "
            "the default feature set to run the tuning sweep"
        )
    # Candidate plays X iff cand_is_x; the other seat is the baseline.
    cand, base = (ex, eo) if gc.cand_is_x else (eo, ex)
    cand.set_eval_shape_weights(list(gc.weights))
    base.set_eval_shape_weights(list(gc.baseline_weights))

    rng = random.Random(gc.seed)
    plies = _random_opening(ex, eo, rng, gc.opening_plies)
    while plies < gc.max_plies:
        if ex.winner() is not None or eo.winner() is not None:
            break
        side = ex.to_move()
        active, mirror = (ex, eo) if side == 0 else (eo, ex)
        _play_stone(active, mirror, gc.time_per_stone_ms)
        plies += 1
        if active.winner() is not None or mirror.winner() is not None:
            break
        if active.halfmove() == 1 and plies < gc.max_plies:
            _play_stone(active, mirror, gc.time_per_stone_ms)
            plies += 1

    winner = ex.winner()  # 0 = X win, 1 = O win, None = draw / cap
    if winner is None:
        return "draw"
    return "win" if (winner == 0) == gc.cand_is_x else "loss"


# ─────────────────────────────────────────────────────────────────────────────
# Sweep driver
# ─────────────────────────────────────────────────────────────────────────────


def _game_configs(
    cell: TuneCell,
    games: int,
    time_ms: int,
    baseline_weights: tuple[int, ...],
    max_plies: int,
    opening_plies: int,
) -> list[_TuneGameConfig]:
    """Per-game configs for one cell. Per-game seeds are drawn up front
    from the cell seed, so the game set is reproducible regardless of
    worker count or completion order. Colours alternate per game."""
    master = random.Random(cell.seed)
    return [
        _TuneGameConfig(
            cand_is_x=(g % 2 == 0),
            weights=cell.weights,
            baseline_weights=baseline_weights,
            time_per_stone_ms=time_ms,
            max_plies=max_plies,
            opening_plies=opening_plies,
            seed=master.getrandbits(64),
        )
        for g in range(games)
    ]


def run_cell(
    cell: TuneCell,
    *,
    games: int,
    time_ms: int,
    n_workers: int,
    baseline_weights: tuple[int, ...] = ZERO_WEIGHTS,
    max_plies: int = 200,
    opening_plies: int = 4,
) -> CellResult:
    """Run one cell — ``games`` A/B games across a process pool."""
    if games < 1:
        raise ValueError("games must be >= 1")
    from hammerhead.promote import wilson_interval

    if n_workers <= 0:
        n_workers = max(1, (os.cpu_count() or 2) - 2)
    n_workers = max(1, min(n_workers, games))

    configs = _game_configs(cell, games, time_ms, baseline_weights, max_plies, opening_plies)

    wins = losses = draws = 0
    ctx = multiprocessing.get_context("spawn")
    with ctx.Pool(processes=n_workers) as pool:
        for outcome in pool.imap_unordered(_run_tune_game, configs):
            if outcome == "win":
                wins += 1
            elif outcome == "loss":
                losses += 1
            else:
                draws += 1

    score = wins + 0.5 * draws
    winrate = score / games
    lb, ub = wilson_interval(score, games)
    return CellResult(
        label=cell.label,
        shape=cell.shape,
        alpha=cell.alpha,
        weights=list(cell.weights),
        games=games,
        time_ms=time_ms,
        wins=wins,
        losses=losses,
        draws=draws,
        score=score,
        winrate=winrate,
        wilson_lb=lb,
        wilson_ub=ub,
        seed=cell.seed,
    )


@dataclass
class SweepState:
    """In-memory mirror of the resumable output JSON."""

    meta: dict = field(default_factory=dict)
    cells: list[dict] = field(default_factory=list)

    @property
    def done_labels(self) -> set[str]:
        return {c["label"] for c in self.cells}


def _load_state(out_path: Path) -> SweepState:
    """Load a partial sweep for resume, or a fresh state."""
    if not out_path.exists():
        return SweepState()
    try:
        data = json.loads(out_path.read_text())
    except (json.JSONDecodeError, OSError):
        return SweepState()
    return SweepState(meta=data.get("meta", {}), cells=list(data.get("cells", [])))


def _write_state(out_path: Path, state: SweepState) -> None:
    """Atomically persist the sweep — temp file + rename, so a crash
    mid-write never corrupts completed cells."""
    out_path.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps({"meta": state.meta, "cells": state.cells}, indent=2)
    tmp = out_path.with_suffix(out_path.suffix + ".tmp")
    tmp.write_text(payload)
    os.replace(tmp, out_path)


def run_tune_sweep(
    cells: list[TuneCell],
    *,
    games: int,
    time_ms: int,
    n_workers: int,
    out_path: Path,
    stage: str = "B",
    baseline_weights: tuple[int, ...] = ZERO_WEIGHTS,
    max_plies: int = 200,
    opening_plies: int = 4,
    progress: bool = True,
) -> list[dict]:
    """Run a sweep over ``cells``, writing each result to ``out_path`` as
    it completes. Resumable: cells already present in ``out_path`` are
    skipped. Returns the full cell-result list (existing + new)."""
    # Fail fast, before spawning a pool, if the engine lacks the runtime
    # weight override (built without the eval_s1s2 feature).
    from hammerhead_engine import Engine

    if not hasattr(Engine, "set_eval_shape_weights"):
        raise RuntimeError(
            "engine built without the eval_s1s2 feature — rebuild with "
            "the default feature set to run the tuning sweep"
        )

    out_path = Path(out_path)
    state = _load_state(out_path)
    state.meta = {
        "stage": stage,
        "games_per_cell": games,
        "time_ms": time_ms,
        "baseline_weights": list(baseline_weights),
        "n_cells": len(cells),
        "shape_order": list(SHAPE_NAMES),
    }

    done = state.done_labels
    pending = [c for c in cells if c.label not in done]
    if progress:
        skipped = len(cells) - len(pending)
        total_games = len(pending) * games
        print(
            f"tune sweep [stage {stage}]: {len(pending)} cells pending "
            f"({skipped} already done), {games} games/cell, "
            f"{total_games} games total",
            flush=True,
        )

    started = time.monotonic()
    for done_count, cell in enumerate(pending, start=1):
        result = run_cell(
            cell,
            games=games,
            time_ms=time_ms,
            n_workers=n_workers,
            baseline_weights=baseline_weights,
            max_plies=max_plies,
            opening_plies=opening_plies,
        )
        state.cells.append(asdict(result))
        _write_state(out_path, state)
        if progress:
            elapsed = time.monotonic() - started
            per_cell = elapsed / done_count
            eta = per_cell * (len(pending) - done_count)
            print(
                f"[{result.shape}] alpha={result.alpha:g}: "
                f"{result.wins}-{result.losses}-{result.draws} (W-L-D)  "
                f"wilson [{result.wilson_lb:.3f}, {result.wilson_ub:.3f}]  "
                f"({done_count}/{len(pending)} cells, ETA {eta / 60:.1f} min)",
                flush=True,
            )

    return state.cells
