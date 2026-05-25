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

### iai-callgrind — deterministic gate

Wall-clock NPS benches resolve ~2% changes; the Phase 26.5 / 28F-2 sweeps
straddled zero at 200 g × 500 ms, leaving sub-1% improvements unprovable
without burning a multi-hour arena run. **iai-callgrind** sidesteps this
by counting instructions / D1 misses / Lmd misses under valgrind —
deterministic to within tens of instructions, runs in seconds.

- **File:** `hammerhead-engine/benches/iai_search.rs`. Two fixtures
  (`midgame_12`, `midgame_30`) at fixed depth 6, calling `search_root`
  directly. No timeout; the search runs to completion.
- **Dep:** `iai-callgrind = "0.16"` under `[dev-dependencies]`.
- **Invocation:** `cargo bench --bench iai_search`. Requires `valgrind`
  (Arch: `pacman -S valgrind`).
- **Determinism bar:** two consecutive runs must agree to ≤ 50
  instructions per bench. Any drift beyond that is a host-state bug
  (background load, CPU pinning, perf-paranoid setting), not a code
  change.
- **Acceptance for refactors expected to be byte-identical:** delta
  ≤ ±50 ins. Otherwise the delta IS the measurement — iai-callgrind
  is the source of truth for sub-1% changes.
- **Relation to `bench-quick`:** iai gates correctness of the change;
  `bench-quick` gates the wall-clock translation. Run both per
  A/B unless the change is build-only (PGO, codegen flag), in
  which case iai is uninformative.
- **Caveat:** PGO changes code layout (function placement, basic-block
  ordering), not instruction count. Expect iai cycle estimates to
  shift slightly under PGO; ins/bench stays roughly constant.

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
in the canonical JSON. Production / SDK builds (cdylib via `make
build` → `maturin develop --release`) do not enable the feature.

The `make bench` and `make bench-baseline` targets depend on
`make build-tt-stats` (`maturin develop --release --features
tt_stats`), so the full sweep / baseline run against a build that
carries the counters and `baseline.json` populates `tt_hit_rate`.
The fast NPS tiers (`make bench-quick`, `make bench-perf`) do *not*
rebuild — they use whatever `make build` installed (feature-free),
to keep per-call NPS unpolluted by the counter overhead.

> **Ordering hazard.** `make bench` / `make bench-baseline` leave a
> `tt_stats` build installed. Run `make build` afterwards to restore
> the feature-free production build before any NPS measurement
> (`bench-perf` / `bench-quick`) or strength run.

When `--tt-stats` is requested but the loaded extension was built
feature-free, every `tt_stats()` snapshot reads `probes == 0`;
`bench reference` then records `tt_hit_rate: null` and emits a
stderr `WARNING` (it does not fail — feature-free builds are a
legitimate choice). Dev / regression builds opt in via
`cargo build --features tt_stats` or `maturin develop --release
--features tt_stats` (equivalently `make build-tt-stats`).

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

## Per-function cycles breakdown (Phase 14, rederived Phase 25)

`hammerhead bench breakdown` reports the share of engine self-time spent
in each top-level module, derived from a flamegraph `folded.txt`
capture. It is a single whole-capture profile — **not** per-fixture (the
Phase 14 per-fixture/per-depth model is retired; see § Methodology fixes
(Phase 25)). Reported as a table:

```json
{
  "breakdown": [
    { "fixture": "flamegraph-2026-05-21T21-44-40-69e2053.folded.txt", "depth": 0, "function": "eval",         "pct_cycles": 35.56 },
    { "fixture": "flamegraph-2026-05-21T21-44-40-69e2053.folded.txt", "depth": 0, "function": "threats",      "pct_cycles":  9.64 },
    { "fixture": "flamegraph-2026-05-21T21-44-40-69e2053.folded.txt", "depth": 0, "function": "moves",        "pct_cycles":  0.00 },
    { "fixture": "flamegraph-2026-05-21T21-44-40-69e2053.folded.txt", "depth": 0, "function": "ordering",     "pct_cycles":  7.94 },
    { "fixture": "flamegraph-2026-05-21T21-44-40-69e2053.folded.txt", "depth": 0, "function": "tt",           "pct_cycles":  0.00 },
    { "fixture": "flamegraph-2026-05-21T21-44-40-69e2053.folded.txt", "depth": 0, "function": "board",        "pct_cycles": 26.20 },
    { "fixture": "flamegraph-2026-05-21T21-44-40-69e2053.folded.txt", "depth": 0, "function": "search_other", "pct_cycles": 20.66 }
  ]
}
```

The JSON shape is unchanged (`fixture` / `depth` / `function` /
`pct_cycles`). `fixture` now carries the folded-file name (the capture
identity); `depth` is always `0`. Function categories: `eval`,
`threats`, `moves`, `ordering`, `tt`, `board`, `search_other` (residual
= 100% − sum). `board` is new in Phase 25 — proximity / coords / zobrist
board-maintenance work is a real ~25% category and was previously hidden
inside `search_other`.

The numbers are a **best-effort profile**, not exact. Their value is
trend tracking across phases. Use `make flamegraph` + `perf report` for
ground-truth profiling.

> **Phase 25 repair (STEP 2.1).** Phase 24 found this metric
> structurally broken: it summed raw criterion micro medians with no
> call-count weighting, so `moves` showed as 53.9 % of `midgame_30`
> d=4 (the `generate(r=4/8)` micros are large but the search runs
> only r=2) and `tt` always read 0.0 % (criterion name mismatch).
> Phase 25 STEP 2.1 rederives the breakdown directly from flamegraph
> self-time samples (ground truth). See § Methodology fixes (Phase 25).

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

Worker model: a `multiprocessing.Pool` of N worker processes
(`spawn` context) fed via `imap_unordered` — one game per task,
work-stealing, so uneven game lengths self-balance. A `Pool`
`initializer` broadcasts the two engine commands to each worker
once. Each task spawns two fresh engine subprocesses, plays one
game, and returns a `ParallelGameResult` (winner, plies,
wall-seconds, `notes` for crashes/timeouts). The coordinator
aggregates results, sorts by `game_idx`, and computes Wilson CI /
Elo CI / SPRT decision; crashed games (`notes` set) are excluded
from the tally.

N defaults to `max(1, cpu_count() - 2)` (leaves headroom for OS +
coordinator). Override via `N_WORKERS` env var or `--workers` CLI
arg.

Reproducibility: `build_game_configs` assigns (game_idx → colour)
deterministically — with `color_balance`, even indices play
`current` as X. That assignment is identical across runs and
worker counts. Game *outcomes* are not reproducible: the harness
runs a time-limited search, which depends on wall-clock. (The `vs`
match harness has no opening book and takes no `--seed`.)

Opening diversity (Phase 28E-2 Stage 0): when
`[promote].opening_diversity = true`, `build_game_configs` calls
`hammerhead.openings.pick_opening(i // 2)` per pair, so games
`2k` and `2k+1` share the same forced opening, colour-swapped.
The opening's plies are applied to both engines in
`play_one_game` before either bot is asked to search. The library
ships ~20 named openings curated from HeXOpedia §6 (Sword family,
Pair / Closed Game variants, Pistol / Shotgun / Revolver, Island
Gambit, Marge, Eclipse, etc., plus a small set of mechanically-
identical control rotations). Selection is `seed % len(OPENINGS)`
— deterministic and reproducible from the seed alone.

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

## Optuna BO sweep driver (Phase 28C-1)

