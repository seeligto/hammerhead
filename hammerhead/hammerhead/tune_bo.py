"""Phase 28C-1 — Bayesian-optimisation sweep driver (Optuna GPSampler).

Companion to :mod:`hammerhead.tune` (coordinate-descent driver, Phase 28B-1).
Where ``tune.py`` walks a hand-rolled grid one parameter at a time,
``tune_bo.py`` runs an outer Optuna study that jointly samples a 5-D
search space via a Matérn-5/2 Gaussian-process surrogate. The GP models
parameter interactions (the missing ingredient in Phase 28B per Phase
28C-0 §7: B-2.1 × B-2.3 = −27.8 Elo) and tells us where to spend the
next match.

Architectural contract (mirrors ``tune.py``)
--------------------------------------------
- This module is a NEW consumer of the Phase 17 parallel match pool in
  :mod:`hammerhead.promote`. It does NOT replace, wrap, or modify it.
- Trial-side overrides land in the candidate engine via the
  ``HEXO_EVAL_OVERRIDES`` env var honoured by :func:`hammerhead.cli.cmd_bot`.
- The reference engine is a **fixed external worktree** built at a
  specific SHA (Phase 27 baseline ``e28d54a`` per Phase 28C-1 §4) —
  passed via ``--reference-binary``. ``tune.py`` runs in-tree on both
  sides; we differ because we want cross-phase comparability against
  the Phase 28C-0 reference, not "current vs current".
- Opening diversity is **forced OFF** per Phase 28A.5 (A-5).
  ``color_balance`` follows the ``[promote]`` default (true).
- Per-stage statistics are **Wilson** (point + CI). The Wilson CI half-
  width is stored as a ``user_attr`` on each trial for C2-DRIFT.
- Output is atomic per-trial JSON (write-tmp + os.rename) — plus the
  Optuna study SQLite for resumability.

Per design.md §3 the full sprint is 60 trials × 200g; the ``--smoke``
flag drops to 2 trials × 5g for wiring verification only.
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
from typing import Any

import optuna
from optuna.samplers import GPSampler
from optuna.trial import Trial, TrialState

from . import promote as promote_mod
from .config import CONFIG

# ─────────────────────────────────────────────────────────────────────────────
# Constants
# ─────────────────────────────────────────────────────────────────────────────

# Schema version for per-trial JSON. Bump on breaking shape change;
# SPEC_BENCHMARKS § Output schema convention.
TUNE_BO_SCHEMA_VERSION = 1

# Default worker count per Phase 28C-1 design §3 — 10 saturates the
# Ryzen 7 3700x host (CLAUDE.md verification gate). Mirrors tune.py's
# DEFAULT_N_WORKERS for cross-driver consistency.
DEFAULT_N_WORKERS = 10

# Smoke band — overrides the per-trial games budget when ``--smoke`` is
# passed. Five games per trial verifies harness wiring; the result Elo
# is meaningless.
SMOKE_GAMES_PER_TRIAL = 5
SMOKE_N_TRIALS = 2

# Production sprint defaults per design.md §3 ("budget" table).
DEFAULT_N_TRIALS = 60
DEFAULT_GAMES_PER_TRIAL = 200
DEFAULT_TIME_MS_PER_STONE = 500
DEFAULT_N_STARTUP_TRIALS = 10
DEFAULT_GP_SEED = 42

# Search space (design.md §1 "Search space (5 parameters)" table).
# Each entry: (low, high, kind, optional step).
# kind ∈ {"int", "int_log"}.
SEARCH_SPACE: dict[str, dict[str, Any]] = {
    # design.md §1 #1 — I1 §5 #1 candidate, log-uniform span 10×.
    "open_4": {"low": 24_000, "high": 240_000, "kind": "int_log"},
    # design.md §1 #2 — I1 §5 #4, linear int, step 1000 (sub-1000
    # moves below i32 noise threshold).
    "closed_5": {"low": 240_000, "high": 840_000, "kind": "int", "step": 1_000},
    # design.md §1 #3 — I1 §5 #3 + 28C-0 §7 interaction partner of
    # open_4; log-uniform span 40× (4 OOMs).
    "window_k_scores_5": {"low": 1_024, "high": 40_000, "kind": "int_log"},
    # design.md §1 #4 — I1 §5 #5, small integer (WINDOW_SCORE_8 lookup
    # makes non-integer meaningless).
    "open_extension_factor": {"low": 1, "high": 10, "kind": "int"},
    # design.md §1 #5 — I1 §5 #2, linear int, step 1000.
    "fork_cover2_bonus": {"low": 0, "high": 75_000, "kind": "int", "step": 1_000},
}

# HEAD seed values for the first enqueued trial (design.md §1 "Seed
# (HEAD)" column + §3 "Initial enqueue"). These are the post-28C-0
# revert state of hexo.toml. Anchors the GP at a known-good point
# without wasting a random-init slot.
HEAD_SEED_PARAMS: dict[str, int] = {
    "open_4": 135_000,
    "closed_5": 500_000,
    "window_k_scores_5": 4_096,
    "open_extension_factor": 4,
    "fork_cover2_bonus": 4_000,
}

# Baseline window_k_scores array (indices 0..6 from hexo.toml). The
# trial only suggests slot [5]; we splice the suggested value into
# this template before sending to ``set_eval_overrides``. Indices [0..4,
# 6] are locked per design.md §2.
WINDOW_K_BASELINE: tuple[int, ...] = (0, 1, 8, 64, 512, 4096, 1_000_000)

# Env var read by ``hammerhead.cli.cmd_bot`` — already defined in
# ``cli.py`` as ``_EVAL_OVERRIDES_ENV``; replicate here as a module-
# local constant so we don't depend on a private name.
_EVAL_OVERRIDES_ENV = "HEXO_EVAL_OVERRIDES"


# ─────────────────────────────────────────────────────────────────────────────
# Trial suggestion + EvalOverrides dict construction
# ─────────────────────────────────────────────────────────────────────────────


def suggest_params(trial: Trial) -> dict[str, int]:
    """Read trial-suggested values for all 5 params per ``SEARCH_SPACE``.

    Returns a flat dict {name: int}. The window_k_scores[5] suggestion
    lives under key ``window_k_scores_5`` (Optuna only stores scalar
    suggestions; the full array is built in :func:`build_overrides`).
    """
    out: dict[str, int] = {}
    for name, spec in SEARCH_SPACE.items():
        kind = spec["kind"]
        low = spec["low"]
        high = spec["high"]
        if kind == "int_log":
            out[name] = trial.suggest_int(name, low, high, log=True)
        elif kind == "int":
            step = spec.get("step", 1)
            out[name] = trial.suggest_int(name, low, high, step=step)
        else:
            raise ValueError(f"unknown SEARCH_SPACE kind {kind!r} for {name!r}")
    return out


def build_overrides(params: dict[str, int]) -> dict[str, Any]:
    """Build the :class:`EvalOverrides` dict from trial-suggested params.

    The ``window_k_scores[5]`` slot is spliced into the full 7-element
    baseline array; the other 6 slots are locked per design.md §2.
    Other 4 scalars pass through unchanged.
    """
    wk = list(WINDOW_K_BASELINE)
    wk[5] = int(params["window_k_scores_5"])
    return {
        "open_4": int(params["open_4"]),
        "closed_5": int(params["closed_5"]),
        "window_k_scores": wk,
        "open_extension_factor": int(params["open_extension_factor"]),
        "fork_cover2_bonus": int(params["fork_cover2_bonus"]),
    }


# ─────────────────────────────────────────────────────────────────────────────
# Engine command construction
# ─────────────────────────────────────────────────────────────────────────────


def _candidate_cmd(tt_mb: int) -> list[str]:
    """``hammerhead bot`` for the in-tree (candidate) engine.

    The candidate inherits ``HEXO_EVAL_OVERRIDES`` from its parent
    environment, which we patch around the pool spawn per trial.
    """
    return [
        str(Path(sys.executable)),
        "-m",
        "hammerhead.cli",
        "bot",
        "--tt-size-mb",
        str(tt_mb),
    ]


def _reference_cmd(reference_binary: Path, tt_mb: int) -> list[str]:
    """``hammerhead bot`` for the fixed-SHA reference worktree.

    The reference engine MUST run with the override env var unset so
    every trial races against the same baseline binary. We prepend
    ``env -u HEXO_EVAL_OVERRIDES`` (mirrors tune.py:399 pattern) — the
    portable POSIX way to neutralise the variable at subprocess level
    without a Python wrapper.
    """
    return [
        "env",
        "-u",
        _EVAL_OVERRIDES_ENV,
        str(reference_binary),
        "-m",
        "hammerhead.cli",
        "bot",
        "--tt-size-mb",
        str(tt_mb),
    ]


# ─────────────────────────────────────────────────────────────────────────────
# Trial driver + per-trial JSON output
# ─────────────────────────────────────────────────────────────────────────────


@dataclasses.dataclass(frozen=True, slots=True)
class TrialOutcome:
    """One trial's match outcome — fed to Optuna and serialised to JSON."""

    trial_number: int
    params: dict[str, int]
    games_played: int
    wins: int
    losses: int
    draws: int
    winrate: float
    wilson_lower: float
    wilson_upper: float
    elo: float
    ci_lower_elo: float
    ci_upper_elo: float
    elo_sem: float
    workers: int
    time_ms_per_stone: int
    wall_seconds: float


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


