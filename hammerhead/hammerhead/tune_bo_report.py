"""Phase 28C-1 — Optuna BO study post-hoc reporter.

Read-only consumer of the SQLite study produced by
:mod:`hammerhead.tune_bo`. Prints:

- top-5 trials by Elo (point-estimate ranking + Wilson CI half-width
  from the per-trial ``user_attr['elo_sem']``);
- ``study.best_params`` (Optuna's running best-observed argmax — GP
  posterior argmax is not directly exposed in Optuna 4.8, so we report
  the best observation as the closest available proxy);
- fANOVA parameter importance via ``optuna.importance``.

Lives in its own module so the main driver commit stays atomic
(commit 3 lands the report; commit 2 is pure driver). The CLI plumbs
it as ``hammerhead tune-bo-report``.
"""

from __future__ import annotations

import argparse
import sys
from typing import Any

import optuna
from optuna.trial import TrialState


# Top-K trials to print. Five is the design.md §6 recommendation.
DEFAULT_TOP_K = 5


def _fmt_params(params: dict[str, Any]) -> str:
    return ", ".join(f"{k}={v}" for k, v in sorted(params.items()))


def print_top_k(study: optuna.Study, k: int) -> None:
    """Print top-``k`` completed trials by Elo (descending)."""
    completed = [t for t in study.trials if t.state is TrialState.COMPLETE]
    if not completed:
        print("(no completed trials)", flush=True)
        return
    sorted_trials = sorted(
        completed,
        key=lambda t: (t.value if t.value is not None else float("-inf")),
        reverse=True,
    )
    print(f"\nTop {min(k, len(sorted_trials))} trials by Elo:", flush=True)
    for t in sorted_trials[:k]:
        sem = t.user_attrs.get("elo_sem")
        sem_s = f"  sem={sem:.1f}" if sem is not None else ""
        value = t.value if t.value is not None else float("nan")
        print(
            f"  #{t.number:>4d}  elo {value:+.1f}{sem_s}  {_fmt_params(t.params)}",
            flush=True,
        )


def print_best(study: optuna.Study) -> None:
    """Print ``study.best_params`` + ``study.best_value`` if available.

    Optuna 4.8's ``GPSampler`` does not expose the GP-posterior argmax
    directly; ``study.best_params`` returns the best-observed trial,
    which is the closest documented proxy. The dispatcher (C2-DRIFT)
    can re-rank trials by drift-corrected Elo if needed.
    """
    if study.best_trial is None:
        print("\n(no best trial — study has no completed trials)", flush=True)
        return
    print("\nBest-observed trial (best_params proxy for GP argmax):", flush=True)
    print(f"  trial #{study.best_trial.number}", flush=True)
    print(f"  elo   {study.best_value:+.1f}", flush=True)
    print(f"  params: {_fmt_params(study.best_params)}", flush=True)


def print_importance(study: optuna.Study) -> None:
    """Print fANOVA parameter importance via ``optuna.importance``."""
    completed = [t for t in study.trials if t.state is TrialState.COMPLETE]
    # fANOVA needs at least 2 completed trials with varied params.
    if len(completed) < 2:
        print(
            "\n(parameter importance skipped: need >= 2 completed trials)",
            flush=True,
        )
        return
    try:
        importance = optuna.importance.get_param_importances(study)
    except (ValueError, RuntimeError) as exc:
        print(f"\n(parameter importance failed: {exc!r})", flush=True)
        return
    print("\nParameter importance (fANOVA):", flush=True)
    for name, score in importance.items():
        print(f"  {name:<28}  {score:.4f}", flush=True)


def cmd_tune_bo_report(ns: argparse.Namespace) -> int:
    """``hammerhead tune-bo-report`` entry point."""
    try:
        study = optuna.load_study(study_name=ns.study_name, storage=ns.storage)
    except (KeyError, RuntimeError, ValueError) as exc:
        print(f"error: failed to load study: {exc!r}", file=sys.stderr)
        return 2
    print(
        f"tune-bo-report: study={ns.study_name} storage={ns.storage} "
        f"trials={len(study.trials)}",
        flush=True,
    )
    print_top_k(study, ns.top_k)
    print_best(study)
    print_importance(study)
    return 0


def add_tune_bo_report_args(p: argparse.ArgumentParser) -> None:
    """Wire the ``hammerhead tune-bo-report`` argparse surface."""
    p.add_argument(
        "--study-name",
        required=True,
        help="Optuna study name to load",
    )
    p.add_argument(
        "--storage",
        required=True,
        help="SQLite storage URL (e.g. sqlite:///path/to/study.db)",
    )
    p.add_argument(
        "--top-k",
        type=int,
        default=DEFAULT_TOP_K,
        help=f"number of top-Elo trials to print (default: {DEFAULT_TOP_K})",
    )
