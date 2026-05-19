#!/usr/bin/env python3
"""Phase 14 STEP 9 — PGO training workload.

Runs a representative HeXO search so the LLVM profile-generate build
records hot-path coverage. Targets ~30 s total wall-clock split across
the canonical bench fixtures. The output `.profraw` files land under
``$HEXO_PGO_DATA`` (set by ``scripts/pgo_build.sh``) and the outer
script merges them via ``llvm-profdata merge``.

Invocation:
    HEXO_PGO_DATA=/tmp/hexo-pgo .venv/bin/python scripts/pgo_training.py
"""
from __future__ import annotations

import os
import sys
import time

# Ensure the engine module is importable even when invoked from a
# non-repo cwd.
HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(REPO_ROOT, "hexo"))

from hexo import benchmark  # noqa: E402


# Order: midgame fixtures dominate hot-path coverage in real games.
# Append `single_origin` so opening-tree node distributions get
# touched too.
FIXTURES = ("midgame_12", "midgame_30", "single_origin")
DEPTHS = (6, 6, 6)


def main() -> int:
    total = 0
    t0 = time.perf_counter()
    for fixture, depth in zip(FIXTURES, DEPTHS):
        eng = benchmark.load_fixture(fixture)
        _q, _r, _s, depth_reached, nodes, t_ms = eng.bench_best_move(
            depth=depth
        )
        total += int(nodes)
        print(
            f"pgo training: {fixture} depth={depth_reached}"
            f" nodes={int(nodes):,} ms={int(t_ms)}",
            flush=True,
        )
    wall = time.perf_counter() - t0
    print(
        f"pgo training: total_nodes={total:,} wall={wall:.2f}s",
        flush=True,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
