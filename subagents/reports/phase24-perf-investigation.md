# Phase 24 — Performance Investigation (HOTSPOTS refresh)

Read-only / measurement-only phase. No engine code changed. Scoping
input for Phase 25 (the next optimization phase).

- **Captured:** 2026-05-21
- **Engine HEAD at baseline:** `44493f6` (`engine: drop stray blank line
  from search split`) — the bench/flamegraph were captured against this
  tree; later Phase-24 commits touch only `flamegraph.sh`, `mem_layout.rs`,
  `baseline.json`, `.gitignore` and reports, never engine source.
- **Host:** AMD Ryzen 7 8845HS (Zen 4, 8C/16T, 16 MB L3), Linux 7.0.3,
  rustc 1.94.0, `target-cpu=native`, default features (`simd_eval`).
- **Bench data:** `benches/results/baseline.json` (`make bench`,
  `--time-ms 1000 --tt-stats`). Full sweep wall-clock **33 m 24 s**.
- **Flamegraph:** `benches/results/flamegraph-2026-05-21T21-44-40-69e2053.svg`
  + `.folded.txt` (local). Captured via the frame-pointer method now in
  `scripts/flamegraph.sh` (see § B methodology note).
- **Phase 17 reference:** the previous `HOTSPOTS.md` + the prior
  `baseline.json` (git `a7adbc9`), which *was* the Phase 17 baseline.

---

## 0. Executive summary

The engine got **+23–28 % NPS across every fixture** since Phase 17 and
**zero search-behaviour drift** — `make bench reference` node counts are
**32/32 byte-identical** to the Phase 17 baseline. Every gain is pure
throughput, banked by Phase 20 (S1/S2 detection removal) and the
Phase 22/23 cleanup. `midgame_30` picked up a depth ply at 1 s (6 → 7).

