"""``hammerhead`` CLI entry point.

Subcommands:

* ``play``     — human vs bot REPL
* ``selfplay`` — bot vs bot, log winners
* ``bench``    — benchmark suite (micro/quick/perf/nps/depth/
  threats/selfplay/reference/scaling/breakdown/all/diff)
* ``bot``      — line-based subprocess protocol (Phase 11 harness)
"""

from __future__ import annotations

import argparse
import sys
from typing import Optional

from hammerhead_engine import Engine

from .config import CONFIG
from .game import GameRecord
from .cli_bench import cmd_bench
from .cli_match import cmd_match, cmd_promote, _default_workers

# re-export for back-compat (e.g. test_benchmark.py: from hammerhead.cli import _bench_diff)
from .cli_bench import _bench_diff  # noqa: F401


Coord = tuple[int, int]


_PLAYER_NAMES = ("X", "O")


def _name(p: Optional[int]) -> str:
    if p is None:
        return "none"
    return _PLAYER_NAMES[p]


# ─────────────────────────────────────────────────────────────────────────────
# play — human vs bot REPL
# ─────────────────────────────────────────────────────────────────────────────


def _engine_play_turn(eng: Engine, time_ms: int) -> list[Coord]:
    """Search and place a full turn (1 stone for X's opening, else 2)."""
    moves: list[Coord] = []
    if eng.winner() is not None:
        return moves
    m = eng.best_move(time_ms=time_ms)
    eng.place(m)
    moves.append(m)
    if eng.winner() is None and eng.halfmove() == 1:
        m = eng.best_move(time_ms=time_ms)
        eng.place(m)
        moves.append(m)
    return moves


def cmd_play(args: argparse.Namespace) -> int:
    eng = Engine(tt_size_mb=CONFIG.bot.default_tt_size_mb)
    print("hammerhead REPL. Bot plays X. Enter your stones as 'q r' (comma-separated).")
    print("Type 'quit' to exit.")
    while eng.winner() is None:
        moves = _engine_play_turn(eng, args.time_ms)
        print(f"bot: {moves}")
        if eng.winner() is not None:
            break
        line = input("you: ").strip()
        if not line or line in {"quit", "exit"}:
            return 0
        try:
            for tok in line.split(","):
                q_str, r_str = tok.strip().split()
                eng.place((int(q_str), int(r_str)))
        except (ValueError, RuntimeError) as exc:
            print(f"error: {exc}")
            continue
    print(f"winner: {_name(eng.winner())}")
    return 0


# ─────────────────────────────────────────────────────────────────────────────
# selfplay — two bots, n games
# ─────────────────────────────────────────────────────────────────────────────


def _play_one_selfplay_game(time_ms: int, max_plies: int) -> tuple[Optional[int], GameRecord]:
    tt_mb = CONFIG.bot.default_tt_size_mb
    ex = Engine(tt_size_mb=tt_mb)
    eo = Engine(tt_size_mb=tt_mb)
    record = GameRecord()

    def step(active: Engine, mirror: Engine) -> bool:
        m = active.best_move(time_ms=time_ms)
        active.place(m)
        mirror.place(m)
        record.append(m)
        return active.winner() is not None or mirror.winner() is not None

    def done() -> bool:
        return (
            record.ply >= max_plies
            or ex.winner() is not None
            or eo.winner() is not None
        )

    while not done():
        side = ex.to_move()
        active, mirror = (ex, eo) if side == 0 else (eo, ex)
        if step(active, mirror) or done():
            break
        if active.halfmove() == 1:
            if step(active, mirror) or done():
                break

    winner = ex.winner() if ex.winner() is not None else eo.winner()
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
# bot — subprocess protocol
# ─────────────────────────────────────────────────────────────────────────────


def cmd_bot(args: argparse.Namespace) -> int:
    eng = Engine(tt_size_mb=args.tt_size_mb)
    sys.stdout.write("hammerhead bot ready\n")
    sys.stdout.flush()
    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            resp = _handle_bot_line(eng, line)
        except Exception as exc:  # noqa: BLE001
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
# argparse wiring
# ─────────────────────────────────────────────────────────────────────────────


