# Hammerhead — Implementation Roadmap

Save as `specs/SPEC_ROADMAP.md`.

## Status

| Phase | Module(s) | State |
|---|---|---|
| 1 | `coords`, `board`, `zobrist` | ✅ done |
| 2 | `axis_bitmap`, `win` | ✅ done |
| 3 | `moves` | ✅ done |
| 4 | `threats` | ✅ done |
| 5 | `eval` (3 layers + tempo) | ✅ done |
| 6 | `tt` + zobrist halfmove parity | ✅ done |
| 7 | `ordering` | ✅ done |
| 8 | `search` | ✅ done |
| 9 | `pybind` + Python `Bot` + CLI | ✅ done |
| 10 | benchmark suite | ✅ done |
| 11 | promotion harness (`vs` / `promote`) | ✅ done |
| 12 | stabilization & reference (warning sweep, reference node counts, TT stats, baseline) | ✅ done |
| 13 | kill hot HashMaps — `AxisBitmaps` flat array, `Board::pieces` removal, bench harness TT amortization | ✅ done |
| 14 | deep optimization sweep — release profile, target-cpu, allocator, piece_at refactor, inline sweep, LineBitmap micro-opts, incremental threats, SIMD encode_ternary, PGO, bench infra extensions | ✅ done |
| 15 | incremental threats + RefCell trim + creates_s0 axis-run cache | ✅ done |
| 16 | fast bench tiers + proximity flat structure + Layer 2 ablation infra | ✅ done |
| 17 | parallel match harness + S1/S2 ablation decision + Layer 1 8-cell window table (scalar + AVX2) | ✅ done |
| 18 | repo hygiene + S1/S2 eval-weight tuning sweep (verdict DROP) | ✅ done |
| 19 | clean SDK / `hammerhead` package | ✅ done |
| 20 | remove idle S1/S2 detection code | ✅ done |
| 21 | SRP audit + deletion-sweep investigation (read-only) | ✅ done |
| 22 | deletion sweep — vestigial incremental machinery, dead config emits, `window6`, `notation.py` | ✅ done |
| 23 | SRP splits — `search`/`engine`, `board` proximity helpers, `cli.py`, `promote.py` | ✅ done |
| 24 | performance investigation — read-only HOTSPOTS refresh | ✅ done |
| 25 | optimization quick wins + measurement cleanup | ✅ done |

Order is fixed. Each phase depends on the previous.

## Universal workflow (applies to every phase prompt)

- **Use `make` commands.** Wire `.venv` paths correctly. Avoid bare
  `pytest` / `cargo` except for targeted runs.
- **Previous-phase agent may still be committing.** Proceed.
- **Commits**: atomic, descriptive, < 72 chars subject, no
  `Co-Authored-By: Claude` or any Claude attribution.
- **End every phase with a review pass.** Independent reviewer checks
  for bugs, missed cases, bad practice, inefficiencies.
  - **Spec-vs-code discrepancies**: pick the more efficient / optimized
    side. Update the loser. Test the change is sound.
  - **Use `make` commands** to verify (uses `.venv`, correct paths).
  - Fix what the reviewer finds. Report changes in "When Done".

## Core decisions (locked)

### Two-stone turn → per-stone minimax

`Board::to_move()` returns the player whose stone goes next, handling
the double-stone case via the `(p-1)/2 % 2` parity rule. Search is
per-stone and uses **minimax form**, not negamax.

**Implementation note (post-Phase 8)**: a single `pvs_node` dispatches
on `board.to_move()` instead of separate `pvs_max` / `pvs_min` — one
function body, side-aware comparisons. Avoids code duplication.
Earlier prompts described two functions; the merged form is correct.

**Eval sign convention**: X-positive globally. Never side-relative.

### Engine API granularity

`Engine::best_move()` returns **one stone**. Python `Bot` calls it
twice per turn. TT entries from stone 1's search warm stone 2's
search.

### Zobrist halfmove parity (Phase 6 deliverable)

Reserved constants `Z_TURN_X`, `Z_HALFMOVE`. Locked interpretation:

- `Z_TURN_X` is XOR'd in **iff side == X**, regardless of halfmove.
- `Z_HALFMOVE` is XOR'd in **iff halfmove == 1**.

These 4 combinations of (side, halfmove) overlay produce 4 distinct
hash contributions. Earlier prompt drafts gated `Z_TURN_X` on
`halfmove == 0`, which collapsed `(X,1)` and `(O,1)`. Phase 6
corrected this; spec text in `SPEC_ENGINE.md` is now correct.

### Bitmap representation

Per-axis sparse line bitmaps shared by `win`, `threats`, `eval`. 3
axes (Q, R, S=q+r) × 2 players. Hex 60° rotation = axis permutation.

### TT (Phase 6 deliverable)

Two-bucket (depth-preferred + always-replace), generation-aged,
u128-verified. 64 MB default, power-of-two slot count. Mate scores
adjusted on store/probe via `score_to_tt` / `score_from_tt`.

### Search (Phase 8 deliverable)

- Iterative deepening + alpha-beta minimax + TT.
- PVS at depth ≥ 2, **3-step LMR dance** (reduced-null → full-null →
  full-window). Earlier 2-step description deprecated.
