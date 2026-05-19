"""``hexo`` CLI entry point.

Subcommands:

* ``play``     — human vs bot REPL
* ``selfplay`` — bot vs bot, log winners
* ``bench``    — single-search NPS smoke
* ``analyze``  — placeholder until BSN parser ships
* ``bot``      — line-based subprocess protocol (Phase 10 harness)
"""

from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
import time
from pathlib import Path
from typing import Optional

from hexo_engine import Engine

from . import promote as promote_mod
from .bot import Bot, BotConfig
from .config import CONFIG
from .game import GameRecord


Coord = tuple[int, int]


_PLAYER_NAMES = ("X", "O")


def _name(p: Optional[int]) -> str:
    if p is None:
        return "none"
    return _PLAYER_NAMES[p]


# ─────────────────────────────────────────────────────────────────────────────
# play — human vs bot REPL
# ─────────────────────────────────────────────────────────────────────────────


def cmd_play(args: argparse.Namespace) -> int:
    cfg = BotConfig(time_per_move_ms=args.time_ms)
    bot = Bot(cfg)
    print("hexo REPL. Bot plays X. Enter your stones as 'q r' (comma-separated).")
    print("Type 'quit' to exit.")
    while bot.winner() is None:
        moves = bot.play_turn()
        print(f"bot: {moves}")
        if bot.winner() is not None:
            break
        line = input("you: ").strip()
        if not line or line in {"quit", "exit"}:
            return 0
        try:
            for tok in line.split(","):
                q_str, r_str = tok.strip().split()
                bot.observe((int(q_str), int(r_str)))
        except (ValueError, RuntimeError) as exc:
            print(f"error: {exc}")
            continue
    print(f"winner: {_name(bot.winner())}")
    return 0


# ─────────────────────────────────────────────────────────────────────────────
# selfplay — two bots, n games
# ─────────────────────────────────────────────────────────────────────────────


def _play_one_selfplay_game(time_ms: int, max_plies: int) -> tuple[Optional[int], GameRecord]:
    bx = Bot(BotConfig(time_per_move_ms=time_ms))
    bo = Bot(BotConfig(time_per_move_ms=time_ms))
    record = GameRecord()

    def step(active: Bot, mirror: Bot) -> bool:
        """Place one stone via ``active``, mirror it on ``mirror``.
        Return True if either side has won."""
        m = active.play_stone()
        mirror.observe(m)
        record.append(m)
        return active.winner() is not None or mirror.winner() is not None

    def done() -> bool:
        return (
            record.ply >= max_plies
            or bx.winner() is not None
            or bo.winner() is not None
        )

    # Drive turns by following whichever side is to move on the X engine.
    while not done():
        side = bx.to_move()
        active, mirror = (bx, bo) if side == 0 else (bo, bx)
        if step(active, mirror) or done():
            break
        # Same-side continuation: stone 2.
        if active.halfmove() == 1:
            if step(active, mirror) or done():
                break

    winner = bx.winner() if bx.winner() is not None else bo.winner()
    record.finish(winner)
    return winner, record


def cmd_selfplay(args: argparse.Namespace) -> int:
    counts: dict[Optional[int], int] = {0: 0, 1: 0, None: 0}
    for i in range(args.n):
        winner, record = _play_one_selfplay_game(args.time_ms, args.max_plies)
        counts[winner] = counts.get(winner, 0) + 1
        print(f"game {i + 1}/{args.n}: winner = {_name(winner)} ({record.ply} plies)")
    print(f"summary: X={counts[0]}, O={counts[1]}, none={counts[None]}")
    return 0


# ─────────────────────────────────────────────────────────────────────────────
# bench — single best_move NPS smoke
# ─────────────────────────────────────────────────────────────────────────────


def cmd_bench(args: argparse.Namespace) -> int:
    eng = Engine(tt_size_mb=CONFIG.tt.default_size_mb)
    # Seed a small mid-game-ish opening so eval/threats have signal.
    for c in [(0, 0), (1, 0), (-1, 1), (0, 1)]:
        eng.place(c)
    t0 = time.perf_counter()
    move = eng.best_move(time_ms=args.time_ms)
    dt_ms = (time.perf_counter() - t0) * 1000.0
    print(
        f"bench: best={move} target={args.time_ms}ms actual={dt_ms:.0f}ms "
        f"eval={eng.cached_eval()}"
    )
    return 0


# ─────────────────────────────────────────────────────────────────────────────
# analyze — placeholder
# ─────────────────────────────────────────────────────────────────────────────


def cmd_analyze(args: argparse.Namespace) -> int:
    del args
    print("analyze: BSN parser not implemented yet (phase 11+).")
    return 1


# ─────────────────────────────────────────────────────────────────────────────
# bot — subprocess protocol
# ─────────────────────────────────────────────────────────────────────────────


def cmd_bot(args: argparse.Namespace) -> int:
    eng = Engine(tt_size_mb=args.tt_size_mb)
    sys.stdout.write("hexo bot ready\n")
    sys.stdout.flush()
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            resp = _handle_bot_line(eng, line)
        except Exception as exc:  # noqa: BLE001 — protocol surfaces any error
            resp = f"error: {exc}"
        sys.stdout.write(f"{resp}\n")
        sys.stdout.flush()
        if line == "quit":
            break
    return 0


