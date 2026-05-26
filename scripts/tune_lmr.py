#!/usr/bin/env python3
"""Sprint 4B — LMR Texel sweep driver.

Three-stage retune of the LMR triplet (lmr_min_depth, lmr_min_move_index,
lmr_reduction) for the post-Sprint-3 NPS regime. Reuses
hammerhead.promote.run_match_parallel as the match pool — same A/B
isolation pattern as hammerhead.tune for eval overrides:

- Candidate worker inherits HEXO_SEARCH_PARAMS in env (set by the
  parent before pool spawn).
- Baseline worker runs under ``env -u HEXO_SEARCH_PARAMS`` so it
  always reads hexo.toml defaults — byte-identical to no setter call.

Usage:

    # Stage 1 — 24-cell grid screen
    python scripts/tune_lmr.py --stage 1 --n 80 --time-ms 250

    # Stage 2 — top-N from Stage 1
    python scripts/tune_lmr.py --stage 2 --n 400 --time-ms 500 \\
        --cells "3,4,1;3,6,2;3,8,2;4,6,1;2,6,1"

    # Stage 3 — single winner vs .bestref defaults
    python scripts/tune_lmr.py --stage 3 --n 400 --time-ms 500 \\
        --cells "3,8,2"

CSV output: /tmp/sprint_4/B_lmr_stage<N>.csv with header
    min_depth,min_move_index,reduction,n,w,l,d,mean_elo,ci_lower,ci_upper

Corrected mean = mean - 10 (per feedback_arena_correction_factor).
The CSV stores RAW values; correction is applied at triage time so
the driver remains a thin layer over the match pool.
"""

from __future__ import annotations

import argparse
import csv
import itertools
import json
import os
import sys
import time
from pathlib import Path
from typing import Iterable

REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "hammerhead"))

from hammerhead import promote as promote_mod  # noqa: E402
from hammerhead.config import CONFIG  # noqa: E402

# Stage 1 grid — 24 cells. Mid-Sprint-4 commit's defaults (3, 6, 1)
# sit at (md=3, mi=6, r=1) inside this grid.
STAGE_1_GRID: list[tuple[int, int, int]] = list(
    itertools.product([2, 3, 4], [4, 6, 8, 12], [1, 2])
)

SEARCH_PARAMS_ENV = "HEXO_SEARCH_PARAMS"
OUT_DIR = Path("/tmp/sprint_4")


def _cell_to_dict(cell: tuple[int, int, int]) -> dict[str, int]:
    md, mi, r = cell
    return {
        "lmr_min_depth": md,
        "lmr_min_move_index": mi,
        "lmr_reduction": r,
    }


def _parse_cells(spec: str) -> list[tuple[int, int, int]]:
    """`"3,4,1;3,6,2"` → `[(3, 4, 1), (3, 6, 2)]`. Order preserved."""
    out: list[tuple[int, int, int]] = []
    for chunk in spec.split(";"):
        chunk = chunk.strip()
        if not chunk:
            continue
        parts = [p.strip() for p in chunk.split(",")]
        if len(parts) != 3:
            raise ValueError(
                f"cell must be 'md,mi,r' (3 ints), got {chunk!r}"
            )
        out.append((int(parts[0]), int(parts[1]), int(parts[2])))
    if not out:
        raise ValueError("--cells must specify at least one cell")
    return out


def _bot_cmd(tt_mb: int) -> list[str]:
    return [
        sys.executable,
        "-m",
        "hammerhead.cli",
        "bot",
        "--tt-size-mb",
        str(tt_mb),
    ]


def _wrap_baseline(cmd: list[str]) -> list[str]:
    """Prefix with `env -u HEXO_SEARCH_PARAMS` so the baseline ignores
    whatever the candidate worker carries."""
    return ["env", "-u", SEARCH_PARAMS_ENV, *cmd]