- Aspiration windows at depth ≥ 4.
- Threat-only quiescence (cap 8 plies).
- Check extensions (cap 4 per root path).
- **No null-move pruning** in v1 (two-stone parity fragile).
- Time split stone 1 / stone 2 = 60% / 40% of turn budget.

### MAX_PLY clarification

`MAX_PLY` (default 128) is the **total recursion ply ceiling**, not the
search target depth. Distinct from `max_depth` (default 64):

- `max_depth` — search-target depth from root.
- `MAX_PLY` — upper bound on ply counter used to size killer / PV
  arrays. Must cover `max_depth + max_check_extensions +
  qsearch_max_plies + slack`. For defaults: `64 + 4 + 8 + 52 = 128`.

Killer slots are a fixed-size array indexed by ply; the cap protects
against unbounded recursion via extensions / quiescence.

### Single source of truth — `hexo.toml`

All engine tuning lives in `hexo.toml`. Sections:

- `[engine.eval]` — weights (incl. 729-table source values)
- `[engine.search]` — alpha-beta params
- `[engine.board]` — board constants
- `[engine.threats]` — recompute radius, cluster radius
- `[engine.ordering]` — buckets, killer slots, history decay, MAX_PLY
- `[engine.tt]` — TT sizing
- `[bot]` — Bot defaults (mirrors search where intentional)
- `[bench]` — benchmark suite defaults (Phase 10)
- `[promote]` — promotion harness defaults (Phase 11)

Build metadata stays in `Cargo.toml` / `pyproject.toml`. **Magic
numbers in code = bug.**

### PyO3 0.28 specifics (Phase 9 deliverable)

- `Python::detach` (not `allow_threads`) for GIL release. Renamed in
  0.28.
- `#[pyclass(unsendable)]` on `Engine` — `Board` owns `RefCell` /
  `Cell` caches, making it `!Sync`. `Send` is still auto-derived,
  sufficient for `Python::detach`'s `Ungil: T: Send` bound.
- `pybind.rs` is **strictly type-conversion + GIL handling**. No
  game logic. Every method delegates to `RustEngine`.

## Performance targets

| Op | Target | Comment |
|---|---|---|
| `place` | < 500 ns | board + bitmap + zobrist |
| `undo` | < 500 ns | symmetric |
| `is_winning_move` | < 100 ns | bitmap line walk |
| `generate(r=2)` | < 5 μs | < 50 candidates typical |
| `cached_eval` cold | < 10 μs | 3-layer incremental cache |
| `cached_eval` warm | < 100 ns | cached |
| `search` NPS | > 200 k nps | release, single-thread |

Phase 8 measured ~150 k NPS on 12-piece midgame. Profile in Phase 10
(benchmark suite) to identify bottlenecks.

## Resolved open questions

1. **Quiescence depth cap**: 8 plies, S0+ move filter.
2. **LMR parameters**: depth ≥ 3, move index ≥ 6, `R = 1`. Disabled
   for TT-move / killer / S0 / S0-block.
3. **TT replacement**: two-bucket from v1.
4. **TT size**: 64 MB default, power-of-two.
5. **Aspiration windows**: initial delta 50, widen 2×, depth ≥ 4.
6. **Eval window weights**: existing `window_k_scores`
   `[0, 1, 8, 64, 512, 4096, 1_000_000]`.
7. **VCF/VCT**: inline as threat quiescence in v1.
8. **Stone-2 ordering**: "completes-stone-1-S0" bucket between
   defensive-win and creates-S0.
9. **Per-turn time split**: 60 / 40.

## Contradictions with research output (decisions retained)

- **Null-move pruning**: research advocates Stage 2 (+150-300 Elo).
  We skip in v1; two-stone parity fragile. Revisit post-baseline.
- **Eval 729-table only vs WSC layers**: 729 table is Layer 1;
  Layers 2/3 capture cross-axis shapes and forks that ternary
  windows cannot express.

## Spec-vs-code corrections applied during Phases 4–9

(Each was caught by the phase reviewer and applied; documented here for
audit.)

- **Phase 4**: OpenFour defense_cells = `{p-1, p+4}` (immediate empty
  neighbours), per the explicit contract paragraph. Earlier prompt
  wording "one beyond each empty neighbour" was contradictory and
  superseded. ClosedFour viability check added: 2-cells-beyond non-opp.
- **Phase 5**: "Two disjoint open-4s → fork mate" was incorrect under
  intersection-based vertex cover. Implementation correctly produces
  cover-2 (= `FORK_COVER2_BONUS`) for two open-4s; cover-≥3 mate
  requires three disjoint S0 instances. SPEC_EVAL updated.
- **Phase 5**: Layer 1 hot-path optimization — switched from
  piece-driven FxHashSet dedup to per-(axis, line_id) iteration via
  `populated_range`. Eliminated hash + alloc in hot path.
- **Phase 6**: `Z_TURN_X` interpretation — XOR'd iff side == X
  (regardless of halfmove). See "Core decisions" above.
- **Phase 7**: `creates_s0` uses virtual-place axis run, not
  `s0_instances` probe. More efficient. SPEC_ENGINE updated.