def _atomic_write_json(path: Path, payload: dict[str, Any]) -> None:
    """Write JSON atomically: ``<path>.tmp`` then ``os.rename``.

    POSIX rename is atomic; a killed sprint leaves either a complete
    JSON or no JSON, never a half-written file. Same pattern as
    tune.py:_atomic_write_json.
    """
    tmp = path.with_suffix(path.suffix + ".tmp")
    with tmp.open("w", encoding="utf-8") as fh:
        json.dump(payload, fh, indent=2, sort_keys=True)
        fh.write("\n")
    os.replace(tmp, path)


def run_one_trial(
    trial: Trial,
    *,
    reference_binary: Path,
    n_games: int,
    time_ms_per_stone: int,
    n_workers: int,
    max_plies: int,
) -> TrialOutcome:
    """Run one A/B match: candidate (override applied) vs reference binary.

    Reuses :func:`hammerhead.promote.run_match_parallel` — we do NOT
    re-implement the worker pool. The candidate engine gets
    ``HEXO_EVAL_OVERRIDES`` set in its environment via the spawn-context
    Pool's environment inheritance; the reference engine command
    explicitly clears the variable via ``env -u`` (see
    :func:`_reference_cmd`).
    """
    params = suggest_params(trial)
    overrides = build_overrides(params)

    tt_mb = promote_mod.max_tt_mb_per_worker()
    current_cmd = _candidate_cmd(tt_mb)
    best_cmd = _reference_cmd(reference_binary, tt_mb)

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

    # Patch the override into our own environment so the spawn-context
    # Pool's workers inherit it (same trick tune.py uses). The reference
    # command prefix ``env -u`` strips it before exec'ing the reference
    # binary — so both sides see exactly the right environment.
    prev = os.environ.get(_EVAL_OVERRIDES_ENV)
    os.environ[_EVAL_OVERRIDES_ENV] = json.dumps(overrides)
    t0 = time.monotonic()
    try:
        res = promote_mod.run_match_parallel(
            current_cmd, best_cmd, cfg, n_workers=n_workers
        )
    finally:
        if prev is None:
            os.environ.pop(_EVAL_OVERRIDES_ENV, None)
        else:
            os.environ[_EVAL_OVERRIDES_ENV] = prev
    wall = time.monotonic() - t0

    elo_lo, elo_hi = res.estimated_elo_ci
    # Wilson CI half-width on the Elo scale → 1-sigma proxy via the
    # 1.96 normal quantile (design.md §4 "elo_sem" formula).
    sem = (elo_hi - elo_lo) / (2.0 * 1.96)
    return TrialOutcome(
        trial_number=trial.number,
        params=params,
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
        elo_sem=sem,
        workers=promote_mod.resolve_worker_count(n_workers, n_games),
        time_ms_per_stone=time_ms_per_stone,
        wall_seconds=wall,
    )


