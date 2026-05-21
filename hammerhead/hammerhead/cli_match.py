"""Match / promote / vs subcommand handlers extracted from cli.py (SRP split)."""

from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys
from pathlib import Path

from . import promote as promote_mod
from .config import CONFIG
from .cli_bench import _git_sha


def _detect_cli_module(venv_python: Path) -> str:
    """Name of the bot CLI package installed in ``venv_python``'s venv.

    `hammerhead` after the project rename; `hexo` for a pre-rename
    `.bestref` worktree. The promotion harness compares the current
    engine against a possibly-older worktree build, so the two sides
    may carry different package names.
    """
    # `-P` keeps the cwd off sys.path: the repo root holds a `hammerhead/`
    # project directory that Python would otherwise pick up as an implicit
    # namespace package, masking what is actually installed in the venv.
    for mod in ("hammerhead", "hexo"):
        rc = subprocess.call(
            [str(venv_python), "-P", "-c", f"import {mod}.cli"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if rc == 0:
            return mod
    raise FileNotFoundError(
        f"no bot CLI package (hammerhead / hexo) found in {venv_python}"
    )


def _bot_cmd(venv_python: Path) -> list[str]:
    module = _detect_cli_module(venv_python)
    return [str(venv_python), "-m", f"{module}.cli", "bot"]


def _default_workers() -> int:
    """Default ``--workers``: ``N_WORKERS`` env var, else the
    ``[bench.vs] default_n_workers`` config (0 = auto)."""
    env = os.environ.get("N_WORKERS")
    if env:
        try:
            return int(env)
        except ValueError:
            pass
    return CONFIG.bench.vs.default_n_workers


def _print_match_result(res: promote_mod.MatchResult, cfg: promote_mod.MatchConfig) -> None:
    print()
    print(f"games:    {res.games_played}")
    print(
        f"current:  {res.current_wins}  best: {res.best_wins}  draws: {res.draws}"
    )
    print(
        f"winrate:  {res.winrate:.4f}  "
        f"wilson95: [{res.wilson_lower:.4f}, {res.wilson_upper:.4f}]"
    )
    elo_lo, elo_hi = res.estimated_elo_ci
    print(
        f"elo:      {res.estimated_elo:+.1f}  "
        f"ci95: [{elo_lo:+.1f}, {elo_hi:+.1f}]"
    )
    if cfg.test == "sprt" and res.sprt_llr is not None:
        log_low, log_high = promote_mod.sprt_thresholds(cfg)
        print(
            f"sprt:     llr={res.sprt_llr:+.3f}  "
            f"bounds=[{log_low:+.3f}, {log_high:+.3f}]  "
            f"verdict={res.sprt_verdict}"
        )
    print(f"verdict:  {res.final_verdict}")


def cmd_match(args: argparse.Namespace) -> int:
    """Generic two-binary match. ``current_cmd`` and ``best_cmd`` are
    shell-quoted strings split via :mod:`shlex`."""
    current_cmd = shlex.split(args.current_cmd)
    best_cmd = shlex.split(args.best_cmd)
    if not current_cmd or not best_cmd:
        print("error: current_cmd and best_cmd must be non-empty", file=sys.stderr)
        return 2

    cfg = promote_mod.MatchConfig.from_promote_config(
        n_games=args.n,
        time_ms_per_stone=args.time_ms,
        test=args.test,
    )
    workers = promote_mod.resolve_worker_count(args.workers, cfg.n_games)
    print(
        f"match: n={cfg.n_games} time_ms={cfg.time_ms_per_stone} "
        f"test={cfg.test} color_balance={cfg.color_balance} workers={workers}"
    )
    print(f"  current: {current_cmd}")
    print(f"  best:    {best_cmd}")
    res = promote_mod.run_match_parallel(
        current_cmd, best_cmd, cfg, n_workers=args.workers
    )
    _print_match_result(res, cfg)
    return 0 if res.final_verdict == "PROMOTE" else 1


def cmd_promote(args: argparse.Namespace) -> int:
    """current (HEAD venv) vs best (worktree venv). Writes ``.bestref`` on
    PROMOTE unless ``--dry-run`` is set."""
    repo_root = CONFIG.source_path.parent
    pc = CONFIG.promote
    bestref_path = repo_root / pc.bestref_path
    worktree_path = repo_root / pc.worktree_path
    setup_script = repo_root / "scripts" / "setup_worktree.sh"

    if not setup_script.is_file():
        print(f"error: {setup_script} not found", file=sys.stderr)
        return 1

    print(f"bootstrap: {setup_script}")
    rc = subprocess.call([str(setup_script)], cwd=repo_root)
    if rc != 0:
        print("error: setup_worktree.sh failed", file=sys.stderr)
        return rc

    if not bestref_path.is_file():
        print(f"error: {bestref_path} missing after bootstrap", file=sys.stderr)
        return 1
    best_sha = bestref_path.read_text().strip()

    current_python = Path(sys.executable)
    best_python = worktree_path / ".venv-best" / "bin" / "python"
    if not best_python.is_file():
        print(f"error: {best_python} not found", file=sys.stderr)
        return 1

    tt_mb = promote_mod.max_tt_mb_per_worker()
    current_cmd = promote_mod.with_tt_bound(_bot_cmd(current_python), tt_mb)
    best_cmd = promote_mod.with_tt_bound(_bot_cmd(best_python), tt_mb)

    cfg = promote_mod.MatchConfig.from_promote_config(
        n_games=args.n,
        time_ms_per_stone=args.time_ms,
        test=args.test,
    )
    head_sha = _git_sha()
    workers = promote_mod.resolve_worker_count(args.workers, cfg.n_games)
    print(
        f"promote: current={head_sha} vs best={best_sha[:8]} "
        f"n={cfg.n_games} time_ms={cfg.time_ms_per_stone} test={cfg.test} "
        f"workers={workers}"
    )
    res = promote_mod.run_match_parallel(
        current_cmd, best_cmd, cfg, n_workers=args.workers
    )
    _print_match_result(res, cfg)

    if res.final_verdict == "PROMOTE":
        if args.dry_run:
            print("dry-run: PROMOTE — not updating .bestref")
            return 0
        full_head = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=repo_root, text=True
        ).strip()
        prior = bestref_path.read_text() if bestref_path.is_file() else None
        bestref_path.write_text(full_head + "\n")
        try:
            subprocess.check_call(
                ["git", "add", "--", str(bestref_path)], cwd=repo_root
            )
            subprocess.check_call(
                [
                    "git",
                    "commit",
                    "--only",
                    "--",
                    str(bestref_path),
                    "-m",
                    f"promote: {full_head[:8]}",
                ],
                cwd=repo_root,
            )
        except subprocess.CalledProcessError as e:
            # Roll back the on-disk write so .bestref doesn't drift past HEAD
            # when the commit (or a pre-commit hook) fails.
            if prior is not None:
                bestref_path.write_text(prior)
            else:
                bestref_path.unlink(missing_ok=True)
            print(f"error: promote commit failed; rolled back .bestref ({e})",
                  file=sys.stderr)
            return 1
        print(f"promoted: .bestref → {full_head}")
        return 0

    return 1
