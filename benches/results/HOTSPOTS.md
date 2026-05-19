# Hotspots — Phase 14 baseline

**Captured:** 2026-05-19 — git `622dfdd`, rustc 1.94.0
**Host:** AMD Ryzen 7 8845HS (16 cores), Linux 7.0.3-arch1-2
**Bench data:** `benches/results/baseline.json` (`make bench` with
`--features tt_stats`, `--time-ms 1000`, `.cargo/config.toml`
`target-cpu=native`, default features include `simd_eval`).
**Flamegraph:** `benches/results/flamegraph-2026-05-19T19-06-00-622dfdd.svg`
(captured via `make flamegraph` — `perf record --call-graph dwarf
-F 997` over `bench_search` criterion runs at depth 2 / 4 / 6).

## Headline numbers

| Metric | Phase 12 | Phase 13 | Phase 14 | Δ vs Phase 13 |
|---|---:|---:|---:|---:|
| NPS, `midgame_12`, t = 1000 ms | 234,933 | 237,449 | **337,077** | **+42 %** |
| NPS, `midgame_30`, t = 1000 ms | 126,480 | 128,308 | **209,285** | **+63 %** |
| NPS, `empty`, t = 1000 ms | 287,321 | 307,200 | 392,674 | +28 % |
| NPS, `single_origin`, t = 1000 ms | 286,720 | 307,200 | 388,473 | +26 % |
| Depth @ 1 s, `midgame_12` | 5 | 5 | 5 | — |
| Depth @ 1 s, `midgame_30` | 6 | 6 | **7** | **+1** |
| Depth @ 1 s, `empty` | 7 | 7 | 7 | — |
| Depth @ 1 s, `single_origin` | 6 | 6 | 6 | — |
| `cached_eval_cold`, `midgame_12` | 8.7 µs | 8.7 µs | **4.08 µs** | **−53 %** |
| `cached_eval_cold`, `midgame_30` | 8.4 µs | 8.7 µs | **7.31 µs** | **−16 %** |
| `cached_eval_warm`, all fixtures | 0.06 µs | 0.06 µs | 0.42 µs† | — |
| `eval::layer1_window_scan`, `midgame_12` | 1.57 µs | 1.36 µs | **0.69 µs** | **−49 %** |
| `eval::layer1_window_scan`, `midgame_30` | 3.10 µs | 2.80 µs | **1.29 µs** | **−54 %** |
| `threats::compute_full`, `midgame_12` | — | 1.33 µs | **1.12 µs** | **−16 %** |
| `threats::compute_full`, `midgame_30` | — | 3.49 µs | **2.68 µs** | **−23 %** |
| Threat latency cold, `midgame_12` | 3.26 µs | 2.96 µs | **1.43 µs** | **−52 %** |
| Threat latency cold, `midgame_30` | 6.64 µs | 6.34 µs | **3.63 µs** | **−43 %** |
| TT hit rate, `midgame_12` d = 6 | 15.08 % | 15.57 % | 16.7 % | +1.1 pt |
| TT hit rate, `midgame_30` d = 6 | 23.30 % | 23.30 % | 23.30 % | — |
| ms-time scaling, `midgame_12` @ 50 ms | n/a | n/a | **depth 3** | new |
| ms-time scaling, `midgame_30` @ 500 ms | n/a | n/a | **depth 6** | new |
| ms-time scaling, `midgame_30` @ 50 ms | n/a | n/a | depth 4 | new |
| Reference d8, `midgame_12` (truly fixed) | (time-truncated) | (time-truncated) | 711,810 | reference now deterministic at every depth |

† the warm `cached_eval` jump comes from a Phase 14 bench-harness
artifact: the latency-measurement loop now runs without the prior
opportunistic JIT-like warm-up, so the warm column reads the
hairpin-style fast-path more honestly. Not a regression in the eval
itself (`cached_eval_warm` is still an atomic load of a cached field).

The midgame headline NPS numbers hit / closely approach the Phase 14
prompt targets:

- midgame_12 NPS ≥ 350k → **337k**, slightly under (close enough that
  cold-cache run-to-run variance occasionally crosses the line; the
  consistent 3-run average is 337k).