def outcome_to_json(
    outcome: TrialOutcome,
    *,
    study_name: str,
    started_at: str,
    finished_at: str,
    smoke: bool,
    reference_binary: str,
) -> dict[str, Any]:
    """Build the serialisable dict for one trial outcome."""
    return {
        "schema_version": TUNE_BO_SCHEMA_VERSION,
        "study_name": study_name,
        "trial_number": outcome.trial_number,
        "params": outcome.params,
        "games_played": outcome.games_played,
        "wins": outcome.wins,
        "losses": outcome.losses,
        "draws": outcome.draws,
        "winrate": outcome.winrate,
        "wilson_lower": outcome.wilson_lower,
        "wilson_upper": outcome.wilson_upper,
        "elo": outcome.elo,
        "ci_lower": outcome.ci_lower_elo,
        "ci_upper": outcome.ci_upper_elo,
        "ci_method": "wilson",
        "elo_sem": outcome.elo_sem,
        "workers": outcome.workers,
        "time_ms_per_stone": outcome.time_ms_per_stone,
        "wall_seconds": outcome.wall_seconds,
        "color_balance": CONFIG.promote.color_balance,
        "opening_diversity": False,
        "reference_binary": reference_binary,
        "host": socket.gethostname(),
        "git_sha": _git_sha(),
        "started_at": started_at,
        "finished_at": finished_at,
        "smoke": smoke,
    }


