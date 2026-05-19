# Phase 15 — threats cache callsite survey

## Board::threats() callsites

| File:line | Caller fn | Reads counts / s0_instances / both | Freshness expectation |
|---|---|---|---|
| eval.rs:61 | `eval()` | BOTH | Hot-path, every leaf eval |
| eval.rs:62 | `eval()` | BOTH | Hot-path, every leaf eval |
| eval.rs:91 | `is_mate_for()` | s0_instances (iterate) | Check mate condition, lazy call |
| eval.rs:108 | `bench_layer2_shapes()` | counts only | Benchmark fixture, non-hot |
| eval.rs:109 | `bench_layer2_shapes()` | counts only | Benchmark fixture, non-hot |
| eval.rs:117 | `bench_layer3_fork_bonus()` | s0_instances (iterate) | Benchmark fixture, non-hot |
| search.rs:726 | `collect_stone1_defense()` | s0_instances (find+iterate) | Stone-1 defense inference (hot) |
| ordering.rs:286 | `blocks_opp_s0()` | s0_instances (any check) | Move ordering, every node |
| bench_eval.rs:83 | `bench_layer2()` bench setup | — (black_box) | Setup only, not measured |
| bench_eval.rs:84 | `bench_layer2()` bench setup | — (black_box) | Setup only, not measured |

## ThreatSet read patterns

| Pattern | Count | Files |
|---|---|---|
| read counts only | 2 | eval.rs (lines 108-109: bench_layer2_shapes) |
| iterate s0_instances only | 3 | eval.rs:91 (is_mate_for), eval.rs:117 (bench_layer3_fork_bonus), ordering.rs:286 (blocks_opp_s0) |
| both counts + s0_instances | 2 | eval.rs:61-62 (eval function) |
| s0_instances with find order sensitivity | 1 | search.rs:726 (collect_stone1_defense: finds **first** matching instance) |

**Key observation**: `search.rs:726` (`collect_stone1_defense`) is the **only callsite where iteration order of `s0_instances` matters**. It uses `.find()` to locate the first instance whose `pieces` contains the just-placed move `m`. The first match is cloned for bucket-7 prioritization. No other callsite depends on order.

## RefCell::borrow on threats cache

| File:line | Borrow type (immutable / mutable) | Released before next mutate? |
|---|---|---|
| board.rs:453-467 | immutable via `Ref::map()` | YES: all callers drop ref immediately |
| board.rs:458 | immutable (`.borrow()` check) | YES: only checks if None |
| board.rs:463 | mutable (`.borrow_mut()`) | YES: during lazy init only, before Ref return |
| board.rs:473 | mutable (`.borrow_mut().take()`) | YES: in invalidate_threats, called only from place/undo |
| board.rs:474 | mutable (`.borrow_mut().take()`) | YES: in invalidate_threats, called only from place/undo |

**Safety analysis**: 
- `eval()` (lines 61-62): acquires two `Ref<ThreatSet>` but drops them by line 85 (function return). No mutations between acquire and drop.
- `is_mate_for()` (line 91): acquires one `Ref<ThreatSet>`, used inline at line 92, dropped before return.
- `collect_stone1_defense()` (line 726): acquires `Ref<ThreatSet>`, immediately iterates and drops (line 732 return).
- `blocks_opp_s0()` (line 286): acquires `Ref<ThreatSet>`, immediately uses `.iter().any()` and drops (line 291 return).

No callsite holds a `Ref<ThreatSet>` across a `place()` or `undo()` call. All borrows are short-lived and released before exiting the function.

## Place + threats-read interleaving

**Finding**: No interleaving detected. All callsites follow the pattern:
```rust
let threats = board.threats(player);  // acquire Ref
use_threats(&threats);                // read counts / s0_instances
// Ref implicitly dropped here on scope exit
// No place/undo between acquire and drop
```

The `eval()` function at lines 61–85 is the most complex: it acquires two refs (tx, to) at lines 61-62, reads from both via layer functions (lines 66-83), and returns without ever calling `place()/undo()`. Layer functions (`layer3_fork_bonus`, `tempo_score`) receive borrowed references, not the board itself.

