# Hotspots — Phase 13 baseline

**Captured:** 2026-05-19 — git `6d33f98`, rustc 1.94.0
**Host:** AMD Ryzen 7 8845HS (16 cores), Linux 7.0.3-arch1-2
**Bench data:** `benches/results/baseline.json` (`make bench` with
`--features tt_stats`, `--time-ms 1000`).
**Flamegraph:** `benches/results/flamegraph-2026-05-19T15-38-21-8b9187f.svg`
(captured via `make flamegraph` — `perf record --call-graph dwarf
-F 997` over `bench_search` criterion runs at depth 2 / 4 / 6, with
the Phase 13 STEP-1 bench-harness fix in place so the TT is allocated
once and reused via `Engine::reset` + `clear_tt`).

## Headline numbers

| Metric | Phase 12 baseline | Phase 13 | Δ |
|---|---|---|---|
| NPS, `midgame_12`, t=1000 ms | 234,933 | **237,449** | +1.1 % |
| NPS, `midgame_30`, t=1000 ms | 126,480 | **128,308** | +1.4 % |
| NPS, `empty`, t=1000 ms | 287,321 | 307,200 | +6.9 % |
| NPS, `single_origin`, t=1000 ms | 286,720 | 307,200 | +7.1 % |
| Depth reached, `midgame_12` @ 1 s | 5 | 5 | — |
| Depth reached, `midgame_30` @ 1 s | 6 | 6 | — |
| `cached_eval_cold`, `midgame_30` | 8.4 µs | 8.7 µs | +3 % |
| `cached_eval_warm`, all fixtures | 0.06 µs | 0.06 µs | — |
| Threat latency cold, `midgame_12` | 3.26 µs | 2.96 µs | −9 % |
| Threat latency cold, `midgame_30` | 6.64 µs | 6.34 µs | −4 % |
| Threat latency warm, all fixtures | 0.06 µs | 0.06 µs | — |
| TT hit rate, `midgame_12` depth=6 | 15.08 % | 15.57 % | +0.5 pt |
| TT hit rate, `midgame_30` depth=6 | 23.30 % | 23.30 % | — |
| Board::place, `midgame_12` | 4.21 µs | **1.45 µs** | −66 % |
| Board::undo, `midgame_12` | 2.91 µs | **1.39 µs** | −52 % |
| eval::layer1_window_scan, `midgame_12` | 1.57 µs | 1.36 µs | −14 % |
| eval::layer1_window_scan, `midgame_30` | 3.10 µs | 2.80 µs | −10 % |

The macro NPS lift (+1.1 % midgame_12, +1.4 % midgame_30) is smaller
than the 20-40 % Phase 13 prompt projected. The bigger story is in
the per-operation costs (board::place/undo cut by ~half) and in the
flamegraph: the hashbrown probes that dominated Phase 12 are gone.
NPS under-delivers because `piece_at` lookups in the threats hot
path went from 1 hashbrown probe to 2 axis-bitmap probes; refactoring
those callsites to use `AxisBitmaps::is_set` directly is a follow-up
(out of Phase 13 scope per prompt).

## Flamegraph-derived ranking (authoritative)

Sampled stacks aggregated by user-space leaf and stack tail (kernel
frames stripped — see "Kernel frame discount" below). Sample counts
are totals across all `bench_search` configurations (depth 2 / 4 / 6
on `midgame_12`).

### #1 — Layer 1 eval `encode_ternary`

| Stack tail | Samples |
|---|---:|
| `eval;layer1_window_scan;scan_line;encode_ternary` | 251 M |
| `eval;layer1_window_scan;scan_line` (without encode children) | 124 M |
| `eval;layer1_window_scan;scan_line;encode_ternary;mul;mul` | ~75 M (in tail) |

Layer 1 of the eval walks every populated `(axis, line)` pair via
`populated_range` + `window6` + `encode_ternary`. With Phase 13's
flat-array AxisBitmaps the per-line probe is cheap (no hashbrown), so
the per-window ternary encode now dominates. The 6-cell window encode
is purely arithmetic — POW3 multiplications + bit extracts — and is
the prime SIMD target.

**Optimization candidates:**
- **SIMD vectorize `encode_ternary`** — Phase 15 target. The 6 lanes
  can be encoded in parallel via PSHUFB / PMOVMSKB tricks on x86 or
  TBL / SHRN on ARM.
- Cache the encoded ternary index per `(axis, line, offset)` and
  invalidate only on cells touched by `place`. Trickier because
  windows overlap.

### #2 — Threats `full_recompute` walk_cross_axis

| Stack tail | Samples |
|---|---:|
| `threats;compute;full_recompute;walk_cross_axis` | 115 M |
| `threats;compute;matches_pattern<4>` | (folded into walk_cross_axis chain) |

Threat detection rescans every cross-axis cluster on every
`compute()` call. The `compute()` API accepts `place_center` / `prior`
hints but currently ignores them (Phase 4 ships full recompute).

**Optimization candidates:**
- **Incremental threat recompute** — Phase 14 target. `ThreatSet`
  already takes `place_center` / `prior` args. Drop instances whose
  cluster overlaps the dirty radius and rescan only that
  neighbourhood; merge with the prior set.