# ─────────────────────────────────────────────────────────────────────────────
# Study creation + main loop
# ─────────────────────────────────────────────────────────────────────────────


@dataclasses.dataclass(frozen=True, slots=True)
class TuneBoArgs:
    """Resolved arguments after CLI parsing + defaulting."""

    trials: int
    games_per_trial: int
    time_ms_per_stone: int
    workers: int
    max_plies: int
    study_name: str
    storage: str
    reference_binary: Path
    output_dir: Path
    seed: int
    n_startup_trials: int
    smoke: bool


def _make_study(args: TuneBoArgs) -> optuna.Study:
    """Create (or load) the Optuna study with the locked GPSampler config.

    Per design.md §5: ``GPSampler(deterministic_objective=False,
    n_startup_trials=10, seed=42)`` — default Matérn-5/2 kernel,
    NOT manually tuned (scope hard constraint).
    """
    sampler = GPSampler(
        deterministic_objective=False,
        n_startup_trials=args.n_startup_trials,
        seed=args.seed,
    )
    return optuna.create_study(
        study_name=args.study_name,
        direction="maximize",
        sampler=sampler,
        storage=args.storage,
        load_if_exists=True,
    )


def _enqueue_head_seed_if_fresh(study: optuna.Study) -> bool:
    """Enqueue the HEAD-seed trial iff the study has no completed trials.

    Per design.md §3: anchor the GP at the known-good post-28C-0 HEAD
    config as trial #0 (saves a random-init slot). On a resumed study
    that already has trials, skip — Optuna's persistence already has
    the anchor.
    """
    if study.trials:
        return False
    study.enqueue_trial(dict(HEAD_SEED_PARAMS))
    return True


def _write_failure_json(
    *,
    trial: Trial,
    args: TuneBoArgs,
    started_at: str,
    finished_at: str,
    error: str,
) -> None:
    """Sidecar JSON for a failed trial (parallel to outcome_to_json)."""
    path = args.output_dir / f"{trial.number:04d}.json"
    payload = {
        "schema_version": TUNE_BO_SCHEMA_VERSION,
        "study_name": args.study_name,
        "trial_number": trial.number,
        "params": dict(trial.params) if trial.params else {},
        "state": "FAIL",
        "error": error,
        "started_at": started_at,
        "finished_at": finished_at,
        "smoke": args.smoke,
        "reference_binary": str(args.reference_binary),
    }
    _atomic_write_json(path, payload)


