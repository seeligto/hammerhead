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

# Shape/fork weights MUST stay strictly below mate_score: search's TT
# mate-score adjustment (score_to_tt / score_from_tt) and aspiration
# windows expect |eval| < MATE for non-terminal positions. A single
# overflowing shape weight makes eval flag every position as mate-
# imminent, which breaks pruning. Cap any clamped to this max int.
_SHAPE_SCORE_CAP = _MATE_SCORE - 1  # 999_999
_SHAPE_BOUND_PARAMS = frozenset({
    "open_5", "closed_5", "open_4", "closed_4",
    "open_3", "closed_3", "open_2", "fork_cover2_bonus",
})

DEFAULT_POPSIZE = 16
DEFAULT_MAX_GEN = 50
DEFAULT_GAMES_PER_CAND = 50
DEFAULT_TIME_MS = 200
DEFAULT_WORKERS = 10
DEFAULT_SEED = 42

# Opponent-pool promotion (S5 Stage 2A — saturation handling).
# When CMA-ES drives population winrate above the trigger sustained for
# `CONSEC` generations, the best-of-gen weight set joins the opponent
# pool. Subsequent candidate matches stratify games across pool members,
# so the fitness signal scales with the pool's strength rather than
# saturating at "always wins vs single fixed baseline".
#
# Statistical caveat: best-of-N selection bias inflates apparent winrate.
# At 16 candidates × 50 games, max-of-16 from a population at true 0.55
# crosses 0.65 ~94% of the time by chance vs only ~24% for 0.70. We
# default to 0.70 with 2-consec-gen requirement (combined FPR ~5%) +
# anchor always in pool — a false promotion only dilutes signal, never
# destroys it, since every candidate still plays ~10g vs anchor.
DEFAULT_POOL_MAX_SIZE = 5
# 0.70 ≈ +147 Elo. Sweet spot: fires reliably once CMA-ES has found a
# +100 Elo improvement (pop mean ≈ 0.62 → max-of-16 ≥ 0.70 about 65% of
# gens, 2-consec ~42%). Near baseline (pop mean 0.55), 2-consec FP rate
# stays under 5% — false promotions just dilute fitness signal, anchor
# keeps clean baseline available. Bump to 0.72/0.75 for fewer/later
# promos; drop to 0.65 only if you want noise in the pool (selection-
# bias FPR at 2-consec is ~88%).
DEFAULT_PROMOTE_MIN_SCORE = 0.70
DEFAULT_PROMOTE_CONSEC_GENS = 2
# Label for the anchor pool member — the .bestref Python reference
# binary with no eval overrides. Always present; never evicted.
_ANCHOR_LABEL = "bestref"

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

    Shape/fork weights in ``_SHAPE_BOUND_PARAMS`` are additionally
    capped at ``_SHAPE_SCORE_CAP`` (= mate_score - 1) to prevent eval
    overflow breaking search's mate-score adjustment. CMA-ES candidates
    that fall above this in log-space all decode to the same capped
    value: gradient is flat in that region, CMA-ES learns to avoid it.
    """
    vals = [max(DECODE_FLOOR, int(round(10 ** xi))) for xi in x]
    by_name = dict(zip(PARAM_NAMES, vals, strict=True))
    for name in _SHAPE_BOUND_PARAMS:
        if by_name[name] > _SHAPE_SCORE_CAP:
            by_name[name] = _SHAPE_SCORE_CAP
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


# ─── Opponent pool ──────────────────────────────────────────────────────────


@dataclass
class PoolMember:
    """One opponent in the rolling pool.

    ``overrides=None`` is the anchor: the .bestref worktree Python binary
    with ``HEXO_EVAL_OVERRIDES`` stripped via ``env -u``. Anchor stays in
    the pool forever — it is the apples-to-apples comparator the final
    promotion-validation match uses, and per-candidate matches always
    play some games against it so we never lose ground-truth signal.

    ``overrides=<dict>`` is a promoted in-tree candidate: same Python
    interpreter as the candidate side, but with its weights pinned via
    a per-cmd ``env HEXO_EVAL_OVERRIDES=…`` prefix (overrides whatever
    the parent set for the candidate).
    """

    label: str
    overrides: dict[str, Any] | None
    promoted_at_gen: int = -1  # -1 for anchor

    @property
    def is_anchor(self) -> bool:
        return self.overrides is None


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


def _opponent_cmd(member: PoolMember, reference_binary: Path,
                   tt_mb: int) -> list[str]:
    """Resolve a pool member to a subprocess cmd line.

    Anchor → reference binary with override env stripped.
    Promoted → in-tree binary with override pinned via per-cmd env.
    Per-cmd `env VAR=val cmd` sets the variable for ONLY that subprocess,
    overriding the parent's `HEXO_EVAL_OVERRIDES` (which is set for the
    candidate side via os.environ).
    """
    if member.is_anchor:
        return _reference_cmd(reference_binary, tt_mb)
    return [
        "env",
        f"{_EVAL_OVERRIDES_ENV}={json.dumps(member.overrides)}",
        sys.executable, "-m", "hammerhead.cli", "bot",
        "--tt-size-mb", str(tt_mb),
    ]


def _split_games(total: int, n_members: int) -> list[int]:
    """Stratify `total` games across `n_members` opponents as evenly as
    possible. Leftover from integer division goes to the first members."""
    base, rem = divmod(total, n_members)
    return [base + (1 if i < rem else 0) for i in range(n_members)]


# ─── Per-candidate match ────────────────────────────────────────────────────


@dataclass(frozen=True, slots=True)
class CandidateResult:
    """One candidate's aggregate match outcome across the opponent pool.

    `score` is the pool-wide (wins + 0.5*draws) / games — the value
    CMA-ES sees and ranks by. `per_member` holds the same statistic
    broken out by pool member so we can audit (a) anchor-only score
    for ground-truth comparison and (b) which promoted members ate
    most of the losses.
    """

    gen: int
    cand_idx: int
    params: dict[str, Any]
    score: float       # pool-aggregate (wins + 0.5*draws) / games
    wins: int
    losses: int
    draws: int
    games_played: int
    wall_seconds: float
    per_member: list[dict[str, Any]]
    anchor_score: float  # score restricted to anchor games, NaN if 0 games


def run_one_candidate(
    *,
    x: list[float],
    gen: int,
    cand_idx: int,
    pool: list[PoolMember],
    reference_binary: Path,
    n_games: int,
    time_ms: int,
    n_workers: int,
    max_plies: int,
) -> CandidateResult:
    """Stratify n_games across the opponent pool; aggregate outcomes.

    Each pool member gets ceil(n_games / |pool|) or floor(n_games / |pool|)
    games against the candidate. One `run_match_parallel` per member —
    cleanest reuse of the existing pool-of-workers infrastructure (each
    call uses a single fixed `best_cmd`).
    """
    overrides = decode_vector(x)
    tt_mb = promote_mod.max_tt_mb_per_worker()
    cur = _candidate_cmd(tt_mb)

    splits = _split_games(n_games, len(pool))

    prev = os.environ.get(_EVAL_OVERRIDES_ENV)
    os.environ[_EVAL_OVERRIDES_ENV] = json.dumps(overrides)
    t0 = time.monotonic()

    tot_w = tot_l = tot_d = tot_n = 0
    per_member: list[dict[str, Any]] = []
    anchor_w = anchor_l = anchor_d = anchor_n = 0
    try:
        for member, gpm in zip(pool, splits, strict=True):
            if gpm < 1:
                per_member.append({
                    "label": member.label, "anchor": member.is_anchor,
                    "games": 0, "wins": 0, "losses": 0, "draws": 0,
                    "score": None,
                })
                continue
            opp_cmd = _opponent_cmd(member, reference_binary, tt_mb)
            cfg = promote_mod.MatchConfig(
                n_games=gpm,
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
            res = promote_mod.run_match_parallel(
                cur, opp_cmd, cfg, n_workers=n_workers
            )
            tot_w += res.current_wins
            tot_l += res.best_wins
            tot_d += res.draws
            tot_n += res.games_played
            m_score = ((res.current_wins + 0.5 * res.draws)
                       / max(1, res.games_played))
            per_member.append({
                "label": member.label, "anchor": member.is_anchor,
                "games": res.games_played,
                "wins": res.current_wins, "losses": res.best_wins,
                "draws": res.draws, "score": m_score,
            })
            if member.is_anchor:
                anchor_w += res.current_wins
                anchor_l += res.best_wins
                anchor_d += res.draws
                anchor_n += res.games_played
    finally:
        if prev is None:
            os.environ.pop(_EVAL_OVERRIDES_ENV, None)
        else:
            os.environ[_EVAL_OVERRIDES_ENV] = prev
    wall = time.monotonic() - t0

    score = (tot_w + 0.5 * tot_d) / max(1, tot_n)
    anchor_score = (
        (anchor_w + 0.5 * anchor_d) / anchor_n if anchor_n > 0 else float("nan")
    )
    return CandidateResult(
        gen=gen, cand_idx=cand_idx, params=overrides,
        score=score, wins=tot_w, losses=tot_l, draws=tot_d,
        games_played=tot_n, wall_seconds=wall,
        per_member=per_member, anchor_score=anchor_score,
    )


# ─── Pool promotion ─────────────────────────────────────────────────────────


def maybe_promote_to_pool(
    *,
    pool: list[PoolMember],
    best_of_gen_history: list[dict[str, Any]],
    gen_results: list[CandidateResult],
    current_gen: int,
    promote_min_score: float,
    promote_consec_gens: int,
    pool_max_size: int,
) -> tuple[list[PoolMember], list[dict[str, Any]], str | None]:
    """Decide whether the best candidate of this gen joins the opponent pool.

    Trigger: best-of-gen pool-aggregate score ≥ ``promote_min_score`` in
    each of the last ``promote_consec_gens`` consecutive generations.

    On promotion: append the most recent best candidate to the pool,
    drop the OLDEST non-anchor entry if the pool exceeds ``pool_max_size``,
    and clear the history (cooldown — need another N consec gens before
    next promotion). Anchor is never evicted.

    Returns (new_pool, new_history, promoted_label_or_None).
    """
    if not gen_results:
        return pool, best_of_gen_history, None
    best = max(gen_results, key=lambda r: r.score)
    history = list(best_of_gen_history) + [{
        "gen": current_gen,
        "score": best.score,
        "anchor_score": best.anchor_score,
        "params": best.params,
        "cand_idx": best.cand_idx,
    }]

    if len(history) < promote_consec_gens:
        return pool, history, None
    recent = history[-promote_consec_gens:]
    if not all(h["score"] >= promote_min_score for h in recent):
        return pool, history, None

    # Don't double-promote identical weights (same dict already in pool).
    promo_params = recent[-1]["params"]
    if any(m.overrides == promo_params for m in pool if not m.is_anchor):
        # Same vector already in pool — clear history so we can promote
        # a different vector after another streak.
        return pool, [], None

    label = (
        f"gen{current_gen:03d}_cand{recent[-1]['cand_idx']:02d}"
        f"_s{recent[-1]['score']:.3f}"
    )
    new_member = PoolMember(
        label=label, overrides=promo_params, promoted_at_gen=current_gen,
    )
    new_pool = list(pool) + [new_member]
    if len(new_pool) > pool_max_size:
        # Evict oldest non-anchor.
        for i, m in enumerate(new_pool):
            if not m.is_anchor:
                new_pool.pop(i)
                break
    # Clear streak — cooldown until another `promote_consec_gens` qualify.
    return new_pool, [], label


# ─── Checkpointing ──────────────────────────────────────────────────────────


# Schema version 2 adds `pool` + `best_of_gen_history`. v1 checkpoints
# load with both defaulted (single-anchor pool, empty history).
_CHECKPOINT_SCHEMA_VERSION = 2


def _checkpoint_path(out_dir: Path) -> Path:
    return out_dir / "checkpoint.pkl"


def _save_checkpoint(out_dir: Path, *, es: cma.CMAEvolutionStrategy,
                     gen: int, best_x: list[float] | None,
                     best_score: float,
                     pool: list[PoolMember],
                     best_of_gen_history: list[dict[str, Any]]) -> None:
    """Pickle CMA-ES state + best-so-far + pool. Atomic via write-tmp."""
    path = _checkpoint_path(out_dir)
    tmp = path.with_suffix(".tmp")
    with tmp.open("wb") as fh:
        pickle.dump({
            "schema_version": _CHECKPOINT_SCHEMA_VERSION,
            "es": es,
            "gen": gen,
            "best_x": best_x,
            "best_score": best_score,
            "pool": pool,
            "best_of_gen_history": best_of_gen_history,
        }, fh)
    os.replace(tmp, path)


def _load_checkpoint(out_dir: Path) -> dict[str, Any] | None:
    """Load checkpoint with backward-compat: v1 checkpoints (pre-pool)
    default pool to [anchor] and history to []."""
    path = _checkpoint_path(out_dir)
    if not path.exists():
        return None
    with path.open("rb") as fh:
        data = pickle.load(fh)
    data.setdefault("pool", None)
    data.setdefault("best_of_gen_history", [])
    return data


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
                            pool: list[PoolMember],
                            promoted_label: str | None,
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
                "anchor_score": (None if math.isnan(r.anchor_score)
                                  else r.anchor_score),
                "wins": r.wins,
                "losses": r.losses,
                "draws": r.draws,
                "games_played": r.games_played,
                "wall_seconds": r.wall_seconds,
                "per_member": r.per_member,
            } for r in results
        ],
        "pool": [
            {
                "label": m.label,
                "anchor": m.is_anchor,
                "promoted_at_gen": m.promoted_at_gen,
                "overrides": m.overrides,
            } for m in pool
        ],
        "promoted_this_gen": promoted_label,
        "es_mean_log10": list(es.mean),
        "es_sigma": float(es.sigma),
        "started_at": started_at,
        "finished_at": finished_at,
        "reference_binary": str(args.reference_binary),
        "time_ms_per_stone": args.time_ms,
        "games_per_cand": args.games_per_cand,
        "workers": args.workers,
        "promote_min_score": args.promote_min_score,
        "promote_consec_gens": args.promote_consec_gens,
        "pool_max_size": args.pool_max_size,
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
    promote_min_score: float
    promote_consec_gens: int
    pool_max_size: int
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

    # Default pool: anchor only. Resume restores whatever was saved.
    default_pool = [PoolMember(label=_ANCHOR_LABEL, overrides=None,
                                promoted_at_gen=-1)]

    ckpt = _load_checkpoint(args.output_dir) if args.resume else None
    if ckpt is not None:
        es: cma.CMAEvolutionStrategy = ckpt["es"]
        start_gen: int = int(ckpt["gen"]) + 1
        best_x: list[float] | None = ckpt["best_x"]
        best_score: float = float(ckpt["best_score"])
        pool: list[PoolMember] = ckpt.get("pool") or default_pool
        best_of_gen_history: list[dict[str, Any]] = ckpt.get(
            "best_of_gen_history") or []
        print(f"resumed from gen {ckpt['gen']}: best_score={best_score:.4f} "
              f"pool={[m.label for m in pool]}", flush=True)
    else:
        es = _make_es(args)
        start_gen = 0
        best_x = None
        best_score = -math.inf
        pool = default_pool
        best_of_gen_history = []

    print(f"CMA-ES dim={DIM} popsize={args.popsize} max_gen={args.max_gen} "
          f"games/cand={args.games_per_cand} time_ms={args.time_ms} "
          f"workers={args.workers} "
          f"promote≥{args.promote_min_score}×{args.promote_consec_gens}gens "
          f"pool_max={args.pool_max_size}", flush=True)

    for gen in range(start_gen, args.max_gen):
        if es.stop():
            print(f"CMA-ES converged at gen {gen}: {es.stop()}", flush=True)
            break

        candidates = es.ask()
        gen_started = _now_iso()
        results: list[CandidateResult] = []
        for i, x in enumerate(candidates):
            r = run_one_candidate(
                x=list(x), gen=gen, cand_idx=i, pool=pool,
                reference_binary=args.reference_binary,
                n_games=args.games_per_cand,
                time_ms=args.time_ms,
                n_workers=args.workers,
                max_plies=args.max_plies,
            )
            results.append(r)
            anchor_str = (
                f" anchor={r.anchor_score:.3f}"
                if not math.isnan(r.anchor_score) else ""
            )
            print(f"  gen {gen:3d} cand {i:2d}/{len(candidates)}: "
                  f"score={r.score:.3f}{anchor_str} "
                  f"W/L/D={r.wins}/{r.losses}/{r.draws} "
                  f"({r.wall_seconds:.1f}s)", flush=True)
            if r.score > best_score:
                best_score = r.score
                best_x = list(x)
                best_path = args.output_dir / "best.json"
                _atomic_write_json(best_path, {
                    "schema_version": SCHEMA_VERSION,
                    "gen": gen, "cand_idx": i,
                    "score": r.score,
                    "anchor_score": (None if math.isnan(r.anchor_score)
                                      else r.anchor_score),
                    "params": r.params,
                    "x_log10": list(x),
                    "per_member": r.per_member,
                    "pool_snapshot": [m.label for m in pool],
                    "found_at": _now_iso(),
                })
                print(f"  ↑ new best score={r.score:.3f} → {best_path}", flush=True)

        # CMA-ES minimises — feed negative pool-aggregate score.
        es.tell(candidates, [-r.score for r in results])

        # Promotion check after es.tell so the gen's signal is fully
        # absorbed into CMA-ES state before we change the opponent set.
        pool, best_of_gen_history, promoted_label = maybe_promote_to_pool(
            pool=pool,
            best_of_gen_history=best_of_gen_history,
            gen_results=results,
            current_gen=gen,
            promote_min_score=args.promote_min_score,
            promote_consec_gens=args.promote_consec_gens,
            pool_max_size=args.pool_max_size,
        )
        if promoted_label is not None:
            print(f"  ↑↑ pool promoted: +{promoted_label}; "
                  f"pool now {[m.label for m in pool]}", flush=True)

        gen_finished = _now_iso()
        _write_generation_json(args.output_dir, gen=gen, results=results,
                               es=es, pool=pool,
                               promoted_label=promoted_label,
                               started_at=gen_started,
                               finished_at=gen_finished, args=args)
        _save_checkpoint(args.output_dir, es=es, gen=gen,
                         best_x=best_x, best_score=best_score,
                         pool=pool,
                         best_of_gen_history=best_of_gen_history)

    print(f"done. best_score={best_score:.4f} "
          f"final_pool={[m.label for m in pool]}", flush=True)
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
    p.add_argument("--promote-min-score", type=float,
                   default=DEFAULT_PROMOTE_MIN_SCORE,
                   help=f"best-of-gen pool-aggregate score required to "
                        f"promote a candidate to the opponent pool "
                        f"(default: {DEFAULT_PROMOTE_MIN_SCORE}). Raise "
                        f"to 0.72/0.75 for stricter filtering.")
    p.add_argument("--promote-consec-gens", type=int,
                   default=DEFAULT_PROMOTE_CONSEC_GENS,
                   help=f"consecutive gens above --promote-min-score "
                        f"required (default: {DEFAULT_PROMOTE_CONSEC_GENS})")
    p.add_argument("--pool-max-size", type=int,
                   default=DEFAULT_POOL_MAX_SIZE,
                   help=f"max opponents in pool incl. anchor (default: "
                        f"{DEFAULT_POOL_MAX_SIZE}); oldest non-anchor evicts")
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
    promote_min = float(ns.promote_min_score)
    if not 0.0 < promote_min <= 1.0:
        raise SystemExit(f"--promote-min-score out of (0, 1]: {promote_min}")
    promote_consec = int(ns.promote_consec_gens)
    if promote_consec < 1:
        raise SystemExit(f"--promote-consec-gens must be >= 1, got {promote_consec}")
    pool_max = int(ns.pool_max_size)
    if pool_max < 1:
        raise SystemExit(f"--pool-max-size must be >= 1, got {pool_max}")
    return CmaArgs(
        reference_binary=ref, output_dir=out,
        popsize=popsize, max_gen=max_gen, games_per_cand=games,
        time_ms=int(ns.time_ms), workers=int(ns.workers),
        max_plies=int(ns.max_plies), seed=int(ns.seed),
        sigma0=float(ns.sigma0),
        promote_min_score=promote_min,
        promote_consec_gens=promote_consec,
        pool_max_size=pool_max,
        smoke=bool(ns.smoke),
        resume=bool(ns.resume),
    )


def main(argv: list[str] | None = None) -> int:
    ns = _build_parser().parse_args(argv)
    return run(_resolve(ns))


if __name__ == "__main__":
    raise SystemExit(main())
