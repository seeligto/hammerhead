"""Phase 28B-1 — coordinate-descent sweep driver.

Resurrected (Phase 20 deleted the legacy variant in commit ``cd72504``)
and rebuilt against the 14-scalar :class:`EvalOverrides` surface that
B-1.1 added. The dispatcher (the operator running the Phase 28B
sprint) drives candidates through three independent stages — A
(endpoint pre-screen), B (5-cell screen), C (single-cell validation) —
and decides on its own whether to advance.

Architectural contract
----------------------
- This module is a NEW *consumer* of the Phase 17 parallel match pool
  in :mod:`hammerhead.promote`. It does NOT replace or wrap it.
- Per-cell A/B matches are run against the engine compiled from the
  current ``hexo.toml``. The candidate engine carries the cell's
  override via the ``HEXO_EVAL_OVERRIDES`` environment variable
  honoured by :func:`hammerhead.cli.cmd_bot`; the baseline engine
  inherits the parent's env with the variable unset, which is
  byte-identical to never having called ``set_eval_overrides``.
- Opening diversity is **forced OFF** for the full Phase 28B sprint
  per Phase 28A.5 (A-5). Even though tune.py is a new harness
  consumer, it does not flip the bit.
- Per-stage statistics are **Wilson** (point + CI). SPRT lives in
  the promote harness; this driver wants a point estimate to rank
  cells, not a binary verdict.
- Output is atomic per-cell JSON (write-tmp + os.rename), so a
  killed sweep is trivially resumable.

The dispatcher decides everything else: it interprets
``STAGE_1_ZERO_DETECTED`` and ``STAGE_2_STRADDLE_DETECTED`` markers
in the emitted JSON and the stdout summary, and it chooses whether
to launch the next candidate. tune.py only signals.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional

from . import promote as promote_mod
from .config import CONFIG

# ─────────────────────────────────────────────────────────────────────────────
# Constants
# ─────────────────────────────────────────────────────────────────────────────

# Schema version for tune-cell JSON output. Bump on any breaking shape
# change; SPEC_BENCHMARKS § Output schema convention.
TUNE_SCHEMA_VERSION = 1

# Default worker count. NOT cpu_count()-2. The host budget for the
# Phase 28B sprint is 10 — set explicitly by the prompt.
DEFAULT_N_WORKERS = 10

# Per-stage default game counts. Stage A pre-screen and Stage B screen
# both run at 200g; Stage C validation runs at 400g. Override with
# ``--games`` on the command line.
STAGE_DEFAULT_GAMES = {
    "A": 200,
    "B": 200,
    "C": 400,
}

# Smoke band — overrides the per-stage default whenever ``--smoke`` is
# passed. Five games per cell is enough to confirm the harness wiring
# and produce a JSON output; the result Elo is meaningless.
SMOKE_GAMES_PER_CELL = 5

# Stage 1 promote-to-Stage-2 gate (per plan § D commit B-2.1).
STAGE_1_PROMOTE_WILSON_UPPER_MIN = 0.0
STAGE_1_PROMOTE_CENTRE_MIN_ELO = -10.0

# Recognised tunable parameter names. Map name → kind:
#   "scalar"   — a single int override.
#   "window_k" — index into the ``window_k_scores`` array.
_SCALAR_PARAMS: tuple[str, ...] = (
    "open_5",
    "closed_5",
    "open_4",
    "closed_4",
    "open_extension_factor",
    "closed_extension_factor",
    "fork_cover2_bonus",
)
_WINDOW_K_PREFIX = "window_k_scores["
_WINDOW_K_SUFFIX = "]"


# ─────────────────────────────────────────────────────────────────────────────
# Param-name parsing + overrides dict construction
# ─────────────────────────────────────────────────────────────────────────────


def _parse_param_name(name: str) -> tuple[str, Optional[int]]:
    """Return ``(kind, index)`` for a sweep param name.

    Examples:
        ``"open_4"`` → ``("open_4", None)``
        ``"window_k_scores[5]"`` → ``("window_k_scores", 5)``
    """
    if name in _SCALAR_PARAMS:
        return name, None
    if name.startswith(_WINDOW_K_PREFIX) and name.endswith(_WINDOW_K_SUFFIX):
        body = name[len(_WINDOW_K_PREFIX) : -len(_WINDOW_K_SUFFIX)]
        try:
            idx = int(body)
        except ValueError as exc:
            raise ValueError(
                f"invalid window_k_scores index in {name!r}: {body!r}"
            ) from exc
        if not 0 <= idx <= 6:
            raise ValueError(
                f"window_k_scores index out of range in {name!r}: {idx}"
            )
        return "window_k_scores", idx
    raise ValueError(
        f"unknown tunable param {name!r}; recognised: "
        f"{sorted(_SCALAR_PARAMS)} or window_k_scores[0..6]"
    )


def baseline_value_for(name: str) -> int:
    """Return the current ``hexo.toml`` default for ``name``.

    Used both as the candidate baseline (against which Wilson Elo is
    reported) and as the embedded ``baseline_value`` field in the per-cell
    JSON output for reproducibility tracking.
    """
    kind, idx = _parse_param_name(name)
    e = CONFIG.eval
    if kind == "window_k_scores":
        assert idx is not None
        return int(e.window_k_scores[idx])
    return int(getattr(e, kind))


def build_overrides_for(name: str, value: int) -> dict[str, Any]:
    """Build the override dict that pins ``name`` to ``value``.

    For window_k slots the override dict carries the *full* 7-element
    array (the Rust setter validates length-7), with the target slot
    replaced and all other slots reset to the codegen'd defaults.
    """
    kind, idx = _parse_param_name(name)
    if kind == "window_k_scores":
        assert idx is not None
        arr = list(CONFIG.eval.window_k_scores)
        arr[idx] = int(value)
        return {"window_k_scores": arr}
    return {kind: int(value)}


# ─────────────────────────────────────────────────────────────────────────────
# Grid parsing
# ─────────────────────────────────────────────────────────────────────────────


def parse_grid(spec: str) -> list[int]:
    """Parse a ``V1,V2,...`` integer grid.

    Empty entries are rejected. Returns the integers in input order
    (the order matters: Stage A uses ``grid[0]`` and ``grid[-1]`` as
    endpoints; Stage B sweeps every entry; Stage C expects exactly one).
    """
    if not spec.strip():
        raise ValueError("grid is empty")
    out: list[int] = []
    for tok in spec.split(","):
        tok = tok.strip()
        if not tok:
            raise ValueError(f"empty grid entry in {spec!r}")
        try:
            out.append(int(tok))
        except ValueError as exc:
            raise ValueError(f"bad grid entry {tok!r}: {exc}") from exc
    return out


def select_cells_for_stage(stage: str, grid: list[int]) -> list[int]:
    """Return the cell values to sweep for the given stage.

    - Stage A (pre-screen): ``[grid[0], grid[-1]]`` (de-duplicated).
    - Stage B (screen): the full grid as supplied.
    - Stage C (validate): the grid (must be a single value).
    """
    if stage == "A":
        if len(grid) < 2:
            raise ValueError(
                "stage A pre-screen needs at least 2 grid endpoints, "
                f"got {grid}"
            )
        cells = [grid[0], grid[-1]]
        # De-dupe in case the caller passed grid[0] == grid[-1].
        seen: list[int] = []
        for v in cells:
            if v not in seen:
                seen.append(v)
        return seen
    if stage == "B":
        if len(grid) < 2:
            raise ValueError(
                "stage B screen needs at least 2 grid points, "
                f"got {grid}"
            )
        return list(grid)
    if stage == "C":
        if len(grid) != 1:
            raise ValueError(
                "stage C validation expects exactly 1 grid value "
                f"(the Stage-1 winner), got {grid}"
            )
        return list(grid)
    raise ValueError(f"unknown stage {stage!r}; want A | B | C")


# ─────────────────────────────────────────────────────────────────────────────
# Engine command construction
# ─────────────────────────────────────────────────────────────────────────────


def _bot_cmd_for(python_exe: Path, tt_mb: int) -> list[str]:
    """``hammerhead bot`` command vector for the in-tree Python.

    tune.py runs against the engine compiled from the current source
    tree on both sides; we do NOT shell out to a worktree (.bestref is
    promote.py's domain). The two engines therefore share the same
    Python and the same .so — what differs is the ``HEXO_EVAL_OVERRIDES``
    env var per worker.
    """
    return [
        str(python_exe),
        "-m",
        "hammerhead.cli",
        "bot",
        "--tt-size-mb",
        str(tt_mb),
    ]


# ─────────────────────────────────────────────────────────────────────────────
# One-cell match driver
# ─────────────────────────────────────────────────────────────────────────────


@dataclasses.dataclass(frozen=True, slots=True)
class CellResult:
    """One A/B match outcome for a single (param, value) cell."""

    param: str
    cell_value: int
    baseline_value: int
    games_played: int
    wins: int  # current (override) wins
    losses: int  # baseline wins
    draws: int
    winrate: float
    wilson_lower: float
    wilson_upper: float
    elo: float
    ci_lower_elo: float
    ci_upper_elo: float
    workers: int
    time_ms_per_stone: int


def _now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _git_sha() -> str:
    """``git rev-parse --short HEAD`` from the repo root; fail-safe."""
    repo_root = CONFIG.source_path.parent
    try:
        out = subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=repo_root,
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
        return out or "unknown"
    except Exception:  # noqa: BLE001
        return "unknown"


def run_one_cell(
    param: str,
    cell_value: int,
    *,
    n_games: int,
    time_ms_per_stone: int,
    n_workers: int,
    max_plies: int,
) -> CellResult:
    """Run one A/B match: candidate (override applied) vs baseline.

    Reuses :func:`hammerhead.promote.run_match_parallel` — we do NOT
    re-implement the worker pool. The candidate engine gets
    ``HEXO_EVAL_OVERRIDES`` set in its environment via the standard
    ``env`` parameter of the subprocess spawn; the baseline engine
    inherits the parent's env (with the override unset, which equals
    the hexo.toml default — byte-identical to no setter call).
    """
    overrides = build_overrides_for(param, cell_value)
    baseline_value = baseline_value_for(param)

    venv_python = Path(sys.executable)
    tt_mb = promote_mod.max_tt_mb_per_worker()
    cmd_base = _bot_cmd_for(venv_python, tt_mb)
    current_cmd = list(cmd_base)
    best_cmd = list(cmd_base)

    # Build a Wilson-only MatchConfig. opening_diversity is FORCED off
    # per Phase 28A.5 (A-5); tune.py MUST NOT enable it even though
    # it is a new consumer of the harness pool. color_balance follows
    # the hexo.toml default (true) — A-5 only locks diversity.
    cfg = promote_mod.MatchConfig(
        n_games=n_games,
        time_ms_per_stone=time_ms_per_stone,
        test="wilson",
        sprt_elo_low=CONFIG.promote.sprt_elo_low,
        sprt_elo_high=CONFIG.promote.sprt_elo_high,
        sprt_alpha=CONFIG.promote.sprt_alpha,
        sprt_beta=CONFIG.promote.sprt_beta,
        wilson_min_lower=CONFIG.promote.wilson_min_lower,
        raw_min_winrate=CONFIG.promote.raw_min_winrate,
        color_balance=CONFIG.promote.color_balance,
        opening_diversity=False,
        max_plies=max_plies,
    )

    # The override has to land in the *worker's* environment, not the
    # coordinator's, so we patch os.environ around the pool spawn. The
    # promote harness uses spawn-context Pool with no env passthrough
    # hook, but the spawn ctx forks the parent's current environment
    # at worker-create time. We restore on the way out.
    prev = os.environ.get("HEXO_EVAL_OVERRIDES")
    os.environ["HEXO_EVAL_OVERRIDES"] = json.dumps(overrides)
    try:
        # NB: this means both `current_cmd` and `best_cmd` inherit the
        # same env. That would normally apply the override to both
        # sides, defeating the A/B. We compensate by giving the BEST
        # engine an explicit empty override via the workers' command
        # line — see _wrap_baseline_with_clear below.
        best_cmd = _wrap_baseline_clear_overrides(best_cmd)
        res = promote_mod.run_match_parallel(
            current_cmd, best_cmd, cfg, n_workers=n_workers
        )
    finally:
        if prev is None:
            os.environ.pop("HEXO_EVAL_OVERRIDES", None)
        else:
            os.environ["HEXO_EVAL_OVERRIDES"] = prev

    elo_lo, elo_hi = res.estimated_elo_ci
    return CellResult(
        param=param,
        cell_value=cell_value,
        baseline_value=baseline_value,
        games_played=res.games_played,
        wins=res.current_wins,
        losses=res.best_wins,
        draws=res.draws,
        winrate=res.winrate,
        wilson_lower=res.wilson_lower,
        wilson_upper=res.wilson_upper,
        elo=res.estimated_elo,
        ci_lower_elo=elo_lo,
        ci_upper_elo=elo_hi,
        workers=promote_mod.resolve_worker_count(n_workers, n_games),
        time_ms_per_stone=time_ms_per_stone,
    )


# ─────────────────────────────────────────────────────────────────────────────
# Baseline-side override clearing
# ─────────────────────────────────────────────────────────────────────────────


# The promote harness's Pool initializer broadcasts the current
# environment to every worker. We rely on that to deliver the candidate
# override. The baseline side must explicitly NEUTRALISE the variable
# at the subprocess level so the parent's HEXO_EVAL_OVERRIDES doesn't
# leak into it. We do that by prefixing the baseline command with a
# wrapper that unsets the env var. The result is byte-identical to
# never having set the var on that bot.

# `env -u VAR` is portable POSIX (coreutils env, BusyBox env). We
# prefer it over a Python wrapper to keep the spawn overhead identical
# on both sides.
def _wrap_baseline_clear_overrides(cmd: list[str]) -> list[str]:
    """Return ``cmd`` prefixed with ``env -u HEXO_EVAL_OVERRIDES`` so the
    baseline subprocess sees the variable unset regardless of what the
    parent has in its environment."""
    return ["env", "-u", "HEXO_EVAL_OVERRIDES", *cmd]


# ─────────────────────────────────────────────────────────────────────────────
# Cell JSON output (atomic per-cell write)
# ─────────────────────────────────────────────────────────────────────────────


def _cell_json_path(
    out_dir: Path,
    param: str,
    stage: str,
    cell_value: int,
    isodate: str,
) -> Path:
    """Build the canonical per-cell JSON path.

    Layout (per plan § C "Commit B-1.2"):

        <out>/<param>/<stage>/<isodate>/<cell_value>.json

    All directory levels are created if missing.
    """
    safe_param = param.replace("[", "_").replace("]", "")
    leaf = out_dir / safe_param / stage / isodate
    leaf.mkdir(parents=True, exist_ok=True)
    return leaf / f"{cell_value}.json"


def _atomic_write_json(path: Path, payload: dict[str, Any]) -> None:
    """Write JSON atomically: write to ``<path>.tmp`` then ``os.rename``.

    The rename is atomic on POSIX. A killed sweep therefore leaves
    either a complete JSON or no JSON, never a half-written file.
    """
    tmp = path.with_suffix(path.suffix + ".tmp")
    with tmp.open("w", encoding="utf-8") as fh:
        json.dump(payload, fh, indent=2, sort_keys=True)
        fh.write("\n")
    os.replace(tmp, path)


def cell_result_to_json(
    res: CellResult,
    *,
    stage: str,
    started_at: str,
    finished_at: str,
    smoke: bool,
) -> dict[str, Any]:
    """Build the serialisable dict for one cell result."""
    return {
        "schema_version": TUNE_SCHEMA_VERSION,
        "param": res.param,
        "stage": stage,
        "cell_value": res.cell_value,
        "baseline_value": res.baseline_value,
        "games_played": res.games_played,
        "wins": res.wins,
        "losses": res.losses,
        "draws": res.draws,
        "winrate": res.winrate,
        "wilson_lower": res.wilson_lower,
        "wilson_upper": res.wilson_upper,
        "elo": res.elo,
        "ci_lower": res.ci_lower_elo,
        "ci_upper": res.ci_upper_elo,
        "ci_method": "wilson",
        "workers": res.workers,
        "time_ms_per_stone": res.time_ms_per_stone,
        "color_balance": CONFIG.promote.color_balance,
        "opening_diversity": False,
        "host": socket.gethostname(),
        "git_sha": _git_sha(),
        "started_at": started_at,
        "finished_at": finished_at,
        "smoke": smoke,
    }


# ─────────────────────────────────────────────────────────────────────────────
# Stage-level stopping-rule markers
# ─────────────────────────────────────────────────────────────────────────────


def stage_1_zero_marker(cells: list[CellResult]) -> bool:
    """True iff ZERO Stage-B cells clear the promote-to-Stage-2 gate.

    Gate per plan § D: ``Wilson upper > 0 Elo AND centre ≥ -10 Elo``.
    Reported via JSON sentinel + stdout so the dispatcher can pause
    the sprint per Phase 28A.5 stop-loss rules.
    """
    for c in cells:
        if (
            c.ci_upper_elo > STAGE_1_PROMOTE_WILSON_UPPER_MIN
            and c.elo >= STAGE_1_PROMOTE_CENTRE_MIN_ELO
        ):
            return False
    return True


def stage_2_straddle_marker(cell: CellResult) -> bool:
    """True iff the Stage-C validation CI straddles zero Elo.

    Trigger for the Phase 26.5-precedent sprint terminator: signal-
    below-floor risk. The dispatcher tracks cumulative straddles
    across candidates and stops the sprint after 3 consecutive.
    """
    return cell.ci_lower_elo < 0 < cell.ci_upper_elo


# ─────────────────────────────────────────────────────────────────────────────
# Stage drivers
# ─────────────────────────────────────────────────────────────────────────────


@dataclasses.dataclass(frozen=True, slots=True)
class SweepArgs:
    """Resolved arguments after CLI parsing + defaulting."""

    stage: str  # "A" | "B" | "C"
    param: str
    grid: list[int]
    games: int
    time_ms_per_stone: int
    workers: int
    max_plies: int
    out_dir: Path
    smoke: bool


def run_stage(args: SweepArgs) -> list[Path]:
    """Run the full single-stage sweep; return the list of written
    cell JSON paths in cell order.

    A single invocation runs ONE stage of ONE candidate — A, B, or C.
    Per plan, the dispatcher is responsible for chaining stages.
    """
    cells = select_cells_for_stage(args.stage, args.grid)
    isodate = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S")
    paths: list[Path] = []
    results: list[CellResult] = []

    print(
        f"tune-sweep: param={args.param} stage={args.stage} "
        f"cells={cells} games/cell={args.games} workers={args.workers} "
        f"time_ms/stone={args.time_ms_per_stone} smoke={args.smoke}",
        flush=True,
    )

    for value in cells:
        started = _now_iso()
        t0 = time.monotonic()
        res = run_one_cell(
            args.param,
            value,
            n_games=args.games,
            time_ms_per_stone=args.time_ms_per_stone,
            n_workers=args.workers,
            max_plies=args.max_plies,
        )
        finished = _now_iso()
        wall = time.monotonic() - t0
        results.append(res)

        path = _cell_json_path(
            args.out_dir, args.param, args.stage, value, isodate
        )
        payload = cell_result_to_json(
            res,
            stage=args.stage,
            started_at=started,
            finished_at=finished,
            smoke=args.smoke,
        )
        _atomic_write_json(path, payload)
        paths.append(path)

        print(
            f"  cell {args.param}={value}: "
            f"W-L-D {res.wins}-{res.losses}-{res.draws}  "
            f"elo {res.elo:+.1f} CI [{res.ci_lower_elo:+.1f}, "
            f"{res.ci_upper_elo:+.1f}]  ({wall:.1f}s)  → {path}",
            flush=True,
        )

    # Stopping-rule markers (printed; the dispatcher also reads them
    # off the JSON via the schema fields).
    if args.stage == "B" and stage_1_zero_marker(results):
        print(
            "STAGE_1_ZERO_DETECTED: no cell cleared the Stage-2 promote "
            "gate (Wilson upper > 0 AND centre >= -10 Elo). The "
            "dispatcher tracks cumulative count across candidates.",
            flush=True,
        )
    if args.stage == "C" and results and stage_2_straddle_marker(results[0]):
        print(
            "STAGE_2_STRADDLE_DETECTED: validation CI straddles zero "
            f"({results[0].ci_lower_elo:+.1f} < 0 < "
            f"{results[0].ci_upper_elo:+.1f}). The dispatcher tracks "
            "cumulative count across candidates.",
            flush=True,
        )

    return paths


# ─────────────────────────────────────────────────────────────────────────────
# CLI surface
# ─────────────────────────────────────────────────────────────────────────────


def add_tune_sweep_args(p: argparse.ArgumentParser) -> None:
    """Wire the ``hammerhead bench tune-sweep`` argparse surface."""
    p.add_argument(
        "--stage",
        required=True,
        choices=("A", "B", "C"),
        help="A=endpoint pre-screen, B=Stage-1 screen (full grid), "
        "C=Stage-2 validation (single winner)",
    )
    p.add_argument(
        "--param",
        required=True,
        help="tunable name, e.g. open_4 or window_k_scores[5]",
    )
    p.add_argument(
        "--grid",
        required=True,
        help="comma-separated integer values; Stage A uses endpoints, "
        "Stage B sweeps all, Stage C expects a single value",
    )
    p.add_argument(
        "--games",
        type=int,
        default=None,
        help="games per cell (default: 200 for A/B, 400 for C; "
        "overridden by --smoke)",
    )
    p.add_argument(
        "--time-ms",
        type=int,
        default=CONFIG.promote.default_time_ms_per_stone // 2,
        help=f"per-stone time budget in ms (default: "
        f"{CONFIG.promote.default_time_ms_per_stone // 2})",
    )
    p.add_argument(
        "--workers",
        type=int,
        default=DEFAULT_N_WORKERS,
        help=f"parallel match workers (default: {DEFAULT_N_WORKERS} "
        f"= host budget per Phase 28B prompt)",
    )
    p.add_argument(
        "--max-plies",
        type=int,
        default=CONFIG.promote.default_max_plies,
        help=f"max plies per game (default: "
        f"{CONFIG.promote.default_max_plies})",
    )
    p.add_argument(
        "--out",
        required=True,
        help="output root directory (per-cell JSONs land at "
        "<out>/<param>/<stage>/<isodate>/<value>.json)",
    )
    p.add_argument(
        "--smoke",
        action="store_true",
        help=(
            f"wiring-verification run at {SMOKE_GAMES_PER_CELL} games/cell; "
            "result Elo is meaningless. Output lands under a "
            "tune/smoke/... subtree to keep it trivially identifiable."
        ),
    )


def resolve_args(ns: argparse.Namespace) -> SweepArgs:
    """Normalise the argparse namespace into a :class:`SweepArgs`."""
    grid = parse_grid(ns.grid)
    games = ns.games
    if ns.smoke:
        games = SMOKE_GAMES_PER_CELL
    elif games is None:
        games = STAGE_DEFAULT_GAMES[ns.stage]
    if games < 1:
        raise ValueError(f"--games must be >= 1, got {games}")

    out_dir = Path(ns.out).expanduser().resolve()
    if ns.smoke:
        # Smoke must NEVER write under the canonical baseline.json
        # subtree. Force a `smoke/` segment so accidental --out
        # pointed at benches/results/ can't shadow real data.
        out_dir = out_dir / "tune" / "smoke"
    out_dir.mkdir(parents=True, exist_ok=True)

    return SweepArgs(
        stage=ns.stage,
        param=ns.param,
        grid=grid,
        games=games,
        time_ms_per_stone=int(ns.time_ms),
        workers=int(ns.workers),
        max_plies=int(ns.max_plies),
        out_dir=out_dir,
        smoke=bool(ns.smoke),
    )


def cmd_tune_sweep(ns: argparse.Namespace) -> int:
    """``hammerhead bench tune-sweep`` entry point."""
    try:
        args = resolve_args(ns)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    paths = run_stage(args)
    print(
        f"\ntune-sweep done: wrote {len(paths)} cell JSON file(s) "
        f"under {args.out_dir}",
        flush=True,
    )
    return 0