- **Phase 7**: `MAX_PLY` moved from `[engine.search]` to
  `[engine.ordering]` since it's primarily a killer-array dimension.
- **Phase 7**: `decay_history` retains only non-zero entries to prevent
  monotonic map growth.
- **Phase 8**: `pvs_max` / `pvs_min` merged into single `pvs_node`.
  3-step LMR (was 2-step). TT mate-score adjustment added.
  `force_parity_for_test` desync fixed via `prev_parity` helper.
- **Phase 8**: `best_move` primes `SearchResult.best_move` with first
  legal candidate before iterative deepening to prevent ORIGIN
  sentinel on tight-budget timeouts.
- **Phase 9**: `py.detach` (not `allow_threads`) — PyO3 0.28 rename.
  `#[pyclass(unsendable)]` for Board's `RefCell` / `Cell`. Engine
  owns `clear_tt` method (shim stays thin).

## Phase 15 — Incremental Threats + Companions

**Goal**: ship the deferred Phase 14 STEP 7 (incremental threat
recompute) under an oracle correctness gate, trim the residual
RefCell chain in `Board::threats`, and add a per-call axis-run cache
to ordering's `creates_s0` predicate.

Three concrete changes, in order of risk × leverage:

1. **Incremental threat recompute** (STEP 2 — biggest leverage,
   highest correctness risk):
   - `Board::threats_dirty_center: Cell<Option<Coord>>` →
     `Board::threats_dirty: Cell<bool>` + `threats_dirty_centers:
     RefCell<SmallVec<[Coord; MAX_INCREMENTAL_CENTERS]>>`.
   - `ThreatSet::compute_with_scratch` honours `centers` / `prior`:
     drops instances anchor-in-dirty-radius, rescans dirty lines and
     cross-axis cluster neighbourhoods, merges.
   - `ThreatInstance` gains an `anchor: Coord` field (linear shapes:
     `pieces[len/2]`; cross-axis: `pieces[0]`) for O(1) dirty-radius
     membership tests without iterating `pieces` per check.
   - Oracle test: 10k-position random walk with full-vs-incremental
     `threat_set_equiv` equality, plus focused tests for round-trip,
     SmallVec overflow fallback, winning-move correctness, and
     anchor determinism. If oracle catches drift after STEP 2 lands,
     **revert STEP 2 entirely**.

