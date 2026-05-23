"""Phase 28C-1 — Bayesian-optimisation sweep driver (Optuna GPSampler).

Scaffolding commit: CLI + study creation + per-trial JSON write only.
The objective body is a stub (returns 0) — commit 2 plugs in the
real EvalOverrides construction and match spawn. This split lets us
verify Optuna wiring + SQLite storage + JSON sidecars before
introducing any match-side variability.

Companion to :mod:`hammerhead.tune` (coordinate-descent driver, Phase
28B-1). Where ``tune.py`` walks a hand-rolled grid one parameter at a
time, ``tune_bo.py`` will (once commit 2 lands) run an outer Optuna
study that jointly samples a 5-D search space via a Matérn-5/2 GP
surrogate — the missing ingredient in Phase 28B per Phase 28C-0 §7.

Architectural contract (mirrors ``tune.py``)
--------------------------------------------
- New consumer of the Phase 17 parallel match pool in
  :mod:`hammerhead.promote`. Does NOT replace, wrap, or modify it.
- Output is atomic per-trial JSON (write-tmp + os.rename) plus the
  Optuna study SQLite for resumability (``load_if_exists=True``).
- Per design.md §3 the full sprint will be 60 trials × 200g; the
  ``--smoke`` flag drops to 2 trials × 5g for wiring verification.
"""

from __future__ import annotations

import argparse
import dataclasses
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import optuna
from optuna.samplers import GPSampler
from optuna.trial import Trial

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


# ─────────────────────────────────────────────────────────────────────────────
# Per-trial JSON sidecar
# ─────────────────────────────────────────────────────────────────────────────


def _now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


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


# ─────────────────────────────────────────────────────────────────────────────
# Study creation + main loop (stub objective)
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


def _stub_objective(trial: Trial) -> float:
    """Scaffolding stub — commit 2 replaces this with real EvalOverrides
    + match spawn. Suggests a single placeholder int so the GPSampler
    actually exercises ask/suggest/tell (a zero-param study would
    short-circuit the sampler); returns 0.0 so there is no engine
    variability in the smoke wiring check."""
    trial.suggest_int("_scaffold_placeholder", 0, 1)
    return 0.0


def run_sprint(args: TuneBoArgs) -> int:
    """Run the BO sprint: ``args.trials`` trials, one per loop iteration.

    Uses ``study.ask`` / ``study.tell`` (manual loop) rather than
    ``study.optimize`` so commit 2 can later route harness-side trial
    failures to ``TrialState.FAIL`` without losing the per-trial JSON
    sidecar.
    """
    args.output_dir.mkdir(parents=True, exist_ok=True)
    study = _make_study(args)

    print(
        f"tune-bo: study={args.study_name} storage={args.storage} "
        f"trials={args.trials} games/trial={args.games_per_trial} "
        f"workers={args.workers} time_ms/stone={args.time_ms_per_stone} "
        f"smoke={args.smoke}",
        flush=True,
    )

    completed = 0
    while completed < args.trials:
        trial = study.ask()
        started = _now_iso()
        value = _stub_objective(trial)
        finished = _now_iso()
        study.tell(trial, value)

        path = args.output_dir / f"{trial.number:04d}.json"
        payload = {
            "schema_version": TUNE_BO_SCHEMA_VERSION,
            "study_name": args.study_name,
            "trial_number": trial.number,
            "value": value,
            "params": dict(trial.params),
            "started_at": started,
            "finished_at": finished,
            "smoke": args.smoke,
            "reference_binary": str(args.reference_binary),
        }
        _atomic_write_json(path, payload)
        print(
            f"  trial {trial.number}: value={value:+.1f} → {path}",
            flush=True,
        )
        completed += 1

    print(
        f"\ntune-bo done: completed {completed} trials. best_value="
        f"{study.best_value:+.1f}",
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
        "e28d54a). Commit 2 wires this into the match spawn.",
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
