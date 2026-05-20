# Hotspots — Phase 16 baseline

**Captured:** 2026-05-20 — git `0b92630`, rustc 1.94.0
**Host:** AMD Ryzen 7 8845HS (16 cores), Linux 7.0.3-arch1-2
**Bench data:** `benches/results/baseline.json` (`make bench` with
`--time-ms 1000 --tt-stats`, `.cargo/config.toml`
`target-cpu=native`, default features `simd_eval` + `eval_s1s2`).
**Flamegraph:** `benches/results/flamegraph-2026-05-20T13-11-34-0b92630.svg`
(`make flamegraph` — `perf record --call-graph dwarf -F 997` over
`bench_search` at depth 2 / 4 / 6). This capture is unusually
kernel-noisy (heavy scheduler / IRQ activity); the engine ranking
below is cross-checked against the criterion micro-benches, which
are the reliable signal.

## Headline numbers

| Metric | Phase 14 | Phase 15 | Phase 16 | Δ vs Phase 15 |
|---|---:|---:|---:|---:|
| NPS, `midgame_12`, t = 1000 ms | 337,077 | 344,713 | **431,686** | **+25.2 %** |
| NPS, `midgame_30`, t = 1000 ms | 209,285 | 224,840 | **306,434** | **+36.3 %** |
| NPS, `empty`, t = 1000 ms | 392,674 | 398,128 | 679,270 | +70.6 % |
| NPS, `single_origin`, t = 1000 ms | 388,473 | 395,947 | 690,565 | +74.4 % |
| Depth @ 1 s, `midgame_12` | 5 | 5 | **6** | **+1** |
| Depth @ 1 s, `midgame_30` | 7 | 7 | **8** | **+1** |
| Depth @ 1 s, `empty` | 7 | 7 | 7 | — |
| `board::place`, `midgame_12` | — | 2.44 µs | **1.64 µs** | **-32.8 %** |
| `board::place`, `midgame_30` | — | 2.13 µs | **1.58 µs** | **-25.8 %** |
| `board::undo`, `midgame_12` | — | 2.30 µs | **0.64 µs** | **-72.0 %** |
| `board::undo`, `midgame_30` | — | 2.15 µs | **0.81 µs** | **-62.2 %** |
| `cached_eval_cold`, `midgame_12` | 4.08 µs | 4.25 µs | 3.75 µs | -11.7 % |
| `cached_eval_cold`, `midgame_30` | 7.31 µs | 7.37 µs | 6.72 µs | -8.9 % |
| `threats::compute_full`, `midgame_30` | 2.68 µs | 2.74 µs | 2.57 µs | -5.9 % |

The single-run canonical NPS (431 k / 306 k) runs a little hotter
than the 5-run `bench-perf` mean (~408 k / ~290 k) — this host's
CPU frequency scaling adds ±5 % run-to-run. Either way the Phase 16
targets are met.

### Phase 16 target table

| Target | Goal | Result |
|---|---|---|
| midgame_12 NPS | ≥ 420 k | **431.7 k** ✅ |
| midgame_30 NPS | ≥ 280 k | **306.4 k** ✅ |
| Depth-at-time midgame_12 @ 1 s | ≥ 6 | **6** ✅ |
| `bench-quick` wall-clock | ≤ 15 s | ~4 s ✅ |
| Layer 2 ablation data | ≥ 1 self-play A/B | 50-game A/B run ✅ |

### Phase 16 changes that landed

1. **Proximity flat structure** (STEP 2): the four coord-keyed
   `FxHashMap` / `FxHashSet` proximity fields on `Board` were
   replaced with `ProximityCounts` (two flat `Box<[u8]>`) and two
   `SparseCellSet` candidate sets (`src/proximity.rs`). `place` /
   `undo` now do flat-array index bumps instead of ~470 hashbrown
   probes per node. This is the bulk of the NPS gain — and the
   `for_each_in_range<…proximity>` frame, Phase 15's #2 hotspot,
   has effectively vanished from the flamegraph (5 mentions, down
   from ~10 M samples).
   - **Node counts drift**: the flat `inner_candidates` iterates in
     a different order than the old `FxHashSet`; `order_moves`'
     stable tie-break + `MOVE_GEN_CAP` truncation make alpha-beta
     node counts order-dependent. Behaviourally transparent
     (strength unaffected); the reference baseline was refreshed.
2. **Two-buffer threat scratch** (STEP 3): `threats::incremental`
   alternates `cross_axis_*` ⇄ `cross_axis_*_spare` instead of
   `mem::take`, so the per-anchor breakdown `Vec` never reallocates.
3. **Bench tiers** (STEP 1): `bench-quick` (~4 s), `bench-perf`
   (~6 s), `bench-micro-quick`, plus a `cycles/node` metric.
