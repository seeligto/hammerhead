"""Head-to-head match between two CMA-ES-tuned eval-weight sets.

Drops the .bestref anchor: both sides are the same in-tree binary,
each with its own override dict supplied via the HEXO_EVAL_OVERRIDES
env-bridge (current → parent os.environ; opponent → per-cmd env prefix).
This sidesteps the noise of "vs anchor" and tells you directly which
of two candidates wins more games.

Usage::

    python tools/cma_h2h.py \\
        --current-best-json tools/cma_output/run01/validate/cand_04_g046c08.json \\
        --opponent-best-json tools/cma_output/run01/validate/cand_01_g017c07.json \\
        --n-games 200 --time-ms 500 --workers 14

Exits 0 on PROMOTE (current > opponent), 1 on REJECT, 2 on INCONCLUSIVE.
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

_REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(_REPO_ROOT / "hammerhead"))

from hammerhead import promote as promote_mod  # noqa: E402
from hammerhead.config import CONFIG  # noqa: E402

_EVAL_OVERRIDES_ENV = "HEXO_EVAL_OVERRIDES"


def _in_tree_cmd(tt_mb: int) -> list[str]:
    return [sys.executable, "-m", "hammerhead.cli", "bot",
            "--tt-size-mb", str(tt_mb)]


def _in_tree_with_overrides_cmd(overrides: dict[str, Any],
                                 tt_mb: int) -> list[str]:
    """Same in-tree binary, but pins its own HEXO_EVAL_OVERRIDES via
    per-cmd `env` prefix (overrides whatever the parent set)."""
    return [
        "env", f"{_EVAL_OVERRIDES_ENV}={json.dumps(overrides)}",
        sys.executable, "-m", "hammerhead.cli", "bot",
        "--tt-size-mb", str(tt_mb),
    ]


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


def _load_overrides(path: Path) -> dict[str, Any]:
    with path.open() as fh:
        data = json.load(fh)
    ov = data.get("params")
    if not isinstance(ov, dict):
        raise SystemExit(f"{path}: missing/invalid 'params' dict")
    return ov


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        prog="cma_h2h",
        description="Head-to-head match between two eval-weight sets.",
    )
    p.add_argument("--current-best-json", required=True,
                   help="JSON with .params for the 'current' side")
    p.add_argument("--opponent-best-json", required=True,
                   help="JSON with .params for the 'opponent' side")
    p.add_argument("--n-games", type=int, default=200)
    p.add_argument("--time-ms", type=int, default=500)
    p.add_argument("--workers", type=int, default=14)
    p.add_argument("--max-plies", type=int,
                   default=CONFIG.promote.default_max_plies)
    p.add_argument("--test", choices=["wilson", "sprt", "raw"],
                   default="wilson")
    p.add_argument("--output", default=None,
                   help="path to write JSON report; defaults to "
                        "<current-best-json>'s dir / h2h_<curr>_vs_<opp>.json")
    ns = p.parse_args(argv)

    cur_path = Path(ns.current_best_json).expanduser().resolve()
    opp_path = Path(ns.opponent_best_json).expanduser().resolve()
    cur_overrides = _load_overrides(cur_path)
    opp_overrides = _load_overrides(opp_path)

    tt_mb = promote_mod.max_tt_mb_per_worker()
    cur_cmd = _in_tree_cmd(tt_mb)
    opp_cmd = _in_tree_with_overrides_cmd(opp_overrides, tt_mb)
    cfg = promote_mod.MatchConfig(
        n_games=int(ns.n_games),
        time_ms_per_stone=int(ns.time_ms),
        test=ns.test,
        sprt_elo_low=CONFIG.promote.sprt_elo_low,
        sprt_elo_high=CONFIG.promote.sprt_elo_high,
        sprt_alpha=CONFIG.promote.sprt_alpha,
        sprt_beta=CONFIG.promote.sprt_beta,
        wilson_min_lower=CONFIG.promote.wilson_min_lower,
        raw_min_winrate=CONFIG.promote.raw_min_winrate,
        color_balance=CONFIG.promote.color_balance,
        opening_diversity=False,
        max_plies=int(ns.max_plies),
    )

    print(f"H2H: {cur_path.name} (current) vs {opp_path.name} (opponent)")
    print(f"  current overrides:  {cur_overrides}")
    print(f"  opponent overrides: {opp_overrides}")
    print(f"  n_games={cfg.n_games} time_ms={cfg.time_ms_per_stone} "
          f"workers={ns.workers} test={cfg.test}")

    started_at = _now_iso()
    prev = os.environ.get(_EVAL_OVERRIDES_ENV)
    os.environ[_EVAL_OVERRIDES_ENV] = json.dumps(cur_overrides)
    t0 = time.monotonic()
    try:
        res = promote_mod.run_match_parallel(
            cur_cmd, opp_cmd, cfg, n_workers=int(ns.workers),
        )
    finally:
        if prev is None:
            os.environ.pop(_EVAL_OVERRIDES_ENV, None)
        else:
            os.environ[_EVAL_OVERRIDES_ENV] = prev
    wall = time.monotonic() - t0
    finished_at = _now_iso()

    print()
    print(f"games:    {res.games_played}")
    print(f"W/L/D:    {res.current_wins}/{res.best_wins}/{res.draws}  "
          f"(current/opponent/draws)")
    print(f"winrate:  {res.winrate:.4f}  Wilson [{res.wilson_lower:.4f}, "
          f"{res.wilson_upper:.4f}]")
    elo_lo, elo_hi = res.estimated_elo_ci
    print(f"Elo:      {res.estimated_elo:+.1f}  CI [{elo_lo:+.1f}, "
          f"{elo_hi:+.1f}]")
    print(f"verdict:  {res.final_verdict}")
    print(f"wall:     {wall:.1f}s")

    if ns.output:
        out_path = Path(ns.output).expanduser().resolve()
    else:
        out_path = cur_path.parent / (
            f"h2h_{cur_path.stem}_vs_{opp_path.stem}_{cfg.n_games}g.json"
        )
    payload: dict[str, Any] = {
        "schema_version": 1,
        "current_best_json": str(cur_path),
        "opponent_best_json": str(opp_path),
        "current_params": cur_overrides,
        "opponent_params": opp_overrides,
        "n_games": cfg.n_games,
        "time_ms_per_stone": cfg.time_ms_per_stone,
        "workers": int(ns.workers),
        "test": cfg.test,
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
        "started_at": started_at,
        "finished_at": finished_at,
        "host": socket.gethostname(),
        "git_sha": _git_sha(),
    }
    tmp = out_path.with_suffix(out_path.suffix + ".tmp")
    with tmp.open("w") as fh:
        json.dump(payload, fh, indent=2, sort_keys=True)
        fh.write("\n")
    os.replace(tmp, out_path)
    print(f"report:   {out_path}")

    if res.final_verdict == "PROMOTE":
        return 0
    if res.final_verdict == "REJECT":
        return 1
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