def _build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="hammerhead")
    sub = p.add_subparsers(dest="cmd", required=True)

    sp = sub.add_parser("play", help="human vs bot REPL")
    sp.add_argument("--time-ms", type=int, default=CONFIG.bot.default_time_per_move_ms)
    sp.set_defaults(fn=cmd_play)

    sp = sub.add_parser("selfplay", help="bot vs bot")
    sp.add_argument("-n", type=int, default=10)
    sp.add_argument("--time-ms", type=int, default=500)
    sp.add_argument("--max-plies", type=int, default=400)
    sp.set_defaults(fn=cmd_selfplay)

    sp_bench = sub.add_parser("bench", help="benchmark suite")
    bsub = sp_bench.add_subparsers(dest="bench_sub", required=True)

    bs = bsub.add_parser("micro", help="run criterion + drain")
    bs.add_argument("--target", default="all")

    bs = bsub.add_parser("nps", help="nodes-per-second on a fixture")
    bs.add_argument("--time-ms", type=int, default=CONFIG.bench.default_time_ms)
    bs.add_argument("--fixture", default="midgame_12")
    bs.add_argument("--runs", type=int, default=CONFIG.bench.default_runs)

    bs = bsub.add_parser("depth", help="depth reached at time budget")
    bs.add_argument("--time-ms", type=int, default=CONFIG.bench.default_time_ms)
    bs.add_argument("--fixture", default="midgame_12")

    bs = bsub.add_parser("threats", help="cached_eval cold vs warm")
    bs.add_argument("--fixture", default="midgame_30")
    bs.add_argument("--samples", type=int, default=64)

    bs = bsub.add_parser("selfplay", help="selfplay throughput")
    bs.add_argument("--time-ms", type=int, default=200)
    bs.add_argument("--games", type=int, default=CONFIG.bench.default_games)
    bs.add_argument("--max-plies", type=int, default=CONFIG.bench.default_max_plies)

    bs = bsub.add_parser(
        "reference",
        help="deterministic fixed-depth node-count table (regression net)",
    )
    bs.add_argument(
        "--fixtures",
        default="",
        help="comma-separated fixture names; defaults to [bench.reference]",
    )
    bs.add_argument(
        "--max-depth",
        type=int,
        default=None,
        help="upper bound on fixed search depth (default: [bench.reference])",
    )
    bs.add_argument(
        "--budget-s",
        type=float,
        default=None,
        help="cumulative per-fixture wall-clock cap in seconds",
    )
    bs.add_argument(
        "--tt-stats",
        action="store_true",
        help="report TT hit rate per row (requires tt_stats feature build)",
    )

    bs = bsub.add_parser("scaling", help="ms-time scaling table (Phase 14)")
    bs.add_argument(
        "--fixtures",
        default="",
        help="comma-separated fixture names; defaults to [bench.scaling]",
    )
    bs.add_argument(
        "--time-ms",
        default="",
        help="comma-separated time budgets in ms; defaults to [bench.scaling]",
    )
    bs.add_argument(
        "--runs",
        type=int,
        default=None,
        help="number of runs per (fixture, time_ms); defaults to [bench.scaling]",
    )

    bs = bsub.add_parser(
        "breakdown",
        help="per-module engine self-time from a flamegraph capture",
    )
    bs.add_argument(
        "--folded",
        default=None,
        help=(
            "path to a flamegraph folded.txt; defaults to the newest "
            "benches/results/flamegraph-*.folded.txt"
        ),
    )

    bs = bsub.add_parser(
        "quick", help="inner-loop NPS+depth+cyc/node check (~5-15s)"
    )
    bs.add_argument(
        "--fixture", default=None, help="fixture; defaults to [bench.quick]"
    )
    bs.add_argument(
        "--time-ms",
        type=int,
        default=None,
        help="time budget; defaults to [bench.quick]",
    )
    bs.add_argument(
        "--runs",
        type=int,
        default=None,
        help="number of runs; defaults to [bench.quick]",
    )

    bsub.add_parser(
        "perf",
        help="two-fixture × multi-budget NPS+cyc/node check (~30-60s)",
    )

    bs = bsub.add_parser("all", help="full sweep → canonical JSON")
    bs.add_argument("--time-ms", type=int, default=CONFIG.bench.default_time_ms)
    bs.add_argument(
        "--tt-stats",
        action="store_true",
        help="populate reference hit-rate column (requires tt_stats build)",
    )

    bs = bsub.add_parser("diff", help="compare two run JSONs")
    bs.add_argument("a")
    bs.add_argument("b")

    sp_bench.set_defaults(fn=cmd_bench)

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
    sp.add_argument(
        "--workers",
        type=int,
        default=_default_workers(),
        help="parallel match workers (0 = auto: cpu_count() - 2)",
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
    sp.add_argument(
        "--workers",
        type=int,
        default=_default_workers(),
        help="parallel match workers (0 = auto: cpu_count() - 2)",
    )
    sp.set_defaults(fn=cmd_promote)

    return p


def main(argv: Optional[list[str]] = None) -> int:
    ns = _build_parser().parse_args(argv)
    return ns.fn(ns)


if __name__ == "__main__":
    sys.exit(main())