- midgame_30 NPS ≥ 200k → **209k**, exceeded.
- Depth-at-time midgame_12 @ 1 s ≥ 7 → 5, missed (still bounded by
  static-eval work per node; STEP 7's incremental threats was the
  intended depth-cliff lever).
- Depth-at-time midgame_30 @ 1 s ≥ 7 → **7**, met.
- ms-time scaling midgame_12 @ 50 ms ≥ 3 → **3**, met.
- ms-time scaling midgame_30 @ 500 ms ≥ 5 → **6**, exceeded.

## Reference-table note

Phase 13's baseline reference column was time-truncated at the
default 1 s budget for depths where the search couldn't finish (d ≥ 6
on `midgame_12`, d ≥ 7 on `single_origin` and `empty`). Phase 14
fixed `Engine::best_move` so a depth-only call honours the
SPEC_BENCHMARKS "no time budget" contract, and the new reference
column records the truly fixed-depth node counts. The `bench-diff`
tool flags the d ≥ 6 cells as "regressions" relative to the Phase 13
baseline; these are a one-time disruption and the new column is the
real regression net going forward.

## Flamegraph-derived ranking (authoritative)

Sampled stacks aggregated by user-space leaf and stack tail (kernel
frames stripped — see "Kernel frame discount" below). Sample counts
are totals across all `bench_search` configurations (depth 2 / 4 / 6
on `midgame_12`).

### #1 — `eval::layer1_window_scan;scan_line`

| Stack tail | Samples |
|---|---:|
| `eval;layer1_window_scan;scan_line` (folded children incl. SIMD batch + lookup) | 9.74 M |
| `quiescence_node;cached_eval;eval;layer1_window_scan;scan_line;extension_factor;classify;is_set` | 9.25 M |
| `quiescence_node;cached_eval;eval;layer1_window_scan;scan_line` | 4.89 M |

The Phase 13 #1 hotspot (`encode_ternary` at 251 M samples) no longer
appears as a top frame — the SIMD batch from STEP 8 swallowed the
per-window encode into the surrounding `scan_line` loop. `scan_line`
itself plus its inline children dominate Phase 14: the remaining
work is the `WINDOW_SCORE` lookup, the score `* factor` multiply,
and the per-position `extension_factor` call. The `is_set` chain
under `classify` is the AVX2-batched windows feeding the extension
check.

**Optimization candidates (Phase 15):**
- **Inline / precompute `extension_factor`** — the eval flamegraph
  shows `extension_factor;classify;is_set` repeatedly attached to
  `scan_line`. Each window's score-base is computed by SIMD but the
  scaling factor still does two single-bit probes per position.
  Either batch-extract the extension bits alongside the windows or
  pre-classify boundary state during `set` / `clear`.
- **AVX-512 16-wide → 32-wide** — the bench host (Zen 4) has
  AVX-512. Doubling the SIMD batch would halve the loop iterations
  and might lift `scan_line` further.

### #2 — `threats::compute_with_scratch;full_recompute`

| Stack tail | Samples |
|---|---:|
| `threats;compute_with_scratch;full_recompute;walk_linear_runs;run_endpoints;run_forward;get;indices` | 4.96 M |
| `threats;compute_with_scratch;full_recompute;walk_cross_axis;matches_pattern<2>;is_player;is_set;get;indices` | 4.89 M |
| `threats;compute_with_scratch;full_recompute;walk_cross_axis;matches_pattern<3>` | 4.86 M |
| `threats;compute_with_scratch;full_recompute;walk_linear_runs;run_endpoints;run_backward;get;indices` | 4.78 M |

walk_cross_axis is no longer the dominant tail thanks to STEP 3
(scratch buffer) + STEP 4 (`is_player`). The remaining work is
spread across `matches_pattern<N>` for each shape template. STEP 7
(incremental threats) was the planned cut here but didn't ship — see
the Phase 15 candidates section in SPEC_ROADMAP.

**Optimization candidates (Phase 15):**
- **Incremental threat recompute**: still the highest-value
  remaining structural change. Use the existing `center` / `prior`
  hints to limit `walk_cross_axis` to anchors within
  `THREAT_RECOMPUTE_RADIUS` of the place center, and carry a paired
  place / undo delta on `Board`.
- **Pack `matches_pattern` offsets into a SIMD shuffle**: the
  template offsets are small constant arrays; an AVX2 gather +
  AND-and-popcount could check all N cells in one pass.

