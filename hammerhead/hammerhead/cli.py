"""``hammerhead`` CLI entry point.

Subcommands:

* ``play``     — human vs bot REPL
* ``selfplay`` — bot vs bot, log winners
* ``bench``    — benchmark suite (micro/quick/perf/ablation/nps/depth/
  threats/selfplay/reference/scaling/breakdown/all/diff)
* ``analyze``  — placeholder until BSN parser ships
* ``bot``      — line-based subprocess protocol (Phase 11 harness)
"""

from __future__ import annotations

import argparse
import json
import shlex
import subprocess
import sys
import time
from dataclasses import asdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

from hammerhead_engine import Engine

from . import benchmark as bench
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
    print("hammerhead REPL. Bot plays X. Enter your stones as 'q r' (comma-separated).")
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

    while not done():
        side = bx.to_move()
        active, mirror = (bx, bo) if side == 0 else (bo, bx)
        if step(active, mirror) or done():
            break
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
# bench — multi-subcommand dispatcher
# ─────────────────────────────────────────────────────────────────────────────


_REPO_ROOT = CONFIG.source_path.parent
_RESULTS_DIR = _REPO_ROOT / CONFIG.bench.results_dir

# Per-developer, per-checkout bench-tier cache (.hexo/ is gitignored).
_QUICK_CACHE = _REPO_ROOT / ".hexo" / "quick_baseline.json"
_PERF_CACHE = _REPO_ROOT / ".hexo" / "perf_baseline.json"


def _git_sha() -> str:
    try:
        out = subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=_REPO_ROOT,
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
        return out or "unknown"
    except Exception:
        return "unknown"


def _isodate_now() -> tuple[str, str]:
    """Return (iso_timestamp, compact_date_for_filename)."""
    now = datetime.now(timezone.utc)
    return (
        now.strftime("%Y-%m-%dT%H:%M:%SZ"),
        now.strftime("%Y%m%d-%H%M%S"),
    )


def _ensure_results_dir() -> None:
    _RESULTS_DIR.mkdir(parents=True, exist_ok=True)


def cmd_bench(args: argparse.Namespace) -> int:
    sub = args.bench_sub
    if sub == "micro":
        return _bench_micro(args)
    if sub == "nps":
        return _bench_nps(args)
    if sub == "depth":
        return _bench_depth(args)
    if sub == "threats":
        return _bench_threats(args)
    if sub == "selfplay":
        return _bench_selfplay(args)
    if sub == "reference":
        return _bench_reference(args)
    if sub == "scaling":
        return _bench_scaling(args)
    if sub == "breakdown":
        return _bench_breakdown(args)
    if sub == "quick":
        return _bench_quick(args)
    if sub == "perf":
        return _bench_perf(args)
    if sub == "ablation":
        return _bench_ablation(args)
    if sub == "all":
        return _bench_all(args)
    if sub == "diff":
        return _bench_diff(args)
    print(f"error: unknown bench subcommand {sub}", file=sys.stderr)
    return 1


def _bench_micro(args: argparse.Namespace) -> int:
    """Run criterion benches (one target or all) then drain into JSON."""
    target = args.target
    engine_dir = _REPO_ROOT / "hammerhead-engine"
    if target == "all":
        cmd = ["cargo", "bench"]
    else:
        cmd = ["cargo", "bench", "--bench", f"bench_{target}"]
    print(f"$ {' '.join(cmd)}  (cwd={engine_dir})")
    r = subprocess.call(cmd, cwd=engine_dir)
    if r != 0:
        return r
    return _run_drain(args)


def _run_drain(args: argparse.Namespace) -> int:
    del args
    _ensure_results_dir()
    iso, date = _isodate_now()
    sha = _git_sha()
    out_path = _RESULTS_DIR / f"{date}-{sha}.json"
    engine_dir = _REPO_ROOT / "hammerhead-engine"
    drain_bin = engine_dir / "target" / "release" / "examples" / "bench_drain"
    if not drain_bin.exists():
        build = subprocess.call(
            ["cargo", "build", "--release", "--example", "bench_drain"],
            cwd=engine_dir,
        )
        if build != 0:
            return build
    crit_dir = engine_dir / "target" / "criterion"
    r = subprocess.call(
        [
            str(drain_bin),
            "--out",
            str(out_path),
            "--criterion-dir",
            str(crit_dir),
        ],
        cwd=_REPO_ROOT,
    )
    if r != 0:
        return r
    print(f"micro drain → {out_path}")
    del iso  # iso recorded by bench_drain itself
    return 0