- `matches_pattern<N>::piece_at` now does 2 axis-bitmap probes per
  call (was 1 hashbrown probe in Phase 12). Adding a dedicated
  `AxisBitmaps::is_player(c, p)` and threading it through threats
  would halve those probes in matches_pattern.

### #3 — LineBitmap `window6` internal cost (no more hashbrown)

| Stack tail | Samples |
|---|---:|
| `window6;get;indices` (LineBitmap internal) | 105 M |
| `window6` | 93 M |
| `window6;get` | 77 M |
| `quiescence_node;cached_eval;window6` | 76 M |

This is **inside** `LineBitmap` — `indices(pos)` computes `(word_index,
bit_offset)` and `get(pos)` extracts the bit. The Phase 12 chain
`window6;get;indices` ran through hashbrown's `find_inner` / `probe_seq`
(~877 M samples). Phase 13's flat array eliminates the hashbrown
chain entirely, leaving only the cheap arithmetic on the LineBitmap
itself.

**Optimization candidates:**
- The arithmetic is already minimal (4 ops + branch on word
  boundary). Hard to improve without changing the bitmap layout.
- A future SIMD `window6_batched` could compute multiple consecutive
  windows in one pass.

### #4 — `extension_factor` + classify + is_set

| Stack tail | Samples |
|---|---:|
| `eval;layer1_window_scan;scan_line;extension_factor;classify;is_set` | 82 M |

`extension_factor` checks the two cells immediately outside a 6-window
to scale the base window score. Each check calls `AxisBitmaps::is_set`
(now a flat-array probe — was a hashbrown probe in Phase 12).

**Optimization candidates:**
- Inline the extension check into `scan_line` to avoid the function
  call overhead.
- Pre-compute the extension classifications during `set` / `clear`
  rather than at every eval. Risky — eval is read-only, classification
  would need invalidation alongside the threat cache.

### #5 — Board place/undo proximity loop (no longer hot)

The Phase 12 #5 was `Board::pieces` HashMap iteration / `piece_at`
hashbrown probes (~250 M samples combined). Phase 13 STEP 3 removed
the HashMap entirely:
- `piece_at` → axis-bitmap `player_at` (short-circuit via unified
  occupancy bitmap on empty cells)
- `pieces()` iterator → `history` + `history_players` parallel-Vec
  walk
- `add_proximity` → axis-bitmap `is_occupied` (single per-axis probe)

The new flamegraph shows `for_each_in_range<…remove_proximity>` /
`for_each_in_range<…add_proximity>` chains at ~50-60 M samples
combined (down from 700 M+ when the inner occupancy check went
through hashbrown). Board::place and ::undo benches reflect the
result: cut by ~50-65 % across all fixtures.

## Kernel frame discount

| Stack tail | Samples Phase 12 | Samples Phase 13 |
|---|---:|---:|
| `unmap_page_range` chain | 1.98 B | 41 M |
| `asm_exc_page_fault` chain | 1.74 B | 34 M |
| `do_anonymous_page` chain | 1.17 B | 26 M |

The huge kernel-memory pressure that dominated the Phase 12 raw
flamegraph traced entirely to the bench harness allocating a fresh
64 MB TT per criterion iteration. Phase 13 STEP 1 amortizes the
TT across iterations via `Engine::reset` + `clear_tt`, dropping the
kernel pressure by ~50× and surfacing the real search costs above.

The residual kernel frames come from criterion's own measurement
state (per-iteration time samples, plot data) and a few short-lived
allocations inside `threats::compute` (FxHashSet `seen` dedup in
`walk_linear_runs`). Those are noise relative to user-space work.

## Phase 14 entry point

**Incremental threat recompute** is now the highest-leverage
remaining structural change. `threats::compute` is the #2 user-space
cost (~115 M samples), and the API already accepts the `center` /
`prior` hints — we just ignore them. Limiting the rescan to the
dirty radius around the most recent placement should cut this 3-5×
on midgame and endgame positions where most of the board is stable
between plies.

Secondary follow-ups (post-Phase-14):

1. **`encode_ternary` SIMD** (Phase 15) — single biggest user-space
   leaf at 251 M samples. The 6-lane encode is the perfect SIMD
   shape (PSHUFB + PMOVMSKB on x86; TBL + SHRN on ARM).
2. **`piece_at` 2-probe → 1-probe in threats** — add
   `AxisBitmaps::is_player(c, p)` and refactor
   `matches_pattern<N>::piece_at` to use it. Recovers the small
   per-call regression that Phase 13 introduced in the threats
   path.
3. **TT bucket layout** (4-bucket, hash-folding) — at 15-25 % hit
   rate the TT pays for itself; tighter packing could squeeze
   another 1-2 % NPS.

## How to refresh this report

```bash
# 1. Build with stats enabled (so reference table has hit-rate)
cd hexo-engine && maturin develop --release --features tt_stats

# 2. Capture flamegraph SVG + folded stacks
make flamegraph

# 3. Run the full bench sweep
make bench BENCH_TIME_MS=1000

# 4. Diff against current baseline
make bench-diff A=baseline B=<latest-isodate-sha>

# 5. Re-rank the top sections above based on the new folded.txt.
```