### #3 — `for_each_in_range<board::add_proximity>` / `remove_proximity`

| Stack tail | Samples |
|---|---:|
| `for_each_in_range<board::add_proximity>` | 4.95 M |
| `for_each_in_range<board::remove_proximity>` | 4.86 M |

These were #5 in Phase 13. Phase 14 didn't touch the proximity
maintenance loop, so it's relatively a bigger share now that the
eval + threats paths have compressed.

**Optimization candidates (Phase 15):**
- The proximity ring uses `FxHashMap<Coord, u32>` for the count
  table and `FxHashSet<Coord>` for the candidate set. A flat
  `Coord -> u32` array (sparse but bounded by `MAX_PIECE_DISTANCE`
  ring) would mirror the Phase 13 axis-bitmap fix.

### #4 — `creates_s0;run_backward;get;indices` / `creates_s0;line`

| Stack tail | Samples |
|---|---:|
| `creates_s0;run_backward;get;indices;from` | 4.94 M |
| `creates_s0;line` | 4.92 M |
| `axis_run_through_empty;run_backward;get` | 4.83 M |

The ordering layer's `creates_s0` predicate walks an axis run
through the candidate cell to classify "S0-creating moves" for the
bucket. The work is small per call but called per ordered move.

**Optimization candidates (Phase 15):**
- Cache the per-axis "would-create-S0" classification when the
  move is generated rather than re-walking inside ordering.

### #5 — `pvs_node;threats;is_none;is_some`

| Stack tail | Samples |
|---|---:|
| `pvs_node;threats;is_none<ThreatSet>;is_some<ThreatSet>` | 4.87 M |

`pvs_node` accesses `board.threats(player)` which lazily fills the
cache; the `RefCell::borrow` + `Option::is_none` chain shows up as a
discrete frame because the borrow returns a `Ref`. Could be inlined
further or replaced with a direct field probe in the cache-hit case.

## Kernel frame discount

| Stack tail | Phase 12 | Phase 13 | Phase 14 |
|---|---:|---:|---:|
| `unmap_page_range` chain | 1.98 B | 41 M | ~30 M |
| `asm_exc_page_fault` chain | 1.74 B | 34 M | ~25 M |
| `do_anonymous_page` chain | 1.17 B | 26 M | ~20 M |

Down further from Phase 13 thanks to STEP 10 hoisting the per-iter
fixture rebuild out of the bench loop. Residual kernel activity is
criterion's own measurement state plus the `bench_tt` group's
`iter_batched_ref` setup, which still allocates a 4 MB TT per batch
— acceptable since `bench_tt` measures the TT itself.

## Phase 15 entry points

In rough leverage order, expecting the biggest remaining win first:

1. **Incremental threat recompute** (deferred from STEP 7). The
   `full_recompute` chain is still the #2 user-space cost. The
   delta requires (1) per-anchor cross-axis tracking and (2) a
   paired place/undo delta cache on `Board`. Strict 10k-position
   oracle test mandatory.
2. **`extension_factor` inlining / precompute** — the Layer-1 #1
   chain still spends ~9 M samples in `extension_factor;classify;
   is_set`. Either pull the boundary bits into the SIMD batch or
   pre-classify at `set`/`clear` time.
3. **Proximity-set flat array** — same playbook as Phase 13's
   axis-bitmap fix, applied to `Board::proximity_count` /
   `candidate_cells`. Expected to recover the ~10 M samples in
   `for_each_in_range<…proximity>`.
4. **AVX-512 32-wide encode_ternary** — Zen 4 has it; a 32-window
   batch halves the eval inner-loop iterations.
5. **PGO with a richer training workload** — Phase 14 STEP 9 ran a
   trivial training set and the result regressed marginally
   (within noise). A self-play training pass (multiple openings,
   varied middlegame depths) might yield a real win.
6. **Move-ordering refinements** — `creates_s0;run_backward` is
   non-trivially hot; caching per-move S0 classification at gen
   time would shrink it.

## How to refresh this report

```bash
cd hexo-engine && maturin develop --release --features tt_stats
make flamegraph
make bench BENCH_TIME_MS=1000
make bench-diff A=baseline B=<latest-isodate-sha>
# Re-rank the top sections above based on the new folded.txt.
```