def _bench_nps(args: argparse.Namespace) -> int:
    r = bench.bench_nps(
        fixture=args.fixture, time_ms=args.time_ms, runs=args.runs
    )
    print(
        f"NPS {r.fixture}: nodes={r.nodes:,} depth={r.depth_reached} "
        f"nps={r.nps:,.0f} time_ms={r.time_ms}"
    )
    return 0


def _bench_depth(args: argparse.Namespace) -> int:
    r = bench.bench_depth_at_time(fixture=args.fixture, time_ms=args.time_ms)
    print(
        f"DEPTH {r.fixture} @ {r.time_ms} ms: depth_reached={r.depth_reached}"
    )
    return 0


def _bench_threats(args: argparse.Namespace) -> int:
    r = bench.bench_threat_latency(
        fixture=args.fixture, n_calls=args.samples
    )
    print(
        f"THREATS {r.fixture}: cold={r.cold_us:.2f}us "
        f"warm={r.warm_us:.2f}us samples={r.samples}"
    )
    return 0


def _bench_selfplay(args: argparse.Namespace) -> int:
    r = bench.bench_selfplay(
        time_per_stone_ms=args.time_ms,
        games=args.games,
        max_plies=args.max_plies,
    )
    print(
        f"SELFPLAY {r.games} games: plies_total={r.plies_total} "
        f"wall={r.wall_seconds:.2f}s plies/sec={r.plies_per_sec:.2f}"
    )
    return 0


def _bench_reference(args: argparse.Namespace) -> int:
    ref_cfg = CONFIG.bench.reference
    fixtures = (
        [s.strip() for s in args.fixtures.split(",") if s.strip()]
        if args.fixtures
        else list(ref_cfg.fixtures)
    )
    max_depth = args.max_depth if args.max_depth is not None else ref_cfg.max_depth
    budget_s = float(args.budget_s) if args.budget_s is not None else float(ref_cfg.budget_s)
    rows = bench.bench_reference(
        fixtures=fixtures,
        max_depth=max_depth,
        budget_s=budget_s,
        use_tt_stats=args.tt_stats,
    )
    label_w = max((len(r.fixture) for r in rows), default=8)
    label_w = max(label_w, 12)
    header = (
        f"{'fixture'.ljust(label_w)}  {'depth':>5}  {'nodes':>12}"
        f"  {'ms':>6}"
    )
    if args.tt_stats:
        header += f"  {'hit_rate':>9}"
    print(header)
    print("─" * len(header))
    for r in rows:
        line = (
            f"{r.fixture.ljust(label_w)}  {r.depth:>5}  {r.nodes:>12,}"
            f"  {r.ms:>6}"
        )
        if args.tt_stats:
            hr = "—" if r.tt_hit_rate is None else f"{r.tt_hit_rate*100:>7.2f}%"
            line += f"  {hr:>9}"
        print(line)
    return 0


def _bench_scaling(args: argparse.Namespace) -> int:
    cfg = CONFIG.bench.scaling
    fixtures = (
        [s.strip() for s in args.fixtures.split(",") if s.strip()]
        if args.fixtures
        else list(cfg.fixtures)
    )
    time_ms_buckets = (
        [int(s) for s in args.time_ms.split(",") if s.strip()]
        if args.time_ms
        else list(cfg.time_ms)
    )
    runs = args.runs if args.runs is not None else cfg.runs
    rows = bench.bench_scaling(
        fixtures=fixtures,
        time_ms_buckets=time_ms_buckets,
        runs=runs,
    )
    label_w = max((len(r.fixture) for r in rows), default=8)
    label_w = max(label_w, 12)
    header = (
        f"{'fixture'.ljust(label_w)}  {'time_ms':>8}  {'depth':>5}"
        f"  {'nodes':>10}  {'nps':>12}  {'ci95_lo':>10}  {'ci95_hi':>10}"
    )
    print(header)
    print("─" * len(header))
    for r in rows:
        print(
            f"{r.fixture.ljust(label_w)}  {r.time_ms:>8}  {r.depth:>5}"
            f"  {r.nodes:>10,}  {r.nps:>12,}  {r.ci95_lo:>10,}"
            f"  {r.ci95_hi:>10,}"
        )
    return 0