`hammerhead tune-bo` is the Bayesian-optimisation companion to the
Phase 28B-1 coordinate-descent driver (`hammerhead bench tune-sweep`).
It wraps Optuna 4.8's `GPSampler` (Matérn-5/2 GP surrogate, ARD per-
dim length-scales, `deterministic_objective=False` so the noise
variance is learned from data) around the same Phase 17 parallel
match harness used by `make vs` and `tune-sweep`. Trial-side
`EvalOverrides` land in the candidate engine via the
`HEXO_EVAL_OVERRIDES` env var (same contract as `tune.py`); the
reference engine is a fixed-SHA worktree binary passed via
`--reference-binary`, invoked through `env -u HEXO_EVAL_OVERRIDES`
so it always runs the baseline config. Opening diversity is forced
off per Phase 28A.5.

The study persists to a SQLite database (`load_if_exists=True` makes
the sprint trivially resumable), with one atomic per-trial JSON
sidecar (`<output-dir>/<trial>.json`) carrying the Wilson CI bounds
and the Wilson-derived `elo_sem` user-attr. The HEAD config is
enqueued as trial #0 to anchor the GP at a known-good point without
wasting a random-init slot; trials 1..9 are random warm-up
(`n_startup_trials=10`), then GP-informed thereafter. Trial budget
defaults to 60 × 200 games; `--smoke` drops to 2 × 5 games for
wiring verification.

`hammerhead tune-bo-report --study-name … --storage …` is the read-
only post-hoc reporter: top-K trials by Elo, `study.best_params`
(Optuna's running argmax; the closest proxy to a GP-posterior
argmax exposed in 4.8), and fANOVA parameter importance via
`optuna.importance.get_param_importances`. Dispatcher-side drift
correction (rolling baseline-vs-baseline self-test every K trials)
is handled outside the driver per Phase 28C-1 design §4 (kept out so
the driver stays a pure consumer of the harness, parallel to
`tune.py`).

## Methodology fixes (Phase 25)

Three measurement-infrastructure repairs surfaced by Phase 24.

### `bench breakdown` — derived from flamegraph self-time

The Phase 14 `bench breakdown` metric (§ Per-function cycles
breakdown) summed raw criterion micro medians with no call-count
weighting — structurally broken (Phase 24 § C). STEP 2.1 rederives
the breakdown by parsing a flamegraph `folded.txt` capture.

**Folded format.** `inferno-collapse-perf` emits one line per unique
stack: `frame_a;frame_b;...;leaf COUNT`. Self-time is attributed to the
**leaf** frame. Frames can contain spaces (generic argument lists), so
the count is the final whitespace-delimited token, not `$2` of a naive
split.

**Classification.** Each leaf maps to a bucket two ways, in order:

1. an explicit leaf-function-name table (`_BREAKDOWN_LEAF_FN` in
   `benchmark.py`), built from the engine source — most hot leaves are
   inlined under `target-cpu=native` + LTO and carry no `module::`
   token, so the bare demangled name is the only signal;
2. otherwise, the nearest `hammerhead_engine_core::<module>::` token
   walking leaf-inward, mapped via `_BREAKDOWN_MODULE`.

A stack with no search-recursion frame (`pvs_node` / `quiescence_node` /
`collect_stone1_defense`) is `harness` — the criterion driver, rayon KDE
analysis, and TT-vec setup allocation — and is **excluded**. Remaining
percentages are renormalised to engine-only self-time and sum to 100.

**Locating the capture.** Defaults to the newest
`benches/results/flamegraph-*.folded.txt`; override with
`bench breakdown --folded PATH`. When no `folded.txt` exists the
subcommand prints an empty `breakdown` array and a stderr warning that
breakdown now requires a `make flamegraph` capture — so the `bench all`
sweep still succeeds when run before a flamegraph has been taken.

