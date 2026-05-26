"""Phase S5 Stage 2A — CMA-ES eval-weight tuning harness.

Sibling to :mod:`hammerhead.tune` (28B coordinate-descent) and
:mod:`hammerhead.tune_bo` (28C-1 Optuna GP-BO). Where BO surrogate-
models a 5-D space and is sample-efficient at small budgets, CMA-ES
scales to higher dim and handles strong parameter interactions —
which is what we want for the 15-D joint Layer-1/2/3 eval space the
S1 magnitude analysis (Pearson r 0.22 midgame, median ratio 9.2×)
implicates as the strength bottleneck.

Lives in ``tools/`` rather than the ``hammerhead`` package because it
is a one-shot driver, not a feature of the engine SDK. Reuses
:func:`hammerhead.promote.run_match_parallel` via the same
``HEXO_EVAL_OVERRIDES`` env-var bridge that tune_bo.py uses (28C-1
contract). The reference engine is a fixed-SHA worktree binary with
the env var stripped via ``env -u`` (see :func:`_reference_cmd`).

Parameter encoding
------------------
15-D real-valued vector in log10 space. Decoded via ``round(10**x)``
clamped to non-negative ints. Window_k_scores [0] and [6] are locked
(0 and mate_score respectively) per build.rs invariant; rhombus is
disabled by default and not part of this sprint.

Vector layout (indices, baseline values):
  0..4  window_k_scores[1..5]      (1, 8, 64, 512, 4096)
  5     open_extension_factor       (4)
  6     closed_extension_factor     (1)
  7..10 open_5, closed_5, open_4, closed_4
  11..13 open_3, closed_3, open_2
  14    fork_cover2_bonus           (4000)

Usage
-----
::

    python tools/cma_tune.py \\
        --reference-binary /home/tom/Work/hammerhead/.worktree-best/.venv-best/bin/python \\
        --output-dir tools/cma_output/run01 \\
        --popsize 16 --max-gen 50 --games-per-cand 50 --time-ms 200 \\
        --workers 10

Smoke (~2 min wiring check)::

    python tools/cma_tune.py --reference-binary <…> --output-dir <…> --smoke
"""

from __future__ import annotations

import argparse
import json
import math
import os
import pickle
import socket
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# Ensure the in-tree `hammerhead` package is importable even when
# running this file directly (``python tools/cma_tune.py …``).
_REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(_REPO_ROOT / "hammerhead"))

import cma  # noqa: E402  (third-party CMA-ES library)

from hammerhead import promote as promote_mod  # noqa: E402
from hammerhead.config import CONFIG  # noqa: E402


# ─── Constants ──────────────────────────────────────────────────────────────

SCHEMA_VERSION = 1
_EVAL_OVERRIDES_ENV = "HEXO_EVAL_OVERRIDES"

# Baseline values mirror hexo.toml [engine.eval]. Source of truth for
# x0; keep in sync with the TOML or the encoder will start CMA-ES from
# the wrong centre.
BASELINE: dict[str, int] = {
    "window_k_1": 1,
    "window_k_2": 8,
    "window_k_3": 64,
    "window_k_4": 512,
    "window_k_5": 4096,
    "open_extension_factor": 4,
    "closed_extension_factor": 1,
    "open_5": 800_000,
    "closed_5": 500_000,
    "open_4": 135_000,
    "closed_4": 20_000,
    "open_3": 90_000,
    "closed_3": 11_250,
    "open_2": 11_250,
    "fork_cover2_bonus": 4_000,
}
PARAM_NAMES: list[str] = list(BASELINE.keys())  # vector order
DIM = len(PARAM_NAMES)  # 15

# CMA-ES explores ±LOG_HALFWIDTH around log10(baseline). 2.0 = 0.01×..100×.
LOG_HALFWIDTH = 2.0

# Hard floor for decoded ints. window_k must be >= 0 (build.rs allows
# 0 at idx 0); extension factors must be >= 0; shape/fork weights >= 0.
# A floor of 0 prevents negative scores from messing up sign invariants.
DECODE_FLOOR = 0

# Window_k[6] is locked to mate_score by build.rs assertion. Pulled
# from the active EvalOverrides dict, NOT hand-typed (magic-number rule).
_MATE_SCORE = 1_000_000

DEFAULT_POPSIZE = 16
DEFAULT_MAX_GEN = 50
DEFAULT_GAMES_PER_CAND = 50
DEFAULT_TIME_MS = 200
DEFAULT_WORKERS = 10
DEFAULT_SEED = 42

SMOKE_POPSIZE = 4
SMOKE_MAX_GEN = 2
SMOKE_GAMES = 4


# ─── Encoder / decoder ──────────────────────────────────────────────────────


