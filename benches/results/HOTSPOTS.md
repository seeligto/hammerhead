# Hotspots — Phase 12 baseline

**Captured:** 2026-05-19 — git `ee1b14c`, rustc 1.94.0
**Host:** AMD Ryzen 7 8845HS (16 cores), Linux 7.0.3-arch1-2
**Bench data:** `benches/results/baseline.json` (`make bench` with
`--features tt_stats`, `--time-ms 1000`).
**Flamegraph:** `benches/results/flamegraph-2026-05-19T12-20-14-ee1b14c.svg`
(captured via `make flamegraph` — `perf record --call-graph dwarf
-F 997` over `bench_search` criterion runs at depth 2 / 4 / 6).

## Headline numbers

| Metric | Value | Target |
|---|---|---|
| NPS, `midgame_12`, t=1000 ms | **234,933** | >200k (met) |
| NPS, `midgame_30`, t=1000 ms | 126,480 | — |
| Depth reached, `midgame_12` @ 1 s | 5 | — |
| Depth reached, `midgame_30` @ 1 s | 6 | — |
| `cached_eval_cold`, `midgame_30` | 8.4 µs | <10 µs (met) |
| `cached_eval_warm`, all fixtures | 0.06 µs | <0.1 µs (met) |
| Threat latency cold, `midgame_30` | 6.64 µs | — |
| Threat latency warm, `midgame_30` | 0.06 µs | (cached read) |
| TT hit rate, `midgame_12` depth=6 | 15.08 % | — |
| TT hit rate, `midgame_30` depth=6 | 23.30 % | — |

## Flamegraph-derived ranking (authoritative)

Sampled stacks were aggregated by user-space leaf and stack tail
(kernel frames — page faults, munmap, IRQ — were stripped before
ranking; see "Kernel frame discount" below). Sample counts shown are
totals across all `bench_search` configurations (depth 2 / 4 / 6 on
`midgame_12`).

### #1 — TT allocation in benchmark setup *(bench-setup artifact, not production)*

| Stack tail | Samples |
|---|---:|
| `from_elem<(TTEntry,TTEntry)>;extend_with;write` | 14.2 B |
| `extend_with;add` | 2.5 B |

`bench_search` constructs a fresh `Engine` per criterion iteration,
which allocates a 64 MB TT each time (`vec![(TTEntry::EMPTY,
TTEntry::EMPTY); n_slots]`). Every allocation triggers
`do_anonymous_page` + `kernel_init_pages` on first touch, then
`unmap_region` on drop. This dominates the profile but **does not
reflect production search cost** — long-running searches allocate the
TT once.

**Action:** not a Phase 13 optimization target. Bench refactor: amortize
the TT across iterations (e.g. construct once in
`bench_function_setup`, call `engine.reset() + engine.clear_tt()`
between iters) so future flamegraphs surface real search work.

### #2 — Layer 1 eval (`encode_ternary` / `window6` / `layer1_window_scan`)

| Stack tail | Samples |
|---|---:|
| `eval;layer1_window_scan;scan_line;encode_ternary` | 719 M |
| `quiescence_node;cached_eval;window6` | 597 M |
| `cached_eval;window6;get;indices` | 522 M |
| `axis_bitmap::LineBitmap::get;find_inner;probe_seq` | 486 M |

Layer 1 of the eval (the 729-entry ternary lookup) walks every
populated `(axis, line)` pair via `populated_range` + `window6` +
`encode_ternary`. Most of the cost is in the inner per-window
ternary encode and the hashbrown probe (`indices` / `probe_seq` /
`find_inner`) used by `axis_bitmap::AxisBitmaps` to find the
`LineBitmap` for a given line index.

**Optimization candidates:**
- Replace the `HashMap<i16, LineBitmap>` in axis bitmaps with a flat
  array indexed by `line_id - line_id_offset`. Hashbrown probe is
  the single largest non-allocator user-space cost.
- Vectorize `encode_ternary` — the 6-cell window encode is purely
  arithmetic and ripe for SIMD.
- Cache the encoded ternary index per `(axis, line, offset)` and
  invalidate only on cells touched by `place`.

### #3 — Threats compute (`matches_pattern` / `walk_cross_axis`)

| Stack tail | Samples |
|---|---:|
| `threats;compute;matches_pattern<4>` | 139 M |
| `threats;compute;full_recompute;walk_cross_axis` | 119 M |
| `threats;compute;matches_pattern<4>;piece_at;get` | 52 M |
| `threats;compute;full_recompute;...;walk_cross_axis;matches_pattern<2>;piece_at;get` | 38 M |

Threat detection scans every axis line for length-2, length-4, and
length-6 patterns via `matches_pattern<N>`. The pattern matcher
fetches each cell via `Board::piece_at`, which dispatches through a
`HashMap<Coord, Player>`. The hashbrown hash + probe shows up
repeatedly under `piece_at`.