**Accuracy / known limits.** Frame-pointer captures are FP-shallow, so a
sizeable share of leaves are generic helpers (`get`, `mul`, `indices`,
`eq`) that cannot be confidently attributed; these land in
`search_other` by design (~20% of the Phase 24 capture). The metric is
deliberately conservative — it attributes only what is identifiable
rather than guessing. Cross-checked against the Phase 24 flamegraph
(`flamegraph-2026-05-21T21-44-40-69e2053.folded.txt`): `tt` reads
**0.0%**, matching the HOTSPOTS.md `< 0.5%` finding exactly (and for the
right reason — the TT is genuinely cold — not the old name-mismatch
artefact); `eval` ~36% and `board` ~26% are in the right order of
magnitude as the top-two engine costs. Demangled symbol spelling is not
stable across rustc versions — the `_BREAKDOWN_LEAF_FN` table may need a
refresh after a toolchain bump; verify against `perf report` when in
doubt.

### Flamegraph capture — frame-pointer based

`make flamegraph` (`scripts/flamegraph.sh`) captures with
`perf --call-graph fp` (frame pointers), **not** `--call-graph dwarf`.
The dwarf unwinder's 8 KiB stack snapshot cannot unwind the LTO'd
`pvs_node → pvs_dance → quiescence_node` recursion and collapses every
search sample into an unattributable `[unknown]` leaf. `fp` is the only
capture mode — there is intentionally no `dwarf` fallback or env-toggle,
since a silent fallback would re-introduce the Phase 24 regression.

Frame pointers are forced by `scripts/flamegraph.sh`, which builds the
bench binary with `RUSTFLAGS=-C force-frame-pointers=yes`. LTO +
`target-cpu=native` would otherwise omit them on leaf functions even
with `debug = true` / `strip = "none"`, collapsing the search recursion
into `[unknown]`. Cargo exposes no `[profile]` key for frame pointers
(`profile.bench.force-frame-pointers` is silently ignored as an unused
manifest key), so the `RUSTFLAGS` route in the script is the single
mechanism. `[profile.bench]` keeps `debug = true` / `strip = "none"` so
`perf` can symbolize the captured frames.

To verify a capture is good: run `make flamegraph`, then inspect the
generated `folded.txt`:

```
F=$(ls -t benches/results/flamegraph-*.folded.txt | head -1)
grep -c 'eval' "$F"        # non-trivial count (hundreds+) — real frames
grep -c '\[unknown\]' "$F" # should be low / zero, not dominating
head -3 "$F"               # top stacks name real fns, not [unknown]/libc
```

Real frames look like `..._search..._node;..._eval...;... N`; a broken
dwarf capture collapses to `bench_search;[unknown] N`.

### TT statistics — `tt_stats` feature on bench builds

TT stats are gated behind the `tt_stats` Cargo feature (§ TT
statistics) so production builds stay zero-overhead. Phase 24 (§ E)
found that the production `make bench` build records
`tt_hit_rate: null` because the feature is off — the `--tt-stats`
flag passed by the `bench` / `bench-baseline` Makefile targets was a
no-op against a feature-free `.so`.

Phase 25 fix:

- New `make build-tt-stats` helper — `maturin develop --release
  --features tt_stats` + editable Python install.
- `make bench` and `make bench-baseline` declare `build-tt-stats` as
  a prerequisite, so the engine is rebuilt with the counters before
  the sweep runs and `baseline.json` populates `tt_hit_rate`.
- `make build` (production / SDK) is unchanged — still feature-free
  `maturin develop --release`. `make bench-quick` / `make bench-perf`
  are unchanged — they do not rebuild and run on whatever `make
  build` installed, so per-call NPS is free of counter overhead.
- **Ordering hazard:** after `make bench` / `make bench-baseline` a
  `tt_stats` build is installed; run `make build` to restore the
  feature-free build before NPS measurement or strength runs. Noted
  in Makefile comments on the `bench` / `bench-quick` targets.
- `bench_reference` now detects `probes == 0` after a real search
  when `--tt-stats` was requested and emits a stderr `WARNING`
  ("TT stats unavailable — build was not compiled with --features
  tt_stats") instead of silently writing `null`. It does not fail:
  a feature-free build is a legitimate deliberate choice.