2. **RefCell::borrow chain trim** (STEP 3 — Phase 14 HOTSPOTS #5):
   the `Cell<bool>` fast path eliminates the `is_none` / `is_some`
   chain. Verify via flamegraph.

3. **`creates_s0` axis-run cache** (STEP 4 — Phase 14 HOTSPOTS #4):
   per-`order_moves` cache keyed by `(axis_id, line_id, player)` so
   multiple candidates on the same line share one bitmap probe.

**Reference node counts are the regression net.** Phase 15 is
behaviourally transparent; `make bench reference` must produce
identical node counts at every `(fixture, depth)` before and after.

See `prompts/PHASE_15_PROMPT.md`.

## Phase 16 — Fast Bench Loop + Proximity Rework + Layer 2 Ablation

**Goal**: cut the bench feedback loop, kill the proximity HashMaps
(Phase 15 HOTSPOTS #2), and add infrastructure to A/B test the
Layer 2 S1/S2 shape contributions.

Four sub-projects, ordered by independence:

1. **Fast bench tiers**: `bench-quick` (~5-15 s, single fixture),
   `bench-perf` (~30-60 s, two fixtures × two budgets), and a
   `cycles/node` metric. `bench` (full) is unchanged. See
   `specs/SPEC_BENCHMARKS.md § Bench tiers`.

2. **Proximity flat structure**: replace `FxHashMap<Coord, u32>`
   (×2) + `FxHashSet<Coord>` (×2) with bounded-key flat arrays
   (`ProximityCounts` = two `Box<[u8]>`) and `SparseCellSet`
   (bitset + insertion-order `Vec` + position index). Same playbook
   as the Phase 13 `AxisBitmaps` kill. See `SPEC_ENGINE.md
   § Candidate maintenance`.

3. **Phase 15 reviewer follow-ups**: `mem::take` realloc in the
   incremental-threats path (two-buffer swap), multi-cluster oracle
   coverage gap.

4. **Layer 2 ablation infrastructure**: Cargo feature `eval_s1s2`
   (default ON) + runtime `set_eval_s1s2` toggle + self-play A/B
   harness. **No removal** — data collection only; the keep/drop
   decision is Phase 17+. See `SPEC_EVAL.md § Layer 2 history`.

**Reference node counts are the regression net.** STEPs 1-3 are
behaviourally transparent; `make bench reference` must produce
identical node counts at every `(fixture, depth)`. STEP 4 with the
default `eval_s1s2` feature ON is also transparent.

See `prompts/PHASE_16_PROMPT.md`.

## Phase 16 resolved follow-ups

- **Proximity HashMap rework** (Phase 15 HOTSPOTS #2): resolved via
  the flat `ProximityCounts` (`Box<[u8]>` ×2) + `SparseCellSet`
  (bitset + `Vec` + `member_index`) structures replacing the four
  coord-keyed maps. See `SPEC_ENGINE.md § Candidate maintenance`.
- **Persist breakdown capacity across `incremental` calls** (Phase 15
  reviewer finding): resolved by two-buffer (current / prior)
  alternation of the threat scratch / cache, so `ThreatSet` capacity
  survives reconciliation without a `Vec::new()` realloc.
- **Multi-cluster oracle gap** (Phase 15 reviewer finding): the
  incremental-threats oracle now stress-tests 2-4 simultaneous dirty
  centers per reconciliation (`incremental_matches_full_multi_cluster`).

## Phase 17 — Parallel Harness + S1/S2 Decision + Layer 1 Extension

**Goal**: parallelize the match harness, settle the S1/S2 ablation
at scale, and replace the Layer-1 (6-cell × runtime extension
factor) scan with a single 8-cell ternary lookup whose table
pre-multiplies the factor.

1. **Parallel match harness**: `make vs` runs games across a
   process pool (N = cpu_count() - 2 by default). A 200-game match
   at 1 s/stone finishes in minutes, not hours. See
   `SPEC_BENCHMARKS.md § Parallel match harness`.

2. **S1/S2 ablation re-verified at scale** (200 games @ 500 ms,
   conditional 100 games @ 1 s) via the new harness. Decision
   matrix in the Phase 17 prompt; verdict committed.

3. **Layer 1 8-cell window table**: `WINDOW_SCORE_8: [i32; 6561]`
   codegen'd by `build.rs`, factor folded in. Eliminates the
   boundary `is_set` probes and the runtime multiply. Scalar +
   AVX2 encode paths, both gated by a 6561-entry identity test.

Resolved Phase 17 candidates:
- **`extension_factor` SIMD batch** → resolved by STEP 4/5: the
  factor is folded into `WINDOW_SCORE_8` at build time, so there is
  no runtime multiplier left to batch.
- **Layer 2 S1/S2 ablation decision** → resolved by STEP 2/3.

## Phase 18 — Repo Hygiene + Eval Tuning Sweep

**Goal**: untrack accumulated workflow artifacts, then settle whether
*corrected* S1/S2 shape weights can restore positional eval without
re-introducing the Phase-16 double-counting fault.

1. **Repo hygiene**: `subagents/`, stale phase notes, and generated
   bench dumps untracked and `.gitignore`'d. Specs + docs + source +
   `baseline.json` are the in-repo persistence surface; per-phase
   scans / reviews / reports live under the now-ignored `subagents/`.

2. **Runtime shape-weight override**: `Engine.set_eval_shape_weights`
   (`[i32; 8]`) overrides the Layer 2 S1/S2 weights at runtime,
   defaulting to the compile-time constants (reference node counts
   byte-identical). Lets the sweep vary weights per A/B cell with no
   rebuild.

3. **Tuning sweep** (`make tune`, `bench tune-sweep`): coordinate
   descent + local pairwise A/B. Stage A anchored each weight to its
   Layer 1 footprint (`weight = α × A_shape`). Stage B swept 8 shapes
   × 7 α (56 cells, 100 games each); Stage C re-tested the one cell
   past the gate at 200 games.

**Verdict: DROP.** No S1/S2 weight — isolated or combined — beats the
Phase 17 baseline at any swept α. `hexo.toml` weights stay 0; Phase
18 ships no weight or engine-behaviour change. A future phase removes
the S1/S2 detection code. Full sweep tables in
`SPEC_EVAL.md § Phase 18`.

## Phase 19 — Clean SDK / `hammerhead` Package

**Goal**: surface the engine through a clean, documented public Python
package — `from hammerhead import Bot` — for embedding in other
projects. Pure API / packaging / documentation work; zero engine
behaviour change (reference node counts byte-identical before/after).

1. **Public `hammerhead.Bot`** replaces the old thin engine wrapper. A
   single stateful class: `play` / `undo` / `reset`, read-only state
   properties (`to_move`, `ply`, `stone_in_turn`, `winner`, `history`,
   …), and non-mutating queries (`suggest`, `evaluate`,
   `principal_variation`). Moves are axial `(q, r)` tuples; sides are
   `"X"` / `"O"` strings — no engine enums or internal terms leak out.
2. **`HammerheadError` hierarchy** (`IllegalMoveError`, `GameOverError`,
   `NotationError`) replaces bare `ValueError` at the SDK boundary.
   `Move` / `Player` aliases + a `py.typed` marker ship inline types.
3. **Internals stay internal**: the `hammerhead_engine` PyO3 `Engine`,
   the CLI, and the subprocess protocol are marked internal in
   `SPEC_API.md`. The CLI / benchmark self-play loops now drive `Engine`
   directly — the old `Bot` / `BotConfig` wrapper is gone, one `Bot`.
4. **Docs**: `docs/sdk.md` full reference, a README quickstart section,
   `pdoc`-clean docstrings on the whole public surface.

Deferred (documented in `SPEC_API.md § Deferred surface`): string move
notation (BKE / BSN / HXN — needs `hammerhead.notation`), `threats()`
and `board_ascii` (need new PyO3 surface), `set_tt_size` (needs a
live-resize engine entry point). The `seed` constructor arg from the
original Phase 19 sketch was dropped — the engine is deterministic.

## Phase 20 — Remove Idle S1/S2 Detection Code

**Goal**: delete the S1/S2 shape detection confirmed idle by the
Phase 18 DROP verdict. Pure removal — zero search-behaviour change,
reference node counts byte-identical before/after.

Phase 17 zeroed the S1/S2 eval weights; Phase 18 swept corrected
weights and re-confirmed DROP. The detection still ran on every
`threats()` call and produced values multiplied by zero. Phase 20
deletes it:

- Cross-axis pattern matchers (triangle / arch / rhombus / bone /
  trapezoid), the eight S1/S2 `ThreatCounts` fields, and the
  axis-line classification arms that fed `open_3` / `closed_3` /
  `open_2`.
- The `layer2_shapes` S1/S2 term, the eight weight constants, the
  `eval_s1s2` Cargo feature, the `creates_s1` ordering predicate.
- The `set_eval_s1s2` / `set_eval_shape_weights` runtime overrides
  (PyO3 + Rust) and the `tune-sweep` / `ablation` bench tooling that
  drove them.

The cross-axis matchers were the sole beneficiary of the Phase 15
incremental threats reconcile path; with them gone that path
collapsed to a single linear-run scan. The dirty-center machinery on
`Board` (and the now-vestigial `centers` / `prior` parameters of
`threats::compute`) is left in place — a Phase 21 cleanup.

**Result**: reference node counts byte-identical (32/32 fixtures ×
depths); ~16–20 % NPS gain from eliminating the per-read detection
cost. See `SPEC_EVAL.md § Layer 2 history`.

## Phase 21 — SRP Audit + Deletion-Sweep Investigation

Read-only investigation. No code changed. Audited every source file
for single-responsibility violations and swept `src/` / `tests/` /
`benches/` / `hammerhead/` for dead and vestigial code. Output:
`subagents/reports/phase21-investigation.md` — the scoping input for
Phases 22–23.

## Phase 22 — Deletion Sweep

**Goal**: subtract the dead and vestigial code confirmed by the
Phase 21 investigation. Pure subtraction — zero search-behaviour
change, reference node counts byte-identical before/after.

Removed:

- **Vestigial incremental-threats machinery**: the `centers` /
  `prior` parameters of `threats::compute` / `compute_with_scratch`
  (Phase 14/15 introduced them for an incremental reconcile path;
  Phase 17 made the full linear scan the only live path), the
  `Board` dirty-center accumulator (`threats_dirty_centers`,
  `threats_dirty_overflow` and their `*_for_test` accessors), and
  the tautological incremental-vs-full oracle tests. The bare
  `threats_dirty: Cell<bool>` flag is retained — it still gates the
  reconcile.
- **Orphan threat-radius config**: `[engine.threats] recompute_radius`
  / `cluster_radius` / `max_incremental_centers` and their `build.rs`
  emits.
- **Dead config emits**: emitted-but-unread `build.rs` constants
  (`OVERLAP_BONUS_X10`, the runtime `WINDOW_K_SCORES` const — the
  toml array is kept, it still feeds the Layer 1 score table —
  `EXTENDED_MOVE_RADIUS`, `MOVE_GEN_OUTER_RADIUS`,
  `FULL_LEGALITY_RADIUS`, `MOVE_CAP`, the `BENCH_*` consts).
- **Dead fork primitive**: `ThreatSet::is_mate_pending` +
  `threats::single_cell_blocks_all` — a duplicate of `eval.rs`'s live
  `single_cell_covers_all`, kept alive only by tests.
- **`window6`** (`LineBitmap` / `AxisBitmaps`) — superseded by the
  8-cell window-scan table in Phase 17.
- **`notation.py`** — an unreferenced stub module (also kills the
  duplicate `GameRecord` shadowing bug).
- **`benchmark.py` match stubs** + the `analyze` CLI subcommand —
  unreferenced shells.
- **`creates_s1` test naming residue** — renamed after the Phase 17
  hybrid removal (naming-only, no behaviour change).

**Fix-not-delete**: `search.rs` hardcoded `depth >= 4` for the
aspiration-window start; `ASPIRATION_START_DEPTH` is now wired in
from `hexo.toml` per the CLAUDE.md magic-number rule.

**Result**: ~500–650 LOC removed; reference node counts
byte-identical (the dirty-accumulator removal is the only
behaviour-adjacent change and the parity gate confirmed it).

## Phase 23 — SRP Splits

**Goal**: split bloated files per the Phase 21 investigation. Pure
file moves on top of the smaller post-Phase-22 tree — zero behaviour
change, reference node counts byte-identical.

Splits (all flat — no subdirectories, per the investigation):

- `search.rs` → `search.rs` (the search algorithm) + `engine.rs`
  (the `Engine` game-state handle: owns board/tt/ordering, exposes
  place/undo/best_move/reset).
- `board.rs` (703 LOC post-Phase-22) → the `Board`-side proximity
  helpers extracted into the existing `proximity.rs`, next to
  `ProximityCounts`.
- `cli.py` → `cli.py` (argparse + dispatch + play/selfplay/bot) +
  `cli_bench.py` (bench subcommands) + `cli_match.py`
  (match/promote/vs subcommands).
- `promote.py` → `promote.py` (match drivers + data model) +
  `promote_sprt.py` (Wilson/Elo/SPRT statistics) +
  `promote_worktree.py` (`.bestref` + worktree management).

`axis_bitmap.rs` (518 LOC) and `benchmark.py` (806 LOC) were
assessed and kept — cohesive enough that a split would be cosmetic.

**Result**: public API surface unchanged; reference node counts
byte-identical pre-Phase-22 / post-Phase-23.

## Phase 24 — Performance Investigation

Read-only / measurement-only phase. No engine code changed. Refreshed
`benches/results/HOTSPOTS.md` against a frame-pointer flamegraph +
criterion sweep and produced `subagents/reports/phase24-perf-investigation.md`
— the scoping input for Phase 25.

Key findings: NPS +23–28 % across every fixture since Phase 17 (the
Phase 20 S1/S2-detection removal dividend), 32/32 byte-identical
reference node counts, the engine is compute-bound (IPC 4.38, branch
mispredict 0.35 %, LLC miss 2.9 %). The TT is 98 % empty with <1 %
collisions — the long-standing "4-bucket TT" candidate is **dead**.
Current hotspot ranking: Layer-1 window scan (~31 % of engine) >
`threats::compute` (~21 %) > `ordering` predicates (~20 %) >
`for_each_in_range`/proximity (~18 %) > search recursion (~6 %).

## Phase 25 — Optimization Quick Wins + Measurement Cleanup

**Goal**: bundle three low-risk, output-identical optimizations from
the Phase 24 candidate ranking plus three measurement-infrastructure
cleanups. Pure throughput phase — reference node counts byte-identical
before/after is the gate; no `make vs` gating needed.

Optimization work stream:

1. **Bit-parallel `LineBitmap` run scan + shared line-lookup cache**
   (Phase 24 candidate #1). `run_forward`/`run_backward` per-cell
   `get()` loops replaced with masked `u64` reads
   (`trailing_ones`/`leading_zeros`); a per-`order_moves` line-lookup
   cache so candidates on a shared `(axis, line_id)` resolve once.
   Speeds `would_make_six`, `creates_s0`, `run_endpoints` and win
   detection. Resolves the Phase 24 `creates_s0` per-axis run cache
   (take 3) candidate — broadened to a bit-parallel run scan — and
   folds in the perf angle of move-ordering refinement.
2. **`threats::compute` micro-opts** (candidate #3). Per-player piece
   iteration (`Board::pieces_of(player)`) replaces the full-history
   filter walk.
3. **`for_each_in_range` precomputed offset tables** (candidate #4).
   Fixed-radius (r=2, r=8) offset tables replace the runtime
   hex-distance `dq/dr` loop.

Cleanup work stream:

4. **`bench breakdown` metric repaired** — the Phase 14 metric summed
   raw criterion medians with no call-count weighting; rederived from
   flamegraph self-time (ground truth).
5. **Flamegraph frame-pointer capture locked down** — Phase 24 fixed
   the dwarf-unwinder breakage; the `force-frame-pointers` /
   `--call-graph fp` requirement is now documented + regression-proofed.
6. **TT stats build hygiene** — `tt_stats` is a Cargo feature off in
   release; `make bench` / `make bench-baseline` now build with it so
   `baseline.json` populates `tt_hit_rate`. Production builds stay
   feature-free.

Out of scope (deferred — see Phase 26 candidates): per-line
`LineContribution` cache, search-internal proximity skipping.

**Reference node counts are the regression net.** All shipped changes
are output-identical by design; `make bench reference` byte-identical
pre/post.

## Phase 26 candidates (deferred follow-ups)

Carried forward — items still open after Phase 25.

- **Per-line `LineContribution` cache** (Phase 24 candidate #2): Layer
  1 (~31 % of engine) re-scans every populated line on every leaf
  eval. Cache per-`(axis,line_id)` Layer-1 contribution on `Board`,
  invalidate the ≤3 lines a placed stone touches. High payoff
  (~+8–15 %), high difficulty (cache-invalidation lifecycle) —
  warrants a dedicated phase.
- **Search-internal `place` / proximity-skip** (Phase 24 candidate
  #5): the r=8 outer-proximity walk is dead work inside search (every
  searched move is a provably-legal r=2 inner candidate). A
  `place_for_search` path could skip it. Behaviour-touching at the
  contract level — needs strength gating.
- **`[bot]` vs `[engine.search]` time-budget drift**: `[bot]
  default_time_per_move_ms` and `[engine.search] default_time_ms` are
  both 1000ms. Config hygiene — fold if always coupled.
- **`find_pv` eviction tolerance**: best-effort; returns shorter PV
  if TT loses entries between root and walk.
- **Radius-theory colony discounting** in eval (deferred eval
  feature; on the v1 out-of-scope list).
- **LMR retune** now that perf headroom exists for deeper search.
- **Incremental threat recompute** (revisit) — the Phase 15 idea
  reverted at `15c9638`; the natural follow-on once the
  `LineContribution` cache proves the invalidation pattern.
- **Algorithm work**: revisit null-move pruning under two-stone
  parity.
- **Lazy-SMP parallel search**.
- **Opening book**, **endgame tables**, **WebSocket live integration**.

Closed since the Phase 24 list:

- **Eval tuning (S1/S2 shapes)** — closed at Phase 18/20 (verdict
  DROP, detection code removed).
- **TT bucket layout (4-bucket / hash-folding)** — dead. Phase 24 § E:
  TT 98 % empty, <1 % collisions, not in flamegraph self-time. Solves
  a non-problem.
- **`creates_s0` per-axis run cache (take 3)** — resolved by Phase 25
  STEP 1.1 (broadened to a bit-parallel run scan).
- **Move-ordering bucket refinement** — the perf angle folded into
  Phase 25 STEP 1.1; pure bucket-*quality* refinement is a strength
  change, deferred to a strength-focused phase (not a perf candidate).

## Phase 15 reviewer-pass fixes

After the STEP 5 baseline landed, an independent reviewer flagged:

- **`ThreatInstance::anchor` was dead metadata** — populated by
  `push_s0` but never read by the incremental path (which uses piece
  coords directly for the dirty-cluster check). Removed per "pick the
  more efficient side" rule; spec text in `SPEC_ENGINE.md
  § ThreatInstance` aligned.
- **Spec drift on `RefCell<Option<ThreatSet>>`** — STEP 3 dropped the
  `Option` wrapper; spec text updated to match.
- **`SPEC_EVAL.md § Detection method`** — described "drop matching
  instances from prior, rescan that line slice, merge" for linear
  shapes; shipped impl does a full linear re-walk (only cross-axis
  is selective). Spec text updated to describe the shipped algorithm.
- **Oracle test seed comment** — referenced `0xHEX0_F00D`; actual
  seed is `0xDEAD_F00D_CAFE_BEEF`. Comment fixed.

## Phase 15 resolved follow-ups

- **Incremental threat recompute** (Phase 14 STEP 7 deferral):
  resolved via `Board::threats_dirty_centers` SmallVec +
  `ThreatSet::compute_with_scratch` incremental path +
  `ThreatInstance::anchor`. Oracle test
  (`tests/threats_oracle.rs`) gates correctness with a fixed-seed
  10k-position random walk.
- **`pvs_node;threats;is_none;is_some` RefCell chain**
  (Phase 14 HOTSPOTS #5): resolved by the `Cell<bool>` dirty flag
  fast path in `Board::threats`. The `Option` check is now
  unreachable on the hot path (debug_assert covers the invariant).
- **`creates_s0;run_backward` axis-run repetition**
  (Phase 14 HOTSPOTS #4): resolved by the per-`order_moves`
  `axis_run_cache` in `OrderingContext`, so multiple candidates on
  the same line share one bitmap snapshot.

## Phase 14 resolved follow-ups

- **`piece_at` 2-probe regression** (Phase 13 carry-over): resolved
  via `AxisBitmaps::is_player` and a short-circuit in
  `threats::matches_pattern<N>` (STEP 4).
- **threats::compute scratch reuse**: `FxHashSet seen` and the
  player-pieces `Vec` now live in a `ThreatScratch` owned by
  `Board`; cleared per call to retain capacity (STEP 3).
- **LineBitmap layout + batched window scan**: `#[repr(align(64))]`
  plus `LineBitmap::windows6_run` cut the per-line eval cost by
  emitting 6-bit windows directly from packed `u64` storage
  (STEP 6).
- **Reference table determinism**: depth-only `Engine::best_move`
  calls now skip the default time fallback, so the reference node
  counts are bit-for-bit reproducible at every depth. Replaces the
  Phase 13 reference column which was time-truncated at d ≥ 6.

## Phase 10 — Benchmark Suite

**Goal**: comprehensive bench infrastructure for optimization cycles.

Two tiers:
- **Rust criterion** micro-benches per module (`hammerhead-engine/benches/`).
- **Python harness** macro-benches (`hammerhead/hammerhead/benchmark.py`).

Outputs canonical JSON to `benches/results/<isodate>-<sha>.json`. Diff
tool compares two result sets. `make bench`, `make bench-micro`,
`make bench-diff`, `make bench-baseline`.

See `specs/SPEC_BENCHMARKS.md` and `prompts/PHASE_10_PROMPT.md`.

## Phase 12 — Stabilization & Reference

**Goal**: pre-optimization cleanup and measurement infrastructure.

No new features. No algorithmic changes. Sweeps warnings, adds the
reference node-count table, gates TT instrumentation behind a Cargo
feature, captures a flamegraph, and commits the live `baseline.json`.

- Cargo target rename: `[lib] name = "hammerhead_engine_core"` (was
  `hammerhead_engine`). The PyO3 module name (`hammerhead_engine`) is independent
  of the cargo target name and stays unchanged; maturin emits the
  cdylib under the module name via `pyproject.toml [tool.maturin]
  module-name`. Resolves the `cargo bench` filename-collision warning
  (cdylib and rlib both produced `libhammerhead_engine.so`).
- Warning sweep: every `make` target completes warning-free. Python
  pytest runs with `-W error`.
- `hammerhead bench reference` subcommand (see `SPEC_BENCHMARKS.md
  § Reference node-counts`).
- TT statistics behind Cargo feature `tt_stats` (see
  `SPEC_BENCHMARKS.md § TT statistics`).
- Flamegraph capture script + top-5 hotspots committed to
  `benches/results/HOTSPOTS.md` for use as Phase 13 entry points.
- Committed `benches/results/baseline.json` from a real bench run.

Resolves Phase-10 deferred item "`baseline.json` committed".

See `prompts/PHASE_12_PROMPT.md`.

## Phase 13 — Kill the Hot HashMaps

**Goal**: replace the two hashbrown probes identified by the Phase 12
flamegraph as the dominant user-space costs.

- `AxisBitmaps[axis][player]` switches from `FxHashMap<i16, LineBitmap>`
  to a fixed-length `Box<[Option<LineBitmap>]>` of length
  `2 * ZOBRIST_WINDOW + 1` (255 at defaults), indexed by
  `line_id - LINE_ID_OFFSET`. Every `get` / `set` / `is_set` /
  `window6` becomes a bounds-checked array load instead of a hashbrown
  probe.
- `Board::pieces: FxHashMap<Coord, Player>` is removed entirely.
  `piece_at` becomes a two-player axis-bitmap probe (Q-axis arbitrarily);
  `is_empty_cell` is `piece_at(c).is_none()`; `piece_count` is
  `history.len()`; `pieces()` iteration walks the history `Vec` and
  derives the player via `player_at_ply(idx)`. Internal helpers that
  needed an occupancy check (`add_proximity`) probe `AxisBitmaps`
  directly.
- Bench harness fix: `bench_search` previously constructed `Engine::new`
  inside `criterion::iter_batched_ref`'s setup closure, allocating a
  64 MB TT every iteration. The Phase 12 flamegraph was dominated by
  `from_elem<(TTEntry,TTEntry)>` / `unmap_region` / kernel page-fault
  frames as a result. STEP 1 amortizes the `Engine` (and TT) across
  iterations via `Engine::reset` + `Engine::clear_tt`. Harness-only —
  production unaffected. Reference node counts identical.

No algorithmic changes. No search-behaviour changes. Reference node
counts must be identical at every `(fixture, depth)` before and after.

Spec-vs-code corrections applied during Phase 13 are recorded in the
existing "Spec-vs-code corrections" section below.

See `prompts/PHASE_13_PROMPT.md` and Phase 12's
`benches/results/HOTSPOTS.md` for the flamegraph rationale.

## Phase 11 — Promotion Harness

**Goal**: validate a candidate version against `.bestref` before
promoting.

- Git worktree at `.worktree-best/` checked out at `.bestref` SHA.
- Per-worktree venv builds the baseline engine.
- Subprocess protocol via `hammerhead bot` (Phase 9).
- `hammerhead/hammerhead/promote.py` — SPRT / Wilson / raw tests.
- `make vs`, `make promote` replace the Phase-9 stubs.

Tuning lives in `hexo.toml § [promote]` (Python-only — Rust does not
consume these constants). The harness is serial (1 game at a time) and
runs each game in a freshly-spawned subprocess pair to guarantee clean
TT/history state.

### `.bestref` bootstrap

`scripts/setup_worktree.sh` is idempotent and self-bootstrapping:

- If `.bestref` is missing, it is initialized to the current `HEAD`.
  The first `make vs` then runs *current vs current* (winrate ≈ 0.5),
  which exercises the harness plumbing without coupling to engine
  strength.
- If `.bestref` SHA differs from the worktree's HEAD, the worktree is
  removed and recreated at the new SHA. The per-worktree venv is then
  rebuilt via `maturin develop --release` and `pip install -e hammerhead`.
- `HEXO_SKIP_BUILD=1` short-circuits the build step (used by the
  idempotency test in `hammerhead/tests/test_promote.py`).

### SPRT details

We use a Bernoulli SPRT: each game contributes **two** Bernoulli trials
(`win → 2/2`, `draw → 1/2`, `loss → 0/2`). The trial-level success
probability for a hypothesis Elo `e` is `1 / (1 + 10^(-e/400))`. The
log-likelihood ratio is `successes·log(p1/p0) + failures·log((1-p1)/(1-p0))`,
checked against the standard Wald bounds `[log(β/(1-α)), log((1-β)/α)]`.

See `prompts/PHASE_11_PROMPT.md`.

## Out of scope for v1

- Null-move pruning
- MCTS / hybrid
- Neural net eval
- Multi-threaded search (lazy-SMP later)
- Opening book
- Endgame tables
- WebSocket / SealBot live integration
- Radius-theory colony discounting

## References

- Connect-6 + Alpha-Beta-TSS: Wu et al., NCKU/NYCU group
- Yixin engine: TT + PVS + LMR + threat-aware quiescence
- Stockfish: alpha-beta best practices
- Schaeffer, "The History Heuristic," ICCA Journal 6(3), 1983
- SealBot: github.com/Ramora0/SealBot (closest comparable)