def encode_x0() -> list[float]:
    """log10 baseline vector — CMA-ES initial mean."""
    return [math.log10(BASELINE[name]) if BASELINE[name] > 0 else 0.0
            for name in PARAM_NAMES]


def _bounds() -> tuple[list[float], list[float]]:
    x0 = encode_x0()
    lo = [v - LOG_HALFWIDTH for v in x0]
    hi = [v + LOG_HALFWIDTH for v in x0]
    return lo, hi


def decode_vector(x: list[float] | tuple[float, ...]) -> dict[str, Any]:
    """Decode log10 vector → `EvalOverrides` dict ready for set_eval_overrides.

    Each scalar: ``int(round(10 ** xi))``, clamped to ``DECODE_FLOOR``.
    ``window_k_scores`` is reassembled into the 7-element array with
    indices 0 (=0) and 6 (=mate) locked per build.rs invariant.
    """
    vals = [max(DECODE_FLOOR, int(round(10 ** xi))) for xi in x]
    by_name = dict(zip(PARAM_NAMES, vals, strict=True))
    wk = [
        0,
        by_name["window_k_1"],
        by_name["window_k_2"],
        by_name["window_k_3"],
        by_name["window_k_4"],
        by_name["window_k_5"],
        _MATE_SCORE,
    ]
    return {
        "window_k_scores": wk,
        "open_extension_factor": by_name["open_extension_factor"],
        "closed_extension_factor": by_name["closed_extension_factor"],
        "open_5": by_name["open_5"],
        "closed_5": by_name["closed_5"],
        "open_4": by_name["open_4"],
        "closed_4": by_name["closed_4"],
        "open_3": by_name["open_3"],
        "closed_3": by_name["closed_3"],
        "open_2": by_name["open_2"],
        "fork_cover2_bonus": by_name["fork_cover2_bonus"],
    }


# ─── Engine command builders (mirrors tune_bo.py) ───────────────────────────


def _candidate_cmd(tt_mb: int) -> list[str]:
    """In-tree candidate engine; inherits HEXO_EVAL_OVERRIDES from parent."""
    return [sys.executable, "-m", "hammerhead.cli", "bot", "--tt-size-mb", str(tt_mb)]


def _reference_cmd(reference_binary: Path, tt_mb: int) -> list[str]:
    """Fixed-SHA reference; env -u strips HEXO_EVAL_OVERRIDES at exec."""
    return [
        "env", "-u", _EVAL_OVERRIDES_ENV,
        str(reference_binary), "-m", "hammerhead.cli", "bot",
        "--tt-size-mb", str(tt_mb),
    ]


# ─── Per-candidate match ────────────────────────────────────────────────────


@dataclass(frozen=True, slots=True)
class CandidateResult:
    gen: int
    cand_idx: int
    params: dict[str, Any]
    score: float       # wins + 0.5 * draws, normalised by n
    wins: int
    losses: int
    draws: int
    games_played: int
    wall_seconds: float


def run_one_candidate(
    *,
    x: list[float],
    gen: int,
    cand_idx: int,
    reference_binary: Path,
    n_games: int,
    time_ms: int,
    n_workers: int,
    max_plies: int,
) -> CandidateResult:
    """Play n_games of candidate-vs-reference. Returns score in [0, 1]."""
    overrides = decode_vector(x)
    tt_mb = promote_mod.max_tt_mb_per_worker()
    cur = _candidate_cmd(tt_mb)
    ref = _reference_cmd(reference_binary, tt_mb)
    cfg = promote_mod.MatchConfig(
        n_games=n_games,
        time_ms_per_stone=time_ms,
        test="raw",
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

    prev = os.environ.get(_EVAL_OVERRIDES_ENV)
    os.environ[_EVAL_OVERRIDES_ENV] = json.dumps(overrides)
    t0 = time.monotonic()
    try:
        res = promote_mod.run_match_parallel(cur, ref, cfg, n_workers=n_workers)
    finally:
        if prev is None:
            os.environ.pop(_EVAL_OVERRIDES_ENV, None)
        else:
            os.environ[_EVAL_OVERRIDES_ENV] = prev
    wall = time.monotonic() - t0

    n = max(1, res.games_played)
    # Score = wins + 0.5*draws normalised: same statistic CMA-ES will
    # rank candidates by (Elo monotone in score, no need for the
    # arctanh transform).
    score = (res.current_wins + 0.5 * res.draws) / n
    return CandidateResult(
        gen=gen,
        cand_idx=cand_idx,
        params=overrides,
        score=score,
        wins=res.current_wins,
        losses=res.best_wins,
        draws=res.draws,
        games_played=res.games_played,
        wall_seconds=wall,
    )


# ─── Checkpointing ──────────────────────────────────────────────────────────


def _checkpoint_path(out_dir: Path) -> Path:
    return out_dir / "checkpoint.pkl"


def _save_checkpoint(out_dir: Path, *, es: cma.CMAEvolutionStrategy,
                     gen: int, best_x: list[float] | None,
                     best_score: float) -> None:
    """Pickle CMA-ES state + best-so-far. Atomic via write-tmp + rename."""
    path = _checkpoint_path(out_dir)
    tmp = path.with_suffix(".tmp")
    with tmp.open("wb") as fh:
        pickle.dump({
            "es": es,
            "gen": gen,
            "best_x": best_x,
            "best_score": best_score,
        }, fh)
    os.replace(tmp, path)


def _load_checkpoint(out_dir: Path) -> dict[str, Any] | None:
    path = _checkpoint_path(out_dir)
    if not path.exists():
        return None
    with path.open("rb") as fh:
        return pickle.load(fh)


# ─── Output JSON ────────────────────────────────────────────────────────────


def _atomic_write_json(path: Path, payload: dict[str, Any]) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    with tmp.open("w", encoding="utf-8") as fh:
        json.dump(payload, fh, indent=2, sort_keys=True)
        fh.write("\n")
    os.replace(tmp, path)


def _git_sha() -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=_REPO_ROOT, stderr=subprocess.DEVNULL, text=True,
        ).strip() or "unknown"
    except Exception:
        return "unknown"