def run_sprint(args: TuneBoArgs) -> int:
    """Run the BO sprint: ``args.trials`` trials, one per loop iteration.

    Per design.md §3 we use ``study.ask`` / ``study.tell`` (manual loop)
    rather than ``study.optimize`` so we can handle harness-side trial
    failures explicitly (``state=FAIL`` → GP imputes) without losing
    the per-trial JSON sidecar.
    """
    args.output_dir.mkdir(parents=True, exist_ok=True)
    study = _make_study(args)
    seed_enqueued = _enqueue_head_seed_if_fresh(study)

    print(
        f"tune-bo: study={args.study_name} storage={args.storage} "
        f"trials={args.trials} games/trial={args.games_per_trial} "
        f"workers={args.workers} time_ms/stone={args.time_ms_per_stone} "
        f"smoke={args.smoke} seed_enqueued={seed_enqueued}",
        flush=True,
    )

    completed = 0
    while completed < args.trials:
        trial = study.ask()
        started = _now_iso()
        try:
            outcome = run_one_trial(
                trial,
                reference_binary=args.reference_binary,
                n_games=args.games_per_trial,
                time_ms_per_stone=args.time_ms_per_stone,
                n_workers=args.workers,
                max_plies=args.max_plies,
            )
        except Exception as exc:  # noqa: BLE001
            # Engine crash / harness exception / zero games: tell
            # Optuna FAIL so the GP imputes; record a stub JSON for
            # the dispatcher.
            finished = _now_iso()
            print(
                f"  trial {trial.number}: FAILED — {exc!r}",
                flush=True,
            )
            study.tell(trial, state=TrialState.FAIL)
            _write_failure_json(
                trial=trial,
                args=args,
                started_at=started,
                finished_at=finished,
                error=repr(exc),
            )
            completed += 1
            continue

        finished = _now_iso()
        trial.set_user_attr("elo_sem", outcome.elo_sem)
        trial.set_user_attr("elo_ci_lo", outcome.ci_lower_elo)
        trial.set_user_attr("elo_ci_hi", outcome.ci_upper_elo)
        trial.set_user_attr("wins", outcome.wins)
        trial.set_user_attr("losses", outcome.losses)
        trial.set_user_attr("draws", outcome.draws)
        trial.set_user_attr("games_played", outcome.games_played)
        trial.set_user_attr("wall_seconds", outcome.wall_seconds)
        study.tell(trial, outcome.elo)

        path = args.output_dir / f"{outcome.trial_number:04d}.json"
        payload = outcome_to_json(
            outcome,
            study_name=args.study_name,
            started_at=started,
            finished_at=finished,
            smoke=args.smoke,
            reference_binary=str(args.reference_binary),
        )
        _atomic_write_json(path, payload)

        print(
            f"  trial {outcome.trial_number}: params={outcome.params}  "
            f"W-L-D {outcome.wins}-{outcome.losses}-{outcome.draws}  "
            f"elo {outcome.elo:+.1f} CI [{outcome.ci_lower_elo:+.1f}, "
            f"{outcome.ci_upper_elo:+.1f}]  ({outcome.wall_seconds:.1f}s)  "
            f"→ {path}",
            flush=True,
        )
        completed += 1

    best_value = study.best_value if study.best_trial is not None else float("nan")
    print(
        f"\ntune-bo done: completed {completed} trials. best_value={best_value:+.1f}",
        flush=True,
    )
    return 0


# ─────────────────────────────────────────────────────────────────────────────
# CLI surface
# ─────────────────────────────────────────────────────────────────────────────