def _bench_breakdown(args: argparse.Namespace) -> int:
    cfg = CONFIG.bench.breakdown
    fixtures = (
        [s.strip() for s in args.fixtures.split(",") if s.strip()]
        if args.fixtures
        else list(cfg.fixtures)
    )
    depth = args.depth if args.depth is not None else cfg.depth
    rows = bench.bench_breakdown(fixtures=fixtures, depth=depth)
    label_w = max((len(r.fixture) for r in rows), default=8)
    label_w = max(label_w, 12)
    header = (
        f"{'fixture'.ljust(label_w)}  {'depth':>5}  {'function':>14}"
        f"  {'pct_cycles':>10}"
    )
    print(header)
    print("─" * len(header))
    for r in rows:
        print(
            f"{r.fixture.ljust(label_w)}  {r.depth:>5}  {r.function:>14}"
            f"  {r.pct_cycles:>9.2f}%"
        )
    return 0


def _bench_quick(args: argparse.Namespace) -> int:
    """Inner-loop tier: single fixture, one budget, multi-run."""
    qcfg = CONFIG.bench.quick
    r = bench.bench_quick(
        fixture=args.fixture or qcfg.default_fixture,
        time_ms=args.time_ms or qcfg.default_time_ms,
        runs=args.runs or qcfg.default_runs,
    )
    delta = ""
    prev = _read_json(_QUICK_CACHE)
    if (
        isinstance(prev, dict)
        and prev.get("fixture") == r.fixture
        and prev.get("time_ms") == r.time_ms
        and prev.get("nps_mean")
    ):
        pct = (r.nps_mean - prev["nps_mean"]) / prev["nps_mean"] * 100.0
        sign = "+" if pct >= 0 else ""
        delta = f" (Δ {sign}{pct:.1f}% vs last)"
    print(
        f"quick: {r.nps_mean / 1000:.0f}k ± {r.nps_stddev / 1000:.0f}k NPS, "
        f"depth {r.depth_reached}, "
        f"{r.cycles_per_node_mean:.0f} cyc/node{delta}"
    )
    _QUICK_CACHE.parent.mkdir(parents=True, exist_ok=True)
    _QUICK_CACHE.write_text(json.dumps(asdict(r), indent=2))
    return 0


def _bench_perf(args: argparse.Namespace) -> int:
    """Pre-commit tier: two fixtures × two budgets, multi-run."""
    del args  # config-driven, no flags
    rows = bench.bench_perf()
    prev_rows = _read_json(_PERF_CACHE)
    prev_map: dict[tuple[str, int], dict] = {}
    if isinstance(prev_rows, list):
        for p in prev_rows:
            if isinstance(p, dict) and "fixture" in p and "time_ms" in p:
                prev_map[(p["fixture"], p["time_ms"])] = p
    header = (
        f"{'fixture':<14} {'budget':>8}  {'nps_mean':>10}  "
        f"{'cyc/node':>9}  {'depth':>5}  {'Δ vs last':>10}"
    )
    print(header)
    print("─" * len(header))
    for r in rows:
        prev = prev_map.get((r.fixture, r.time_ms))
        if prev and prev.get("nps_mean"):
            pct = (r.nps_mean - prev["nps_mean"]) / prev["nps_mean"] * 100.0
            sign = "+" if pct >= 0 else ""
            delta = f"{sign}{pct:.1f}%"
        else:
            delta = "—"
        print(
            f"{r.fixture:<14} {str(r.time_ms) + 'ms':>8}  "
            f"{r.nps_mean / 1000:>9.0f}k  {r.cycles_per_node_mean:>9.0f}  "
            f"{r.depth_reached:>5}  {delta:>10}"
        )
    _PERF_CACHE.parent.mkdir(parents=True, exist_ok=True)
    _PERF_CACHE.write_text(json.dumps([asdict(r) for r in rows], indent=2))
    return 0


def _bench_ablation(args: argparse.Namespace) -> int:
    """Layer 2 S1/S2 ablation self-play A/B (Phase 16)."""
    r = bench.bench_ablation(
        games=args.games,
        time_per_stone_ms=args.time_ms,
    )
    print(
        f"ablation: {r.games} games at {r.time_per_stone_ms}ms/stone, "
        f"S1/S2 vs no-S1/S2"
    )
    print(
        f"  S1/S2 wins: {r.s1s2_wins} / {r.games} "
        f"({r.s1s2_winrate * 100:.1f}%)  "
        f"[losses {r.s1s2_losses}, draws {r.draws}]"
    )
    print(
        f"  Wilson 95%: [{r.wilson_lo * 100:.1f}%, {r.wilson_hi * 100:.1f}%]"
    )
    print(f"  Verdict: {r.verdict}")
    return 0


