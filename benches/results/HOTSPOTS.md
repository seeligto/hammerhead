# Hotspots — Phase 15 baseline

**Captured:** 2026-05-20 — git `15c9638`, rustc 1.94.0
**Host:** AMD Ryzen 7 8845HS (16 cores), Linux 7.0.3-arch1-2
**Bench data:** `benches/results/baseline.json` (`make bench` with
`--time-ms 1000 --tt-stats`, `.cargo/config.toml`
`target-cpu=native`, default features include `simd_eval` but **not**
`tt_stats` — TT hit-rate column is null in this run; re-build with
`--features tt_stats` and re-bench to populate it).
**Flamegraph:** `benches/results/flamegraph-2026-05-20T00-20-19-15c9638.svg`
(captured via `make flamegraph` — `perf record --call-graph dwarf
-F 997` over `bench_search` criterion runs at depth 2 / 4 / 6).

## Headline numbers

| Metric | Phase 13 | Phase 14 | Phase 15 | Δ vs Phase 14 |
|---|---:|---:|---:|---:|
| NPS, `midgame_12`, t = 1000 ms | 237,449 | 337,077 | **344,713** | **+2.3 %** |
| NPS, `midgame_30`, t = 1000 ms | 128,308 | 209,285 | **224,840** | **+7.4 %** |
| NPS, `empty`, t = 1000 ms | 307,200 | 392,674 | 398,128 | +1.4 % |
| NPS, `single_origin`, t = 1000 ms | 307,200 | 388,473 | 395,947 | +1.9 % |
| Depth @ 1 s, `midgame_12` | 5 | 5 | 5 | — |
| Depth @ 1 s, `midgame_30` | 6 | 7 | 7 | — |
| Depth @ 1 s, `empty` | 7 | 7 | 7 | — |
| Depth @ 1 s, `single_origin` | 6 | 6 | 6 | — |
| `cached_eval_cold`, `midgame_12` | 8.7 µs | 4.08 µs | 4.10 µs | ≈ |
| `cached_eval_cold`, `midgame_30` | 8.7 µs | 7.31 µs | 7.10 µs | -2.9 % |
| `threats::compute_full`, `midgame_12` | 1.33 µs | 1.12 µs | 1.10 µs | -1.8 % |
| `threats::compute_full`, `midgame_30` | 3.49 µs | 2.68 µs | 2.72 µs | +1.5 % |
| Threat latency cold, `midgame_12` | 2.96 µs | 1.43 µs | **1.31 µs** | **-8.6 %** |
| Threat latency cold, `midgame_30` | 6.34 µs | 3.63 µs | **2.58 µs** | **-28.7 %** |
| Threat latency cold, `endgame_60` | n/a | 3.84 µs | **2.76 µs** | **-28.0 %** |
| TT hit rate, `midgame_12` d = 6 | 15.57 % | 16.7 % | (not measured†) | — |
| Reference d8, `midgame_30` (truly fixed) | n/a | 815 ms | **465 ms** | **-43.0 %** |
| Reference d8, `midgame_12` (truly fixed) | n/a | (varied) | 4894 ms | — |

† This run was built without the `tt_stats` Cargo feature, so the
reference-table `tt_hit_rate` column is null. Phase 14 baseline had
it populated. Rebuild with `--features tt_stats` to repopulate.

### Phase 15 target table

| Target | Goal | Result |
|---|---|---|
| midgame_12 NPS | ≥ 400 k | 345 k (missed; +2.3 %) |
| midgame_30 NPS | ≥ 260 k | 225 k (missed; +7.4 %) |
| Depth-at-time midgame_12 @ 1 s | ≥ 6 | 5 (missed) |
| Depth-at-time midgame_30 @ 1 s | ≥ 7 | 7 (held) |
| `threats::compute_full` midgame_30 | ≤ 1 µs | 2.72 µs (missed — full path unchanged by design; incremental path is the win below) |
| Threat latency midgame_30 (cold) | implicit follow-up | **2.58 µs (-29 %)** ✅ |

The headline NPS goals were ambitious; the actual wins concentrate
in the `threat_latency` and reference-table columns. midgame_30
reference d=8 dropping from 815 ms → 465 ms (-43 %) reflects the
incremental-threats payoff at fixed depth — search reaches the same
depth in noticeably less time. At fixed time budget (1000 ms) the
NPS gain compounds with the same iterative-deepening schedule, so
the macro NPS jump is more modest (+7 %).

### Phase 15 changes that landed

