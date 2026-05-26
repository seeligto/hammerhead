"""Phase S5 Stage 2A — final validation match for CMA-ES-tuned weights.

Loads the CMA-ES best.json, plays a long-format match (default 400g
@ 500ms) against the original .bestref binary (no overrides), reports
Wilson 95% CI on winrate + Elo + verdict. This is the apples-to-apples
gate the S5 plan calls for: even if CMA-ES used a pool of opponents
internally, the final number we publish must be vs the same fixed
baseline as every prior phase.

Usage::

    python tools/cma_validate.py \\
        --best-json tools/cma_output/run01/best.json \\
        --reference-binary /home/tom/Work/hammerhead/.worktree-best/.venv-best/bin/python \\
        --n-games 400 --time-ms 500 --workers 14

Exits 0 on PROMOTE, 1 on REJECT, 2 on INCONCLUSIVE.
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

DEFAULT_N_GAMES = 400
DEFAULT_TIME_MS = 500
DEFAULT_WORKERS = 14


def _candidate_cmd(tt_mb: int) -> list[str]:
    """In-tree engine with overrides inherited from parent env."""
    return [sys.executable, "-m", "hammerhead.cli", "bot",
            "--tt-size-mb", str(tt_mb)]


def _reference_cmd(reference_binary: Path, tt_mb: int) -> list[str]:
    """Fixed-SHA reference, override env stripped."""
    return ["env", "-u", _EVAL_OVERRIDES_ENV,
            str(reference_binary), "-m", "hammerhead.cli", "bot",
            "--tt-size-mb", str(tt_mb)]


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


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        prog="cma_validate",
        description="Validate CMA-ES best.json vs original .bestref "
                    "at long format (Phase S5 Stage 2A gate).",
    )
    p.add_argument("--best-json", required=True,
                   help="path to best.json emitted by cma_tune.py")
    p.add_argument("--reference-binary", required=True,
                   help="path to .bestref worktree's .venv-best/bin/python")
    p.add_argument("--n-games", type=int, default=DEFAULT_N_GAMES)
    p.add_argument("--time-ms", type=int, default=DEFAULT_TIME_MS)
    p.add_argument("--workers", type=int, default=DEFAULT_WORKERS)
    p.add_argument("--max-plies", type=int,
                   default=CONFIG.promote.default_max_plies)
    p.add_argument("--test", choices=["wilson", "sprt", "raw"],
                   default="wilson",
                   help="Wilson lower-bound (default), SPRT, or raw cutoff "
                        "— mirrors promote.MatchConfig.test")
    p.add_argument("--output", default=None,
                   help="optional path to write the JSON report; defaults "
                        "to <best-json>'s dir / validate_<n>g.json")
    ns = p.parse_args(argv)

    best_path = Path(ns.best_json).expanduser().resolve()
    if not best_path.exists():
        print(f"error: --best-json not found: {best_path}", file=sys.stderr)
        return 1
    with best_path.open() as fh:
        best = json.load(fh)
    overrides = best.get("params")
    if not isinstance(overrides, dict):
        print(f"error: best.json missing/invalid 'params' dict",
              file=sys.stderr)
        return 1

    ref = Path(ns.reference_binary).expanduser().absolute()
    if not ref.exists():
        print(f"error: --reference-binary not found: {ref}", file=sys.stderr)
        return 1

    tt_mb = promote_mod.max_tt_mb_per_worker()
    cur_cmd = _candidate_cmd(tt_mb)
    ref_cmd = _reference_cmd(ref, tt_mb)
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

    print(f"validating best.json={best_path} vs {ref}")
    print(f"  candidate weights: {overrides}")
    print(f"  n_games={cfg.n_games} time_ms={cfg.time_ms_per_stone} "
          f"workers={ns.workers} test={cfg.test}")
    started_at = _now_iso()
    os.environ[_EVAL_OVERRIDES_ENV] = json.dumps(overrides)
    t0 = time.monotonic()
    res = promote_mod.run_match_parallel(
        cur_cmd, ref_cmd, cfg, n_workers=int(ns.workers),
    )
    wall = time.monotonic() - t0
    finished_at = _now_iso()

    print()
    print(f"games:    {res.games_played}")
    print(f"W/L/D:    {res.current_wins}/{res.best_wins}/{res.draws}")
    print(f"winrate:  {res.winrate:.4f}  Wilson [{res.wilson_lower:.4f}, "
          f"{res.wilson_upper:.4f}]")
    elo_lo, elo_hi = res.estimated_elo_ci
    print(f"Elo:      {res.estimated_elo:+.1f}  CI [{elo_lo:+.1f}, "
          f"{elo_hi:+.1f}]")
    print(f"verdict:  {res.final_verdict}")
    print(f"wall:     {wall:.1f}s")

    out_path = (Path(ns.output).expanduser().resolve() if ns.output
                else best_path.parent / f"validate_{cfg.n_games}g.json")
    payload: dict[str, Any] = {
        "schema_version": 1,
        "best_json": str(best_path),
        "reference_binary": str(ref),
        "params": overrides,
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