def _read_json(path: Path):
    """Best-effort JSON load; returns ``None`` on missing / corrupt file."""
    if not path.is_file():
        return None
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return None


def _bench_all(args: argparse.Namespace) -> int:
    """Full sweep: cargo bench → drain → macro → merged JSON."""
    _ensure_results_dir()
    engine_dir = _REPO_ROOT / "hammerhead-engine"
    iso, date = _isodate_now()
    sha = _git_sha()
    out_path = _RESULTS_DIR / f"{date}-{sha}.json"

    # 1. Run criterion full sweep.
    print("$ cargo bench")
    r = subprocess.call(["cargo", "bench"], cwd=engine_dir)
    if r != 0:
        return r

    # 2. Drain into a temporary micro JSON.
    drain_bin = engine_dir / "target" / "release" / "examples" / "bench_drain"
    if not drain_bin.exists():
        r = subprocess.call(
            ["cargo", "build", "--release", "--example", "bench_drain"],
            cwd=engine_dir,
        )
        if r != 0:
            return r
    micro_path = _RESULTS_DIR / f"{date}-{sha}.micro.json"
    crit_dir = engine_dir / "target" / "criterion"
    r = subprocess.call(
        [
            str(drain_bin),
            "--out",
            str(micro_path),
            "--criterion-dir",
            str(crit_dir),
        ],
        cwd=_REPO_ROOT,
    )
    if r != 0:
        return r

    micro = json.loads(micro_path.read_text())

    # 3. Macro benches.
    macro = bench.run_all(time_ms=args.time_ms, use_tt_stats=args.tt_stats)

    # 4. Merge canonical schema.
    canonical = {
        "schema_version": CONFIG.bench.schema_version,
        "timestamp": iso,
        "git_sha": sha,
        "rustc_version": micro.get("rustc_version", ""),
        "host": micro.get("host", {}),
        "micro": micro.get("micro", []),
        "macro": macro,
    }
    out_path.write_text(json.dumps(canonical, indent=2))
    micro_path.unlink(missing_ok=True)
    print(out_path)
    return 0


def _bench_diff(args: argparse.Namespace) -> int:
    a_path = _resolve_result_path(args.a)
    b_path = _resolve_result_path(args.b)
    a = json.loads(a_path.read_text())
    b = json.loads(b_path.read_text())
    if a.get("schema_version") != b.get("schema_version"):
        print(
            f"error: schema mismatch {a.get('schema_version')} vs "
            f"{b.get('schema_version')}",
            file=sys.stderr,
        )
        return 1
    rows = _diff_rows(a, b)
    _print_diff_table(rows)
    regressions = sum(1 for r in rows if r["pct"] > 5.0)
    if regressions:
        print(f"\n{regressions} regression(s) > 5%")
        return 1
    return 0


def _resolve_result_path(name: str) -> Path:
    """Accept a bare name (relative to results dir) or a path."""
    p = Path(name)
    if p.exists():
        return p
    candidate = _RESULTS_DIR / name
    if candidate.exists():
        return candidate
    candidate = _RESULTS_DIR / f"{name}.json"
    if candidate.exists():
        return candidate
    raise FileNotFoundError(f"no such bench JSON: {name}")


