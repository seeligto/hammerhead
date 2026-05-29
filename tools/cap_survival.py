#!/usr/bin/env python3
"""move_gen_cap probe — STEP 1: fixed-depth candidate-survival curve.

Question: at candidate cap N, does the deep-search-best move survive the
post-ordering truncation? Reuses the Stage-0 turn-boundary position
distribution (curated HeXOpedia openings + random seeds + off-policy
injection) and labels each position with a WIDE-cap (uncapped) deep search
so the labeled best move is NOT itself cap-biased.

Metric: rank of the deep-best move within the FRESH-state, uncapped ordered
candidate list (`Engine.debug_ordered_moves`) — exactly the ordering of the
search's first iterative-deepening iteration on a cleared TT, no killers, no
history. survival(N) = fraction of positions with rank < N.

Confound (per probe): this sizes the RAW iteration-1 drop only. TT/history/ID
recover dropped moves across later iterations — STEP 2 (fixed-time H2H) is the
real test. STEP 1 only tells whether H-wide is even plausible.

Run (smoke):
  .venv/bin/python tools/cap_survival.py --target 40 --deep-ms 800 --workers 8
Run (full):
  .venv/bin/python tools/cap_survival.py --target 300 --deep-ms 3000 --workers 14
"""
from __future__ import annotations

import argparse
import random
import sys
import time
from multiprocessing import Pool

from hammerhead_engine import Engine
from hammerhead.openings import OPENINGS

Coord = tuple[int, int]

CAPS = [8, 12, 16, 24, 32, 48, 64, 96]
_LABEL_CAP = 256  # >= max r=2 candidate count: effectively uncapped labelling

# Per-worker globals (set in pool initializer).
_ENG: Engine | None = None
_DEEP_MS: int = 0
_DEEP_DEPTH: int | None = None


# ── position generation (main process) ───────────────────────────────────────
def selfplay_moves(
    seed_plies: list[Coord], gen_ms: int, max_ply: int,
    rng: random.Random, inject_prob: float,
) -> list[Coord]:
    """Self-play from a seed opening; optional off-policy random injection."""
    e = Engine()
    moves: list[Coord] = []
    for (q, r) in seed_plies:
        e.place((q, r))
        moves.append((q, r))
    while e.ply() < max_ply and e.winner() is None:
        if inject_prob > 0.0 and rng.random() < inject_prob:
            cand = e.debug_ordered_moves()
            if not cand:
                break
            q, r = cand[rng.randrange(len(cand))]
        else:
            q, r = e.best_move(time_ms=gen_ms)
        e.place((q, r))
        moves.append((q, r))
    return moves


def turn_boundary_prefixes(moves: list[Coord]) -> list[tuple[Coord, ...]]:
    """Prefixes ending where the NEXT player starts a 2-stone turn
    (halfmove==0, ply>=1, no winner)."""
    out = []
    e = Engine()
    for i, (q, r) in enumerate(moves):
        e.place((q, r))
        if e.winner() is not None:
            break
        if e.halfmove() == 0 and e.ply() >= 1:
            out.append(tuple(moves[: i + 1]))
    return out