**Optimization candidates:**
- Incremental threat recompute — deferred from Phase 4.
  `ThreatSet` already takes `place_center` / `prior` args but ignores
  them. Use them.
- Drop the `Board::pieces` HashMap in favour of the axis bitmaps
  (which already encode "is there a piece here?" by axis). `piece_at`
  becomes a sub-`LineBitmap` set-bit check — no hash, no probe.

### #4 — `quiescence_node` threat / hop checks

| Stack tail | Samples |
|---|---:|
| `quiescence_node;is_threat_move;would_make_six` | 62 M |
| `quiescence_node;is_threat_move;would_make_six` (dup) | 43 M |

Quiescence enumerates threat-creating moves and re-checks via
`would_make_six` (line walk for terminal-position detection). Modest
cost compared to #2/#3, but on the critical path of every leaf.

**Optimization candidates:**
- `would_make_six` could share its line walk with the threat-set
  cache rather than running fresh.

### #5 — Board piece HashMap (`Board::pieces`, `Board::piece_at`)

Top single-call leaves:
- `hashbrown::find_inner;probe_seq` (via `piece_at`)
- `Board::pieces` iteration in `threats::full_recompute`
  (`from_iter` / `spec_extend` chain at 43 M samples)

`Board::pieces` returns `Iter<Coord, Player>` over a HashMap, which
gets collected into a Vec inside `full_recompute`. This is alloc +
hashing + iteration — all avoidable.

**Optimization candidates:**
- Replace `HashMap<Coord, Player>` with a `Vec<(Coord, Player)>`
  parallel to the move history (insertion-ordered, O(1) iteration).
  `piece_at` becomes an axis-bitmap query (already required for #2).

## Kernel frame discount

The raw flamegraph top-of-list is dominated by kernel-side memory
management:

| Stack tail | Samples | Why |
|---|---:|---|
| `unmap_page_range` / `unmap_region` | 1.98 B | TT teardown per iter |
| `asm_exc_page_fault` | 1.74 B | first-touch faults on TT pages |
| `__irqentry_text_end` | 1.20 B | timer/sched IRQs |
| `do_anonymous_page;vma_alloc_folio_noprof;...;prep_new_page;kernel_init_pages` | 1.17 B | zero-fill new TT pages |

These reflect **bench harness allocator pressure**, not search work.
The ranking above strips them so the user-space costs are visible.
The next flamegraph capture should refactor the bench to allocate the
TT once.

## Inferred ranking (superseded by flamegraph above)

Kept for record. The pre-flamegraph ranking was derived from
criterion micro-bench medians × estimated calls-per-search:

1. `moves::generate(r=8)` — 7–30 µs at midgame/endgame
2. `eval::cached_eval_cold` + `threats::compute`
3. TT hit rate (15–25 % at midgame)
4. `ordering::order_moves`
5. `board::place`/`undo` roundtrip

The flamegraph contradicts this in two ways:
- **Layer 1 eval** is the dominant production-relevant cost, more so
  than threat recompute. The criterion `cached_eval_cold` median
  hid that the cost is concentrated in `encode_ternary` + the
  hashbrown probe in `window6`, not in the threat call.
- **`moves::generate`** does not appear in the flamegraph's top
  user-space frames. The criterion micro-bench measures `generate`
  in isolation (at maximum r=8), but the real search uses r=2
  default and the cost is amortized over move-ordering.
- **TT hit rate** remains a real win, but the per-probe cost is
  small. The biggest TT win would come from removing the
  `HashMap<Coord, Player>` collisions that show up under
  `threats::compute` and `piece_at`.

## Phase 13 entry point

**Replace the hashbrown HashMaps in the hot path with flat arrays.**
Two maps account for a combined ~1 B samples of user-space work:

1. `axis_bitmap::AxisBitmaps`'s `HashMap<i16, LineBitmap>` (#2 above —
   ~500 M samples in `find_inner`/`probe_seq`/`indices`).
2. `Board::pieces` / `Board::piece_at`'s `HashMap<Coord, Player>` (#3
   and #5 — combined ~250 M samples).

Both have small, bounded key spaces:
- Axis line IDs are `i16` in `[-ZOBRIST_WINDOW, +ZOBRIST_WINDOW]` —
  bounded by `2 * ZOBRIST_WINDOW + 1 = 255` entries. A flat array is
  trivially correct.
- `Board::pieces` is at most `~400` entries on `endgame_60`. Iteration
  order matters in only one place (`full_recompute`'s `pieces()`
  collect), which is order-independent.

Expected NPS lift: 20–40 % at midgame, larger at endgame. Difficulty:
medium (mechanical refactor across `axis_bitmap.rs`, `board.rs`,
`threats.rs`, `eval.rs` — but well-bounded by tests).

Secondary: vectorize `encode_ternary` (#2) and start incremental
threat recompute (#3). Either is good follow-up after the HashMap
work lands.

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
