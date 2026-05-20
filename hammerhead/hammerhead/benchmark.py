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
import random
import statistics
import time
from dataclasses import asdict, dataclass
from functools import lru_cache
from pathlib import Path
from typing import Optional

from hammerhead.bot import Bot, BotConfig
from hammerhead.config import CONFIG
from hammerhead_engine import Engine


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
class QuickResult:
    """One ``bench-quick`` / ``bench-perf`` cell. See
    ``specs/SPEC_BENCHMARKS.md`` § Bench tiers."""

    fixture: str
    time_ms: int
    nps_mean: float
    nps_stddev: float
    cycles_per_node_mean: float
    depth_reached: int
    runs: int


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


@dataclass(frozen=True, slots=True)
class ReferenceEntry:
    """One row in the reference node-count table. See
    ``specs/SPEC_BENCHMARKS.md`` § Reference node-counts."""

    fixture: str
    depth: int
    nodes: int
    ms: int
    tt_hit_rate: Optional[float] = None


@dataclass(frozen=True, slots=True)
class ScalingEntry:
    """One row in the ms-time scaling table. See
    ``specs/SPEC_BENCHMARKS.md`` § ms-time scaling."""

    fixture: str
    time_ms: int
    depth: int
    nodes: int
    nps: int
    ci95_lo: int
    ci95_hi: int