def _diff_rows(a: dict, b: dict) -> list[dict]:
    """Join two canonical bench JSONs by (group, name) for micro and
    (metric, fixture, time_ms) for macro. Smaller is better for ns/us
    metrics; larger is better for nps and plies/sec.
    """
    rows: list[dict] = []

    # Micro: median_ns per (group, name). Lower is better.
    a_micro = {(m["group"], m["name"]): m["median_ns"] for m in a.get("micro", [])}
    b_micro = {(m["group"], m["name"]): m["median_ns"] for m in b.get("micro", [])}
    for key in sorted(set(a_micro) | set(b_micro)):
        a_v = a_micro.get(key)
        b_v = b_micro.get(key)
        if a_v is None or b_v is None:
            continue
        rows.append(_row(f"{key[0]} / {key[1]}", a_v, b_v, lower_is_better=True))

    # Macro: nps (higher better), depth_at_time (higher better),
    # threat_latency (lower better — both fields), selfplay plies/sec (higher).
    a_macro = a.get("macro", {})
    b_macro = b.get("macro", {})

    def _join(metric: str, key_fn, value_fn, lower_is_better: bool) -> None:
        a_items = {key_fn(r): value_fn(r) for r in a_macro.get(metric, [])}
        b_items = {key_fn(r): value_fn(r) for r in b_macro.get(metric, [])}
        for key in sorted(set(a_items) & set(b_items)):
            rows.append(
                _row(
                    f"{metric} / {key}",
                    a_items[key],
                    b_items[key],
                    lower_is_better=lower_is_better,
                )
            )

    _join(
        "nps",
        lambda r: f"{r['fixture']}@{r['time_ms']}ms",
        lambda r: r["nps"],
        lower_is_better=False,
    )
    _join(
        "depth_at_time",
        lambda r: f"{r['fixture']}@{r['time_ms']}ms",
        lambda r: r["depth_reached"],
        lower_is_better=False,
    )
    _join(
        "threat_latency",
        lambda r: f"{r['fixture']}.cold",
        lambda r: r["cold_us"],
        lower_is_better=True,
    )
    _join(
        "threat_latency",
        lambda r: f"{r['fixture']}.warm",
        lambda r: r["warm_us"],
        lower_is_better=True,
    )
    _join(
        "selfplay_throughput",
        lambda r: f"@{r['time_per_stone_ms']}ms",
        lambda r: r["plies_per_sec"],
        lower_is_better=False,
    )

    # Reference node counts (regression net — see SPEC_BENCHMARKS.md).
    # Any drift indicates a behaviour change: same fixture × depth must
    # explore the same tree. We treat *any* delta as a regression so a
    # 0.1 % drift still surfaces, but only flag the failure exit code
    # when the magnitude exceeds the standard 5 % threshold the diff
    # tool already uses for other metrics.
    a_ref = {
        (r["fixture"], r["depth"]): r["nodes"]
        for r in a_macro.get("reference", [])
    }
    b_ref = {
        (r["fixture"], r["depth"]): r["nodes"]
        for r in b_macro.get("reference", [])
    }
    for key in sorted(set(a_ref) & set(b_ref)):
        rows.append(
            _row(
                f"reference / {key[0]}.d{key[1]}",
                float(a_ref[key]),
                float(b_ref[key]),
                lower_is_better=True,
            )
        )
    return rows


def _row(label: str, a: float, b: float, *, lower_is_better: bool) -> dict:
    delta = b - a
    pct_change = (delta / a * 100.0) if a else 0.0
    # `pct` is the directional "worse-than-baseline" magnitude in % —
    # positive means regression. For lower-is-better, b>a is bad.
    pct = pct_change if lower_is_better else -pct_change
    return {
        "label": label,
        "a": a,
        "b": b,
        "delta": delta,
        "pct": pct,
    }


def _print_diff_table(rows: list[dict]) -> None:
    if not rows:
        print("(no comparable benches)")
        return
    label_w = max(len(r["label"]) for r in rows)
    label_w = max(label_w, 32)
    header = (
        f"{'metric'.ljust(label_w)}  {'baseline':>14}  {'candidate':>14}"
        f"  {'delta':>12}  {'pct':>7}"
    )
    print(header)
    print("─" * len(header))
    for r in rows:
        sign = "+" if r["pct"] > 0 else " "
        print(
            f"{r['label'].ljust(label_w)}  {r['a']:>14.2f}  {r['b']:>14.2f}"
            f"  {r['delta']:>+12.2f}  {sign}{r['pct']:>5.1f}%"
        )


# ─────────────────────────────────────────────────────────────────────────────
# analyze — placeholder
# ─────────────────────────────────────────────────────────────────────────────


def cmd_analyze(args: argparse.Namespace) -> int:
    del args
    print("analyze: BSN parser not implemented yet (phase 12+).")
    return 1


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
# match / promote — Phase 11 promotion harness
# ─────────────────────────────────────────────────────────────────────────────


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
    head_sha = _git_sha()
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
        help="per-function cycles breakdown estimate (Phase 14)",
    )
    bs.add_argument(
        "--fixtures",
        default="",
        help="comma-separated fixture names; defaults to [bench.breakdown]",
    )
    bs.add_argument(
        "--depth",
        type=int,
        default=None,
        help="fixed search depth; defaults to [bench.breakdown]",
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

    bs = bsub.add_parser(
        "ablation",
        help="Layer 2 S1/S2 ablation self-play A/B (Phase 16)",
    )
    bs.add_argument("--games", type=int, default=50)
    bs.add_argument("--time-ms", type=int, default=500)

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
