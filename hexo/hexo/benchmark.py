"""Macro-level benchmark library.

End-to-end engine metrics: nodes-per-second at a time budget,
depth-at-time, threat-detection latency, self-play throughput.

Each ``bench_*`` function returns a frozen dataclass; ``run_all``
orchestrates the full sweep and returns a dict suitable for the
``macro`` key of the canonical bench JSON.

Fixtures load from ``benches/fixtures/positions.json`` — same file the
Rust criterion benches read at compile time, so micro and macro layers
operate on identical positions.
"""

from __future__ import annotations

import json
import time
from dataclasses import asdict, dataclass
from functools import lru_cache
from pathlib import Path
from typing import Optional

from hexo.bot import Bot, BotConfig
from hexo.config import CONFIG
from hexo_engine import Engine


# ─────────────────────────────────────────────────────────────────────────────
# Result dataclasses (frozen → safe to embed in canonical JSON)
# ─────────────────────────────────────────────────────────────────────────────


@dataclass(frozen=True, slots=True)
class NpsResult:
    fixture: str
    time_ms: int
    depth_reached: int
    nodes: int
    nps: float


@dataclass(frozen=True, slots=True)
class DepthAtTimeResult:
    fixture: str
    time_ms: int
    depth_reached: int


@dataclass(frozen=True, slots=True)
class ThreatLatencyResult:
    fixture: str
    cold_us: float
    warm_us: float
    samples: int


@dataclass(frozen=True, slots=True)
class SelfplayThroughputResult:
    games: int
    plies_total: int
    wall_seconds: float
    plies_per_sec: float
    time_per_stone_ms: int


# ─────────────────────────────────────────────────────────────────────────────
# Fixture loader — single source of truth shared with the criterion side
# ─────────────────────────────────────────────────────────────────────────────


def _fixtures_path() -> Path:
    """Resolve the fixtures path. Relative paths in [bench] are anchored to
    the directory containing ``hexo.toml`` (workspace root), not the cwd."""
    p = Path(CONFIG.bench.fixtures_path)
    if p.is_absolute():
        return p
    return CONFIG.source_path.parent / p


@lru_cache(maxsize=1)
def _load_fixtures_all() -> dict:
    """Parse ``positions.json`` once per process; subsequent calls reuse."""
    return json.loads(_fixtures_path().read_text())


def _load_fixture_moves(name: str) -> list[tuple[int, int]]:
    data = _load_fixtures_all()
    if name not in data:
        raise KeyError(f"unknown fixture: {name}")
    return [(int(q), int(r)) for q, r in data[name]["moves"]]


def load_fixture(name: str, tt_size_mb: Optional[int] = None) -> Engine:
    """Build a fresh :class:`hexo_engine.Engine` from the named JSON fixture.

    Move colours follow standard HeXO turn parity, so the fixture must
    encode a legal sequence (ply 0 = X at origin, etc).
    """
    eng = Engine(tt_size_mb=tt_size_mb or CONFIG.tt.default_size_mb)
    for q, r in _load_fixture_moves(name):
        eng.place((q, r))
    return eng


# ─────────────────────────────────────────────────────────────────────────────
# Individual macro benches
# ─────────────────────────────────────────────────────────────────────────────


def bench_nps(
    fixture: str,
    time_ms: int,
    runs: int = 3,
    tt_size_mb: Optional[int] = None,
) -> NpsResult:
    """Average NPS over ``runs`` searches on the named fixture.

    Each run rebuilds the engine to clear TT, so the result reflects a
    cold-cache search — matches what a fresh game start would see.
    """
    if runs < 1:
        raise ValueError("runs must be >= 1")
    total_nodes = 0
    total_time_ms = 0
    last_depth = 0
    for _ in range(runs):
        eng = load_fixture(fixture, tt_size_mb=tt_size_mb)
        _q, _r, _score, depth_reached, nodes, t_ms = eng.bench_best_move(
            time_ms=time_ms
        )
        total_nodes += int(nodes)
        total_time_ms += int(t_ms)
        last_depth = int(depth_reached)
    avg_time_ms = total_time_ms / runs
    nps = (total_nodes / runs) / max(avg_time_ms / 1000.0, 1e-9)
    return NpsResult(
        fixture=fixture,
        time_ms=time_ms,
        depth_reached=last_depth,
        nodes=total_nodes // runs,
        nps=nps,
    )


