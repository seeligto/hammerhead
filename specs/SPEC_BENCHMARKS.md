# Hammerhead — Benchmark Spec

Save as `specs/SPEC_BENCHMARKS.md`.

## Goal

Measure. Cannot optimize what you cannot measure.

Two-tier benchmark infrastructure:

1. **Rust criterion** — module-level micro-benches. Deterministic, with
   statistical CI. Used to catch regressions in hot paths.
2. **Python harness** — engine-level macro-benches. NPS at time budget,
   depth-at-time, self-play throughput, threat-detection latency,
   eval-cost percentiles.

Both tiers write JSON to `benches/results/`. A small diff tool compares
two result sets.

## Principle

- One file per responsibility. No mega-bench files.
- Stable JSON output schema. Diffable across commits.
- All benches reproducible: seeded RNG, fixed fixtures.
- `make bench` runs everything. Subset targets exist for each module.
- Results go in `benches/results/`, gitignored except the latest
  baseline `benches/results/baseline.json`.

## Rust micro-benches (criterion)

Layout:

```
hammerhead-engine/
├── benches/
│   ├── bench_board.rs
│   ├── bench_axis_bitmap.rs
│   ├── bench_moves.rs
│   ├── bench_threats.rs
│   ├── bench_eval.rs
│   ├── bench_ordering.rs
│   ├── bench_tt.rs
│   ├── bench_search.rs
│   └── common/
│       ├── mod.rs            # shared fixtures: positions, RNG seeds
│       └── positions.rs      # named position library
```

### `Cargo.toml`

Add per bench under `[[bench]]`:

```toml
[[bench]]
name = "bench_board"
harness = false

[[bench]]
name = "bench_threats"
harness = false

# ... one entry per file
```

`harness = false` because criterion supplies its own main.

### `[dev-dependencies]`

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
```

### Per-file structure

Every bench file follows:

```rust
//! Micro-benchmarks for <module>.
//! Run via `make bench TARGET=<module>` or `cargo bench --bench bench_<module>`.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hammerhead_engine::*;

mod common;
use common::positions;