## Dirty-center tracking

**Current implementation** (`board.rs` lines 92, 459, 475):
- `Cell<Option<Coord>>` single point marker
- Set to the center `Coord` of the mutation in `invalidate_threats(c)` (called from `place()` and `undo()`)
- Read in `threats()` at line 459 when lazy-loading the cache

**Implications for Phase 15 redesign**:
- Moving to `Cell<bool>` flag + `SmallVec<[Coord; 4]>` dirty-center list is **architecturally compatible**
- All invalidations happen in one place: `invalidate_threats()` (board.rs line 472)
- No caller depends on a specific value of the dirty marker; it's only read by the lazy-init path
- Compute API (`compute_with_scratch` at threats.rs:152) already accepts `center: Option<Coord>` and `prior: Option<&ThreatSet>` — Phase 8 incremental will use these, but current code ignores them (see line 159: `full_recompute`).

## Risk summary

### Patterns that depend on iteration order
- **Single callsite**: `search.rs:726` (`collect_stone1_defense`). Uses `.find()` to locate the **first** S0 instance matching a condition. If the order of `s0_instances` changes, the move selected for stone-1 defense may differ, affecting move ordering and search quality.
  - **Impact**: Moderate. Move ordering is advisory; reordering would not break correctness, only tuning.
  - **Mitigation**: Document the iteration-order dependency; ensure integration tests capture bench/selfplay regression.

### Patterns that hold borrow across mutations
- **None detected**. All callsites drop the `Ref<ThreatSet>` before returning, and no caller invokes `place()`/`undo()` while holding an active borrow.
- **Consequence**: Safe to introduce the new `Cell<bool>` flag; no RefCell panic risk.

### Recommended dirty-tracking strategy: **Single-bbox (current approach) → Multi-center SmallVec**

**Rationale**:
1. **Average dirty-center cluster**: HeXO moves are scattered across the board; rarely will two consecutive moves' dirty centers be within a small radius. `SmallVec<[Coord; 4]>` with inline capacity is ideal for amortizing allocations.
2. **Incremental benefit**: Phase 8 incremental compute will reduce threat set recomputation radius. Multiple dirty centers allow tracking the union of affected neighborhoods, minimizing false-positive rescan regions.
3. **No regression risk**: Phase 15 (this task) will still do full recompute; the `SmallVec` is a data-structure upgrade, not a compute-path change. Phase 16+ can leverage multi-center hints for incremental scans.
4. **Adoption path**: Single dirty-center (current) → multi-center SmallVec (Phase 15) → use centers for radius-based filter (Phase 8+).

**Specific recommendation**:
- Change `Cell<Option<Coord>>` → `Cell<SmallVec<[Coord; 4]>>` (empty vec = clean cache)
- In `invalidate_threats(center)`, **append** `center` to the vec instead of replacing
- In `threats()` lazy-load, pass `Some(&vec)` (or `Some(vec[0])` if incremental is not ready) to `compute_with_scratch`
- Update tests to ensure multi-center accumulation works correctly across sequences of place/undo

**Phase 8 incremental unlock**: Once full incremental compute is ready, use the SmallVec to filter which threat instances to keep (prune those > dirty-radius from every center) and which to rescan.

---

### Bench fixture compatibility
- `bench_eval.rs:83-84` and `eval.rs:108-109`, `eval.rs:117` are isolated benchmarks (bench crate only). Reordering or changing `s0_instances` will not invalidate their correctness, only their microsecond timings. Update fixtures and re-baseline after Phase 15.

### Test coverage required for Phase 15
1. **Iteration-order invariant**: Add a test asserting that `collect_stone1_defense()` always returns the same `defense_cells` before/after the reordering, if any happens.
2. **Multi-center accumulation**: Verify that two consecutive `place()` calls record both centers in the dirty list.
3. **Lazy-load correctness**: Ensure that whether passed a single `center` or multiple, `compute_with_scratch()` still produces the same ThreatSet (full recompute).
4. **Performance**: Baseline the new `SmallVec` allocation pattern against the old `Option<Coord>` in flamegraph.