def bench_depth_at_time(
    fixture: str,
    time_ms: int,
    tt_size_mb: Optional[int] = None,
) -> DepthAtTimeResult:
    """Deepest completed iteration within ``time_ms``."""
    eng = load_fixture(fixture, tt_size_mb=tt_size_mb)
    _q, _r, _score, depth_reached, _nodes, _t = eng.bench_best_move(
        time_ms=time_ms
    )
    return DepthAtTimeResult(
        fixture=fixture,
        time_ms=time_ms,
        depth_reached=int(depth_reached),
    )


def bench_threat_latency(
    fixture: str,
    n_calls: int = 1000,
    tt_size_mb: Optional[int] = None,
) -> ThreatLatencyResult:
    """Cold vs warm cached-eval latency.

    Cold = first ``cached_eval`` after invalidating the cache via a
    place+undo round-trip; this forces a fresh threat + eval recompute.
    Warm = subsequent ``cached_eval`` calls on the same unchanged board.

    Cached_eval is dominated by the threat-set recompute for non-trivial
    fixtures, so this also approximates threat-detection latency — the
    cleanest engine-level proxy available without piercing the PyO3
    surface.
    """
    if n_calls < 1:
        raise ValueError("n_calls must be >= 1")
    eng = load_fixture(fixture, tt_size_mb=tt_size_mb)
    # Pre-warm once so the first cold reading isn't fighting JIT-like
    # first-touch effects (allocator, page faults, etc).
    eng.cached_eval()

    # Pick the invalidation target once. Scanning candidates per
    # iteration would dwarf the cached_eval cost we're trying to measure.
    target = _legal_invalidation_target(eng)
    cold_total = 0.0
    warm_total = 0.0
    if target is None:
        # Empty board: cached_eval is trivially cheap and place+undo
        # cycle has nowhere to invalidate. Time warm twice for a useful
        # nonzero result; cold = warm under this regime.
        for _ in range(n_calls):
            t0 = time.perf_counter_ns()
            eng.cached_eval()
            cold_total += time.perf_counter_ns() - t0
            t0 = time.perf_counter_ns()
            eng.cached_eval()
            warm_total += time.perf_counter_ns() - t0
    else:
        for _ in range(n_calls):
            eng.place(target)
            eng.undo()
            t0 = time.perf_counter_ns()
            eng.cached_eval()
            cold_total += time.perf_counter_ns() - t0
            t0 = time.perf_counter_ns()
            eng.cached_eval()
            warm_total += time.perf_counter_ns() - t0
    return ThreatLatencyResult(
        fixture=fixture,
        cold_us=cold_total / n_calls / 1000.0,
        warm_us=warm_total / n_calls / 1000.0,
        samples=n_calls,
    )


def _legal_invalidation_target(eng: Engine) -> Optional[tuple[int, int]]:
    """Pick an empty cell on `eng` legal for the next placement.

    Walks a small spiral of candidates around origin; returns the first
    cell that ``place`` accepts without raising. Returns ``None`` only on
    pathological boards where no such cell exists in the radius (the
    empty-board case after origin is placed).
    """
    for dq in range(-8, 9):
        for dr in range(-8, 9):
            if dq == 0 and dr == 0:
                continue
            try:
                eng.place((dq, dr))
            except Exception:
                continue
            eng.undo()
            return (dq, dr)
    return None