def _now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _write_generation_json(out_dir: Path, *, gen: int,
                            results: list[CandidateResult],
                            es: cma.CMAEvolutionStrategy,
                            started_at: str, finished_at: str,
                            args: "CmaArgs") -> None:
    payload = {
        "schema_version": SCHEMA_VERSION,
        "generation": gen,
        "popsize": len(results),
        "candidates": [
            {
                "cand_idx": r.cand_idx,
                "params": r.params,
                "score": r.score,
                "wins": r.wins,
                "losses": r.losses,
                "draws": r.draws,
                "games_played": r.games_played,
                "wall_seconds": r.wall_seconds,
            } for r in results
        ],
        "es_mean_log10": list(es.mean),
        "es_sigma": float(es.sigma),
        "started_at": started_at,
        "finished_at": finished_at,
        "reference_binary": str(args.reference_binary),
        "time_ms_per_stone": args.time_ms,
        "games_per_cand": args.games_per_cand,
        "workers": args.workers,
        "host": socket.gethostname(),
        "git_sha": _git_sha(),
        "smoke": args.smoke,
    }
    _atomic_write_json(out_dir / f"gen_{gen:04d}.json", payload)


# ─── Main loop ──────────────────────────────────────────────────────────────


@dataclass(frozen=True, slots=True)
class CmaArgs:
    reference_binary: Path
    output_dir: Path
    popsize: int
    max_gen: int
    games_per_cand: int
    time_ms: int
    workers: int
    max_plies: int
    seed: int
    sigma0: float
    smoke: bool
    resume: bool


def _make_es(args: CmaArgs) -> cma.CMAEvolutionStrategy:
    x0 = encode_x0()
    lo, hi = _bounds()
    opts: dict[str, Any] = {
        "popsize": args.popsize,
        "maxiter": args.max_gen,
        "seed": args.seed,
        "bounds": [lo, hi],
        "verbose": -9,  # we do our own logging
        "tolfun": 1e-6,
    }
    return cma.CMAEvolutionStrategy(x0, args.sigma0, opts)