1. **Incremental threat recompute** (STEP 2.1–2.3): `Board::threats()`
   short-circuits on `Cell<bool>` dirty flag; on dirty reads,
   `compute_with_scratch` dispatches to `incremental` when the
   scratch breakdown is populated. Linear-walk preserves
   `s0_instances` iteration order (load-bearing for
   `collect_stone1_defense`); cross-axis pattern matching runs only
   for anchors within `THREAT_CLUSTER_RADIUS` of any dirty center.
   Per-anchor breakdown lives in `ThreatScratch` (not `ThreatSet`)
   to keep the public type small.
2. **`RefCell<Option<ThreatSet>>` → `RefCell<ThreatSet>`** (STEP 3):
   the `Option` projection inside `Ref::map` is gone. Phase 14
   HOTSPOTS #5 (`pvs_node;threats;is_none;is_some<ThreatSet>`)
   disappears from the flamegraph.
3. **10k-position oracle test** (STEP 2.3,
   `tests/threats_oracle.rs`): fixed-seed random walk asserting
   `threat_set_equiv(incremental, full)` after every place / undo
   across 4 starting fixtures. Runtime ~5 s.
4. **`creates_s0` axis-run cache** (STEP 4): **REVERTED**. A 6-slot
   per-(axis, side) cache populated lazily in `OrderingContext`
   showed a consistent +22-30 % `bucket_value` micro-bench regression
   across every fixture (verified across 2 macro runs + 2 ordering
   micro-runs). The cache hit rate was too low to amortize the
   per-access overhead — candidate iteration order is hashset-random,
   so consecutive lookups rarely shared `(axis, line_id, side)`.
   Re-attempting it would need either candidate pre-sorting or a
   larger / cheaper cache shape — Phase 16 follow-up.

### Reviewer-pass fixes (post-STEP-5)

- `ThreatInstance::anchor` was dead metadata — removed (saves ~8
  bytes per instance × MAX_S0_INSTANCES × 2 players ≈ 1 KB per
  board).
- `SPEC_ENGINE.md` / `SPEC_EVAL.md` updated to reflect the shipped
  algorithm shape (no `Option` wrapper, linear-walk recompute).
- Oracle test seed comment fixed (was `0xHEX0_F00D`, actually
  `0xDEAD_F00D_CAFE_BEEF`).

## Flamegraph-derived ranking (authoritative)

Sampled stacks aggregated by user-space leaf and stack tail. Sample
counts are totals across `bench_search` configurations (depth 2 / 4
/ 6 on `midgame_12`).

### #1 — `eval::layer1_window_scan;scan_line` (unchanged)

| Stack tail | Samples |
|---|---:|
| `quiescence_node;cached_eval;eval;layer1_window_scan;scan_line;extension_factor;classify;is_set` | 4.98 M |
| `quiescence_node;cached_eval;eval;layer1_window_scan;scan_line;encode_ternary_batch;…;encode_ternary` | 4.98 M |
| `eval;layer1_window_scan;scan_line;extension_factor;classify;is_set;get;indices` | 4.87 M |

Phase 14's #1 hotspot remains the top user-space chain. The
SIMD `encode_ternary_batch` shows up alongside the
`extension_factor;classify;is_set` chain — both per-window costs.

**Phase 16 candidates:**
- Inline `extension_factor` into the AVX2 batch.
- AVX-512 32-wide windows on Zen 4 hosts.

### #2 — `for_each_in_range<…proximity>` (now relatively #2)

| Stack tail | Samples |
|---|---:|
| `for_each_in_range<board::remove_proximity>` | 9.35 M (folded) |
| `for_each_in_range<board::add_proximity>;...;find_inner;likely` | 4.90 M |
| `for_each_in_range<…remove_proximity>;{closure#0};get_mut<…>;find_inner;full` | 4.92 M |

Phase 14 #3 promoted to #2 since the cross-axis path shrank.
`Board::proximity_count` / `inner_proximity_count` are still
`FxHashMap<Coord, u32>`; the per-`place` ring update spends ~10 M
samples on hashbrown probes.

**Phase 16 candidates:**
- Flat-array proximity counts (same playbook as Phase 13
  `AxisBitmaps`). Key space is bounded by `MAX_PIECE_DISTANCE` rings
  around live pieces; needs design work — see SPEC_ROADMAP
  "Phase 16 candidates".

### #3 — `axis_run_through_empty` / `creates_s0` chain

| Stack tail | Samples |
|---|---:|
| `axis_run_through_empty` | 4.99 M |
| `creates_s0;line;idx` | 4.97 M |
| `creates_s0;next<Axis, 3>;…` | 4.90 M |
| `creates_s0;run_backward;wrapping_sub` | 4.57 M |