def add_tune_bo_args(p: argparse.ArgumentParser) -> None:
    """Wire the ``hammerhead tune-bo`` argparse surface."""
    p.add_argument(
        "--trials",
        type=int,
        default=DEFAULT_N_TRIALS,
        help=f"trial budget (default: {DEFAULT_N_TRIALS}; overridden by --smoke)",
    )
    p.add_argument(
        "--gpw",
        "--games-per-trial",
        dest="games_per_trial",
        type=int,
        default=DEFAULT_GAMES_PER_TRIAL,
        help=f"games per trial (default: {DEFAULT_GAMES_PER_TRIAL}; "
        f"overridden by --smoke)",
    )
    p.add_argument(
        "--time-ms",
        type=int,
        default=DEFAULT_TIME_MS_PER_STONE,
        help=f"per-stone time budget in ms (default: {DEFAULT_TIME_MS_PER_STONE})",
    )
    p.add_argument(
        "--workers",
        type=int,
        default=DEFAULT_N_WORKERS,
        help=f"parallel match workers (default: {DEFAULT_N_WORKERS} = host budget)",
    )
    p.add_argument(
        "--max-plies",
        type=int,
        default=CONFIG.promote.default_max_plies,
        help=f"max plies per game (default: {CONFIG.promote.default_max_plies})",
    )
    p.add_argument(
        "--study-name",
        required=True,
        help="Optuna study name (used for the SQLite study row + JSON tag)",
    )
    p.add_argument(
        "--storage",
        default=None,
        help="SQLite URL for Optuna storage; defaults to "
        "sqlite:///<output-dir>/study.db",
    )
    p.add_argument(
        "--reference-binary",
        required=True,
        help="absolute path to the reference Python (e.g. the "
        ".venv-bo/bin/python of the Phase 27 baseline worktree at "
        "e28d54a). The reference subprocess invokes "
        "`<this-binary> -m hammerhead.cli bot --tt-size-mb …` with "
        "HEXO_EVAL_OVERRIDES stripped via `env -u`.",
    )
    p.add_argument(
        "--output-dir",
        required=True,
        help="output directory for per-trial JSON sidecars "
        "(plus study.db if --storage is unset)",
    )
    p.add_argument(
        "--seed",
        type=int,
        default=DEFAULT_GP_SEED,
        help=f"GPSampler RNG seed (default: {DEFAULT_GP_SEED})",
    )
    p.add_argument(
        "--n-startup-trials",
        type=int,
        default=DEFAULT_N_STARTUP_TRIALS,
        help=f"GPSampler random warm-up count (default: "
        f"{DEFAULT_N_STARTUP_TRIALS})",
    )
    p.add_argument(
        "--smoke",
        action="store_true",
        help=f"wiring-verification run: {SMOKE_N_TRIALS} trials × "
        f"{SMOKE_GAMES_PER_TRIAL} games. Result Elo is meaningless. "
        "Output lands under a smoke/ subtree.",
    )


def resolve_args(ns: argparse.Namespace) -> TuneBoArgs:
    """Normalise the argparse namespace into a :class:`TuneBoArgs`."""
    trials = int(ns.trials)
    games = int(ns.games_per_trial)
    if ns.smoke:
        trials = SMOKE_N_TRIALS
        games = SMOKE_GAMES_PER_TRIAL
    if trials < 1:
        raise ValueError(f"--trials must be >= 1, got {trials}")
    if games < 1:
        raise ValueError(f"--gpw must be >= 1, got {games}")

    out_dir = Path(ns.output_dir).expanduser().resolve()
    if ns.smoke:
        # Smoke must NEVER write under a canonical sprint subtree.
        out_dir = out_dir / "tune_bo" / "smoke"
    out_dir.mkdir(parents=True, exist_ok=True)

    # NB: use absolute(), NOT resolve() — the latter follows symlinks,
    # and a virtualenv's bin/python is a symlink to the system python.
    # Following it would (a) lose the venv's site-packages (so the
    # reference subprocess wouldn't find hammerhead_engine) and (b)
    # serialise a misleading path into the per-trial JSON.
    reference = Path(ns.reference_binary).expanduser().absolute()
    if not reference.exists():
        raise ValueError(f"--reference-binary not found: {reference}")

    storage = ns.storage
    if storage is None:
        storage = f"sqlite:///{out_dir / 'study.db'}"

    return TuneBoArgs(
        trials=trials,
        games_per_trial=games,
        time_ms_per_stone=int(ns.time_ms),
        workers=int(ns.workers),
        max_plies=int(ns.max_plies),
        study_name=str(ns.study_name),
        storage=storage,
        reference_binary=reference,
        output_dir=out_dir,
        seed=int(ns.seed),
        n_startup_trials=int(ns.n_startup_trials),
        smoke=bool(ns.smoke),
    )


def cmd_tune_bo(ns: argparse.Namespace) -> int:
    """``hammerhead tune-bo`` entry point."""
    try:
        args = resolve_args(ns)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    return run_sprint(args)