def random_opening(rng: random.Random, depth: int) -> list[Coord]:
    """X opens at origin, then `depth` near-random plies (top-half spread)."""
    e = Engine()
    seed: list[Coord] = [(0, 0)]
    e.place((0, 0))
    for _ in range(depth):
        if e.winner() is not None:
            break
        cand = e.debug_ordered_moves()
        if not cand:
            break
        k = max(1, len(cand) // 2)
        q, r = cand[rng.randrange(k)]
        e.place((q, r))
        seed.append((q, r))
    return seed


def generate_positions(args) -> list[tuple[Coord, ...]]:
    rng = random.Random(args.seed)
    seen: set[tuple[Coord, ...]] = set()
    positions: list[tuple[Coord, ...]] = []

    def harvest(moves: list[Coord]) -> None:
        for pref in turn_boundary_prefixes(moves):
            if pref not in seen:
                seen.add(pref)
                positions.append(pref)

    openings = list(OPENINGS)
    for _pass in range(args.curated_passes):
        for op in openings:
            seed_plies = [(q, r) for (_p, q, r) in op.plies]
            mv = selfplay_moves(seed_plies, args.gen_ms, args.max_ply, rng,
                                args.inject_prob)
            harvest(mv)
            if len(positions) >= args.target:
                return positions[: args.target]

    while len(positions) < args.target:
        depth = rng.randint(args.rand_open_min, args.rand_open_max)
        seed_plies = random_opening(rng, depth)
        mv = selfplay_moves(seed_plies, args.gen_ms, args.max_ply, rng,
                            args.inject_prob)
        harvest(mv)

    return positions[: args.target]


# ── deep-search labelling + rank (worker process) ────────────────────────────
def _worker_init(deep_ms: int, deep_depth: int | None, tt_mb: int) -> None:
    global _ENG, _DEEP_MS, _DEEP_DEPTH
    _ENG = Engine(tt_size_mb=tt_mb)
    _DEEP_MS = deep_ms
    _DEEP_DEPTH = deep_depth


def _label_one(moves: tuple[Coord, ...]) -> dict | None:
    e = _ENG
    e.reset()
    e.clear_tt()  # clean search per position
    e.set_search_params({"move_gen_cap": _LABEL_CAP})  # unbiased (uncapped) label
    for (q, r) in moves:
        e.place((q, r))
    if e.winner() is not None:
        return None
    if _DEEP_DEPTH is not None:
        bq, br, _s, depth, nodes, _t = e.bench_best_move(depth=_DEEP_DEPTH)
    else:
        bq, br, _s, depth, nodes, _t = e.bench_best_move(time_ms=_DEEP_MS)
    deep_best = (bq, br)
    # Fresh-state, uncapped ordering (independent of the search just run).
    ordered = e.debug_ordered_moves()
    n = len(ordered)
    try:
        rank = ordered.index(deep_best)
    except ValueError:
        # deep-best not a root candidate at DEFAULT_MOVE_RADIUS — drops at
        # every finite cap below n. Should be ~never (same generate()).
        rank = n
    return {"rank": rank, "n": n, "depth": int(depth), "nodes": int(nodes)}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", type=int, default=300)
    ap.add_argument("--deep-ms", type=int, default=3000)
    ap.add_argument("--deep-depth", type=int, default=None,
                    help="fixed-depth label instead of time (deterministic)")
    ap.add_argument("--gen-ms", type=int, default=80)
    ap.add_argument("--max-ply", type=int, default=30)
    ap.add_argument("--curated-passes", type=int, default=2)
    ap.add_argument("--inject-prob", type=float, default=0.15)
    ap.add_argument("--rand-open-min", type=int, default=2)
    ap.add_argument("--rand-open-max", type=int, default=8)
    ap.add_argument("--workers", type=int, default=14)
    ap.add_argument("--tt-mb", type=int, default=64)
    ap.add_argument("--seed", type=int, default=20260529)
    args = ap.parse_args()

    t0 = time.time()
    print(f"[gen] target={args.target} gen_ms={args.gen_ms} "
          f"max_ply={args.max_ply} passes={args.curated_passes} "
          f"inject={args.inject_prob}", file=sys.stderr)
    positions = generate_positions(args)
    print(f"[gen] {len(positions)} turn-boundary positions in "
          f"{time.time() - t0:.1f}s", file=sys.stderr)

    label = "depth=%d" % args.deep_depth if args.deep_depth else f"{args.deep_ms}ms"
    print(f"[label] deep={label} cap={_LABEL_CAP} workers={args.workers}",
          file=sys.stderr)
    t1 = time.time()
    with Pool(args.workers, initializer=_worker_init,
              initargs=(args.deep_ms, args.deep_depth, args.tt_mb)) as pool:
        recs = [r for r in pool.map(_label_one, positions) if r is not None]
    print(f"[label] {len(recs)} labelled in {time.time() - t1:.1f}s",
          file=sys.stderr)

    ranks = [r["rank"] for r in recs]
    ns = sorted(r["n"] for r in recs)
    depths = [r["depth"] for r in recs]
    m = len(ranks)
    if m == 0:
        print("no labelled positions", file=sys.stderr)
        return 1

    def pct(x: float) -> str:
        return f"{100.0 * x:5.1f}"

    med_n = ns[len(ns) // 2]
    med_depth = sorted(depths)[len(depths) // 2]
    print()
    print(f"positions: {m}   median candidates: {med_n}   "
          f"max candidates: {ns[-1]}   median label depth: {med_depth}")
    print()
    header = "cap:      " + "".join(f"{c:6d}" for c in CAPS)
    surv = "survive%: " + "".join(
        f"{pct(sum(1 for rk in ranks if rk < c) / m):>6}" for c in CAPS
    )
    print(header)
    print(surv)
    print()
    drop24 = sum(1 for rk in ranks if rk >= 24) / m
    print(f"deep-best drop at cap=24: {pct(drop24)}%   "
          f"(S7.4 reported 29-33%)")
    rsorted = sorted(ranks)
    print(f"rank distribution: min={rsorted[0]} "
          f"p50={rsorted[m // 2]} p90={rsorted[int(0.9 * m)]} "
          f"p99={rsorted[min(m - 1, int(0.99 * m))]} max={rsorted[-1]}")
    n_beyond = {c: sum(1 for rk in ranks if rk >= c) for c in CAPS}
    print("dropped count by cap: " +
          " ".join(f"{c}:{n_beyond[c]}" for c in CAPS))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