The ordering predicates haven't changed structurally (the STEP 4
cache that targeted them was reverted). They're now the dominant
ordering cost. Phase 16 should look at either:
- Caching with a cheaper structure (e.g., a candidate-pre-sort that
  improves locality enough to help even a 6-slot last-query cache).
- Pre-computing the per-axis run-length for each candidate at move
  generation time rather than per-bucket-value call.

### #4 — `walk_linear_runs;run_endpoints`

| Stack tail | Samples |
|---|---:|
| `walk_linear_runs;run_endpoints;idx` | 4.94 M |

Linear-walk in `threats::full_recompute` and
`threats::incremental` (both walk every piece's linear runs to
preserve `s0_instances` iteration order). Phase 14 had a
`walk_linear_runs;run_endpoints;run_forward;get;indices` chain at
~5 M — roughly unchanged.

**Phase 16 candidate:**
- Per-line classification cache on `ThreatScratch` (the
  cross-axis-per-piece pattern, applied to linear). Would let
  incremental skip line classifications on lines outside every
  dirty radius. Requires retaining per-line counts contributions
  (open_3 / closed_3 / open_2), not just s0_instances.

### #5 — `matches_pattern<4>` (much reduced)

| Stack tail | Samples |
|---|---:|
| `matches_pattern<4>;is_player;is_set;get;indices` | 4.88 M |

Phase 14 had `walk_cross_axis;matches_pattern<2,3>` at ~5 M each.
Incremental cross-axis (only dirty-cluster anchors recompute)
collapsed those frames — only `matches_pattern<4>` (trapezoid /
bone) remains visible. Reflects the ~29 % drop in
`threat_latency / midgame_30.cold`.

### #6 — `compute_with_scratch;incremental;reset;clear<FxHashSet>`

| Stack tail | Samples |
|---|---:|
| `compute_with_scratch;incremental;reset;clear<FxHashSet>;…;clear_no_drop;bucket_mask_to_capacity` | 4.40 M |

New entry in Phase 15. The `ThreatScratch::reset` call at the top
of every incremental compute clears the `seen` `FxHashSet`. With
the hashset's `clear` walking buckets to drop entries, this is the
visible cost.

**Phase 16 candidate:**
- Replace the `FxHashSet<(Axis, i16, i16)>` line-dedup with a flat
  `[bool; LINE_ID_RANGE × 3 axes]` bitset — same idea as Phase 13's
  axis-bitmap flat-array refactor, applied here.

## Kernel frame discount

| Stack tail | Phase 13 | Phase 14 | Phase 15 |
|---|---:|---:|---:|
| `unmap_page_range` chain | 41 M | ~30 M | ~30 M |
| `asm_exc_page_fault` chain | 34 M | ~25 M | ~25 M |
| `do_anonymous_page` chain | 26 M | ~20 M | ~20 M |

Stable since Phase 14. Residual kernel activity is criterion
measurement + bench-harness TT allocation.

## Phase 16 entry points

In rough leverage order:

1. **Proximity-set flat array** (now #2 in flamegraph): replicate
   the Phase-13 axis-bitmap flat-array fix on
   `Board::proximity_count` / `inner_proximity_count` and
   `candidate_cells`. Recovers ~10 M samples per place/undo cycle.
2. **`extension_factor` inlining / SIMD batch** (Layer 1 follow-up):
   pull the boundary `is_set` probes into the AVX2 encode batch so
   the per-window multiplier is computed in-register.
3. **`creates_s0` per-axis run cache, take 2**: candidate
   pre-sorting by `(axis, line_id)` before bucket_value would lift
   the 6-slot last-query cache hit rate substantially. Worth
   measuring whether sort + amortized cache > current uncached.
4. **Per-line `LineContribution` cache on `ThreatScratch`**: extend
   the per-anchor cross-axis cache pattern to linear, so
   incremental can skip line classifications outside every dirty
   radius. Doubles the savings of STEP 2.2.
5. **`FxHashSet<(Axis, i16, i16)> seen` → flat bitset**: incremental
   threats' top user-space frame (4.4 M samples) is the hashset
   clear. Flat bitset is `O(LINE_ID_RANGE × 3)` clear plus O(1) set.
6. **AVX-512 32-wide `encode_ternary`**: doubles Layer 1 batch
   width on Zen 4 / Sapphire Rapids.
7. **PGO with self-play training set** (Phase 14 STEP 9 left this
   open).

## How to refresh this report

```bash
cd hexo-engine && maturin develop --release --features tt_stats
cd .. && make flamegraph
make bench BENCH_TIME_MS=1000
make bench-diff A=baseline B=<latest-isodate-sha>
# Re-rank the top sections above based on the new folded.txt.
```