fn bench_<operation>(c: &mut Criterion) {
    let mut group = c.benchmark_group("<module>::<operation>");
    for fixture in &positions::FIXTURES {
        group.bench_function(fixture.name, |b| {
            let board = fixture.build();
            b.iter(|| {
                black_box(perform_op(&board));
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_<operation>);
criterion_main!(benches);
```

### Shared fixtures `benches/common/positions.rs`

Library of named positions. Each fixture is a builder returning a fresh
`Board`. Examples:

```rust
pub struct Fixture {
    pub name: &'static str,
    pub build: fn() -> Board,
}

pub static FIXTURES: &[Fixture] = &[
    Fixture { name: "empty",           build: build_empty },
    Fixture { name: "single_origin",   build: build_single },
    Fixture { name: "open_3_x_axis",   build: build_open_3 },
    Fixture { name: "open_4_x_axis",   build: build_open_4 },
    Fixture { name: "rhombus",         build: build_rhombus },
    Fixture { name: "midgame_12",      build: build_midgame_12 },
    Fixture { name: "midgame_30",      build: build_midgame_30 },
    Fixture { name: "endgame_60",      build: build_endgame_60 },
    Fixture { name: "fork_two_open_4", build: build_fork },
];
```

Builders use `Board::place_for_test` (Phase 4 test helper) to construct
arbitrary positions deterministically.

### Per-module bench targets

| File | Operations to bench |
|---|---|
| `bench_board.rs` | `place`, `undo`, place+undo round trip, hash compute |
| `bench_axis_bitmap.rs` | `set`, `clear`, `window6`, `run_through`, `populated_range` |
| `bench_moves.rs` | `generate(r=2)`, `generate(r=4)`, `generate(r=8)` across fixtures |
| `bench_threats.rs` | `compute` full, `compute` incremental (when implemented), defense-cell extraction, fork-cover compute |
| `bench_eval.rs` | `cached_eval` cold, `cached_eval` warm, `layer1_window_scan`, `layer2_shapes`, `layer3_fork_bonus` isolated |
| `bench_ordering.rs` | `order_moves` over 20 / 40 / 80 candidates, bucket scoring per move |
| `bench_tt.rs` | `probe` hit / miss, `store` depth-preferred / always-replace path |
| `bench_search.rs` | `search_root` at fixed depth (no time budget), TT hit rate stat |

### Output: JSON results

Criterion writes to `target/criterion/<group>/<bench>/`. We **also**
emit a single canonical JSON via a custom drain. Layout:

```json
{
  "schema_version": 1,
  "timestamp": "2026-05-19T14:32:11Z",
  "git_sha": "abc1234",
  "rustc_version": "1.85.0",
  "host": { "cpu": "...", "cores": 8 },
  "benches": [
    {
      "group": "threats::compute",
      "name": "midgame_30",
      "median_ns": 4321,
      "mad_ns": 87,
      "samples": 100
    }
  ]
}
```

Drain implementation: a tiny `bench_drain` binary in
`hammerhead-engine/src/bin/bench_drain.rs` that walks `target/criterion/` and
collects results into `benches/results/<isodate>-<sha>.json`. Called by
`make bench` at the end of the criterion run.

Schema versioned. Diff tool refuses to compare across schema versions.

## Python macro-benches

`hammerhead/hammerhead/benchmark.py` exposes a small library. CLI exposes
`hammerhead bench` subcommand (already stub'd Phase 9; extended here).

### Library API

```python
from dataclasses import dataclass

@dataclass(frozen=True)
class NpsResult:
    nodes: int
    time_ms: int
    depth_reached: int
    nps: float
    fixture: str

@dataclass(frozen=True)
class DepthAtTimeResult:
    time_ms: int
    depth_reached: int
    fixture: str

@dataclass(frozen=True)
class ThreatLatencyResult:
    fixture: str
    cold_us: float    # full recompute
    warm_us: float    # cached read
    samples: int

@dataclass(frozen=True)
class SelfplayThroughputResult:
    games: int
    plies_total: int
    wall_seconds: float
    plies_per_sec: float
    time_per_stone_ms: int

def bench_nps(fixture: str, time_ms: int, runs: int = 3) -> NpsResult: ...
def bench_depth_at_time(fixture: str, time_ms: int) -> DepthAtTimeResult: ...
def bench_threat_latency(fixture: str, n_calls: int = 1000) -> ThreatLatencyResult: ...
def bench_selfplay(time_per_stone_ms: int, games: int = 5,
                   max_plies: int = 200) -> SelfplayThroughputResult: ...
```

### Fixture loader

Python fixtures mirror Rust fixtures via a JSON description loaded from
`benches/fixtures/positions.json`. Both Rust and Python read the same
position library — ensures bench parity.

```json
{
  "midgame_12": {
    "moves": [[0,0],[1,0],[-1,0],[0,1],[0,-1],[1,1],[-1,-1],[2,0],[-2,0],[0,2],[1,-1],[-1,1]]
  },
  ...
}
```

Builder applies moves in sequence via `Engine.place`. Rust side reads
this JSON at compile time via `build.rs` and emits a constant table for
the criterion `common::positions` module — single source of truth for
fixtures.

### CLI subcommands

Extend `hammerhead/hammerhead/cli.py`:

```
hammerhead bench micro [--target NAME]    # runs criterion; calls bench_drain
hammerhead bench nps      --time-ms 1000 --fixture midgame_12 [--runs 3]
hammerhead bench depth    --time-ms 1000 --fixture midgame_12
hammerhead bench threats  --fixture midgame_30 [--samples 1000]
hammerhead bench selfplay --time-ms 200 --games 5 [--max-plies 200]
hammerhead bench all      [--time-ms 1000]
hammerhead bench diff <run_a.json> <run_b.json>
```

`hammerhead bench all` runs everything and writes
`benches/results/<isodate>-<sha>.json`.

### Output schema (Python macro layer)

Single canonical JSON file per run, extending the Rust schema with a
`macro` array:

```json
{
  "schema_version": 1,
  "timestamp": "...",
  "git_sha": "...",
  "host": { ... },
  "micro": [ /* criterion drain output */ ],
  "macro": {
    "nps": [
      { "fixture": "midgame_12", "time_ms": 1000,
        "depth_reached": 7, "nodes": 245677, "nps": 245677.0 }
    ],
    "depth_at_time": [ ... ],
    "threat_latency": [ ... ],
    "selfplay_throughput": [ ... ]
  }
}
```

## Diff tool

`hammerhead bench diff <a.json> <b.json>` — table output:

```
metric                            baseline    candidate    delta    pct
─────────────────────────────────────────────────────────────────────────
threats::compute / midgame_30      4321 ns      3987 ns    -334    -7.7%
eval::cached_eval / midgame_12     6512 ns      6234 ns    -278    -4.3%
search NPS / midgame_12          245677         263114   +17437    +7.1%
selfplay plies/sec @ 200ms          18.4         19.2     +0.8    +4.3%
─────────────────────────────────────────────────────────────────────────
regressions: 0, improvements: 4 (significant: 3 at p<0.05)
```

Significance test: paired comparison using criterion's per-sample data.
For macro benches without per-sample data: report delta only, no p-value.

## Makefile additions

```make
bench:
	@$(VENV)/bin/python -m hammerhead.cli bench all --time-ms $(BENCH_TIME_MS)

bench-micro:
	@cd hammerhead-engine && cargo bench --bench bench_$(TARGET)

bench-diff:
	@$(VENV)/bin/python -m hammerhead.cli bench diff \
	    benches/results/$(A).json benches/results/$(B).json

bench-baseline:
	@$(VENV)/bin/python -m hammerhead.cli bench all --time-ms 1000
	@cp benches/results/$$(ls -t benches/results/ | head -1) \
	    benches/results/baseline.json
```

Defaults:
```
BENCH_TIME_MS ?= 1000
TARGET        ?= all
```

## Gitignore

```
benches/results/*.json
!benches/results/baseline.json
target/criterion/
```

`baseline.json` checked in. Updated via `make bench-baseline` after a
verified-improvement change. PR description should include
`make bench-diff A=baseline B=<latest>` output for any change that
touches a hot path.

## What to bench, what not to

Bench:
- Anything in the search inner loop.
- Place / undo / hash.
- Threat compute (full + incremental).
- Eval (each layer, cached vs cold).
- Move generation (all radii).
- TT probe / store.
- End-to-end NPS at standard time budgets.

Do not bench:
- Build / codegen / startup.
- One-off init paths (`Engine::new`).
- Python ↔ Rust boundary (covered by macro benches indirectly).
- Notation parsing.

## Reference node-counts (Phase 12)

`hammerhead bench reference` produces a deterministic node-count table:
search at fixed depths `1..=N` on specific fixtures, no time budget,
TT cleared between runs. Each call uses a freshly-constructed
`Engine`, so move ordering, killer slots, and history all start from
defaults — output is bit-for-bit reproducible.

Output: appended to the canonical JSON under a new `reference` array
(stays empty when the subcommand is not part of the run):

```json
{
  "reference": [
    { "fixture": "empty",      "depth": 1, "nodes": 1,    "ms": 0 },
    { "fixture": "empty",      "depth": 2, "nodes": 12,   "ms": 0 },
    ...
    { "fixture": "midgame_12", "depth": 8, "nodes": 1234, "ms": 187 }
  ]
}
```

Fixtures used (default): `empty`, `single_origin`, `midgame_12`,
`midgame_30`. Depth range: `1..=8`. The cumulative wall-clock budget
per fixture (`[bench.reference] budget_s`) caps total runtime — once
exceeded, subsequent depths for that fixture are skipped. Configured
in `hexo.toml`:

```toml
[bench.reference]
fixtures   = ["empty", "single_origin", "midgame_12", "midgame_30"]
max_depth  = 8
budget_s   = 60
```

This table is the regression net for optimization phases: any change
that touches search must produce identical `nodes` at every
`(fixture, depth)` unless the change is explicitly about move
ordering or pruning. Node-count drift = behaviour change = explain
or revert.

## TT statistics (Cargo feature `tt_stats`)

Behind Cargo feature `tt_stats`, the TT tracks probe / hit / store /
collision counts via `AtomicU64` counters. Zero-cost when the feature
is off: no fields, no code paths.

```rust
#[derive(Clone, Copy, Debug, Default)]
pub struct TTStatsSnapshot {
    pub n_slots: usize,
    pub occupied: usize,
    pub generation: u8,
    pub probes: u64,      // always 0 without feature
    pub hits: u64,        // always 0 without feature
    pub stores: u64,      // always 0 without feature
    pub collisions: u64,  // always 0 without feature
}
```

Exposed via `Engine::tt_stats() -> TTStatsSnapshot` and the PyO3
wrapper `PyEngine.tt_stats() -> dict` (always present; `probes` etc.
read as `0` when the feature is off, so callers can branch on
`probes == 0` to detect "no stats" rather than guarding the import).

`hammerhead bench reference --tt-stats` and `hammerhead bench nps --tt-stats`
read the snapshot after the run and include hit rate (`hits/probes`)
in the canonical JSON. Production builds (cdylib via `maturin
develop --release`) do not enable the feature. Dev / regression
builds opt in via `cargo build --features tt_stats` or `maturin
develop --release --features tt_stats`.

The `new_generation` and `clear` paths reset all counters, so a
fresh `Engine` starts at zero regardless of generation cycles.

## ms-time scaling table (Phase 14)

`hammerhead bench scaling` produces a table of `(fixture, time_budget_ms) →
(depth_reached, nodes, NPS)`. Time budgets: `[1, 10, 50, 100, 250,
500, 1000]` ms. Fixtures: same default set as reference. Each cell
is the median over `runs` runs (cold TT each run — fresh Engine).

Output: `scaling` array appended under `macro` in canonical JSON:

```json
{
  "scaling": [
    {
      "fixture": "midgame_12",
      "time_ms": 50,
      "depth": 3,
      "nodes": 8200,
      "nps": 164000,
      "ci95_lo": 158000,
      "ci95_hi": 169000
    }
  ]
}
```

Purpose: validate ms-time + sub-second strength claims. ms-time
scaling is a separate axis from steady-state NPS — at very short
budgets the iterative-deepening overhead and first-iteration latency
dominate, so raw NPS isn't predictive.

`ci95_lo` / `ci95_hi` are the percentile-bootstrap 95% CI on the
per-run NPS (not a Wilson CI — Wilson is for binomial proportions).
With small `runs` (e.g. 5) we fall back to min / max as a conservative
band.

## Per-function cycles breakdown (Phase 14)

`hammerhead bench breakdown` runs each fixture at depth 4 (fixed, no time
budget) and estimates the share of total search cycles spent in each
top-level module by combining criterion micro-bench medians with
calls-per-search counts. Reported as a table:

```json
{
  "breakdown": [
    { "fixture": "midgame_12", "depth": 4, "function": "eval",        "pct_cycles": 38.2 },
    { "fixture": "midgame_12", "depth": 4, "function": "threats",     "pct_cycles": 21.4 },
    { "fixture": "midgame_12", "depth": 4, "function": "moves",       "pct_cycles":  5.1 },
    { "fixture": "midgame_12", "depth": 4, "function": "ordering",    "pct_cycles":  8.0 },
    { "fixture": "midgame_12", "depth": 4, "function": "tt",          "pct_cycles":  4.5 },
    { "fixture": "midgame_12", "depth": 4, "function": "search_other","pct_cycles": 22.8 }
  ]
}
```

Function categories: `eval`, `threats`, `moves`, `ordering`, `tt`,
`search_other` (residual = 100% − sum). Hard-coded mapping from
criterion group names to categories.

The numbers are **estimates**, not a profile — caveat their use.
Their value is trend tracking across phases. Use `make flamegraph`
for ground-truth profiling.

## Future extensions (post-baseline)

- Memory benchmarks: `peak_rss` per fixture at depth N.
- Cache-miss profiling via `perf`.
- TT hit-rate over a typical game (instrumentation hook in `search.rs`).
- A/B harness: tweak one `hexo.toml` parameter, re-run, compare.
- Regression CI: GitHub Actions job runs `make bench all` on every PR;
  fails if any micro-bench regresses by > 5% at p < 0.01.

Out of scope for v1.

## Bench tiers

Three tiers for different feedback latencies:

### `bench-quick` (~5-15s)

Single-fixture, single-budget NPS+depth check. Used for inner-loop
iteration during a sub-step.

- Fixture: midgame_12 (configurable via `--fixture`)
- Time budget: 500 ms
- Runs: 3
- Output: `nps_mean / nps_stddev / depth_reached`
- Comparison: against `.hexo/quick_baseline.json` (last `bench-quick`
  cached locally; `.hexo/` is gitignored — per-developer, not shared)

CLI:
    hammerhead bench quick [--fixture F] [--time-ms T] [--runs N]

Output format (single line, machine-readable + human-friendly):
    quick: 348k ± 4k NPS, depth 5, 11600 cyc/node (Δ +3.1% vs last)

### `bench-perf` (~30-60s)

Two-fixture, multi-budget check. Used at end of a sub-step before
commit.

- Fixtures: midgame_12, midgame_30
- Budgets: 250 ms, 1000 ms
- Runs: 5 each
- Output: per-fixture × budget NPS + depth + cycles/node

CLI:
    hammerhead bench perf

### `bench-full` (~3-5 min)

Current `make bench all`. Used at the end of a phase + for baseline
commits. No semantic change from current behaviour.

### `cycles/node` metric

For each NPS measurement, compute `cycles_per_node = (cpu_ghz * 1e9
* time_s) / nodes`. Reported alongside NPS in all tiers. More
sensitive than NPS at picking up inner-loop changes — NPS lifts
from depth-shift can mask per-node regressions, but cycles/node
is monotonic in per-node work.

The `cpu_ghz` value is auto-detected from `/proc/cpuinfo` on Linux,
falls back to `4.0` if unavailable. Documented in the output.

## Parallel match harness (Phase 17)

`make vs` parallelizes games across cores. The two engines per game
stay in-process via the existing subprocess-Bot model, but multiple
games run concurrently in worker processes.

Worker model: a pool of N worker processes, each playing a
configurable batch of games (default: ceil(total / N) games per
worker). Workers receive game configs (opening, side assignment,
seed) via a queue, return per-game records (result, plies,
fastpath/timeout flags) via a result queue. Coordinator aggregates
results and computes Wilson CI / Elo CI / SPRT decision.

N defaults to `max(1, cpu_count() - 2)` (leaves headroom for OS +
coordinator). Override via `N_WORKERS` env var or `--workers` CLI
arg.

Reproducibility: with `--seed SEED`, the assignment of (opening,
side, RNG-stream-seed) → game-index is deterministic across runs
and across worker counts. Two runs at the same total game count
and seed produce identical match records (modulo timer noise; the
engine's search is deterministic at fixed depth but a time-limited
search depends on wall-clock).

Memory bound: 2 engines per game × 64 MB TT per engine × N workers
~ N × 128 MB resident. At default N=14 on a 16-core host: ~1.8 GB.
Cap configurable via `MAX_TT_MB_PER_WORKER` (default 64).

Wall-clock target: a 200-game match at 1000 ms/stone (~120 plies/
game, ~240 s/game sequential) finishes in
   ceil(200 / N) × 240 s = ~15 × 240 s / 1 worker
   ≈ 60 minutes / 14 workers ≈ 4 minutes wall-clock.

CLI / Makefile:
   make vs N_GAMES=200 TIME_MS=1000 N_WORKERS=14
   make vs N_GAMES=50 TIME_MS=500 TEST=sprt   # SPRT mode unchanged