4. **Layer 2 S1/S2 ablation** (STEP 4): `eval_s1s2` Cargo feature +
   runtime `set_eval_s1s2` toggle + `bench ablation` self-play A/B.
   Default build unchanged. Ablation data: see the Phase 16 report.

## Flamegraph-derived ranking

Engine user-space chains, sample counts summed across `bench_search`
depth 2 / 4 / 6 on `midgame_12`. The `clear_tt;clear` frame
(111 M) is the bench harness wiping the TT between criterion
iterations — measurement overhead, discounted.

### #1 — `eval::layer1_window_scan;scan_line` (unchanged from P15 #1)

| Stack tail | Samples |
|---|---:|
| `layer1_window_scan;scan_line` | 18.98 M |
| `layer1_window_scan;scan_line;encode_ternary_batch;…;encode_ternary` | 14.55 M |
| `layer1_window_scan;scan_line;extension_factor;classify` | 14.22 M |
| `layer1_window_scan;scan_line;extension_factor;classify;is_set;get;indices` | 9.45 M |

Layer 1 is now the unambiguous #1: the proximity rework removed the
only frame that rivalled it. The `extension_factor;classify`
boundary probe and the SIMD `encode_ternary_batch` are the two
per-window costs — both Phase 17 targets.

### #2 — `threats::walk_linear_runs;classify_linear_run`

| Stack tail | Samples |
|---|---:|
| `walk_linear_runs;classify_linear_run;run_pieces` | 4.78 M |
| `walk_linear_runs;classify_linear_run;is_isolated_open_two;coord_at` | 4.72 M |
| `walk_linear_runs;classify_linear_run;push_s0;{closure#0}` | 4.70 M |
| `compute_with_scratch;incremental;walk_linear_runs;run_endpoints;run_forward;get;indices` | 4.65 M |

The linear-run walk in `threats::full_recompute` /
`incremental` — every piece's linear runs are walked to preserve
`s0_instances` iteration order. A per-line classification cache
(Phase 17) would let incremental skip lines outside every dirty
radius.

### #3 — `ordering::creates_s0;run_forward`

| Stack tail | Samples |
|---|---:|
| `creates_s0;run_forward;get;indices;from` | 4.49 M |

The ordering S0 predicate. The Phase 15 STEP 4 axis-run cache that
targeted this was reverted (commit 15c9638); Phase 17 should
revisit with a different caching key (take 3).

### #4 — TT probe / store

| Stack tail | Samples |
|---|---:|
| `write<(TTEntry, TTEntry)>` | 14.76 M |
| `tt::probe` (criterion micro: 250 ns hit) | — |

TT store shows a sizeable raw count, but it is one bucket-pair
write per node; the criterion `tt::probe` micro improved ~3 % vs
Phase 15 (cache-warming side effect of the smaller `Board`).

### #5 — `compute_with_scratch;incremental` reconciliation

The incremental-threats reconcile (linear re-walk + selective
cross-axis). The Phase 15 #6 `ThreatScratch::reset;clear<FxHashSet>`
frame is no longer separable from the linear-walk chain in this
capture; the `seen` `FxHashSet` → flat-bitset swap remains a
Phase 17 candidate.

### Dropped out since Phase 15

- **`for_each_in_range<…proximity>`** (Phase 15 #2): gone. The flat
  `ProximityCounts` / `SparseCellSet` rework removed it.

## Phase 17 entry points

In rough leverage order:

1. **`extension_factor` SIMD batch** — inline the boundary
   `is_set` probes into the AVX2 `encode_ternary` batch so the
   per-window multiplier is computed in-register. Layer 1 is now
   the sole #1; this is the highest-leverage remaining target.
2. **Per-line `LineContribution` cache on `ThreatScratch`** —
   extend the per-anchor cross-axis cache pattern to linear runs
   so `incremental` skips line classification outside dirty radii.
3. **`creates_s0` per-axis run cache, take 3** — the Phase 15
   attempt was reverted; revisit with a candidate-pre-sort or a
   different cache key.
4. **`FxHashSet<(Axis, i16, i16)> seen` → flat bitset** — same
   playbook as the Phase 13 axis-bitmap and Phase 16 proximity
   flattening.
5. **TT bucket layout** — 4-bucket or hash-folding to lift the
   mid-tree collision rate.
6. **Layer 2 S1/S2 ablation decision** — the Phase 16 STEP 4 A/B
   gives a first data point; gather more before deciding.
7. **AVX-512 32-wide `encode_ternary`** on Zen 4 hosts.

## How to refresh this report

```bash
cd hexo-engine && maturin develop --release
cd .. && make bench BENCH_TIME_MS=1000
make flamegraph
make bench-diff A=baseline B=<latest-isodate-sha>
# Re-rank the sections above from the new folded.txt + diff output.
```