def run_cell(
    cell: tuple[int, int, int],
    *,
    n_games: int,
    time_ms: int,
    workers: int,
) -> tuple[int, int, int, int, float, float, float]:
    """Return (n, w, l, d, mean_elo, ci_lower, ci_upper) for one cell."""
    params_json = json.dumps(_cell_to_dict(cell))
    tt_mb = promote_mod.max_tt_mb_per_worker()
    base_cmd = _bot_cmd(tt_mb)
    current_cmd = list(base_cmd)
    best_cmd = _wrap_baseline(list(base_cmd))

    cfg = promote_mod.MatchConfig(
        n_games=n_games,
        time_ms_per_stone=time_ms,
        test="wilson",
        sprt_elo_low=CONFIG.promote.sprt_elo_low,
        sprt_elo_high=CONFIG.promote.sprt_elo_high,
        sprt_alpha=CONFIG.promote.sprt_alpha,
        sprt_beta=CONFIG.promote.sprt_beta,
        wilson_min_lower=CONFIG.promote.wilson_min_lower,
        raw_min_winrate=CONFIG.promote.raw_min_winrate,
        color_balance=CONFIG.promote.color_balance,
        opening_diversity=False,
        max_plies=CONFIG.promote.default_max_plies,
    )

    prev = os.environ.get(SEARCH_PARAMS_ENV)
    os.environ[SEARCH_PARAMS_ENV] = params_json
    try:
        res = promote_mod.run_match_parallel(
            current_cmd, best_cmd, cfg, n_workers=workers
        )
    finally:
        if prev is None:
            os.environ.pop(SEARCH_PARAMS_ENV, None)
        else:
            os.environ[SEARCH_PARAMS_ENV] = prev
    elo_lo, elo_hi = res.estimated_elo_ci
    return (
        res.games_played,
        res.current_wins,
        res.best_wins,
        res.draws,
        res.estimated_elo,
        elo_lo,
        elo_hi,
    )


def _csv_path(stage: int) -> Path:
    return OUT_DIR / f"B_lmr_stage{stage}.csv"


def _open_csv(stage: int):
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    f = _csv_path(stage).open("w", newline="")
    w = csv.writer(f)
    w.writerow(
        [
            "min_depth",
            "min_move_index",
            "reduction",
            "n",
            "w",
            "l",
            "d",
            "mean_elo",
            "ci_lower",
            "ci_upper",
        ]
    )
    return f, w


def run_stage(
    stage: int,
    cells: Iterable[tuple[int, int, int]],
    *,
    n_games: int,
    time_ms: int,
    workers: int,
) -> None:
    f, writer = _open_csv(stage)
    try:
        for i, cell in enumerate(cells, start=1):
            md, mi, r = cell
            t0 = time.time()
            print(
                f"\n[stage {stage} cell {i}] md={md} mi={mi} r={r} "
                f"n={n_games} time_ms={time_ms} workers={workers}",
                flush=True,
            )
            n, w, l, d, mean, lo, hi = run_cell(
                cell, n_games=n_games, time_ms=time_ms, workers=workers
            )
            corrected = mean - 10.0
            dt = time.time() - t0
            print(
                f"  ({md},{mi},{r}) -> n={n} w-l-d={w}-{l}-{d} "
                f"mean={mean:+.1f} (corrected {corrected:+.1f}) "
                f"CI=[{lo:+.1f}, {hi:+.1f}] {dt:.1f}s",
                flush=True,
            )
            writer.writerow(
                [md, mi, r, n, w, l, d, f"{mean:.2f}",
                 f"{lo:.2f}", f"{hi:.2f}"]
            )
            f.flush()
    finally:
        f.close()


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--stage", type=int, required=True, choices=(1, 2, 3))
    ap.add_argument("--n", type=int, default=80,
                    help="games per cell (default 80 for stage 1; "
                         "use 400 for stages 2 and 3)")
    ap.add_argument("--time-ms", type=int, default=250,
                    help="per-stone time budget (default 250)")
    ap.add_argument("--cells", type=str, default=None,
                    help="cell list for stage 2/3, "
                         "format 'md,mi,r;md,mi,r;...'")
    ap.add_argument("--workers", type=int, default=10,
                    help="parallel match workers (default 10)")
    args = ap.parse_args(argv)

    if args.stage == 1:
        cells = STAGE_1_GRID
    else:
        if not args.cells:
            ap.error(f"--cells required for stage {args.stage}")
        cells = _parse_cells(args.cells)
        if args.stage == 3 and len(cells) != 1:
            ap.error("stage 3 expects exactly 1 cell (the Stage-2 winner)")

    run_stage(
        args.stage,
        cells,
        n_games=args.n,
        time_ms=args.time_ms,
        workers=args.workers,
    )
    print(f"\nWrote {_csv_path(args.stage)}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