The Phase 17 hotspot ranking is **stale and partly wrong** — it predicted
the threats path would "collapse" after Phase 20; it did the opposite of
collapse in *share* terms (it shrank ~40 % in absolute cost but is still
the #2 engine hotspot because everything else shrank too). The current
ranking, from a frame-pointer flamegraph cross-checked against the
criterion micro suite:

1. **`eval` / Layer-1 window scan** — ~31 % of engine self-time
2. **`threats::compute_with_scratch`** — ~21 %
3. **`ordering` predicates** (`would_make_six` + `creates_s0`) — ~20 %
4. **`for_each_in_range` / proximity** (board place/undo) — ~18 %
5. search recursion overhead (`pvs_node`/`quiescence_node` self) — ~6 %

`perf stat` says the engine is **compute-bound** (IPC 4.38, branch
mispredict 0.35 %, LLC miss 2.9 %) — Phase 25 should cut *work*, not
chase cache locality. The TT is **98 % empty with ~0.5–1 % collisions**,
so the long-standing "4-bucket TT" candidate is **dead** — it solves a
non-problem.

Top 3 Phase 25 candidates (full ranking § K):

1. **Shared run-scan cache + bit-parallel `LineBitmap` run scan** —
   `would_make_six`/`creates_s0`/`run_endpoints` all re-walk ±5-cell runs
   one `get()` at a time. ~+5–9 % NPS, medium difficulty, low risk.
2. **Per-line `LineContribution` cache for Layer 1** — Layer 1 re-scans
   every populated line on every leaf eval. ~+8–15 %, high difficulty.
3. **`threats::compute` micro-opts** — kill the full-history filter walk
   with per-player piece lists. ~+3–5 %, medium difficulty.

---

## A. Headline numbers vs Phase 17

NPS / depth are single-run `macro.nps` cells (the noisier source); the
`scaling` table (5-run median, § D) corroborates each. Micros are
criterion medians.

| Metric (fixture) | Phase 17 | Phase 24 | Δ |
|---|---:|---:|---:|
| NPS @ 1 s, empty | 671,364 | 828,710 | **+23.4 %** |
| NPS @ 1 s, single_origin | 679,270 | 836,890 | **+23.2 %** |
| NPS @ 1 s, midgame_12 | 433,813 | 532,480 | **+22.7 %** |
| NPS @ 1 s, midgame_30 | 314,297 | 401,569 | **+27.8 %** |
| NPS @ 1 s, endgame_60 | (degenerate) | (degenerate) | — |
| Depth @ 1 s, empty | 7 | 7 | — |
| Depth @ 1 s, single_origin | 6 | 6 | — |
| Depth @ 1 s, midgame_12 | 5 | 5 | — |
| Depth @ 1 s, midgame_30 | 6 | **7** | **+1** |
| Depth @ 1 s, endgame_60 | 64 | 64 | — (terminal) |
| `cached_eval_cold`, midgame_12 | 3.18 µs | 1.91 µs | **−39.9 %** |
| `cached_eval_cold`, midgame_30 | 5.40 µs | 2.97 µs | **−45.0 %** |
| `cached_eval_cold`, endgame_60 | 7.71 µs | 3.22 µs | **−58.2 %** |
| `threats::compute_full`, midgame_12 | 1.106 µs | 0.671 µs | **−39.3 %** |
| `threats::compute_full`, midgame_30 | 2.709 µs | 1.624 µs | **−40.1 %** |
| `threats::compute_full`, endgame_60 | 5.453 µs | 3.008 µs | **−44.8 %** |
| `board::place`, midgame_30 | 1.22 µs | 1.60 µs | +31 % (noise) |
| `board::undo`, midgame_30 | 0.508 µs | 0.660 µs | +30 % (noise) |
| `eval::layer1_window_scan`, midgame_12 | 450 ns | 435 ns | −3.3 % |
| `eval::layer1_window_scan`, midgame_30 | 849 ns | 810 ns | −4.6 % |
| TT hit rate, midgame_12 d=4 / d=6 | n/a | 26.7 % / 13.7 % | (new) |
| TT hit rate, midgame_30 d=4 / d=6 | n/a | 14.1 % / 11.4 % | (new) |

Row commentary:

- **NPS +23–28 %.** This is the Phase 20 S1/S2-detection removal
  (`~16–20 %` claimed) plus the Phase 22 dead-code subtraction, slightly
  exceeding the Phase 20 estimate. `midgame_30` benefited most (+27.8 %)
  — its denser board ran the now-deleted cross-axis matchers hardest.
- **Depth @ 1 s.** `midgame_30` 6→7 (the extra throughput crossed the
  d7 cliff); `midgame_12` is still pinned at d5 — a *branching-factor*
  cliff, not a perf cliff (see § D). The Phase 17 prompt's NPS/depth
  targets (550 k / depth-7) remain unmet, but they were always going to
  be: Phase 17 reshaped the tree for strength, not speed.
- **`cached_eval_cold` −40 to −58 %.** `cached_eval` is dominated by the
  two-player threat recompute; the S1/S2 cross-axis matchers were the
  bulk of that. Their removal (Phase 20) is the whole story.
- **`threats::compute_full` −39 to −45 %.** Same cause. This is the
  single biggest micro improvement and it is the Phase-20 dividend made
  visible. The Phase 17 prompt predicted "the threats path collapsed" —
  the *function* did get ~40 % cheaper, but it is still the #2 engine
  hotspot (§ B) because the rest of the engine got faster too.
- **`board::place`/`undo` "+31 %" is noise.** These two criterion groups
  have MAD ≈ the median itself (place/undo at midgame_30: MAD 844 ns on a
  1216 ns median in Phase 17). Treat every place/undo micro as
  ±30 % unreliable; the flamegraph (§ B) and `perf stat` are the real
  signal for the proximity path, and they say it is fine.
- **`layer1_window_scan` flat (−3 to −5 %).** Within noise. Layer 1 was
  not touched by Phases 18–23; the small move is LTO re-layout. Layer 1
  is still the #1 engine hotspot — it just did not change.
- **TT hit rate.** Phase 17's `baseline.json` has `tt_hit_rate: null`
  everywhere — the bench build carried no `tt_stats` Cargo feature, so
  `--tt-stats` recorded zeros. Phase 24 captured TT stats from a
  dedicated `--features tt_stats` build (§ E). No Phase-17 number exists
  to diff against.

---

## B. Top 10 user-space hotspots (flamegraph-derived)

### Methodology note — the flamegraph had to be rebuilt twice

`make flamegraph` as it stood was **silently broken**. `perf record
--call-graph dwarf` (8 KiB stack snapshot) cannot unwind the LTO'd
`pvs_node → pvs_dance → quiescence_node` recursion: the unwinder runs
out of captured stack and collapses *every* search sample into an
unattributable `[unknown]` / `[libc.so.6]` leaf — only the shallow
setup code (`Engine::new`, `clear_tt`) was ever attributed. Widening
the dump to 64 KiB did not help. The fix (committed): build
`bench_search` with `-C force-frame-pointers=yes` and record with
`--call-graph fp` — frame-pointer unwinding is depth-unlimited and
needs no `.eh_frame` CFI. `flamegraph.sh` now does this.

Even with frame pointers the *inferno-collapsed* folded stacks are
shallow (perf interrupts / vDSO break the chain), so **the authoritative
ranking below is `perf report --no-children` self-time**, not the folded
SVG. The SVG is committed as the visual artifact.

The capture profiles `bench_search --bench` (search_root at depth 2/4/6
on `midgame_12`). The process splits **~63 % engine search / ~37 %
criterion harness + analysis** (criterion's rayon-parallel KDE
statistics + the `iter_custom` measurement loop + libm `exp`). Engine
percentages below are quoted as **% of the whole capture**; multiply by
~1.6 for % of engine-only.

| # | Function (self-time) | % capture | ≈ % engine |
|---|---|---:|---:|
| 1 | `eval::eval` | 14.70 % | ~23 % |
| 2 | `threats::compute_with_scratch` | 12.91 % | ~21 % |
| 3 | `ordering::would_make_six` | 7.07 % | ~11 % |
| 4 | `coords::for_each_in_range` (proximity #1) | 6.75 % | ~11 % |
| 5 | `ordering::creates_s0` | 5.79 % | ~9 % |
| 6 | `axis_bitmap::LineBitmap::windows8_run` | 5.02 % | ~8 % |
| 7 | `coords::for_each_in_range` (proximity #2) | 4.80 % | ~8 % |
| 8 | `search::pvs_node` (self) | 1.96 % | ~3 % |
| 9 | `search::quiescence_node` (self) | 1.79 % | ~3 % |
| 10 | `moves::generate` | 0.63 % | ~1 % |

(criterion noise excluded: `rayon …bridge_producer_consumer::helper`
12.78 %, `criterion …iter_custom` 8.81 %, `exp` + unsymbolized libm
~13 %, `core::slice::sort` ~0.8 %. Kernel frames < 0.1 % — `paranoid=2`
hides kernel callchains, so kernel time is both genuinely small and
partly invisible; either way the user-space picture is clean.)

`for_each_in_range` appears twice — it is generic over the closure and
monomorphizes once for `add_proximity` and once for `remove_proximity`;
**combined it is 11.55 % of capture (~18 % of engine)** and is the
single most-split hotspot. `eval::eval` self-time *includes* the inlined
`layer1_window_scan_8cell`, `scan_line_8cell`, and (under
`target-cpu=native`) the AVX2 `encode_ternary_8_batch_avx2` — Layer 1 has
no separate symbol. **Grouped, Layer 1 + `windows8_run` ≈ 19.7 % of
capture (~31 % of engine) — still the #1 cost centre.**

### #1 — `eval::eval` (Layer 1 window scan)

| Stack tail | Self % |
|---|---:|
| `eval::eval` (incl. inlined `layer1_window_scan_8cell` → `scan_line_8cell` → `encode_ternary_8_batch[_avx2]` → `WINDOW_SCORE_8` gather) | 14.70 % |
| `axis_bitmap::LineBitmap::windows8_run` (not inlined) | 5.02 % |

**What it does:** Layer 1 of the static eval. For every populated
`(axis, line)` it slides an 8-cell window, packs each window's
X/O occupancy into a ternary index `0..6561`, and sums
`WINDOW_SCORE_8[idx]` (a build-time table with the open/closed extension
factor folded in). `windows8_run` extracts the raw 8-bit windows from
the packed `u64` line storage; `encode_ternary_8_batch_avx2` converts 16
windows/iteration into ternary indices.

**Why it's expensive:** it is run on **every leaf** and touches every
populated line every time — there is no per-line memoisation. Three
sub-costs: (a) per-line setup — `scan_line_8cell` builds a `SmallVec` of
`line_ids` with an **O(n²) linear-dedup** (`line_ids.contains`) and calls
`populated_range` (a full word-array walk) twice per line; (b)
`windows8_run` is called twice per line (X and O); (c) the
`WINDOW_SCORE_8` summation is a **scalar gather** — `for &idx in idx_buf
{ total += WINDOW_SCORE_8[idx] }` — a dependent-load chain that the AVX2
encode feeds but does not itself vectorize.

**Optimization candidates:**
- *Per-line `LineContribution` cache* (med-high impact, high difficulty)
  — cache each line's Layer-1 score on the board, invalidate only the
  3 lines a placed stone touches. Turns a full re-scan into a 3-line
  delta. Eval value identical → reference-node-count-safe. ~+8–15 %.
- *Bit-parallel window extraction* (med impact, med difficulty) — fuse
  the X and O `windows8_run` passes and the ternary encode so the line
  is read once. ~+2–4 %.
- *Drop the O(n²) `line_ids` dedup* (low impact, low difficulty) — the
  two per-axis `populated_ids` lists are already insertion-ordered;
  merge them with a small bitset keyed by `line_id`. ~+0.5–1 %.

**Phase 25 priority:** **TARGET** (the `LineContribution` cache is the
single biggest available swing; gate on reference parity).

### #2 — `threats::compute_with_scratch`

| Stack tail | Self % |
|---|---:|
| `threats::compute_with_scratch` (incl. inlined `walk_linear_runs`, `classify_linear_run`, `run_endpoints`) | 12.91 % |

**What it does:** rebuilds a player's `ThreatSet` (S0 shape counts + S0
instances with defense cells) by a single linear-run scan: collect the
player's stones, then for each stone × 3 axes find the maximal run,
classify it (open/closed 4/5), dedup repeats via an `FxHashSet`.

**Why it's expensive:** it is recomputed *from scratch for both players*
on **every** `Board::threats()` read whose dirty flag is set — and the
flag is set by every `place`/`undo`. In the search that means a full
two-player recompute at essentially every leaf (eval), at every
`halfmove==1` node (`collect_stone1_defense`), and inside the quiescence
frontier filter (`is_threat_move` → `blocks_opp_s0`). Two structural
inefficiencies: (a) `compute_with_scratch` opens with `for (c,p) in
board.pieces()` — a walk over the **entire** move history to filter one
player's stones (O(total stones), e.g. 60 for endgame); (b) the `seen`
`FxHashSet` does a hashbrown insert per `(piece, axis)` to dedup runs
that share a line.

**Optimization candidates:**
- *Per-player piece lists on `Board`* (med impact, med difficulty) —
  maintain `Vec<Coord>` per player in `place`/`undo`; `compute` skips the
  full-history filter. ~+2–4 %.
- *Cheaper run dedup* (low-med impact, med difficulty) — replace the
  `FxHashSet<(Axis,i16,i16)>` with a per-axis line-id bitset reset per
  call. ~+1–2 %.
- *Incremental recompute* (high impact, high risk) — recompute only the
  lines a placed stone touched. This is the Phase 15 incremental-threats
  idea that was reverted (`15c9638`) and whose machinery Phase 22
  removed; re-attempting is oracle-gated, complex. ~+8–12 % but the
  Phase 15 history is a real warning.

**Phase 25 priority:** **SECONDARY** (the piece-list + dedup micro-opts
are safe wins; full incremental is WAIT-class).

### #3 / #5 — `ordering::would_make_six` & `ordering::creates_s0`

| Stack tail | Self % |
|---|---:|
| `ordering::would_make_six` (→ `axis_run_through_empty` → `LineBitmap::run_backward`/`run_forward`) | 7.07 % |
| `ordering::creates_s0` (→ `run_backward`/`run_forward`/`get`/`indices`) | 5.79 % |

**What it does:** the move-ordering bucket predicates.
`would_make_six` virtual-places a stone and checks for a ≥6 run on any
axis; `creates_s0` checks for a 4/5 run with an open end. `bucket_value`
calls `would_make_six` **twice** per move (own side, then opponent) and
`creates_s0` once — for every one of ~24 candidate moves at every
interior node. `is_threat_move` (the quiescence frontier filter) calls
all three again.

**Why it's expensive:** each call walks `run_backward` + `run_forward`,
and each of those is a loop of up to 5 `LineBitmap::get()` calls — and
each `get()` re-derives the `(word_index, bit_offset)` from scratch
(`indices()`). So one predicate ≈ 3 axes × ~10 `get()` ≈ 30 bit probes,
×3 predicates ×24 moves ≈ **~2,000 bit probes per interior node**, and
many candidate moves lie on the *same* axis line. Combined the two are
~20 % of engine self-time. The flamegraph's inclusive view puts
`quiescence_node` at ~47 % of the whole capture, and `is_threat_move`
(these same predicates) is its hot frontier — so this path pays double.

**Optimization candidates:**
- *Bit-parallel `run_backward`/`run_forward`* (med-high impact, low-med
  difficulty) — replace the 5×`get()` loop with one masked `u64` read +
  `trailing_ones`/`leading`. This speeds *every* caller —
  `would_make_six`, `creates_s0`, `run_endpoints` (threats), win
  detection. Byte-identical result. ~+3–6 %.
- *Per-`order_moves` line cache* (med impact, med difficulty) — cache the
  `(axis, line_id) → &LineBitmap` resolution so 24 moves on a handful of
  lines share the Option-probe. NB: this caches the *line lookup*, not
  the per-cell run walk — the Phase 15 "creates_s0 axis-run cache"
  reverted at `15c9638` conflated the two; the run walk is per-cell and
  cannot be cached, only made faster (previous bullet). ~+1–3 %.
- *Fold the double `would_make_six`* (low impact, low difficulty) — the
  own/opponent calls scan the same cell; one pass could test both player
  bitmaps. ~+0.5–1 %.

**Phase 25 priority:** **TARGET** (bit-parallel run scan is the
best impact×difficulty in the report — low risk, wide reach).

### #4 / #7 — `coords::for_each_in_range` (proximity maintenance)

| Stack tail | Self % |
|---|---:|
| `for_each_in_range` ← `add_proximity` closure | 6.75 % |
| `for_each_in_range` ← `remove_proximity` closure | 4.80 % |

**What it does:** `Board::place`/`undo` maintain two flat proximity
refcount fields — outer (r=8, legality) and inner (r=2, move-gen) — by
walking the hex neighbourhood of the placed/removed stone.
`for_each_in_range` generates each neighbour coord; the closure does
`prox_idx` (a multiply-add), a `saturating_add`/`-=` on the `u8` field,
and an occupancy probe.

**Why it's expensive:** the outer walk visits **~217 cells per place**
(`hex_area(8)`), `for_each_in_range` re-derives every coord with a
nested `dq/dr` loop whose inner bounds (`lo`/`hi`) are recomputed each
`dq` with branches. `place`/`undo`'s *own* self-time is negligible
(< 0.4 % each) — essentially all of place/undo is this neighbourhood
walk. `add_proximity`'s closure additionally probes `axes.is_occupied`
per cell.

**Optimization candidates:**
- *Precomputed offset tables* (low-med impact, low-med difficulty) —
  `coords.rs` already has `RANGE_OFFSETS` (a const slice of all r≤8
  offsets) but `for_each_in_range` does **not** use it; it recomputes the
  `lo/hi` loop. Iterate the flat table instead (and add an r=2 table for
  the inner field). Removes the per-`dq` branch math. ~+1–3 %.
- *Skip outer-proximity maintenance inside search* (med-high impact,
  high risk) — the r=8 outer field exists only for `is_legal`; every
  move the search tries already came from the r=2 inner candidate set
  and is provably legal, so a search-internal `place` could skip the
  217-cell outer walk entirely. High risk: it changes the `place`
  contract; needs a separate `place_searched` path. Big potential win.
- *SIMD the refcount bump* (low impact, high difficulty) — gather/scatter
  on the flat `u8` field; AMD gather is weak, likely not worth it.

**Phase 25 priority:** **SECONDARY** (offset tables are a safe quick
win; the skip-outer-maintenance idea is high-value but needs its own
careful phase).

### #6 — `axis_bitmap::LineBitmap::windows8_run`

Covered under #1 (Layer 1). 5.02 % self; the per-line 8-bit window
extractor. Called twice per line (X, O) per leaf eval. A
`LineContribution` cache (#1) removes most of its calls; fusing the X/O
passes removes the rest of the doubling.

**Phase 25 priority:** **SECONDARY** (subsumed by the #1 candidates).

### #8 / #9 — `search::pvs_node` & `search::quiescence_node` (self)

1.96 % + 1.79 % self-time — the recursion's *own* code (TT probe/store,
the move loop, alpha/beta bookkeeping, the PVS dance). Low self-time is
healthy: the search node is a thin orchestrator over eval/threats/
ordering. Note the inclusive view: `quiescence_node` is ~47 % of the
*whole* capture — quiescence is where the engine lives, so anything that
speeds the qsearch frontier (the §3 ordering predicates) compounds.

**Phase 25 priority:** **WAIT** (no per-node-overhead problem; TT
probe/store does not even appear in self-time — see § E).

### #10 — `moves::generate`

0.63 % — at the search radius (r=2) `generate` just clones the
maintained `inner_candidates` set; cheap. The r=4/r=8 sweep paths
(`sweep_neighbourhood`, an `FxHashSet` dedup) are benched but **the
search never calls them**.

**Phase 25 priority:** **WAIT** (not on the hot path).

---

## C. Per-module cycles breakdown

### The `bench breakdown` metric is broken — do not trust it

`make bench` emits a `breakdown` array. Phase 24's values:

| Module | midgame_12 d=4 | midgame_30 d=4 | endgame_60 |
|---|---:|---:|---|
| eval | 0.0004 % | 14.76 % | (not run) |
| threats | 0.0001 % | 4.77 % | (not run) |
| moves | 0.0008 % | 53.93 % | (not run) |
| ordering | 0.0004 % | 14.93 % | (not run) |
| tt | 0.00 % | 0.00 % | (not run) |
| search_other | 99.998 % | 11.61 % | (not run) |

This is **meaningless** and was meaningless in Phase 17 too. Root causes,
from reading `benchmark.py::bench_breakdown`:

1. It **sums raw criterion micro medians** with *no* call-count
   weighting (the SPEC claims "× call-counts" — the code does not).
2. `midgame_12`'s `search::search_root(depth=6)` micro is **1.26 s**;
   it maps to `search_other`, which therefore eats 99.998 %.
3. `midgame_30` has no `search_root` micro, so its `moves` bucket is
   dominated by `moves::generate(r=4)`/`(r=8)` (4–15 µs) — radii the
   search **never uses** (it runs r=2). 53.93 % `moves` is a fiction.
4. `tt` is always 0 %: the tt micro names are `hit/<fixture>` /
   `miss/<fixture>`, never bare `<fixture>`, so the fixture-name filter
   never matches them.
5. `[bench.breakdown]` only lists `midgame_12`/`midgame_30` — no
   `endgame_60` row exists.

**Flamegraph-derived per-module split (the real answer)**, % of engine
self-time (capture %, ÷0.63):

| Module | ≈ % engine |
|---|---:|
| eval (Layer 1 + orchestration) | ~31 % |
| threats | ~21 % |
| ordering (`would_make_six`/`creates_s0`/`order_moves`) | ~20 % |
| board / proximity (`for_each_in_range`, place/undo) | ~20 % |
| search recursion (`pvs_node`/`quiescence_node` self) | ~6 % |
| tt | < 0.5 % |
| moves | ~1 % |

vs Phase 17's ranking (Layer 1, proximity, ordering, threats, TT): the
*order* barely moved, but **TT fell off entirely** (Phase 17 #5 → < 0.5 %
now) and threats shrank in absolute terms (~−40 %) while holding its
rank. Phase 25 should not consult `bench breakdown`; fixing or deleting
it is a minor follow-up (§ J).

---

## D. ms-time scaling table

From `macro.scaling` (5-run median per cell, cold TT). Δ vs Phase 17.

| Fixture | t (ms) | depth | nodes | NPS | Δ NPS |
|---|---:|---:|---:|---:|---:|
| empty | 1 | 3 | 4,096 | 1,365,333 | +33 % |
| empty | 50 | 4 | 32,768 | 1,057,032 | +22 % |
| empty | 100 | 6 | 61,440 | 1,024,000 | +23 % |
| empty | 250 | 7 | 147,456 | 957,506 | +19 % |
| empty | 500 | 7 | 258,048 | 846,506 | +16 % |
| empty | 1000 | 7 | 495,616 | 824,652 | +22 % |
| single_origin | 50 | 3 | 32,768 | 1,057,032 | +18 % |
| single_origin | 100 | 5 | 61,440 | 1,007,213 | +19 % |
| single_origin | 500 | 6 | 253,952 | 840,900 | +22 % |
| single_origin | 1000 | 6 | 503,808 | 835,502 | +22 % |
| midgame_12 | 50 | 3 | 16,384 | 712,347 | +22 % |
| midgame_12 | 100 | 4 | 28,672 | 666,790 | +28 % |
| midgame_12 | 250 | 5 | 61,440 | 596,504 | +25 % |
| midgame_12 | 500 | 5 | 110,592 | 534,260 | +26 % |
| midgame_12 | 1000 | 5 | 217,088 | 538,679 | +27 % |
| midgame_30 | 50 | 5 | 11,750 | 489,583 | +32 % |
| midgame_30 | 100 | 5 | 20,480 | 417,959 | +25 % |
| midgame_30 | 250 | 5 | 40,960 | 409,600 | +24 % |
| midgame_30 | 500 | 6 | 81,920 | 403,546 | +24 % |
| midgame_30 | 1000 | 7 | 163,840 | 406,550 | +34 % |

(1 ms / 10 ms cells omitted — pure iterative-deepening startup noise.)

**Sub-second strength** — the user's stated 0.5 s target:

- **`midgame_12` depth-time curve:** 50 ms → d3, 100 ms → d4, 250 ms →
  d5, 500 ms → d5, 1 s → d5. Steep d2→d5 over 10–250 ms, then **flat at
  d5 from 250 ms to 1 s**. This is a *branching-factor cliff*, not a
  time-management bug: the reference table puts `midgame_12` d5 at
  46,980 nodes and d6 at 388,215 — an **8.3× tree-growth step** that 1 s
  cannot fund. Nothing in time management fixes this; only raw NPS (or
  better pruning at d5→d6) crosses it. At 500 ms `midgame_12` is solidly
  at d5 — the curve is *not* chaotic, just plateaued.
- **`midgame_30` depth-time curve:** 50 ms → d5, 100/250 ms → d5,
  500 ms → d6, 1 s → d7. A healthy, monotone ladder — d5 is reached by
  50 ms, then a clean ply roughly every doubling. The Phase-17→24
  throughput gain is exactly what moved the 1 s cell from d6 to d7.
- **Verdict:** the depth-time curves are *smooth and monotone*, not
  flat/chaotic — **no time-management / iterative-deepening tuning is
  warranted as a Phase 25 candidate**. `midgame_12`'s d5 plateau is a
  search-tree fact; the lever is NPS or pruning, both already on the
  candidate list.

---

## E. TT diagnostics

Captured from a dedicated `maturin develop --release --features
tt_stats` build (the production / `make bench` build carries no
`tt_stats` feature, which is why `baseline.json`'s `tt_hit_rate` is
`null` — a real finding: the `--tt-stats` flag the `bench`/`bench-baseline`
Makefile targets pass is a **no-op against a production build**).

**Hit rate by depth** (`bench reference --tt-stats`, fresh Engine per
`(fixture, depth)`):

| Fixture | d=2 | d=4 | d=6 |
|---|---:|---:|---:|
| empty | 12.5 % | 34.3 % | 19.5 % |
| single_origin | 0.7 % | 15.1 % | 13.6 % |
| midgame_12 | 2.7 % | 26.7 % | 13.7 % |
| midgame_30 | 4.6 % | 14.1 % | 11.4 % |

**Full-search snapshot** (one 1 s `bench_best_move`, fresh Engine):

| Fixture | depth | probes | hit rate | collision rate | stores | occupancy |
|---|---:|---:|---:|---:|---:|---:|
| midgame_12 | 5 | 82,743 | 12.7 % | 1.10 % | 20,647 | 18,658 / 1,048,576 = **1.8 %** |
| midgame_30 | 7 | 62,397 | 13.4 % | 0.54 % | 13,801 | 11,600 / 1,048,576 = **1.1 %** |
| empty | 7 | 235,091 | 9.6 % | 0.90 % | 31,167 | 22,249 / 1,048,576 = **2.1 %** |

- **Hit rate ~10–15 %** at realistic depths (d4 is inflated by
  iterative-deepening re-search overlap). Phase 17 did not capture it;
  the last on-record figure is Phase 14's ~16 % — but that was on the
  *pre-Phase-17 search tree* (Phase 17 reshaped it; node counts differ),
  so it is not directly comparable. Call it **flat-to-slightly-low and
  unchanged in nature.**
- **Collision rate 0.5–1.1 % of probes.** Negligible.
- **Occupancy 1–2 %.** The 64 MB / 1,048,576-bucket TT is **98 % empty**
  after a 1 s search.

**Phase 25 candidate — 4-bucket / hash-folding TT layout: DEAD.** That
candidate's premise is "lift the mid-tree collision rate." There is no
collision rate to lift — the table is near-empty and index collisions
are already < 1 %. The low hit rate is structural (HeXO's two-stone
parity yields few transpositions; a cold per-search TT) and is not a
bucket-layout problem. TT probe/store do not appear in flamegraph
self-time at all (§ B). **The TT is not a bottleneck and should not be
touched in Phase 25.** If anything it is *oversized* — a TT ≤ 16 MB
would fit the 8845HS L3 and make probes L3-hits instead of DRAM — but
`perf stat` (§ G) shows the engine is not memory-bound, so even that is
marginal.

---

## F. Memory layout audit

From `cargo run --release --example mem_layout` (the inspection example
added this phase). Stack `size_of` / `align_of`; heap derived from the
`hexo.toml` constants.

| Struct | Stack (B) | Align | Heap (B) | Notes |
|---|---:|---:|---:|---|
| `Coord` | 4 | 2 | — | `{q,r: i16}`, register-passed |
| `Player` / `Axis` / `TTFlag` / `ThreatKind` | 1 | 1 | — | `repr(u8)` enums |
| `Option<Coord>` | 6 | 2 | — | no niche — discriminant widens |
| `LineBitmap` | 64 | 64 | — | **exactly 1 cache line** (`repr(align(64))`, by design) |
| `Option<LineBitmap>` | 64 | 64 | — | SmallVec niche absorbs the discriminant — **no bloat** |
| `TTEntry` | 32 | 16 | — | `u128` hash forces align 16 |
| `(TTEntry, TTEntry)` | 64 | 16 | — | **TT bucket pair = exactly 1 cache line** (deliberate) |
| `ThreatCounts` | 4 | 1 | — | 4×`u8` |
| `ThreatInstance` | 72 | 8 | (SmallVec spill) | `kind` + 2 `SmallVec`s; inline caps 5+4 `Coord` |
| `ThreatSet` | 32 | 8 | `Vec<ThreatInstance>` | counts + `s0_instances` |
| `ThreatScratch` | 56 | 8 | hashset + vec | reused across calls |
| `KillerSlot` | 12 | 2 | — | `[Option<Coord>; 2]` |
| `SearchConfig` | 40 | 8 | — | 11 tunables, `Copy` |
| `SearchResult` | 32 | 8 | — | — |
| `AxisBitmaps` | 624 | 8 | **293,184** | 9 flat arrays × 509 × 64 B |
| `ProximityCounts` | 32 | 8 | **146,882** | 2 × `u8`[73,441] |
| `SparseCellSet` | 40 | 8 | **293,764** | `u32`[73,441] slot map + members `Vec`; **×2 in `Board`** |
| `ZobristTable` | 80 | 8 | **2,080,800** | `u128`[130,050] window (~2 MB) |
| `OrderingState` | 40 | 8 | 1,536 | killer table `[KillerSlot; 128]` |
| `TranspositionTable` | 32 | 8 | **67,108,864** | 1,048,576 bucket-pairs × 64 B = **64 MB** |
| `Board` | 1,040 | 16 | ~3.0 MB | sum of the above (axes 624 inline, etc.) |
| `Engine` | 1,152 | 16 | ~67 MB | `Board` + `TT` + `OrderingState` + `cfg` |

Commentary:

- **`LineBitmap` = 64 B and `(TTEntry,TTEntry)` = 64 B** — both exactly
  one cache line, both deliberate (`repr(align(64))` on `LineBitmap`;
  `TTEntry` sized so the pair lands on a line). The two most-probed
  structures are cache-line-perfect. No action.
- **`Option<LineBitmap>` is still 64 B** — `SmallVec` exposes a niche the
  `Option` discriminant reuses, so `AxisBitmaps`'s 9 × 509-slot arrays do
  *not* suffer the 2× padding blow-up one might fear. Good.
- **`Board` = 1,040 B stack, ~3 MB heap.** It is a single long-lived
  object, not array-packed, so its size is not itself a problem. The
  heap is dominated by the **2 MB `ZobristTable` window** and the
  ~734 KB of proximity/candidate flat fields (2× `SparseCellSet` slot +
  `ProximityCounts`). These are large but cold-ish — touched
  scatter-wise in `place`/`undo`. The hot field is `axes`
  (`AxisBitmaps`, 624 B inline + 286 KB heap).
- **`Engine` ≈ 67 MB resident**, ~99.5 % of it the 64 MB TT — see § E:
  the TT is 98 % empty in a 1 s search. Not a perf issue, but the
  match harness's "N × 128 MB" memory budget is dominated by a table
  that is barely used.
- **`AxisBitmaps` 286 KB heap.** The `occupied` axis-bitmap (3 of the 9
  arrays) is a Phase-13/17 addition for a single-probe `is_occupied`;
  the layout is fine. No layout pathology found anywhere — the Phase
  13/16 flat-array reworks did their job.

---

## G. Branch prediction + microarch counters

`perf stat` over `bench nps --fixture midgame_30 --time-ms 1000 --runs 5`
(production build; ~83 % event-multiplexing coverage):

| Counter | Value |
|---|---:|
| instructions | 45,499,507,786 |
| cycles | 10,387,419,536 |
| **IPC** | **4.38** |
| branches | 8,924,143,390 |
| branch-misses | 31,335,835 |
| **branch-misprediction rate** | **0.351 %** |
| cache-references (LLC) | 117,253,958 |
| cache-misses (LLC) | 3,380,443 |
| **LLC miss rate** | **2.88 %** |

**Interpretation: the engine is firmly compute-bound.**

- **IPC 4.38** is very high (Zen 4 peak is ~6). The core is well-fed —
  it is *not* stalling on memory or mispredicts. This is the dominant
  finding of § G.
- **Branch mispredict 0.35 %** — excellent. The search's branches (move
  loops, ±5 run scans, bucket dispatch) are highly predictable. No
  branch-hint / layout work is warranted.
- **LLC miss 2.88 %** — low. The ~3.4 M misses correlate closely with
  the ~2–3 M TT probes (random 64 MB access → guaranteed LLC miss), but
  the out-of-order core hides that latency behind other work — which is
  exactly why TT probe does not show as a flamegraph hotspot (§ B/E).

**Consequence for Phase 25:** the wins come from **doing less work**
(fewer instructions per node, fewer re-scans, fewer recomputes) — *not*
from cache-locality tricks or branch hints. Every § K candidate that
ranks well is a work-reducer (caching a recompute, bit-parallelising a
loop, skipping redundant maintenance). Memory-layout candidates are
explicitly *not* indicated by the data.

---

## H. Per-module micro-bench medians (full table)

Criterion medians, Phase 17 → Phase 24 (ns). `<<` marks a move outside
the ±8 % criterion noise band. Place/undo groups have MAD ≈ median —
treat as noise regardless of sign.

| Group | fixture | P17 ns | P24 ns | Δ |
|---|---|---:|---:|---:|
| axis_bitmap::populated_range | midgame_12 | 54.5 | 54.6 | +0.2 % |
| axis_bitmap::populated_range | midgame_30 | 92.5 | 93.9 | +1.6 % |
| axis_bitmap::populated_range | endgame_60 | 125.9 | 127.9 | +1.6 % |
| axis_bitmap::run_through | midgame_12 | 118.3 | 127.9 | +8.1 % << |
| axis_bitmap::run_through | midgame_30 | 131.6 | 131.6 | 0.0 % |
| axis_bitmap::run_through | endgame_60 | 141.2 | 139.0 | −1.5 % |
| axis_bitmap::set_clear | midgame_12 | 1355 | 1464 | +8 % (noise, MAD≫) |
| axis_bitmap::set_clear | midgame_30 | 1253 | 1014 | −19 % (noise) |
| board::place | midgame_12 | 1310 | 1115 | −14.9 % (noise) |
| board::place | midgame_30 | 1216 | 1596 | +31 % (noise) |
| board::place | endgame_60 | 1037 | 1159 | +11.7 % (noise) |
| board::undo | midgame_12 | 525 | 458 | −12.8 % (noise) |
| board::undo | midgame_30 | 508 | 660 | +29.8 % (noise) |
| board::place_undo_roundtrip | midgame_12 | 1568 | 1290 | −17.8 % (noise) |
| board::place_undo_roundtrip | midgame_30 | 1423 | 1697 | +19 % (noise) |
| eval::cached_eval_cold | midgame_12 | 3176 | 1908 | **−39.9 % <<** |
| eval::cached_eval_cold | midgame_30 | 5400 | 2969 | **−45.0 % <<** |
| eval::cached_eval_cold | endgame_60 | 7712 | 3223 | **−58.2 % <<** |
| eval::cached_eval_cold | open_3_x_axis | 2606 | 999 | −61.7 % << |
| eval::cached_eval_cold | rhombus | 3294 | 1534 | −53.4 % << |
| eval::cached_eval_cold | fork_two_open_4 | 4190 | 2481 | −40.8 % << |
| eval::cached_eval_warm | (all) | ~0.42 | ~0.39 | −5 % (cached read) |
| eval::layer1_window_scan | midgame_12 | 450 | 435 | −3.3 % |
| eval::layer1_window_scan | midgame_30 | 849 | 810 | −4.6 % |
| eval::layer1_window_scan | endgame_60 | 1368 | 1309 | −4.3 % |
| eval::layer2_shapes | (all) | ~1.03 | ~0.97 | −5 % |
| eval::layer3_fork_bonus | midgame_30 | 6.11 | 6.00 | −1.9 % |
| eval::layer3_fork_bonus | endgame_60 | 50.0 | 51.0 | +2.1 % |
| moves::generate(r=2) | midgame_12 | 21.4 | 20.7 | −3.7 % |
| moves::generate(r=2) | midgame_30 | 25.7 | 25.0 | −2.9 % |
| moves::generate(r=2) | endgame_60 | 20.6 | 18.8 | −8.7 % << |
| moves::generate(r=4) | midgame_30 | 4080 | 3860 | −5.4 % |
| moves::generate(r=8) | midgame_30 | 14,795 | 14,477 | −2.1 % |
| ordering::bucket_value | midgame_12 | 1584 | 1511 | −4.6 % |
| ordering::bucket_value | midgame_30 | 2337 | 2217 | −5.1 % |
| ordering::bucket_value | endgame_60 | 3316 | 3166 | −4.5 % |
| ordering::order_moves | midgame_12 | 2098 | 1994 | −5.0 % |
| ordering::order_moves | midgame_30 | 2976 | 2865 | −3.7 % |
| ordering::order_moves | endgame_60 | 4235 | 3978 | −6.1 % |
| search::search_root(d=2) | midgame_12 | 4.18 ms | 3.31 ms | −20.7 % << |
| search::search_root(d=4) | midgame_12 | 40.9 ms | 32.3 ms | −21.1 % << |
| search::search_root(d=6) | midgame_12 | 1.258 s | 0.893 s | **−29.0 % <<** |
| threats::compute_full | midgame_12 | 1106 | 671 | **−39.3 % <<** |
| threats::compute_full | midgame_30 | 2709 | 1624 | **−40.1 % <<** |
| threats::compute_full | endgame_60 | 5453 | 3008 | **−44.8 % <<** |
| threats::compute_full | open_3_x_axis | 407 | 227 | −44.2 % << |
| threats::compute_full | single_origin | 94.5 | 51.0 | −46.0 % << |
| threats::defense_cells_read | midgame_30 | 0.83 | 0.79 | −5.1 % |
| tt::probe hit | midgame_12 | 310 | 243 | −21.7 % << |
| tt::probe hit | midgame_30 | 308 | 240 | −22.1 % << |
| tt::probe miss | midgame_30 | 222 | 219 | −1.2 % |
| tt::store always_replace | midgame_12 | 5249 | 2432 | −53.7 % << |
| tt::store depth_preferred | midgame_30 | 5904 | 3057 | −48.2 % << |

Dropped since Phase 17 (Phase 22 deletions): `axis_bitmap::window6`
(9 fixtures), `threats::single_cell_blocks_all` (9 fixtures) — 218 → 200
micro entries.

Notes:
- The `eval::cached_eval_cold` and `threats::compute_full` rows are the
  real signal — a clean, reproducible ~40–58 % drop, the Phase 20
  dividend. `search::search_root(d=6)` −29 % is the same dividend at the
  end-to-end level (consistent with the +23–28 % macro NPS — the micro
  is a fixed-depth search, the macro a fixed-time search).
- `tt::store` "−50 %" and `tt::probe hit` "−22 %": `tt.rs` was **not
  touched** in Phases 18–23. The `tt::store` micro has MAD ≈ ½ median
  and prints criterion's "unable to complete 100 samples" warning — it
  is unreliable. The `tt::probe hit` move is consistent and low-MAD,
  but with `tt.rs` byte-identical it can only be **fat-LTO re-layout**:
  removing ~1,400 LOC (Phases 20+22) re-optimised the whole binary,
  shifting `tt::probe`'s code placement / inlining. Not a deliberate
  change, not a Phase 25 lever.
- `axis_bitmap::run_through` / `set_clear` small moves are within MAD.

---

## I. Cross-check vs the Phase 21 SRP investigation

`subagents/reports/phase21-investigation.md` audited every file for
single-responsibility violations. Cross-referencing its verdicts against
the current hotspot list:

| Hotspot (file) | Phase 21 verdict | Implication |
|---|---|---|
| `eval::eval` (`eval.rs`, 472 LOC) | KEEP — "one concern, clear layer banners" | Cohesive → **optimize in place**, no split needed |
| `threats::compute_with_scratch` (`threats.rs`, 304) | KEEP — "cohesive post-Phase-20" | **optimize in place** |
| `would_make_six`/`creates_s0` (`ordering.rs`, 348) | KEEP AS-IS — "a split here would be cosmetic" | **optimize in place** |
| `for_each_in_range` (`coords.rs`, 139) | KEEP | **optimize in place** |
| `windows8_run` (`axis_bitmap.rs`, 518) | BORDERLINE split, "KEEP AS-IS recommended" | **optimize in place** |
| `pvs_node`/`quiescence_node` (`search.rs`) | the Phase 23 split already happened (`search.rs`+`engine.rs`) | settled |

**Every current hotspot lives in a file Phase 21 marked cohesive /
KEEP.** There is no hotspot in a file Phase 21 flagged as
SRP-violating — `board.rs` and `search.rs` were the two split targets
and Phase 23 already split them; neither split file is now a hotspot
beyond the thin `pvs_node`/`quiescence_node` self-time. **Conclusion for
Phase 25: every optimization target is "fix in place" — no
split-first refactor is a prerequisite for any candidate.** This is the
clean outcome — the Phase 22/23 cleanup left the hot code in tidy,
appropriately-sized modules.

One Phase 21 naming finding is relevant: `threats.rs::coord_at` and
`ordering.rs::coord_on_axis` are the *same* `(axis,line_id,pos)→Coord`
reconstruction, duplicated. Both are on hot paths (run-scan candidates).
If Phase 25 touches either, fold them into one `axis_bitmap` helper.

---

## J. Open Phase 24-candidate list — status check

From `SPEC_ROADMAP.md § Phase 24 candidates`:

| Candidate | Status | Reason |
|---|---|---|
| **Eval tuning (S1/S2)** | **closed** | Already closed in the roadmap — Phase 18 DROP, Phase 20 removed the code. |
| **TT bucket layout (4-bucket / hash-folding)** | **SUPPLANTED — drop** | § E: collisions 0.5–1.1 %, occupancy 1–2 %, TT not in flamegraph self-time. It solves a non-problem. |
| **Move-ordering bucket refinement** | **partly relevant** | The *perf* angle is the run-scan cost (§ B #3) → folded into the § K run-cache candidate. Pure bucket-*quality* refinement is a strength change, not perf — deferred to a strength phase. |
| **`creates_s0` per-axis run cache (take 3)** | **RELEVANT — top candidate** | `would_make_six`+`creates_s0` ≈ 20 % of engine. Promoted to § K #1, broadened (bit-parallel run scan + line-lookup cache). The Phase 15 revert (`15c9638`) is noted: it cached the wrong thing (it tried to cache the per-cell run, which is not cacheable). |
| **Per-line `LineContribution` cache** | **RELEVANT — strong candidate** | Layer 1 (~31 % of engine) re-scans every line every leaf. § K #2. |
| **`[bot]` vs `[engine.search]` time-budget drift** | **relevant but not perf** | `[bot] default_time_per_move_ms` and `[engine.search] default_time_ms` are both 1000. A config-hygiene cleanup, ~0 perf impact. Not a Phase 25 target. |
| **`find_pv` eviction tolerance** | **not perf** | Robustness nicety; `find_pv` is off the search hot path. Not a Phase 25 perf target. |
| **Radius-theory colony discounting** | **out of scope** | An eval *feature*, on the `SPEC_ROADMAP` "out of scope for v1" list. Not perf. |

So of the seven, **one is a top Phase 25 target** (`creates_s0` cache,
broadened), **one is a strong target** (`LineContribution` cache), **one
is dead** (TT layout), and the rest are non-perf cleanups.

---

## K. Phase 25 candidate ranking

Ranked by impact × difficulty. NPS estimates are hedged ranges; all of
candidates 1–5 are **behaviour-transparent** (must hold `make bench
reference` node counts 32/32 byte-identical — that is the gate, not
`make vs`).

### 1. Bit-parallel `LineBitmap` run scan + shared line-lookup cache
- **Expected NPS impact:** +5–9 %
- **Difficulty:** medium — `run_backward`/`run_forward` become one
  masked `u64` read with `trailing_ones`/`leading_zeros`; the
  `order_moves` line-lookup cache is a small `OrderingContext` addition.
- **Risk:** low — byte-identical run lengths → identical predicates →
  identical move order → identical node counts. Reference-gated.
- **Dependencies:** none. Touches `axis_bitmap.rs` (run scan — also
  speeds `run_endpoints`/threats and win detection) + `ordering.rs`.
- **Quick-win:** yes — executable in a single focused prompt.
- **Tag:** pure perf.

### 2. Per-line `LineContribution` cache for Layer 1
- **Expected NPS impact:** +8–15 %
- **Difficulty:** high — add a per-`(axis,line_id)` cached Layer-1
  contribution on `Board`, invalidate the (≤3) lines a stone touches in
  `place`/`undo`. Lifecycle mirrors the existing `threats`/`eval` caches.
- **Risk:** medium — the summed eval value must stay bit-identical, so
  node counts stay identical; the risk is cache-invalidation bugs, which
  the reference table catches. The Phase 15 incremental-threats revert
  is the cautionary precedent for this *class* of change.
- **Dependencies:** none, but conceptually the bigger sibling of
  candidate 5; do one, learn, then the other.
- **Quick-win:** no — phase-level orchestration.
- **Tag:** pure perf (strength-neutral by construction).

### 3. `threats::compute` micro-opts (per-player piece lists)
- **Expected NPS impact:** +3–5 %
- **Difficulty:** medium — maintain `Vec<Coord>` per player on `Board`
  in `place`/`undo`; `compute_with_scratch` drops its full-history
  filter walk. Optionally swap the `seen` `FxHashSet` for a per-axis
  line-id bitset.
- **Risk:** low-medium — the `ThreatSet` output is unchanged; reference-
  gated.
- **Dependencies:** none.
- **Quick-win:** borderline.
- **Tag:** pure perf.

### 4. `for_each_in_range` precomputed offset tables
- **Expected NPS impact:** +2–4 %
- **Difficulty:** low-medium — iterate the existing `RANGE_OFFSETS`
  const slice (and a new r=2 table) instead of the `dq/dr` loop with
  per-row `lo/hi` branches.
- **Risk:** low — same cells visited, same order-insensitive updates.
- **Dependencies:** none.
- **Quick-win:** yes.
- **Tag:** pure perf.

### 5. Search-internal `place` that skips outer-proximity maintenance
- **Expected NPS impact:** +4–8 %
- **Difficulty:** high — moves the search tries are all r=2 inner
  candidates and provably legal, so the r=8 outer-field walk
  (~217 cells/place) is dead work *inside search*. Needs a separate
  `place_for_search`/`undo_for_search` path that maintains only what the
  search reads (axes, hash, inner candidates, threat-dirty).
- **Risk:** medium-high — splits the `place` contract; a mis-scoped skip
  corrupts legality. Reference-gated, but the blast radius is wider.
- **Dependencies:** none; independent of 1–4.
- **Quick-win:** no.
- **Tag:** pure perf.

### 6. Incremental threat recompute (revisit)
- **Expected NPS impact:** +8–12 %
- **Difficulty:** high; **risk:** high — this is the Phase 15 idea that
  was reverted (`15c9638`) and whose machinery Phase 22 deleted.
  Oracle-gated, fiddly, two-stone parity makes the dirty-set subtle.
- **Recommendation:** **WAIT** — do candidate 2 first; if per-line eval
  caching proves the invalidation pattern is maintainable, this becomes
  the natural follow-on. Do not open Phase 25 with it.
- **Tag:** pure perf.

### Not recommended for Phase 25
- **TT 4-bucket / hash-folding** — § E: dead, solves a non-problem.
- **Time-management / iterative-deepening tuning** — § D: the depth-time
  curves are smooth and monotone; nothing to fix.
- **Memory-layout work** — § G: the engine is compute-bound, not
  memory-bound; `LineBitmap` and the TT bucket are already cache-line
  perfect.

**Recommended Phase 25 shape:** lead with candidates **1, 3, 4** (the
low-risk work-reducers, ~+10–18 % combined, all reference-gated quick
wins), then **2** as the structural centrepiece (~+8–15 %). Hold 5 and 6
for a later phase.

---

## L. Strength-vs-throughput note

**Reference node counts are 32/32 byte-identical Phase 17 → Phase 24.**
Every number in this report is a *throughput* change — Phases 18–23
moved no search behaviour at all. The strength curve and the NPS curve
are genuinely separate axes here:

- **Phase 17** spent 1–2 ply of search depth to gain **+301 Elo** from
  the S1/S2 eval ablation — a strength win paid for in throughput.
- **Phase 20** handed back **+18–28 % NPS** by deleting the now-idle
  S1/S2 detection — a throughput win at zero strength cost.
- **Phase 24** confirms the Phase 20 dividend and finds the engine
  faster but behaviourally identical to Phase 17.

Phase 25 candidate tags (per the prompt's three classes):

- **Pure perf — no strength change expected** (reference-gated only):
  candidates 1, 2, 3, 4, 5, 6 in § K. All preserve node counts by
  construction; none needs a `make vs` gate, only `make bench
  reference` parity.
- **Perf + likely strength:** *none* of the § K top candidates. A
  *true* `creates_s0` predicate (it is currently an approximation — see
  `SPEC_ENGINE.md`) would change move ordering and thus node counts and
  thus strength — but that is a deliberate ordering-quality change, not
  on the § K list.
- **Strength + perf-neutral:** move-ordering *bucket-quality*
  refinement, LMR retune. These reshape the tree → change node counts →
  **must** be gated with `make vs` against `.bestref`. They are
  deferred to a strength-focused phase, not Phase 25.

The clean implication: **Phase 25 as scoped (candidates 1–4) is a pure
throughput phase** — its gate is reference-node-count parity, fast and
deterministic, no match harness needed. Only if Phase 25 expands into
ordering/LMR territory does `make vs` gating come into play.

---

## M. Strength smoke vs current `.bestref`

`make vs N_GAMES=20 TIME_MS=300 N_WORKERS=12` — current HEAD vs
`.bestref` (`70d86a03`, a pre-Phase-17 engine).

| Field | Value |
|---|---|
| Games | 20 |
| Result (current W-L-D) | **16 – 4 – 0** |
| Winrate | **80.0 %**, Wilson95 [58.4 %, 91.9 %] |
| Elo | **+240.8**, CI95 [+58.9, +422.7] |
| SPRT | llr +0.341, bounds [−2.944, +2.944] → continuing |
| `make vs` exit code | 2 (non-zero) |

**Verdict: healthy.** Current HEAD beats the locked best by a wide
margin — the Wilson lower bound (58.4 %) clears 50 %, so even
conservatively current is the stronger engine. This is expected:
`.bestref` (`70d86a03`) predates Phase 17, so current HEAD carries
Phase 17's +301-Elo S1/S2 ablation *plus* the Phase 18–23 throughput
gains (more depth at a fixed budget). No strength regression snuck in
across Phases 18–23 — consistent with the 32/32 byte-identical
reference node counts.

The non-zero `make vs` exit is **by design**: the SPRT verdict at N=20
is "continuing" (neither Wald bound reached — 20 games is far short of
SPRT resolution), and the dry-run promote harness exits non-zero on any
non-PROMOTE verdict. The match ran cleanly. `.bestref` was **not**
promoted — Phase 24 is an investigation phase.

---

## Appendix — environment limitations & deviations

- **`make flamegraph` was broken** (dwarf unwinder vs LTO'd recursion).
  Fixed this phase: switched to frame-pointer capture (commit
  `bench: capture flamegraph via frame pointers`). Two dead captures
  were discarded before the working one.
- **`perf` runs at `perf_event_paranoid = 2`** — kernel callchains are
  hidden, so the flamegraph's kernel-frame share (< 0.1 %) is partly an
  artefact of the restriction, not purely a real measurement. User-space
  attribution (the part that matters) is unaffected.
- **`perf stat` events multiplexed at ~83 %** — the six counters did not
  all fit simultaneously; values are scaled estimates, accurate to a few
  %.
- **TT stats** required a separate `--features tt_stats` build; the
  committed `baseline.json` (production build) carries `tt_hit_rate:
  null`. The `--tt-stats` flag in the `bench` Makefile targets is a
  no-op against a production build — minor tooling wart, noted in § E.
- **`bench breakdown` is structurally broken** (§ C) — reported as found,
  not fixed (Phase 24 is measurement-only). Fixing or deleting it is a
  minor follow-up.
- **`endgame_60`** is a 60-stone near-terminal fixture; its search
  resolves in 64 nodes "instantly", so its NPS cell is a meaningless
  ~6.4e10. It is informative only for the dense-board micro-benches
  (`cached_eval_cold`, `threats::compute_full`).
- The investigation report lives under `subagents/` (git-ignored by the
  Phase 18 hygiene rule); it was force-added per this phase's explicit
  STEP 3 commit instruction.
