"""Bench subcommand handlers extracted from cli.py (SRP split)."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import asdict
from datetime import datetime, timezone
from pathlib import Path

from . import benchmark as bench
from .config import CONFIG
from .tune import cmd_tune_sweep  # re-export for cli.py wiring


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
    if sub == "all":
        return _bench_all(args)
    if sub == "diff":
        return _bench_diff(args)
    if sub == "tune-sweep":
        return cmd_tune_sweep(args)
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
    folded = Path(args.folded) if getattr(args, "folded", None) else None
    rows = bench.bench_breakdown(folded=folded)
    if not rows:
        # Empty: no flamegraph capture found. bench_breakdown already
        # warned on stderr; nothing to print, still a success exit.
        return 0
    capture = rows[0].fixture
    print(f"breakdown: {capture}  (% of engine self-time)")
    label_w = max(len(r.function) for r in rows)
    header = f"{'function'.ljust(label_w)}  {'pct_cycles':>10}"
    print(header)
    print("─" * len(header))
    for r in rows:
        print(f"{r.function.ljust(label_w)}  {r.pct_cycles:>9.2f}%")
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