def run(args: CmaArgs) -> int:
    args.output_dir.mkdir(parents=True, exist_ok=True)

    ckpt = _load_checkpoint(args.output_dir) if args.resume else None
    if ckpt is not None:
        es: cma.CMAEvolutionStrategy = ckpt["es"]
        start_gen: int = int(ckpt["gen"]) + 1
        best_x: list[float] | None = ckpt["best_x"]
        best_score: float = float(ckpt["best_score"])
        print(f"resumed from gen {ckpt['gen']}: best_score={best_score:.4f}", flush=True)
    else:
        es = _make_es(args)
        start_gen = 0
        best_x = None
        best_score = -math.inf

    print(f"CMA-ES dim={DIM} popsize={args.popsize} max_gen={args.max_gen} "
          f"games/cand={args.games_per_cand} time_ms={args.time_ms} "
          f"workers={args.workers}", flush=True)

    for gen in range(start_gen, args.max_gen):
        if es.stop():
            print(f"CMA-ES converged at gen {gen}: {es.stop()}", flush=True)
            break

        candidates = es.ask()
        gen_started = _now_iso()
        results: list[CandidateResult] = []
        for i, x in enumerate(candidates):
            r = run_one_candidate(
                x=list(x), gen=gen, cand_idx=i,
                reference_binary=args.reference_binary,
                n_games=args.games_per_cand,
                time_ms=args.time_ms,
                n_workers=args.workers,
                max_plies=args.max_plies,
            )
            results.append(r)
            print(f"  gen {gen:3d} cand {i:2d}/{len(candidates)}: "
                  f"score={r.score:.3f} W/L/D={r.wins}/{r.losses}/{r.draws} "
                  f"({r.wall_seconds:.1f}s)", flush=True)
            if r.score > best_score:
                best_score = r.score
                best_x = list(x)
                best_path = args.output_dir / "best.json"
                _atomic_write_json(best_path, {
                    "schema_version": SCHEMA_VERSION,
                    "gen": gen, "cand_idx": i,
                    "score": r.score,
                    "params": r.params,
                    "x_log10": list(x),
                    "found_at": _now_iso(),
                })
                print(f"  ↑ new best score={r.score:.3f} → {best_path}", flush=True)

        # CMA-ES minimises — feed negative score.
        es.tell(candidates, [-r.score for r in results])
        gen_finished = _now_iso()
        _write_generation_json(args.output_dir, gen=gen, results=results,
                               es=es, started_at=gen_started,
                               finished_at=gen_finished, args=args)
        _save_checkpoint(args.output_dir, es=es, gen=gen,
                         best_x=best_x, best_score=best_score)

    print(f"done. best_score={best_score:.4f}", flush=True)
    if best_x is not None:
        print(f"best params: {decode_vector(best_x)}", flush=True)
    return 0


# ─── CLI ────────────────────────────────────────────────────────────────────


def _build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="cma_tune",
        description="CMA-ES tuner for 15-D Hammerhead eval weights "
                    "(Phase S5 Stage 2A)",
    )
    p.add_argument("--reference-binary", required=True,
                   help="absolute path to reference Python (.bestref worktree's "
                        ".venv-best/bin/python). HEXO_EVAL_OVERRIDES is "
                        "stripped at exec via `env -u`.")
    p.add_argument("--output-dir", required=True,
                   help="directory for per-gen JSON, best.json, checkpoint.pkl")
    p.add_argument("--popsize", type=int, default=DEFAULT_POPSIZE)
    p.add_argument("--max-gen", type=int, default=DEFAULT_MAX_GEN)
    p.add_argument("--games-per-cand", type=int, default=DEFAULT_GAMES_PER_CAND)
    p.add_argument("--time-ms", type=int, default=DEFAULT_TIME_MS)
    p.add_argument("--workers", type=int, default=DEFAULT_WORKERS)
    p.add_argument("--max-plies", type=int,
                   default=CONFIG.promote.default_max_plies)
    p.add_argument("--seed", type=int, default=DEFAULT_SEED)
    p.add_argument("--sigma0", type=float, default=0.3,
                   help="initial CMA-ES step size in log10 space (default: 0.3)")
    p.add_argument("--resume", action="store_true",
                   help="resume from checkpoint.pkl in --output-dir")
    p.add_argument("--smoke", action="store_true",
                   help=f"wiring check: popsize={SMOKE_POPSIZE} × "
                        f"max_gen={SMOKE_MAX_GEN} × games={SMOKE_GAMES}. "
                        "Results meaningless.")
    return p


def _resolve(ns: argparse.Namespace) -> CmaArgs:
    ref = Path(ns.reference_binary).expanduser().absolute()
    if not ref.exists():
        raise SystemExit(f"--reference-binary not found: {ref}")
    out = Path(ns.output_dir).expanduser().resolve()
    popsize = SMOKE_POPSIZE if ns.smoke else int(ns.popsize)
    max_gen = SMOKE_MAX_GEN if ns.smoke else int(ns.max_gen)
    games = SMOKE_GAMES if ns.smoke else int(ns.games_per_cand)
    if popsize < 2:
        raise SystemExit(f"--popsize must be >= 2, got {popsize}")
    if max_gen < 1:
        raise SystemExit(f"--max-gen must be >= 1, got {max_gen}")
    if games < 1:
        raise SystemExit(f"--games-per-cand must be >= 1, got {games}")
    return CmaArgs(
        reference_binary=ref, output_dir=out,
        popsize=popsize, max_gen=max_gen, games_per_cand=games,
        time_ms=int(ns.time_ms), workers=int(ns.workers),
        max_plies=int(ns.max_plies), seed=int(ns.seed),
        sigma0=float(ns.sigma0), smoke=bool(ns.smoke),
        resume=bool(ns.resume),
    )


def main(argv: list[str] | None = None) -> int:
    ns = _build_parser().parse_args(argv)
    return run(_resolve(ns))


if __name__ == "__main__":
    raise SystemExit(main())
