#!/usr/bin/env python3
"""move_gen_cap probe — STEP 2: fixed-TIME head-to-head cap sweep.

THE GATE. Cap trades branching for depth, so it must be measured at fixed
time (real play), not fixed depth or bench NPS. Both sides are the SAME
in-tree binary (byte-identical at the default cap=24, which equals the
.bestref dedfbbb engine — no cross-commit build noise). The only difference
is the runtime `move_gen_cap` injected per side via the HEXO_SEARCH_PARAMS
env-bridge (current -> parent os.environ; opponent -> per-cmd env prefix).

For each swept cap N: current(cap=N) vs opponent(cap=24), opening-diverse.
The 24-vs-24 row is a control: byte-identical sides, expect Elo ~ 0.

Run (smoke):
  .venv/bin/python tools/cap_h2h.py --caps 12 --n-games 4 --workers 4
Run (full):
  .venv/bin/python tools/cap_h2h.py --caps 24 12 16 32 48 \\
      --n-games 200 --time-ms 500 --workers 14
"""
from __future__ import annotations

import argparse
import json
import os
import socket
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

_REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(_REPO_ROOT / "hammerhead"))

from hammerhead import promote as promote_mod  # noqa: E402
from hammerhead.config import CONFIG  # noqa: E402

_SEARCH_PARAMS_ENV = "HEXO_SEARCH_PARAMS"
_EVAL_OVERRIDES_ENV = "HEXO_EVAL_OVERRIDES"


def _in_tree_cmd(tt_mb: int) -> list[str]:
    return [sys.executable, "-m", "hammerhead.cli", "bot",
            "--tt-size-mb", str(tt_mb)]


def _in_tree_with_cap_cmd(cap: int, tt_mb: int) -> list[str]:
    """Same in-tree binary, pins its own HEXO_SEARCH_PARAMS via per-cmd env
    prefix (overrides whatever the parent set)."""
    return [
        "env", f"{_SEARCH_PARAMS_ENV}={json.dumps({'move_gen_cap': cap})}",
        sys.executable, "-m", "hammerhead.cli", "bot",
        "--tt-size-mb", str(tt_mb),
    ]


def _now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _run_one(cap: int, baseline: int, ns, tt_mb: int) -> dict:
    cur_cmd = _in_tree_cmd(tt_mb)
    opp_cmd = _in_tree_with_cap_cmd(baseline, tt_mb)
    cfg = promote_mod.MatchConfig(
        n_games=int(ns.n_games),
        time_ms_per_stone=int(ns.time_ms),
        test="wilson",
        sprt_elo_low=CONFIG.promote.sprt_elo_low,
        sprt_elo_high=CONFIG.promote.sprt_elo_high,
        sprt_alpha=CONFIG.promote.sprt_alpha,
        sprt_beta=CONFIG.promote.sprt_beta,
        wilson_min_lower=CONFIG.promote.wilson_min_lower,
        raw_min_winrate=CONFIG.promote.raw_min_winrate,
        color_balance=CONFIG.promote.color_balance,
        opening_diversity=True,
        max_plies=int(ns.max_plies),
    )

    print(f"\n=== cap {cap} (current) vs cap {baseline} (opponent) ===",
          flush=True)
    prev_sp = os.environ.get(_SEARCH_PARAMS_ENV)
    prev_ev = os.environ.get(_EVAL_OVERRIDES_ENV)
    os.environ[_SEARCH_PARAMS_ENV] = json.dumps({"move_gen_cap": cap})
    os.environ.pop(_EVAL_OVERRIDES_ENV, None)  # keep eval at hexo.toml default
    t0 = time.monotonic()
    try:
        res = promote_mod.run_match_parallel(
            cur_cmd, opp_cmd, cfg, n_workers=int(ns.workers),
        )
    finally:
        if prev_sp is None:
            os.environ.pop(_SEARCH_PARAMS_ENV, None)
        else:
            os.environ[_SEARCH_PARAMS_ENV] = prev_sp
        if prev_ev is not None:
            os.environ[_EVAL_OVERRIDES_ENV] = prev_ev
    wall = time.monotonic() - t0
    elo_lo, elo_hi = res.estimated_elo_ci
    print(f"  W/L/D: {res.current_wins}/{res.best_wins}/{res.draws}  "
          f"winrate {res.winrate:.4f}", flush=True)
    print(f"  Elo {res.estimated_elo:+.1f}  CI [{elo_lo:+.1f}, {elo_hi:+.1f}]  "
          f"verdict {res.final_verdict}  wall {wall:.1f}s", flush=True)
    return {
        "cap": cap,
        "baseline": baseline,
        "games_played": res.games_played,
        "wins": res.current_wins,
        "losses": res.best_wins,
        "draws": res.draws,
        "winrate": res.winrate,
        "wilson_lower": res.wilson_lower,
        "wilson_upper": res.wilson_upper,
        "elo": res.estimated_elo,
        "elo_ci_lower": elo_lo,
        "elo_ci_upper": elo_hi,
        "final_verdict": res.final_verdict,
        "wall_seconds": wall,
    }


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="cap_h2h")
    p.add_argument("--caps", type=int, nargs="+",
                   default=[24, 12, 16, 32, 48],
                   help="current-side caps to sweep (each vs --baseline-cap)")
    p.add_argument("--baseline-cap", type=int, default=24)
    p.add_argument("--n-games", type=int, default=200)
    p.add_argument("--time-ms", type=int, default=500)
    p.add_argument("--workers", type=int, default=14)
    p.add_argument("--max-plies", type=int,
                   default=CONFIG.promote.default_max_plies)
    p.add_argument("--output", default="tools/cap_output/h2h_sweep.json")
    ns = p.parse_args(argv)

    tt_mb = promote_mod.max_tt_mb_per_worker()
    print(f"cap H2H sweep: caps={ns.caps} baseline={ns.baseline_cap} "
          f"n_games={ns.n_games} time_ms={ns.time_ms} workers={ns.workers} "
          f"tt_mb={tt_mb} opening_diverse=True", flush=True)

    started = _now_iso()
    rows = [_run_one(c, ns.baseline_cap, ns, tt_mb) for c in ns.caps]

    print("\n" + "=" * 64)
    print(f"{'cap':>4}  {'W-L-D':>14}  {'winrate':>8}  "
          f"{'Elo':>7}  {'CI':>18}  verdict")
    for r in rows:
        wld = f"{r['wins']}-{r['losses']}-{r['draws']}"
        ci = f"[{r['elo_ci_lower']:+.0f},{r['elo_ci_upper']:+.0f}]"
        base = " *control*" if r["cap"] == ns.baseline_cap else ""
        print(f"{r['cap']:>4}  {wld:>14}  {r['winrate']:>8.4f}  "
              f"{r['elo']:>+7.1f}  {ci:>18}  {r['final_verdict']}{base}")

    out_path = Path(ns.output).expanduser()
    if not out_path.is_absolute():
        out_path = _REPO_ROOT / out_path
    out_path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema_version": 1,
        "baseline_cap": ns.baseline_cap,
        "n_games": ns.n_games,
        "time_ms_per_stone": ns.time_ms,
        "workers": ns.workers,
        "max_plies": ns.max_plies,
        "opening_diversity": True,
        "rows": rows,
        "started_at": started,
        "finished_at": _now_iso(),
        "host": socket.gethostname(),
    }
    tmp = out_path.with_suffix(out_path.suffix + ".tmp")
    with tmp.open("w") as fh:
        json.dump(payload, fh, indent=2, sort_keys=True)
        fh.write("\n")
    os.replace(tmp, out_path)
    print(f"\nreport: {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