def bench_selfplay(
    time_per_stone_ms: int,
    games: int = 5,
    max_plies: int = 200,
) -> SelfplayThroughputResult:
    """Run ``games`` complete self-play matches; report plies/sec.

    Each game uses two fresh :class:`Bot` instances (X and O). Game ends
    on a winner or when ``max_plies`` is reached.
    """
    if games < 1:
        raise ValueError("games must be >= 1")
    plies_total = 0
    t0 = time.perf_counter()
    for _ in range(games):
        plies_total += _run_one_game(time_per_stone_ms, max_plies)
    wall = time.perf_counter() - t0
    pps = plies_total / wall if wall > 0 else 0.0
    return SelfplayThroughputResult(
        games=games,
        plies_total=plies_total,
        wall_seconds=wall,
        plies_per_sec=pps,
        time_per_stone_ms=time_per_stone_ms,
    )


def _run_one_game(time_per_stone_ms: int, max_plies: int) -> int:
    bx = Bot(BotConfig(time_per_move_ms=time_per_stone_ms))
    bo = Bot(BotConfig(time_per_move_ms=time_per_stone_ms))
    plies = 0
    while plies < max_plies:
        if bx.winner() is not None or bo.winner() is not None:
            break
        side = bx.to_move()
        active, mirror = (bx, bo) if side == 0 else (bo, bx)
        m = active.play_stone()
        mirror.observe(m)
        plies += 1
        if active.winner() is not None or mirror.winner() is not None:
            break
        if active.halfmove() == 1 and plies < max_plies:
            m = active.play_stone()
            mirror.observe(m)
            plies += 1
    return plies


# ─────────────────────────────────────────────────────────────────────────────
# Top-level orchestrator
# ─────────────────────────────────────────────────────────────────────────────


DEFAULT_FIXTURES: tuple[str, ...] = (
    "empty",
    "single_origin",
    "midgame_12",
    "midgame_30",
    "endgame_60",
)


def run_all(
    time_ms: int,
    fixtures: Optional[list[str]] = None,
    threat_samples: Optional[int] = None,
    selfplay_games: Optional[int] = None,
    selfplay_max_plies: Optional[int] = None,
) -> dict:
    """Run every macro bench across the standard fixture set.

    Returns a dict suitable for the ``macro`` key of the canonical JSON.
    """
    fx_list = list(fixtures) if fixtures else list(DEFAULT_FIXTURES)
    threat_n = threat_samples or 64
    sp_games = selfplay_games or CONFIG.bench.default_games
    sp_max_plies = selfplay_max_plies or CONFIG.bench.default_max_plies
    # Self-play with the full time_ms is too slow at large budgets;
    # cap it at a low fraction so `bench all --time-ms 1000` finishes
    # in a reasonable wall-clock window.
    sp_time_per_stone_ms = max(20, time_ms // 4)

    nps = [
        asdict(bench_nps(fixture=name, time_ms=time_ms, runs=1))
        for name in fx_list
    ]
    depth_at_time = [
        asdict(bench_depth_at_time(fixture=name, time_ms=time_ms))
        for name in fx_list
    ]
    threat_latency = [
        asdict(bench_threat_latency(fixture=name, n_calls=threat_n))
        for name in fx_list
    ]
    selfplay = asdict(
        bench_selfplay(
            time_per_stone_ms=sp_time_per_stone_ms,
            games=sp_games,
            max_plies=sp_max_plies,
        )
    )

    return {
        "nps": nps,
        "depth_at_time": depth_at_time,
        "threat_latency": threat_latency,
        "selfplay_throughput": [selfplay],
    }


# ─────────────────────────────────────────────────────────────────────────────
# Match / promotion stubs — Phase 11
# ─────────────────────────────────────────────────────────────────────────────


def match(bot_a, bot_b, max_plies: int = 200):  # noqa: D401
    """Stub: full match harness lives in the Phase 11 promotion module."""
    raise NotImplementedError("Phase 11 — see specs/SPEC_ROADMAP.md § Phase 11")


def vs_sealbot(bot, num_games: int):
    """Stub: SealBot interop is post-baseline."""
    raise NotImplementedError("Phase 11+ — out of scope")
