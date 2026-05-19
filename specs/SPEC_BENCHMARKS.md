# HeXO Bot — Benchmark Spec

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
hexo-engine/
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
use hexo_engine::*;

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
`hexo-engine/src/bin/bench_drain.rs` that walks `target/criterion/` and
collects results into `benches/results/<isodate>-<sha>.json`. Called by
`make bench` at the end of the criterion run.

Schema versioned. Diff tool refuses to compare across schema versions.

## Python macro-benches

`hexo/hexo/benchmark.py` exposes a small library. CLI exposes
`hexo bench` subcommand (already stub'd Phase 9; extended here).

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

Extend `hexo/hexo/cli.py`:

```
hexo bench micro [--target NAME]    # runs criterion; calls bench_drain
hexo bench nps      --time-ms 1000 --fixture midgame_12 [--runs 3]
hexo bench depth    --time-ms 1000 --fixture midgame_12
hexo bench threats  --fixture midgame_30 [--samples 1000]
hexo bench selfplay --time-ms 200 --games 5 [--max-plies 200]
hexo bench all      [--time-ms 1000]
hexo bench diff <run_a.json> <run_b.json>
```

`hexo bench all` runs everything and writes
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

`hexo bench diff <a.json> <b.json>` — table output:

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
	@$(VENV)/bin/python -m hexo.cli bench all --time-ms $(BENCH_TIME_MS)

bench-micro:
	@cd hexo-engine && cargo bench --bench bench_$(TARGET)

bench-diff:
	@$(VENV)/bin/python -m hexo.cli bench diff \
	    benches/results/$(A).json benches/results/$(B).json

bench-baseline:
	@$(VENV)/bin/python -m hexo.cli bench all --time-ms 1000
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

## Future extensions (post-baseline)

- Memory benchmarks: `peak_rss` per fixture at depth N.
- Cache-miss profiling via `perf`.
- TT hit-rate over a typical game (instrumentation hook in `search.rs`).
- A/B harness: tweak one `hexo.toml` parameter, re-run, compare.
- Regression CI: GitHub Actions job runs `make bench all` on every PR;
  fails if any micro-bench regresses by > 5% at p < 0.01.

Out of scope for v1.