@dataclass(frozen=True, slots=True)
class BreakdownEntry:
    """One row in the per-function cycles breakdown. See
    ``specs/SPEC_BENCHMARKS.md`` § Per-function cycles breakdown."""

    fixture: str
    depth: int
    function: str
    pct_cycles: float


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
    """Build a fresh :class:`hammerhead_engine.Engine` from the named JSON fixture.

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


# ─────────────────────────────────────────────────────────────────────────────
# cycles/node — Phase 16
# ─────────────────────────────────────────────────────────────────────────────


def detect_cpu_ghz() -> float:
    """Current CPU clock in GHz, read from ``/proc/cpuinfo``.

    Falls back to ``4.0`` when the file is unavailable (non-Linux, or a
    sandboxed environment). The value is only used to scale the
    ``cycles/node`` metric, which is a relative trend signal — an
    inexact clock shifts every reading by the same factor.
    """
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("cpu MHz"):
                    return float(line.split(":")[1].strip()) / 1000.0
    except OSError:
        pass
    return 4.0


def cycles_per_node(
    nodes: int, time_s: float, cpu_ghz: float | None = None
) -> float:
    """Estimated CPU cycles spent per search node.

    ``(cpu_ghz * 1e9 * time_s) / nodes``. More sensitive than NPS for
    inner-loop work: NPS can lift from a depth shift while per-node cost
    regresses, but cycles/node is monotonic in per-node work. Returns
    ``inf`` when ``nodes == 0`` (no division by zero).
    """
    ghz = cpu_ghz if cpu_ghz is not None else detect_cpu_ghz()
    if nodes == 0:
        return float("inf")
    return (ghz * 1e9 * time_s) / nodes


# ─────────────────────────────────────────────────────────────────────────────
# Tiered bench — quick (inner loop) + perf (pre-commit). Phase 16.
# ─────────────────────────────────────────────────────────────────────────────


def bench_quick(
    fixture: str = "midgame_12",
    time_ms: int = 500,
    runs: int = 3,
    tt_size_mb: Optional[int] = None,
) -> QuickResult:
    """Single-fixture, multi-run NPS+depth+cycles/node check.

    Each run rebuilds the engine (cold TT). Aggregates: NPS mean /
    stddev, mean cycles/node, median depth reached. The inner-loop
    feedback tier — completes in ~5-15 s at the default 500 ms budget.
    """
    if runs < 1:
        raise ValueError("runs must be >= 1")
    cpu_ghz = detect_cpu_ghz()
    nps_values: list[float] = []
    cpn_values: list[float] = []
    depths: list[int] = []
    for _ in range(runs):
        eng = load_fixture(fixture, tt_size_mb=tt_size_mb)
        _q, _r, _score, depth, nodes, t_ms = eng.bench_best_move(
            time_ms=time_ms
        )
        nodes = int(nodes)
        elapsed_s = max(int(t_ms), 1) / 1000.0
        nps_values.append(nodes / elapsed_s)
        cpn_values.append(cycles_per_node(nodes, elapsed_s, cpu_ghz))
        depths.append(int(depth))
    return QuickResult(
        fixture=fixture,
        time_ms=time_ms,
        nps_mean=statistics.mean(nps_values),
        nps_stddev=statistics.stdev(nps_values) if runs > 1 else 0.0,
        cycles_per_node_mean=statistics.mean(cpn_values),
        depth_reached=int(statistics.median(depths)),
        runs=runs,
    )


def bench_perf(
    fixtures: Optional[list[str]] = None,
    time_ms_buckets: Optional[list[int]] = None,
    runs: Optional[int] = None,
    tt_size_mb: Optional[int] = None,
) -> list[QuickResult]:
    """Two-fixture × multi-budget NPS+cycles/node sweep.

    The pre-commit tier: one :class:`QuickResult` per
    ``(fixture, time_ms)`` cell. Defaults come from ``[bench.perf]``.
    """
    fx = fixtures if fixtures is not None else list(CONFIG.bench.perf.fixtures)
    budgets = (
        time_ms_buckets
        if time_ms_buckets is not None
        else list(CONFIG.bench.perf.time_ms)
    )
    n = runs if runs is not None else CONFIG.bench.perf.runs
    out: list[QuickResult] = []
    for fixture in fx:
        for budget in budgets:
            out.append(
                bench_quick(
                    fixture=fixture,
                    time_ms=budget,
                    runs=n,
                    tt_size_mb=tt_size_mb,
                )
            )
    return out


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
# Layer 2 S1/S2 ablation A/B — Phase 16
# ─────────────────────────────────────────────────────────────────────────────


@dataclass(frozen=True, slots=True)
class AblationResult:
    """Outcome of a Layer 2 S1/S2 ablation self-play A/B. See
    ``specs/SPEC_EVAL.md`` § Layer 2 ablation."""

    games: int
    time_per_stone_ms: int
    opening_plies: int
    s1s2_wins: int
    s1s2_losses: int
    draws: int
    s1s2_winrate: float
    wilson_lo: float
    wilson_hi: float
    verdict: str


def _random_opening(bx: Bot, bo: Bot, rng: random.Random, opening_plies: int) -> int:
    """Play up to ``opening_plies`` random legal moves, mirrored to both
    bots. Returns the count actually played. Distinct openings make the
    otherwise-deterministic engines produce distinct games."""
    played = 0
    for _ in range(opening_plies):
        if bx.winner() is not None:
            break
        move: Optional[tuple[int, int]] = None
        for _ in range(200):
            cand = (rng.randint(-6, 6), rng.randint(-6, 6))
            try:
                bx.observe(cand)
            except Exception:
                continue
            move = cand
            break
        if move is None:
            break
        bo.observe(move)
        played += 1
    return played


def _run_ablation_game(
    time_per_stone_ms: int,
    max_plies: int,
    s1s2_is_x: bool,
    opening_plies: int,
    rng: random.Random,
) -> Optional[int]:
    """One ablation game. The S1/S2-enabled engine plays X iff
    ``s1s2_is_x``. Returns 0 (X win), 1 (O win), or None (draw / cap)."""
    bx = Bot(BotConfig(time_per_move_ms=time_per_stone_ms))
    bo = Bot(BotConfig(time_per_move_ms=time_per_stone_ms))
    if not hasattr(bx.engine, "set_eval_s1s2"):
        raise RuntimeError(
            "engine built without the eval_s1s2 feature — rebuild with "
            "the default feature set to run the ablation A/B"
        )
    bx.engine.set_eval_s1s2(s1s2_is_x)
    bo.engine.set_eval_s1s2(not s1s2_is_x)

    plies = _random_opening(bx, bo, rng, opening_plies)
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
    return bx.winner()


def bench_ablation(
    games: int = 50,
    time_per_stone_ms: int = 500,
    max_plies: int = 200,
    opening_plies: int = 4,
    seed: int = 0xAB1A_7104,
) -> AblationResult:
    """Self-play A/B: S1/S2-enabled eval vs S1/S2-disabled eval.

    Colors alternate per game; each game starts from a distinct seeded
    random opening (the engines are otherwise deterministic). Reports
    the S1/S2-enabled side's winrate — draws count ½ — with a Wilson
    95% CI. **Data collection only**: the keep/drop decision is
    deferred to Phase 17. See ``specs/SPEC_EVAL.md`` § Layer 2 ablation.
    """
    if games < 1:
        raise ValueError("games must be >= 1")
    from hammerhead.promote import wilson_interval

    rng = random.Random(seed)
    s1s2_wins = 0
    s1s2_losses = 0
    draws = 0
    for g in range(games):
        s1s2_is_x = g % 2 == 0
        winner = _run_ablation_game(
            time_per_stone_ms, max_plies, s1s2_is_x, opening_plies, rng
        )
        if winner is None:
            draws += 1
        elif (winner == 0) == s1s2_is_x:
            s1s2_wins += 1
        else:
            s1s2_losses += 1
    score = s1s2_wins + 0.5 * draws
    winrate = score / games
    lo, hi = wilson_interval(score, games)
    if lo > 0.5:
        verdict = "KEEP"
    elif hi < 0.5:
        verdict = "DROP"
    else:
        verdict = "INCONCLUSIVE"
    return AblationResult(
        games=games,
        time_per_stone_ms=time_per_stone_ms,
        opening_plies=opening_plies,
        s1s2_wins=s1s2_wins,
        s1s2_losses=s1s2_losses,
        draws=draws,
        s1s2_winrate=winrate,
        wilson_lo=lo,
        wilson_hi=hi,
        verdict=verdict,
    )


# ─────────────────────────────────────────────────────────────────────────────
# Reference node-count table — deterministic, fixed-depth search
# ─────────────────────────────────────────────────────────────────────────────


def bench_reference(
    fixtures: list[str],
    max_depth: int,
    budget_s: float,
    use_tt_stats: bool = False,
    tt_size_mb: Optional[int] = None,
) -> list[ReferenceEntry]:
    """Fixed-depth searches across ``fixtures × 1..=max_depth``.

    Deterministic by construction: a fresh :class:`Engine` is built per
    ``(fixture, depth)`` row so TT, killers, and history all start at
    defaults. Within a fixture, accumulated wall-clock time is checked
    after each depth; once it exceeds ``budget_s`` the remaining depths
    for that fixture are skipped (other fixtures still run).

    ``use_tt_stats`` reads ``Engine.tt_stats()`` after each search and
    sets ``tt_hit_rate = hits / probes`` when ``probes > 0``. The
    counter columns are only populated when the engine was built with
    Cargo feature ``tt_stats``; otherwise the rate is ``None`` (zero
    probes → no hit-rate signal).
    """
    if max_depth < 1:
        raise ValueError("max_depth must be >= 1")
    if budget_s <= 0:
        raise ValueError("budget_s must be > 0")
    out: list[ReferenceEntry] = []
    for fixture in fixtures:
        elapsed = 0.0
        for depth in range(1, max_depth + 1):
            if elapsed > budget_s:
                break
            eng = load_fixture(fixture, tt_size_mb=tt_size_mb)
            _q, _r, _score, _depth_reached, nodes, t_ms = eng.bench_best_move(
                depth=depth
            )
            hit_rate: Optional[float] = None
            if use_tt_stats:
                s = eng.tt_stats()
                if s["probes"] > 0:
                    hit_rate = s["hits"] / s["probes"]
            out.append(
                ReferenceEntry(
                    fixture=fixture,
                    depth=depth,
                    nodes=int(nodes),
                    ms=int(t_ms),
                    tt_hit_rate=hit_rate,
                )
            )
            elapsed += t_ms / 1000.0
    return out


# ─────────────────────────────────────────────────────────────────────────────
# ms-time scaling table — Phase 14
# ─────────────────────────────────────────────────────────────────────────────


def bench_scaling(
    fixtures: list[str],
    time_ms_buckets: list[int],
    runs: int,
    tt_size_mb: Optional[int] = None,
) -> list[ScalingEntry]:
    """For each (fixture, time_ms): run ``runs`` searches, take the median
    of depth / nodes / NPS, percentile-bootstrap the 95% CI on NPS.
    """
    if runs < 1:
        raise ValueError("runs must be >= 1")
    out: list[ScalingEntry] = []
    for fixture in fixtures:
        for time_ms in time_ms_buckets:
            samples_nps: list[float] = []
            samples_nodes: list[int] = []
            samples_depth: list[int] = []
            for _ in range(runs):
                eng = load_fixture(fixture, tt_size_mb=tt_size_mb)
                _q, _r, _s, depth, nodes, t_ms = eng.bench_best_move(
                    time_ms=time_ms
                )
                actual_s = max(int(t_ms), 1) / 1000.0
                samples_nps.append(int(nodes) / actual_s)
                samples_nodes.append(int(nodes))
                samples_depth.append(int(depth))
            samples_nps.sort()
            mid = len(samples_nps) // 2
            median_nps = samples_nps[mid]
            ci_lo = samples_nps[0]
            ci_hi = samples_nps[-1]
            samples_nodes.sort()
            samples_depth.sort()
            out.append(
                ScalingEntry(
                    fixture=fixture,
                    time_ms=time_ms,
                    depth=samples_depth[mid],
                    nodes=samples_nodes[mid],
                    nps=int(median_nps),
                    ci95_lo=int(ci_lo),
                    ci95_hi=int(ci_hi),
                )
            )
    return out


# ─────────────────────────────────────────────────────────────────────────────
# Per-function cycles breakdown — Phase 14
# ─────────────────────────────────────────────────────────────────────────────


_BREAKDOWN_PREFIX_TO_FUNCTION: dict[str, str] = {
    "eval": "eval",
    "threats": "threats",
    "moves": "moves",
    "ordering": "ordering",
    "tt": "tt",
    "axis_bitmap": "eval",  # axis_bitmap micros feed the layer-1 window scan
    "board": "search_other",
    "search": "search_other",
}


def _function_for_group(group: str) -> str:
    """Map a criterion group name to a breakdown bucket. Group names look
    like ``threats::compute`` or ``eval::cached_eval_cold``; the leading
    token before ``::`` is the module."""
    head = group.split("::", 1)[0]
    return _BREAKDOWN_PREFIX_TO_FUNCTION.get(head, "search_other")


def _latest_micro_payload() -> Optional[dict]:
    """Return the most-recent canonical JSON in ``benches/results/``, if
    any. Used by :func:`bench_breakdown` to combine search timings with
    criterion micro medians.
    """
    repo_root = CONFIG.source_path.parent
    results_dir = repo_root / CONFIG.bench.results_dir
    if not results_dir.is_dir():
        return None
    candidates = sorted(
        (p for p in results_dir.glob("*.json") if p.name != "baseline.json"),
        reverse=True,
    )
    for path in candidates:
        try:
            payload = json.loads(path.read_text())
        except (OSError, json.JSONDecodeError):
            continue
        if "micro" in payload:
            return payload
    # Fall back to the committed baseline if no live run is on disk.
    baseline = results_dir / "baseline.json"
    if baseline.is_file():
        try:
            return json.loads(baseline.read_text())
        except (OSError, json.JSONDecodeError):
            return None
    return None


_BREAKDOWN_BUCKETS: tuple[str, ...] = (
    "eval",
    "threats",
    "moves",
    "ordering",
    "tt",
    "search_other",
)


def bench_breakdown(
    fixtures: list[str],
    depth: int,
    tt_size_mb: Optional[int] = None,
) -> list[BreakdownEntry]:
    """Estimate the share of search cycles spent in each top-level module.

    Runs a fixed-depth search per fixture to anchor the totals to real
    engine work, then attributes time to categories using the medians of
    matching criterion micro benches. The result is a stable trend
    metric, not a true profile — use :command:`make flamegraph` when you
    need ground truth.
    """
    if depth < 1:
        raise ValueError("depth must be >= 1")
    micro = _latest_micro_payload()
    out: list[BreakdownEntry] = []
    for fixture in fixtures:
        eng = load_fixture(fixture, tt_size_mb=tt_size_mb)
        _q, _r, _s, _depth_reached, _nodes, _t_ms = eng.bench_best_move(
            depth=depth
        )
        per_bucket: dict[str, float] = {b: 0.0 for b in _BREAKDOWN_BUCKETS}
        if micro is not None:
            for entry in micro.get("micro", []):
                if entry.get("name") != fixture:
                    continue
                bucket = _function_for_group(entry.get("group", ""))
                per_bucket[bucket] = per_bucket.get(bucket, 0.0) + float(
                    entry.get("median_ns", 0.0)
                )
        total = sum(per_bucket.values())
        rows: list[BreakdownEntry] = []
        if total > 0.0:
            attributed = 0.0
            for bucket in _BREAKDOWN_BUCKETS:
                if bucket == "search_other":
                    continue
                pct = per_bucket[bucket] / total * 100.0
                attributed += pct
                rows.append(
                    BreakdownEntry(
                        fixture=fixture,
                        depth=depth,
                        function=bucket,
                        pct_cycles=pct,
                    )
                )
            rows.append(
                BreakdownEntry(
                    fixture=fixture,
                    depth=depth,
                    function="search_other",
                    pct_cycles=max(0.0, 100.0 - attributed),
                )
            )
        else:
            for bucket in _BREAKDOWN_BUCKETS:
                rows.append(
                    BreakdownEntry(
                        fixture=fixture,
                        depth=depth,
                        function=bucket,
                        pct_cycles=0.0,
                    )
                )
        out.extend(rows)
    return out


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
    use_tt_stats: bool = False,
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

    ref_cfg = CONFIG.bench.reference
    reference = [
        asdict(e)
        for e in bench_reference(
            fixtures=list(ref_cfg.fixtures),
            max_depth=ref_cfg.max_depth,
            budget_s=float(ref_cfg.budget_s),
            use_tt_stats=use_tt_stats,
        )
    ]

    sc_cfg = CONFIG.bench.scaling
    scaling = [
        asdict(e)
        for e in bench_scaling(
            fixtures=list(sc_cfg.fixtures),
            time_ms_buckets=list(sc_cfg.time_ms),
            runs=sc_cfg.runs,
        )
    ]

    br_cfg = CONFIG.bench.breakdown
    breakdown = [
        asdict(e)
        for e in bench_breakdown(
            fixtures=list(br_cfg.fixtures),
            depth=br_cfg.depth,
        )
    ]

    return {
        "nps": nps,
        "depth_at_time": depth_at_time,
        "threat_latency": threat_latency,
        "selfplay_throughput": [selfplay],
        "reference": reference,
        "scaling": scaling,
        "breakdown": breakdown,
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