def _handle_bot_line(eng: Engine, line: str) -> str:
    parts = line.split()
    if not parts:
        return "error: empty command"
    cmd = parts[0]
    if cmd == "reset":
        eng.reset()
        return "ok"
    if cmd == "place":
        if len(parts) != 3:
            return "error: place needs Q R"
        q, r = int(parts[1]), int(parts[2])
        eng.place((q, r))
        return "ok"
    if cmd == "best_move":
        if len(parts) != 2:
            return "error: best_move needs TIME_MS"
        t = int(parts[1])
        q, r = eng.best_move(time_ms=t)
        return f"{q} {r}"
    if cmd == "winner":
        return _name(eng.winner())
    if cmd == "ply":
        return str(eng.ply())
    if cmd == "halfmove":
        return str(eng.halfmove())
    if cmd == "to_move":
        return _name(eng.to_move())
    if cmd == "eval":
        return str(eng.cached_eval())
    if cmd == "hash":
        return f"{eng.hash():032x}"
    if cmd == "quit":
        return "bye"
    return f"error: unknown command {cmd}"


# ─────────────────────────────────────────────────────────────────────────────
# match / promote — Phase 11 promotion harness
# ─────────────────────────────────────────────────────────────────────────────


def _bot_cmd(venv_python: Path) -> list[str]:
    return [str(venv_python), "-m", "hexo.cli", "bot"]


def _short_head_sha(repo_root: Path) -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=repo_root,
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip() or "unknown"
    except Exception:  # noqa: BLE001
        return "unknown"


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


def _on_game(i: int, r: promote_mod.GameResult, llr: Optional[float]) -> None:
    side = "X" if r.current_was_x else "O"
    winner = r.winner if r.winner is not None else "draw"
    llr_s = f"  llr={llr:+.3f}" if llr is not None else ""
    print(f"game {i + 1}: current={side} → {winner} ({r.plies} plies){llr_s}")


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
    print(
        f"match: n={cfg.n_games} time_ms={cfg.time_ms_per_stone} "
        f"test={cfg.test} color_balance={cfg.color_balance}"
    )
    print(f"  current: {current_cmd}")
    print(f"  best:    {best_cmd}")
    res = promote_mod.run_match(current_cmd, best_cmd, cfg, on_game=_on_game)
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

    current_cmd = _bot_cmd(current_python)
    best_cmd = _bot_cmd(best_python)

    cfg = promote_mod.MatchConfig.from_promote_config(
        n_games=args.n,
        time_ms_per_stone=args.time_ms,
        test=args.test,
    )
    head_sha = _short_head_sha(repo_root)
    print(
        f"promote: current={head_sha} vs best={best_sha[:8]} "
        f"n={cfg.n_games} time_ms={cfg.time_ms_per_stone} test={cfg.test}"
    )
    res = promote_mod.run_match(current_cmd, best_cmd, cfg, on_game=_on_game)
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


# ─────────────────────────────────────────────────────────────────────────────
# argparse wiring
# ─────────────────────────────────────────────────────────────────────────────


def _build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="hexo")
    sub = p.add_subparsers(dest="cmd", required=True)

    sp = sub.add_parser("play", help="human vs bot REPL")
    sp.add_argument("--time-ms", type=int, default=CONFIG.bot.default_time_per_move_ms)
    sp.set_defaults(fn=cmd_play)

    sp = sub.add_parser("selfplay", help="bot vs bot")
    sp.add_argument("-n", type=int, default=10)
    sp.add_argument("--time-ms", type=int, default=500)
    sp.add_argument("--max-plies", type=int, default=400)
    sp.set_defaults(fn=cmd_selfplay)

    sp = sub.add_parser("bench", help="single-search NPS smoke")
    sp.add_argument("--time-ms", type=int, default=CONFIG.bot.default_time_per_move_ms)
    sp.set_defaults(fn=cmd_bench)

    sp = sub.add_parser("analyze", help="analyze a BSN game (stub)")
    sp.add_argument("bsn")
    sp.set_defaults(fn=cmd_analyze)

    sp = sub.add_parser("bot", help="subprocess protocol on stdin/stdout")
    sp.add_argument("--tt-size-mb", type=int, default=CONFIG.bot.default_tt_size_mb)
    sp.set_defaults(fn=cmd_bot)

    sp = sub.add_parser(
        "match",
        help="generic match between two subprocess bot commands",
    )
    sp.add_argument("current_cmd", help="shell-quoted command for the current side")
    sp.add_argument("best_cmd", help="shell-quoted command for the best side")
    sp.add_argument(
        "--n", type=int, default=CONFIG.promote.default_n_games, dest="n"
    )
    sp.add_argument(
        "--time-ms",
        type=int,
        default=CONFIG.promote.default_time_ms_per_stone,
    )
    sp.add_argument(
        "--test",
        choices=("sprt", "wilson", "raw"),
        default=CONFIG.promote.default_test,
    )
    sp.set_defaults(fn=cmd_match)

    sp = sub.add_parser(
        "promote",
        help="run match vs .bestref worktree; advance .bestref on PROMOTE",
    )
    sp.add_argument(
        "--n", type=int, default=CONFIG.promote.default_n_games, dest="n"
    )
    sp.add_argument(
        "--time-ms",
        type=int,
        default=CONFIG.promote.default_time_ms_per_stone,
    )
    sp.add_argument(
        "--test",
        choices=("sprt", "wilson", "raw"),
        default=CONFIG.promote.default_test,
    )
    sp.add_argument(
        "--dry-run",
        action="store_true",
        help="run match but do not write .bestref on PROMOTE",
    )
    sp.set_defaults(fn=cmd_promote)

    return p


def main(argv: Optional[list[str]] = None) -> int:
    ns = _build_parser().parse_args(argv)
    return ns.fn(ns)


if __name__ == "__main__":
    sys.exit(main())
