#!/usr/bin/env python3
"""move_gen_cap probe — NPS + depth-reached context across caps.

CONTEXT ONLY (not a gate). Shows the branching<->depth tradeoff: tighter
cap -> higher NPS + deeper; wider cap -> lower NPS + shallower. Measured at
the play-like per-stone budget (500ms) on representative fixtures so the
depth numbers reflect real play, not micro-bench conditions.

Run: .venv/bin/python tools/cap_nps.py --time-ms 500 --trials 3
"""
from __future__ import annotations

import argparse
import json
import statistics
import sys

from hammerhead_engine import Engine

CAPS = [8, 12, 16, 24, 32, 48, 64, 96]
FIXTURES = ["midgame_12", "midgame_30", "endgame_60", "fork_two_open_4"]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--time-ms", type=int, default=500)
    ap.add_argument("--trials", type=int, default=3)
    ap.add_argument("--tt-mb", type=int, default=64)
    args = ap.parse_args()

    fx = json.load(open("benches/fixtures/positions.json"))
    moves_by_fx = {name: fx[name]["moves"] for name in FIXTURES}

    print(f"budget={args.time_ms}ms  trials={args.trials}  "
          f"fixtures={FIXTURES}", file=sys.stderr)
    rows = []
    for cap in CAPS:
        nps_samples = []
        depth_samples = []
        for name, mvs in moves_by_fx.items():
            for _ in range(args.trials):
                e = Engine(tt_size_mb=args.tt_mb)
                e.set_search_params({"move_gen_cap": cap})
                for (q, r) in mvs:
                    e.place((q, r))
                _q, _r, _s, depth, nodes, tms = e.bench_best_move(
                    time_ms=args.time_ms)
                if tms > 0:
                    nps_samples.append(nodes / (tms / 1000.0))
                depth_samples.append(depth)
        nps = statistics.median(nps_samples)
        dep = statistics.median(depth_samples)
        rows.append((cap, nps, dep))

    base_nps = next(n for c, n, _ in rows if c == 24)
    base_dep = next(d for c, _, d in rows if c == 24)
    print()
    print(f"{'cap':>4}  {'NPS':>10}  {'NPS/24':>7}  {'depth':>6}  {'Δdepth':>6}")
    for cap, nps, dep in rows:
        tag = " (baseline)" if cap == 24 else ""
        print(f"{cap:>4}  {nps:>10,.0f}  {nps / base_nps:>6.2f}x  "
              f"{dep:>6.1f}  {dep - base_dep:>+6.1f}{tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
