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
| 27 | per-line `LineContribution` cache (Layer-1 delta-update memo) | ✅ done |
| 28B | eval-value tuning sprint (coordinate-descent, 5 candidates) | ✅ done |
| 28C-0 | subset verification (2³ factorial); reverted B-2.3 + B-2.5 per non-replication | ✅ done |
| 28C-1 | BO sprint (60 trials × 200g, GPSampler); winner reverted on 400g validation | ✅ done |
| 28D-1 | 800g cycle-break match HEAD vs `.bestref`; Outcome C, `.bestref` 932c5d8 → 5bd89648 | ✅ done |
| 28D-3 | eval revival (S1 detection: open_3 / closed_3 / open_2) + I3 bug sweep; KEEP commits, no `.bestref` advance | ✅ done |
| 28E-0 | per-stone time-fix + SDK SearchStats + fixed-depth + engine audit (1 MAJOR + 1 MINOR); KEEP, no `.bestref` advance | ✅ done |
| Sprint 1 | free-wins bundle: iai-callgrind gate + PGO ship + TT prefetch; +7.4% bench-quick NPS; `.bestref` cfefb3b → cac186e | ✅ done |
| Sprint 2 | proximity bundle + supporting items: worktree-PGO opt-in (A) + bounds-elim/inline (D) + SparseCellSet u16 (E) + EvalCache align (G); C/F/H aborted by falsification branches; +3.3% bench-quick NPS / -7% iai instructions; 200g vs `.bestref` -12.2 Elo INCONCLUSIVE; `.bestref` UNCHANGED (Outcome C, plan § I.5) | ✅ done |
| Sprint 3 | place_for_search (B, with design-pass A first) + history flat table (C) + AxisBitmap unchecked indexing (D); E LMR retune + F staged 2.5 deferred to Sprint 4; +39.2% bench-quick NPS / -25% iai instructions; Phase B 400g raw +19 corrected +9 Elo PASS, Phase C 200g raw +58 corrected +48 Elo PASS, Phase D 400g raw +6 corrected -4 Elo (accepted on evidence); `.bestref` advance per final 400g gate | ✅ done |
| Sprint 4 | runtime tuning surface (A) + LMR Texel sweep (B, REVERTED) + aspiration/extension override (C) + side-quest pgo_build.sh cross-venv fix; D SB-perf re-baseline SKIPPED (adapter absent) + E staged 2.5 DEFERRED to Sprint 5; Phase B (3,4,2) Stage 2 same-source +24.4 Elo / Stage 3 v2 raw +12.2 corrected +2.2 / final SPRT raw −8.7 corrected −18.7 → REVERTED on disagreement; bench-quick 944k recovered to baseline ±1% after revert; `.bestref` UNCHANGED (Outcome B) | ✅ done |

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

**Outcome.** The three optimization candidates were each attempted by
an independent subagent with an A/B comparison and **all three were
reverted** — none shipped:

- Bit-parallel `LineBitmap` run scan + cache — regressed −15/−16 % NPS
  (the existing fully-unrolled `get()` loop branch-predicts better on
  the typically-short runs).
- `threats::compute` per-player iteration — flat, within ±3 % noise
  (the cost is the linear run-scan, not the `pieces()` history filter).
- `for_each_in_range` offset tables — regressed −10/−11 % NPS (the
  bounded `dq/dr` loop is register-resident and compiler-unrolled; a
  flat table walk adds memory loads + L1 pressure).

Per the phase rule ("revert and skip — do not debug in-phase") nothing
landed; the **engine source is byte-identical to Phase 24** (`44493f6`).
Only the cleanup work stream (4/5/6) shipped. The headline NPS targets
(≥ 580 k / ≥ 440 k) were not met — the engine is unchanged. The three
candidates carry forward to Phase 26. The lesson: the engine is
compute-bound at IPC 4.38 and its hot loops are already well-formed for
the branch predictor / register allocator — micro-rewrites lose to the
existing code; real wins need algorithmic work-reduction. See
`benches/results/HOTSPOTS.md § Phase 25 status`.

**Reference node counts are the regression net** — 32/32 byte-identical
pre/post (trivially, since no engine code changed).

## Phase 28B — Eval-Value Tuning Sprint

Match-driven coordinate-descent sweep of the top-5 unswept eval
scalars (`open_4`, `fork_cover2_bonus`, `window_k_scores[5]`,
`closed_5`, `open_extension_factor`). Phase 28A audit found that
the live S0 + window + extension + fork surface had never been
game-time-tuned since the codebase existed — the "Phase 10 self-
play tuning" provenance claim in SPEC_EVAL was unsubstantiated
(no commit trail in git history).

Built on Phase 27 (`e28d54a`). Resurrected the Phase 20-deleted
sweep infrastructure: 14-scalar `EvalOverrides` runtime override
surface + `hammerhead bench tune-sweep` driver (`tune.py`).
Pre-screened all 5 candidates at endpoints, ran Stage 1 (200g)
+ Stage 2 (400g) per surviving candidate. Stopping rule (3
consecutive Stage 2 CI straddles → auto-terminate) triggered at
B-2.3; continued past per documented judgment.

**Outcome.** 3 of 5 candidates landed on master as MARGINAL-LANDs
(Phase 27 shape — positive point estimate, CI straddles zero):

- `open_4`: 60_000 → 135_000 (commit `b35936b`, Stage 2 +12.2 Elo)
- `window_k_scores[5]`: 4_096 → 2_048 (commit `5283059`, +20.9 Elo)
- `open_extension_factor`: 4 → 8 (commit `13dc73a`, +6.9 Elo)

2 reverted (Stage 2 point negative or essentially zero):
- `fork_cover2_bonus`: stays 4_000 (Stage 2 -15.6 Elo)
- `closed_5`: stays 500_000 (Stage 2 -1.7 Elo, despite strongest
  pre-screen signal of all 5)

Combined-best probe (HEAD with 3 wins vs HEAD-with-3-wins-undone
at 400g): **-3.5 Elo CI [-37.5, +30.5]**. The 3 wins do NOT
compose additively — sum-of-per-axis +40 Elo, joint -3.5 Elo
(43 Elo delta below additive prediction).

Final promote-match HEAD vs `.bestref` (`932c5d8`) at 400g:
**+17.4 Elo CI [-16.7, +51.4]**, REJECT (strict gate CI lower > 0
not cleared). **`.bestref` UNCHANGED.** Outcome B per Phase 28A.5
plan § G (modal expectation, matches Phase 27 shape).

**Key meta-findings:**

1. Eval surface is noise-resolution-limited. ALL 5 candidates
   produced Stage 2 CIs straddling zero. Signal exists but
   amplitude is below the 400g harness floor (±34 Elo CI).
2. Combined-best shows negative interaction between per-axis
   wins (Layer-1 vs Layer-2 balance shift).
3. Pre-screen single-run Elo is noise-dominated at 200g
   per endpoint — useful only for routing (dead-substrate
   detection), not cell-quality ranking.
4. Baseline-vs-baseline self-test noise stdev ~19 Elo at 200g.

**Reference node counts** rebaselined per landing (3 fresh
`baseline.json` macro.reference blocks — Phase 25.5 rule
applied per value-tuning rebaseline event). B-0/B-1 commits
byte-identical (no behaviour change); B-2.x commits intentionally
drift.

**Sprint wall-clock**: ~6h 22min (vs 16.3h plan worst-case).
2.6× faster than plan estimate — host throughput exceeded
budget (games complete ~7 min/200g vs plan's ~20 min assumption).

**Phase 28C handoff items**:

- Combo test at higher N (800g/1600g): Phase 27 + Phase 28B
  winners vs `.bestref` to see if cumulative bumps clear strict
  gate.
- Subset experiments: combined-best showed -43 Elo delta vs
  sum; find which 28B winners are net-positive when stacked
  (drop B-2.1 alone, drop B-2.5 alone).
- Opening diversity validation A/B (per Phase 28A.5 A-5 forward
  commitment): test HEAD vs HEAD diversity ON vs OFF.
- Tempo proxy (per Phase 28A I1 § 3): requires detector revival
  or proxy invention. Strongest PDF evidence (TT p. 11) of any
  deferred item.
- Refined stopping rule: "point < +5 Elo" instead of "CI straddles
  zero" for consecutive-straddle terminator.

Full retrospective: `/tmp/phase_28b/PHASE_28B_RETRO.md` (gitignored
per Phase 25.5 repo hygiene). Per-candidate audit:
`/tmp/phase_28b/SPRINT_STATE.md`. Per-stage A/B logs and JSONs:
`/tmp/phase_28b/B-{0..3}/`. HOTSPOTS detail:
`benches/results/HOTSPOTS.md § Phase 28B status`.

## Phase 28C-0 — Master State Verification

2026-05-23. Subset-verification sprint following Phase 28B handoff
item "subset experiments". Ran an 8-config 2³ factorial vs Phase 27
baseline (`e28d54a`) at 400g (3200 games total, ~1h46min wall) with
self-test drift correction (-6.9 Elo). C0-SYN drift-corrected
verdict: **revert 2 of 3 Phase 28B landings**.

Per-landing decision:
- `open_4` = 135_000 (B-2.1, `b35936b`): **KEEP**. Main effect +4.4
  Elo (in noise band but positive); C1 = {B-2.1 only} = best
  observed subset (+24.4 Elo).
- `window_k_scores[5]` = 4_096 (B-2.3 `5283059` reverted in
  `5fe133e`): main effect -15.7 Elo, just outside noise band;
  B-2.1×B-2.3 interaction = -27.85 Elo (~2.27σ, borderline
  significant — strongest 2-way in the design).
- `open_extension_factor` = 4 (B-2.5 `13dc73a` reverted in
  `11ab31a`): main effect -9.6 Elo (in noise band); Occam
  tiebreak — simpler config wins.

Post-revert master HEAD ≡ C1 = drift-corrected +24.4 Elo vs
`e28d54a` CI [-9.7, +58.5] — CI straddles zero (same shape as
Phase 27/28B MARGINAL-LANDs, expected at 400g resolution floor).
**`.bestref` UNCHANGED** (`932c5d8`) — strict-promote rules
unchanged; reverting bad landings is not promotion.

**Key finding**: eval surface is confirmed non-separable. Sum of
2-way interactions = -22 Elo; C7 (HEAD pre-revert) underperformed
additive main-effect prediction by ~14 Elo. Per-axis coordinate
descent (Phase 28B approach) systematically underexplores joint
optima.

**Phase 28C-1 methodology**: Optuna 4.8.0 GPSampler (per
`/tmp/phase_28c/0/feasibility_research.md`). Matérn-5/2 kernel
models cross-dimensional interactions implicitly; learns
per-dimension length-scales via marginal likelihood.
`deterministic_objective=False` for the ~±34 Elo Wilson noise.
Seeds at C1 (best observed). 50-80 trials, 6-10h wall on 10-worker
host.

Commits (3 on master):

| SHA | Subject |
|---|---|
| `11ab31a` | revert: B-2.5 open_extension_factor per Phase 28C-0 |
| `5fe133e` | revert: B-2.3 window_k_scores[5] per Phase 28C-0 |
| (this commit) | bench: Phase 28C-0 master state verification |

Reference node counts rebaselined per revert (Phase 25.5 rule —
value-tuning rebaseline event; both reverts shift search behavior).
NPS bench-quick: 524k (pre-revert HEAD) → 551k (post-B-2.5 revert)
→ 554k (post-B-2.3 revert) — recovers the -4.9% B-2.5 NPS penalty.

Artifacts: `/tmp/phase_28c/0/synthesis.md` (full drift-corrected
2³ factorial), `verification_runner.md` (match protocol + raw
results), `feasibility_research.md` (BO library decision),
`matches/C{0..7}.json` (raw match data). Gitignored per Phase 25.5
repo hygiene.

## Phase 28C-1 — BO Sprint (Outcome A: REVERT)

2026-05-23 → 2026-05-24. 60-trial Optuna 4.8.0 GPSampler sprint
over 5 eval scalars (`open_4`, `closed_5`, `window_k_scores[5]`,
`open_extension_factor`, `fork_cover2_bonus`) at 200g per trial
vs Phase 27 baseline (`e28d54a`). Sprint clean: 60/60 COMPLETE,
0 FAIL, **6h 40min** wall-clock on 10-worker host.

**Outcome A — REVERT, zero eval landings.** BO winner trial 34
(raw +63.23 Elo at 200g, hit 4 of 5 search bounds) collapsed under
400g validation:

- Trial 34 vs `.bestref` (`932c5d8`) at 400g, direct:
  **-14.77 Elo CI [-48.80, +19.25]**, W-L-D 191-208-1. Strict
  gate FAILED, marginal gate FAILED (point < 0).
- Trial 34 vs `e28d54a` at 400g (smoke): -10.43 Elo, drift-corrected
  -8.69. Vs current HEAD C1 (additive): ~-33 Elo regression.

**Cumulative reference (fresh 400g measurement)**:
`e28d54a` vs `.bestref` = **+33.11 Elo CI [-1.04, +67.25]** —
Phase 27 marginal-positive shape repeats at the cumulative anchor.
C1 (current HEAD) implied vs `.bestref` ≈ **+57.5 Elo** (additive
+24.4 + 33.11).

**Key findings:**

- fANOVA importance: `window_k_scores[5]` dominant at **0.521**
  share of variance — confirms 28C-0 §7 finding (B-2.1×B-2.3
  interaction is real). The dimensional coupling is real; trial
  34's specific corner was a 200g noise spike, not a real
  interaction payoff.
- 4 of 5 search bounds hit at the winner: `open_4=240k` HIGH,
  `closed_5=240k` LOW, `wk[5]=1024` LOW, `oef=1` LOW (only
  `fork_cover2_bonus=12k` interior). Bounds were too narrow on
  4 dims.
- 200g best-of-60 applies +20 Elo positive selection bias;
  Wilson CI half-width ±48 Elo at 200g makes top-1 selection
  unreliable. Sample size is the binding constraint, not the
  sampler.
- Convergence early-stop unimplemented (design.md §3 rule);
  would have fired at trial 48 (post-trial-34 plateau) and
  saved ~1.4h.

**Path C taken (no commits to eval/hexo.toml):**

| SHA | Subject | Type |
|---|---|---|
| `36b8cdc` | tune: add Optuna BO driver scaffolding | infra |
| `fb36ddd` | tune: integrate Optuna study with EvalOverrides | infra |
| `e46869c` | tune: BO study report + spec update | infra |
| `3d5ae7e` | bench: HOTSPOTS Phase 28C BO sprint section | doc |
| (this commit) | spec: mark Phase 28C done in roadmap | doc |

Engine byte-identical to `0c3cc6b` (Phase 28C-0 close).
`.bestref` UNCHANGED (`932c5d8`, 6 phases).

**Third consecutive Phase-27-shape outcome (27 / 28B / 28C).**
The eval-tuning lever is at the harness resolution floor.
Phase 28D should test the cumulative C1 implied +57.5 Elo via
an 800g promote-match at HEAD vs `.bestref` (no eval change)
to break the cycle.

C3-DIVERSITY DEFERRED: opening-diversity infra is missing
(`promote.py:372-376` + `:553-557` raise `NotImplementedError`,
no opening library in `positions.json`). Cannot run A/B at HEAD;
infra (deferred B-1.3 / B-1.4 from 28B C-DEFERRED) must land
first.

Artifacts: `/tmp/phase_28c/PHASE_28C_RETRO.md` (full retro),
`/tmp/phase_28c/{1,2,3}/{design,implementer,sprint,drift,validation,diversity,landed}.md`,
`/tmp/phase_28c/2/study.db` + `trials/{0000..0059}.json`,
`/tmp/phase_28c/3/val_trial34_vs_bestref.json`. Gitignored per
Phase 25.5 repo hygiene.

## Phase 28D-1 — Cycle-Break Match (Outcome C: ADVANCE)

2026-05-24. 800g promote-match HEAD (`5bd8964`, engine state C1
= Phase 25.5 + Phase 27 LineContribution cache + Phase 28B-B-2.1
`open_4=135_000`) vs prior `.bestref` (`932c5d8`, Phase 25.5
final). No eval / `hexo.toml` / source change — pure cumulative
measurement. The natural cycle-breaker after three consecutive
Phase-27-shape outcomes (27 / 28B / 28C all REJECT on strict
gate without eval regression).

**Outcome C — strict-positive. Cycle BROKEN. `.bestref` advanced
932c5d8 → 5bd89648.** First `.bestref` advance in 6 phases
(since Phase 25.5, commit `432ddba`).

**Match result**:
- 800 games, 500 ms/stone, 10 workers, Wilson 95%, color-balance
  ON, opening-diversity OFF (no library exists).
- W-L-D **429-371-0**, winrate 0.5363 Wilson [0.5016, 0.5706].
- **Elo +25.2, Wilson 95% CI [+1.1, +49.4]**, half-width ±24.2 Elo
  (matches dispatcher prediction).
- CI lower +1.1 > 0 — strict gate cleared by **razor-thin margin**
  (~12 fewer wins would have flipped to Outcome B).

**Additive-prediction comparison**:
- 28C C2-DRIFT: `e28d54a` vs `.bestref` = +33.11 Elo @ 400g.
- 28C-0 drift-corrected: C1 vs `e28d54a` = +24.4 Elo @ 400g.
- Sum (additive): C1 vs `.bestref` ≈ +57.5 Elo.
- Observed: **+25.2 Elo** — ~32 Elo BELOW additive prediction,
  ~1.3σ below the sum-of-variances std dev (~25 Elo).

Most likely explanation: partial regression to mean from
CI-straddling prior measurements. Both anchor measurements
(+33 and +24) were straddling-CI single 400g points with point
estimates that could each be 5–20 Elo upward noise excursions;
summing compounds the optimistic bias. **Real cumulative Elo at
HEAD vs prior `.bestref` ≈ +25 Elo point estimate, not +57.**

Consistent with Phase 27 alone (which measured +27 Elo at 400g
vs `.bestref` and REJECTed on `CI lower > 0`): the B-2.1
increment may contribute net ≈ 0 over Phase 27 at 800g
resolution, or 400g sample noise dominated both prior anchor
measurements equally. The data don't decisively distinguish; the
cycle-break is real (CI lower > 0), the magnitude is more
modest than additive arithmetic suggested.

**Drift recalibration SKIPPED** per Outcome C protocol —
correction could only tighten an already-cleared verdict
(28C drift measurement on the `e28d54a` anchor was -1.74 Elo,
statistically zero).

**Commit** (1 atomic, on master, engine source byte-identical):

| SHA | Subject | Notes |
|---|---|---|
| `b95a672` | `promote: advance .bestref to 5bd89648 (Phase 28D-1)` | Only `.bestref` config file changed. Reference node counts trivially byte-identical. |
| (this commit) | `spec: mark Phase 28D-1 done in roadmap` | Doc — this section. |
| (next commit) | `bench: HOTSPOTS Phase 28D-1 .bestref advance` | Doc — HOTSPOTS Outcome C section. |

**Promote-harness bug** (incidental, NOT FIXED — logged for
follow-up phase): the auto-commit branch in
`hammerhead/hammerhead/promote.py` invokes
`git commit --only -- <path> -m <msg>` — with `-m` AFTER the
`--` pathspec separator. Per `git commit(1)`, everything after
`--` is a pathspec, so `-m` + message text become invalid
pathspecs and the commit fails. Auto-commit rolled back the
working-tree `.bestref` but left the staged index dirty; D1-LAND
performed manual cleanup + atomic commit. Trivial fix
(reorder to `-m <msg> --only -- <path>`); high-priority follow-
up for the next phase that touches `promote.py`. Reviewer
additionally noted `specs/SPEC_BENCHMARKS.md` lacks an explicit
`[promote]` section despite roadmap references — reconcile
alongside.

Full retrospective: `/tmp/phase_28d/PHASE_28D_1_RETRO.md`
(gitignored per Phase 25.5 repo hygiene). D1-RUN /
D1-LAND / D1-REV reports under `/tmp/phase_28d/1/`.

## Phase 28D-3 — Eval Revival + Bug Sweep (KEEP commits, no .bestref advance)

2026-05-24. 11 sub-phases targeting two parallel hypotheses: revive S1
detection (open_3 / closed_3 / open_2) per D3-DIAG eval correlation
diagnostic; sweep four I3-flagged SealBot-perf bug patterns in HH (TT
EXACT quantization, root ordering after aspiration fail-high, search
inner-loop heap alloc, TimeUp killer rollback). Per-shape atomic
landings + arena measurements at 50g per cell + GATE n=200 final
external match.

**Outcome: KEEP all 12 commits on master; `.bestref` NOT advanced.**
External arena flat (GATE Cond B 4.5% vs I4 baseline 8.7%, CIs overlap);
internal REJECT (-33 Elo, CI [-67, +1] — CI upper touches zero, NOT
strict-negative). Per the D3-GATE decision matrix this resolves
between "DO NOT LAND" (external <15%) and "REVERT" (strict-negative);
per-landing arena cells were each NEUTRAL not REGRESSION, so individual
landings are not defective. **Keep commits, do not advance .bestref.**

### Sub-phase summary

| Sub-phase | Commits | Outcome | Notes |
|---|---|---|---|
| D3-DIAG | (no commit) | PROCEED-WITH-CAUTION | HH-SB Pearson 0.963 hand-curated S1, -0.113 i2 real-game. Predicts modest gain; bigger lever is Gap #1. |
| D3-INFRA | `fca4dad`, `8542938` | byte-identical | S1 ThreatType + ThreatCounts + hexo.toml weights (all 0); detection skeletons; codegen + EvalOverrides + Python facade plumbing. |
| A.1 open_3 | `65ed2dc`, `5011ea3` | NEUTRAL externally | All 5 sweep cells negative (-35 to -108 Elo). Landed least-negative 90000. Cond B 8.0% / Cond A 0.0%. |
| A.2 closed_3 | `392e410`, `9a25ef6` | NEUTRAL externally | Two cells TIED at 0 Elo (100-100-0 each). Landed smaller weight 11250. Cond B 6.0% / Cond A 2.0%. |
| A.3 open_2 | `c656e0d`, `ab72ec2` | NEUTRAL externally | **FIRST positive A.X cell**: open_2=11250 = +52 Elo CI [+4, +101] internally. Length-2 doesn't collide with Layer-1 length-3. Cond B 2.0% / Cond A 6.0%. |
| B.1 TT EXACT | `8de7979` | REFUTED | HH stores `score: i32` raw, no quantization. Test-only commit (release binary byte-identical). `score_round_trip_is_bitwise_exact_no_quantization` lock-in. |
| B.2 root ordering | `8d75f8d` | REFUTED | HH routes ordering through TT; unconditional `pvs_node` tail TT store writes fail-high move with LowerBound flag. Test-only commit. `pvs_root_fail_high_writes_failing_move_to_tt` lock-in. |
| B.3 inner-loop alloc | `a3c7753` | REFUTED | No heap alloc in HH recursive search path (audited every site). Test-only commit. `search_hot_path_zero_alloc_structural_invariants` lock-in. |
| B.4 TimeUp rollback | `f1032ba` | REFUTED | Test-only commit; TimeUp killer-rollback invariant lock-in. |
| D3-GATE | (no new commit) | external arena flat / internal REJECT | Cond A 7.0%, Cond B 4.5%, vanilla SB 9.0%; internal 400g -33 Elo CI [-67, +1]. |

### Per-landing arena trajectory (Cond B per-stone 500ms)

| State | Win/Lose/Draw | Winrate | Wilson 95% | Internal Elo (200g sweep) |
|---|---|---|---|---|
| pre-D3 (I4) | 4-46-0 | 8.7% | [3.4%, 20.0%] | n/a |
| post-A.1 | 4-46-0 | 8.0% | [3.2%, 18.8%] | -35 Elo CI [-83, +13] |
| post-A.2 | 3-47-0 | 6.0% | [2.1%, 16.2%] | 0 Elo CI [-48, +48] |
| post-A.3 | 1-49-0 | 2.0% | [0.4%, 10.5%] | **+52 Elo CI [+4, +101]** |
| **GATE n=200 Cond B** | **9-191-0** | **4.5%** | **[2.4%, 8.4%]** | n/a |

Cumulative D3 (A.1+A.2+A.3+GATE) Cond B: 17/346 = 4.9% Wilson [3.1%, 7.7%].
I4 CI overlap [3.4%, 7.7%]. External arena flat within sampling noise.

### Layer-1 length-3 double-counting finding (the durable lesson)

Per-shape atomic landings allowed clean confound-free attribution.
open_3 (length-3) and closed_3 (length-3) cells uniformly non-positive;
open_2 (length-2) cleared Wilson-lower > 0. Length is the discriminator:
Layer-1 `window_k_scores[3] = 64` (codegen'd into `WINDOW_SCORE_8`)
already fires on length-3 own-stone configurations, double-counting
any additive Layer-2 OPEN_3 / CLOSED_3 weight. Layer-1 is silent on
length-2, so OPEN_2 carries independent information.

The two NULL closed_3 cells (11250 and 33750, both exactly 100-100-0)
plus uniformly negative open_3 cells are direct evidence of collision.
Open_2's positive cell is the cleanest counter-example. Refines
D3-DIAG's "Layer-1 double-counting" hypothesis to a length-3-specific
finding; directly seeds Phase 28E Gap #1 (window pattern table
redesign) as the next substantive lever.

### Per-stone vs per-turn observation (GATE n=200)

Cond A (per-turn-equiv HH 500 vs SB-perf 1000) 7.0% > Cond B (per-stone
equal 500/500) 4.5%. HH does BETTER when SB-perf has 2× per-stone but
equal per-turn time. Suggests HH's locked 60/40 stone1/stone2 split may
be suboptimal vs whole-turn-planning opponents (SB-perf plans whole
turns jointly per `bots/external/INTEGRATION_NOTES.md` §3). Cheap to
A/B in Phase 28E.

### Commits (12 atomic on master, + 3 doc commits this retro)

| SHA | Subject |
|---|---|
| `fca4dad` | `eval: add S1 ThreatType + ThreatCounts fields` |
| `8542938` | `eval: surface S1 weights in hexo.toml + EvalOverrides` |
| `65ed2dc` | `eval: implement open-3 detection` |
| `5011ea3` | `eval: tune open_3 weight to 90000` |
| `8de7979` | `tt: add score round-trip regression test (no quantization)` |
| `8d75f8d` | `search: add M5 root-tt fail-high invariant test (refutation lock-in)` |
| `392e410` | `eval: implement closed-3 detection` |
| `9a25ef6` | `eval: tune closed_3 weight to 11250` |
| `a3c7753` | `search: lock zero-alloc invariants for hot path (B.3)` |
| `f1032ba` | `search: lock TimeUp killer-rollback invariant (B.4)` |
| `c656e0d` | `eval: implement open-2 detection` |
| `ab72ec2` | `eval: tune open_2 weight to 11250` |
| (this commit) | `spec: mark Phase 28D-3 done in roadmap` |
| (next) | `spec(eval): document Phase 28D-3 revived S1 detection` |
| (prev/sibling) | `bench: HOTSPOTS Phase 28D-3 eval revival + bug sweep` |

### Honest assessment

External arena DID NOT MOVE. The S1 revival hypothesis ("HH lacks S1
detection, that's why SB-perf wins") is FALSIFIED at GATE n=200. The
phase has methodology value (per-shape atomic attribution, B.X invariant
test pattern, length-3 collision finding) and one productive forward
landing (open_2 detection + 11250 weight) but did not close the SB-perf
gap. Phase 28E Gap #1 is the next substantive lever.

Full retrospective: `/tmp/phase_28d/PHASE_28D_3_RETRO.md` (gitignored
per Phase 25.5 repo hygiene). Per-sub-phase reports under
`/tmp/phase_28d/3/{diag,infra,A.{1,2,3},B.{1,2,3,4},gate}/`.

## Phase 28E-0 — Time-Fix + SDK + Audit (KEEP commits, no .bestref advance)

2026-05-24. Four sub-waves: TIME-DIAG + TIME-FIX (per-stone vs
per-turn budget mechanism correction), SDK-DESIGN + SDK-IMPL (opt-in
`SearchStats` observability + `depth=N` fixed-depth kwarg + arena
adapter consumption), AUDIT + AUDIT-FIX (engine-wide 6-area bug sweep,
1 MAJOR + 1 MINOR landed), VERIFY (100g external + 400g internal final
read). 5 code/spec commits on master + 3 on hexo-arena main + 2 doc
this retro.

**Outcome: KEEP all 5 commits on master; `.bestref` NOT advanced.**
External arena flat (VERIFY 100g 2.0% [0.5, 7.0] vs D3 GATE baseline
4.5% [2.4, 8.4], CIs overlap); internal +40 Elo center shift vs D3
(51.12% / +7.8 Elo [-26.2, +41.8] vs D3 45.25% / -33.1 Elo [-67.3,
+1.0]) but CI spans 0 → REJECT (no strict-positive advance). `.bestref`
stays at `5bd89648`.

### Per-sub-phase summary

| Sub-phase | Commits | Outcome | Notes |
|---|---|---|---|
| E0-TIME-DIAG | (no commit) | CONFIRMED Hypothesis A | Root cause: `engine.rs:106` `split_budget(t, halfmove, stone1_time_pct)` halved per-stone budget (60/40 stone1/stone2 split applied to incoming per-stone time). SDK + arena always passed per-stone; engine re-divided. Mean per-stone utilisation 50.5%. |
| E0-TIME-FIX | `1f10f6c` | mechanism corrected; arena NEUTRAL | Drop `split_budget` + `stone1_time_pct`; wire `local.time_ms = provided_time;` directly. Post-fix utilisation 100.2%. 50g cross-checks (Cond A 4.0%, Cond B 0.0%) within ±5pp of D3 baselines. Confirms 28D-3 retro eval-gap hypothesis: deficit dominated by Layer-1 length-3 double-count, not time budget. |
| E0-SDK-DESIGN | (no commit) | A4 + B2 chosen | Bundled `SearchStats` dataclass via opt-in `return_stats=True` (A4 over A3 cached-property hazard). `depth=N` as kwarg on `suggest` (B2 over B3 separate method). |
| E0-SDK-IMPL | `7c53fd7`, `c799f57` | additive surface live | `SearchStats(max_depth_reached, nodes, nps, time_ms, score)`; `Bot.suggest(time_ms=T, depth=N, return_stats=True)`. Default callers byte-identical. 12 new tests in `test_bot_stats.py`. Arena adapter (`5fd77f3`) consumes both; `--depth N` works end-to-end. No PyO3 changes (reused `bench_best_move` 6-tuple). |
| E0-AUDIT | (no commit) | 1 MAJOR + 1 MINOR + 5 areas clean | 6 focus areas A-F. 12 confirm/refute scratch tests (10 pass / 2 fail-confirm D-1). |
| E0-AUDIT-FIX | `34fa870`, `012c327` | MAJOR + MINOR landed | D-1: `Board::undo` re-derives winner from axes via `#[cold] rederive_winner(player)`. Silent since Phase 4; hot search path insulated by `pvs_node` entry-guard. NPS Δ -3.2% (within ±5% threshold). MINOR: `search.rs:304` aspiration widen-count doc comment off-by-one. |
| E0-VERIFY | (no commit) | KEEP commits, no `.bestref` | 100g HH vs SB-perf 500ms: 2/100 = 2.0% [0.5, 7.0]; D3 baseline 4.5% [2.4, 8.4] (CI overlap). 400g internal HH vs `.bestref`: 51.12% / +7.8 Elo [-26.2, +41.8] vs D3 45.25% / -33.1 Elo [-67.3, +1.0] (+40 Elo center shift). |

### Methodology finding (load-bearing)

Pre-`1f10f6c`, Phase 28 arena measurements ran at ~50% intended HH
wall-clock per stone. Cross-phase arena winrate comparisons across the
entire Phase 28 arc are not directly comparable. The INTEGRATION_NOTES
"at `--time T`, HH gets 2T per turn vs SB-perf's T per turn"
characterization was inaccurate for vendored HH SHAs pre-fix and is now
accurate again. E-1 baseline measurements must be re-taken at the post-
fix HH effective time. D3 within-condition findings (Layer-1 length-3
collision pattern, open_2 counter-example) hold; absolute external
winrates do not transfer.

### Commits (5 atomic on hammerhead master + 2 doc this retro)

| SHA | Subject |
|---|---|
| `1f10f6c` | `search: fix per-stone vs per-turn time budget` |
| `7c53fd7` | `sdk: add SearchStats + depth/return_stats kwargs to suggest` |
| `c799f57` | `spec(api): document SearchStats + depth/return_stats kwargs` |
| `34fa870` | `board: re-derive winner on undo of cached-winner stone (D-1)` |
| `012c327` | `search: fix aspiration widen count doc comment (MINOR)` |
| (prev) | `bench: HOTSPOTS Phase 28E-0 time-fix + SDK + audit` |
| (this commit) | `spec: mark Phase 28E-0 done in roadmap` |

Plus 3 on hexo-arena main: `e47e5c0` (vendor refresh), `fe7b775`
(`--time-a/--time-b` asymmetric CLI), `5fd77f3` (adapter consumes
SearchStats + fixed-depth).

### Honest assessment

External arena DID NOT MOVE. Time-fix mechanism corrected; eval-gap
hypothesis from 28D-3 confirmed. Internal +40 Elo center shift is real
but does not clear strict-positive advance gate. One MAJOR audit bug
(D-1) silent since Phase 4 is fixed. SDK observability + fixed-depth
surface ready for E-1 measurement work. Methodology finding
(50%-utilization implicit calibration error across all of Phase 28)
re-frames the prior arc and carries forward to E-1 baseline
documentation. Phase 28E-1 Gap #1 (window pattern table redesign) is
the next substantive lever.

Full retrospective: `/tmp/phase_28e/PHASE_28E_0_RETRO.md` (gitignored
per Phase 25.5 repo hygiene). Per-sub-phase reports under
`/tmp/phase_28e/0/{time_diag,time_fix,sdk_design,sdk_impl,audit,audit_fix,verify}.md`.

## Phase 28E-2 — Cluster shape falsification + opening diversity (KEEP commits, no .bestref advance)

2026-05-25. Two stages: Stage 0 (opening diversity library, 20 HeXOpedia
§6 openings, pair-seeded harness wiring), Stage 1 (rhombus cluster
detector + 3-sweep arc Step 0 → 1 → 3). Stages 2 (bone) + 3
(arch/trapezoid) SKIPPED per user direction after Stage 1 NO-LAND × 3
sweeps falsified the cluster-detector lever a-priori for all
cluster shapes. 4 code/spec commits on master + 1 on hexo-arena main +
2 doc this retro.

**Outcome: KEEP all 4 commits on master; `.bestref` NOT advanced.**
NO weight applied (rhombus detector dormant at default `rhombus = 0`);
no engine behavior change → no arena gate run. E-0 VERIFY baseline (HH
2.0% per-stone 500ms vs SB-perf) stands unchanged. `.bestref` stays at
`5bd89648`.

### Per-stage summary

| Stage | Commits | Outcome | Notes |
|---|---|---|---|
| Stage 0 — opening diversity | `a1245a1` (HH) + `d6b91ba` (arena) | PASS-WITH-MINOR (S0-REV) | 20 HeXOpedia §6 openings, pair-seeded via `pick_opening(i // 2)`. Two `NotImplementedError` raises in `promote.py:372-376`, `:553-557` DELETED. 29 new HH tests + 9 new arena tests. Smoke: 15 distinct game lengths in 20-game self-play. Arena adapter exposes `--opening hh:curated`. Measurement infrastructure, NOT strength. |
| Stage 1 — rhombus detector | `042f020` (detector + 13 tests, weight=0 default) + `6d57f8e` (`--diversity` flag on tune-sweep) + `5295561` (Step 3 cube-round centroid + 2 new tests, 15 total) | NO-LAND × 3 sweeps | Distance-multiset `{1,1,1,1,1,2}` detector, all 12 orientations covered; Ring-C isolation via centroid. Step 0 V-pattern → Step 1 negative-weight (refutes double-count) → Step 3 cube-round (refutes isolation geometry). Mechanism: per-axis S1 sum + Layer-1 bucket already implicitly evaluate cluster shapes; any explicit weight at any sign with any isolation algorithm disrupts eval balance. |
| Stages 2 + 3 — bone / arch / trapezoid | (no commits) | SKIPPED | A-priori generalization: bone / arch / trapezoid share per-axis decomposability mechanism with rhombus; same negative result expected. Saves 4-6 days arena time. |

### Mechanism falsification arc (Stage 1)

Three sweeps × 5 cells = 15 cells total, 200g/cell, 500ms/stone, 10
workers, `opening_diversity=ON`. None point-positive with CI upper > 0.

| Step | Centroid algo | Grid | Outcome | Mechanism refuted |
|---|---|---|---|---|
| Step 0 | vertex (per-component `round_div4`) | akra-anchored 22500-90000 | V-pattern, anchor 45000 flat at 0.0, two cells stat-sig negative | (initial sweep — no mechanism refuted, three hypotheses surfaced) |
| Step 1 | vertex | neg-pos -22500 to +22500 | symmetric-negative around 0; negative weights WORSE than positive | **Double-count REFUTED as primary mechanism** (canonical rhombus structurally generates 5 open_2 firings = 56250 Elo before weight per S1-REV Check 3, but negative weight doesn't recover) |
| Step 3 | cube-round (Red Blob Games) | akra-anchored 22500-90000 | monotonic-negative, WORSE on avg vs Step 0 | **Isolation geometry REFUTED** (more theoretically-faithful centroid did NOT flip any cell positive) |

### Commits (4 atomic on hammerhead master + 2 doc this retro)

| SHA | Subject |
|---|---|
| `a1245a1` | `harness: implement opening diversity library` |
| `042f020` | `eval: implement rhombus detection with isolation` |
| `6d57f8e` | `tune: add --diversity flag to tune-sweep` |
| `5295561` | `eval: cube-round centroid for rhombus isolation` |
| (prev) | `bench: HOTSPOTS Phase 28E-2 cluster falsification + diversity` |
| (this commit) | `spec: mark Phase 28E-2 done in roadmap` |

Plus 1 on hexo-arena main: `d6b91ba` (`adapter: consume opening
diversity from HH harness`).

### Honest assessment

External arena DID NOT MOVE (no weight applied → no behavior change).
Phase 28E-2 DID empirically falsify Path 3 (cluster detector revival)
as a standalone Elo-positive lever via 3 cleanly-diagnosed sweeps. That
is real progress — Path 3 was the largest unmeasured arm in E-1 SYN's
decision matrix; falsifying it sharpens E-3's path-2B-or-Texel choice.
Opening diversity library is real infrastructure (DIAG-1 fixed-depth
determinism collapse can no longer re-trip with diversity ON). Rhombus
detector code (~430 LOC + 15 tests) is preserved in repo for any future
revisit if eval is restructured to subtract per-axis S1 from
cluster-positions. SPEC_EVAL NOT updated this phase (detector landed but
weight did NOT — eval architecture's behavior is unchanged from a
caller's perspective).

Full retrospective: `/tmp/phase_28e/PHASE_28E_2_RETRO.md` (gitignored
per Phase 25.5 repo hygiene). Per-stage reports under
`/tmp/phase_28e/2/{stage-0,stage-1}/{implementer,review}.md`. Sweep
outputs at `benches/results/tune/rhombus/B/{20260524T210750,20260524T225139,20260525T013817}/*.json`.

## Phase 28F-3.1 — Qsearch cap raise (REVERT, .bestref unchanged)

2026-05-25. Single-constant experiment to test the dispatcher's
sub-phase 0 finding: 92.9% of cluster decisive positions hit HH's
qsearch cap of 8. Cap-raise to 16 brings parity with SB-perf's
`MAX_QDEPTH = 16` (`engine/constants.h:50`). Tried cap=16, then
cap=12 fallback.

**Outcome: REVERT. `.bestref` unchanged at `5bd89648`.** Hypothesis
fully falsified.

### Results

| Variant | Internal vs cap=8 (200g, 500ms) | External vs SB-perf (200g, 500ms) |
|---|---|---|
| cap=16 | −63.2 Elo (CI −112 to −14) | (not run; internal failed gate) |
| cap=12 | −20.9 Elo (CI −69 to +27) | 0/200 (0.0%, baseline 2–5%) |

### Why the smoking gun misled us

The 92.9% cap-hit rate measured **cap reach**, not **truncated
useful resolution**. Many cap hits occur on lines already resolved
at stand-pat, or where further extension would not flip the score.
Raising the cap pays the cost on all of these without proportional
signal gain.

`bench-quick` on quiet fixture `midgame_12` was deceptive: NPS
unchanged, qsearch_max_depth utilized, qsearch_nodes_mean nearly
flat. Real games hit forcing positions where qsearch DOES expand
much more under cap=12/16, propagating cost across main search
and reducing effective depth.

### Cross-cut: SB-perf parity does not transfer

SB-perf cap=16 works for SB-perf because of properties HH lacks
(TT/ordering/eval-stability at depth). Matching the constant
without matching the surrounding design is net-negative.

### Commits

| SHA | Subject |
|---|---|
| `99afbeb` | `qsearch: raise max_plies cap from 8 to 16` (kept for history) |
| `2a9bc84` | `qsearch: try cap=12 after cap=16 lost -63 Elo internally` (kept for history) |
| `64c1e34` | `qsearch: revert cap to 8 — raise to 12/16 falsified (28F-3.1)` |
| `1d82e13` | `Revert "TEMP bench: consume 9-tuple from bench_best_move..."` |
| `9d48986` | `Revert "Reapply 'TEMP search: add rank + qsearch stats...'"` |

Bench/match raw outputs under `/tmp/phase_28f/3/1/` (gitignored).

### 28F-3.2 seed

Cluster bucket still search-bound (per Phase 28F-3 sub-phase 0).
Three branches in priority order:

1. **Diagnose why qsearch deeper-than-8 is *worse* in HH.** Does
   qsearch at depth 9–16 generate *wrong* stand-pat scores that
   propagate into main search? Temporary per-ply qsearch-result
   distribution vs shallow-search ground truth on cluster
   positions. Cheapest, no code change.
2. **Pivot to a different search-side lever.** Sub-phase 0 ruled
   out root ordering (100% rank 0). Next candidates: TT collision
   rate at depth, killer-move hit rate, PVS re-search rate.
3. **Selective extension on forced-only paths** (dispatcher option
   3). No global cap; extend unbounded where branching factor ≤ 2.
   Avoids global cost-blowup that killed cap=12/16. Most expensive,
   regression risk.

## Phase 28F-3.3 — Qsearch threat filter (PROMOTE on internal evidence)

2026-05-25. Sub-phase 0 diagnosis: qsearch BF averages 4.7 at all
plies in cluster decisive. `is_threat_move` filter too permissive —
`creates_s0` and non-immediate `blocks_opp_s0` drive BF without
resolving immediate threats. Three filter variants implemented behind
`qsearch_filter_mode` config: `current`, `resolution`, `urgent`.

**Outcome: PROMOTE on internal evidence. `.bestref` advanced to
the urgent-mode commit.**

### Results

| Variant | Internal vs `current` (direct) | NPS (cyc/node) | External vs SB-perf (400g) |
|---|---|---|---|
| `current` (baseline) | 0 by definition | 532k (7973) | 2.0% (100g baseline) |
| `resolution` | ~−21 Elo (triangulated, 200g) | 595k (6287) | (not run, weaker triangulation) |
| `urgent` (winner) | **+50.7 Elo, CI [+16.4, +85.1]** (400g direct) | **650k (6529)** | **4.5%, CI [2.9%, 7.0%]** |

External delta +2.5pp vs baseline; below strict +3pp gate but +CI
direction. PROMOTE chosen on the dominant internal signal.

### Methodology note: direct > triangulation

First-pass 200g vs old `.bestref` triangulated `urgent` vs `current`
at point estimate ≈−7 Elo. Direct 400g head-to-head: **+50.7 Elo**.
Triangulation via shared opponent accumulates independent noise;
direct match is canonical. Locked as a methodology rule.

### Cap-raise hypothesis closed

The "high BF caused the 28F-3.1 cap-raise failure" hypothesis is
falsified. Combo test (`urgent` + cap=12, 200g): −17.4 Elo vs
(`current` + cap=8). Inferring from the +50.7 baseline,
`urgent` + cap=12 vs `urgent` + cap=8 ≈ −68 Elo. cap=8 is a hard
ceiling for reasons unrelated to filter mode (likely speculative
extension past useful-knowledge horizon). `qsearch_max_plies = 8`
stays locked.

### Commits

| SHA | Subject |
|---|---|
| `958d27b` | `search: add qsearch_filter_mode config parameter` |
| `0aafe6f` | `search: implement resolution-only qsearch filter` |
| `368169e` | `search: implement urgent-only qsearch filter` |
| `28470ce` | `search: unit tests for qsearch filter modes` |
| `2af12ec` | `promote: advance .bestref to 28470ce (Phase 28F-3.3 baseline)` |
| `24c8c8d` | `search: set qsearch_filter_mode to urgent` |

Bench/match/arena raw outputs under `/tmp/phase_28f/3/3/` (gitignored).

## Phase 28F-3.4 — Qsearch TT probe + store (PROMOTE on internal evidence)

2026-05-25. Add TT probe + store inside `quiescence_node`, gated by
`engine.search.qsearch_tt_enabled` (default true). No eval changes.
Mirrors `pvs_node`'s TT pattern: probe BEFORE stand-pat, try
TT-move-first when it passes `is_threat_move`, store at function tail
AND on inline beta/alpha cutoffs (provided `searched_any_move`). All
qsearch entries use `depth = -1` so they cannot displace main-search
entries from the depth-preferred bucket.

**Outcome: PROMOTE on internal evidence. `.bestref` advanced to
`cfefb3b`.** Internal 200g @ 500ms: HH +33.1 Elo (109-90-1,
W/L margin monotonically non-decreasing through the run, Wilson95
[47.83%, 61.49%], SPRT inconclusive at 200g). External arena
200g vs SB-perf @ 500ms: 10/200 = 5.00% (Wilson95 [2.74%, 8.96%])
vs 28F-3.3 baseline 18/400 = 4.50% (Wilson95 [2.87%, 7.00%]) →
Δ +0.5pp, fails sticky +3pp gate.

Per dispatcher §5 STEP 5 decision matrix row 2 ("internal >= 0,
external < +3pp with positive trend → PROMOTE on internal evidence")
and project memory ("arena +3pp external gate is sticky"; "internal
positive → promote"): PROMOTE.

NPS A/B (tools/qsearch_tt_ab.sh, single-run bench-perf): midgame_12
essentially unchanged; midgame_30 shows cyc/node up 7-111% but depth
+1 at long budget (8 → 9). Trade-off matches internal Elo signal.

### Commits

| SHA | Subject |
|---|---|
| `5465f00` | `spec: document qsearch TT probe + store (28F-3.4)` |
| `7e302c4` | `config: add qsearch_tt_enabled flag (default true)` |
| `d9efcb8` | `search: thread &mut tt into quiescence_node` |
| `760375b` | `qsearch: TT store at tail when moves were searched` |
| `2606d2f` | `qsearch: TT probe at top with depth=-1 cutoffs` |
| `16ee043` | `bench: capture cyc/node delta with qsearch TT on vs off` |
| `cfefb3b` | `fix: correct .bestref SHA typo (dbd72037->dbd7203f)` |
| `34fec36` | `promote: advance .bestref to cfefb3b (Phase 28F-3.4)` |

Bench/match/arena raw outputs under `/tmp/phase_28f/3/4/` (gitignored).

## Phase 28E candidates (updated post-28E-2)

Phase 28E-0 closed: time-fix mechanism corrected, SDK observability
landed, audit clean, methodology recalibration documented. Phase 28E-1
closed as diagnostic synthesis (no production commits; surfaced Path
2B / Path 3 / Texel decision matrix). Phase 28E-2 closed: cluster
detector revival empirically falsified via 3 sweeps × 15 cells on
rhombus; opening diversity infrastructure landed; Stages 2-3 SKIPPED
a-priori.

### Phase 28E-3 candidates — priority ordering

- **28E-3 — Path 2B (SB-perf 729-table port) — PRIMARY**: E-1 SYN
  Section C ranked Path 2B as Arm B; Phase 28E-2 Stage 1 falsifies Arm A
  (Path 3 cluster detector revival). DIAG-4 + DIAG-5 established
  Path 2B is M effort (codegen-only swap on existing 6561-entry
  `WINDOW_SCORE_8`, 3-5 commits, 1-3 days). Runtime override path
  already plumbed via `EvalOverrides::build_window_score_8`.
  **Honest caveat**: DIAG-2 §"Implication" + DIAG-4 §"Per-axis tables"
  flag that per-axis 729-table cannot architecturally fix the
  load-bearing cluster gap; literal port would still see rhombus as 5
  independent per-axis open-2s (refined from DIAG-2's 3 to S1-REV
  Check 3's 5). Path 2B's expected gain is from denser per-pattern
  evaluation of LINEAR shapes (where HH is already at 100% per DIAG-2),
  NOT cluster recovery. Honest expectation: Path 2B may move external
  winrate via linear-eval density refinement but will NOT close the
  cluster gap DIAG-2 highlighted.

- **28E-3+ — Texel pipeline (SECONDARY, contingent on Path 2B
  failure)**: L effort per DIAG-5 (8+ commits, 1-2 weeks, mostly infra).
  ML-trained eval is the remaining lever if Path 2B + cluster detector
  revival both fail to move arena. Deferred per E-1 SYN unless Path 2B
  prototype shows enough gain to justify chasing the optimum, OR Path 2B
  fails and Texel becomes the sole architectural option.

### Phase 28E residual carry-forward (across phases)

- **Tempo proxy investigation**: carried 28B → 28C → 28D-1 → 28D-3 →
  28E-0 → 28E-2. Detector revival or proxy invention. TT p.11 "tempo is
  the most important currency" — strongest PDF evidence of any
  deferred item. Structurally different from value tuning.
- **Promote-harness commit-bug fix**: trivial reorder `-m <msg> --only
  -- <path>` in `promote.py` auto-commit branch. Bundle with next
  phase that touches `promote.py`. Carried from Phase 28D-1.
- **Per-turn-joint vs per-stone-split scheduling A/B**: re-scoped from
  defunct `stone1_fraction` A/B (the split fraction concept no longer
  applies post-TIME-FIX — `stone1_time_pct` is removed). SB-perf plans
  whole turns jointly in a single `MinimaxBot.get_move` call; HH plans
  per-stone. Investigate whether HH benefits from a per-turn-joint
  scheduling mode (sharing TT state + time budget across two stones in
  one call) for arena gate symmetry with SB.
- **Phase 28E-2 Stage 0 S0-REV MINOR items**: (a) `openings.py:308`
  docstring "Three" → "Two" 5-char fix; (b) arena `bots/external/
  INTEGRATION_NOTES.md § 1. Hammerhead` lacks `--opening hh:curated`
  operator-doc line; (c) optional Shotgun / Revolver BKE→axial fidelity
  to HeXOpedia §6.2 with full mapped table.
- **Triangle detection — NOT REVISITED**: per Phase 28E-2 scope close.
  Cluster-detector mechanism falsification (3 sweeps, 15 cells, NO
  positive cell across vertex/cube-round + positive/negative weight
  space) argues against per-shape Layer-2 revival generally; triangle
  would face the same mechanism (decomposable into per-axis sums via
  existing S1 detectors).
- **Eval architecture restructure (long-form, contingent on Path 2B
  failure)**: load-bearing finding from Phase 28E-2 Stage 1 is that
  per-axis S1 + Layer-1 already implicitly evaluate cluster shapes.
  Options: (a) DISABLE per-axis S1 from cluster-positions (runtime
  per-position S1-suppression — complex); (b) REPLACE per-axis S1 with
  per-pattern table (≈ Path 2B). Path 2B is cheaper. Open question for
  E-3+: if Path 2B fails to move arena, the cluster gap may be
  untouchable without ML-trained eval (Texel pipeline).

## Phase 28D-2+ candidates (legacy, superseded by 28E above)

Carried forward from Phase 28C retrospective + Phase 28D-1
Outcome C dispatch column. Ordering not yet locked; dispatcher
subagent sequences per host budget + risk.

- **28D-2-A — BO sprint v2 vs new `.bestref` (`5bd89648`)**:
  400g/trial (or averaged-200g) with widened bounds on 4 of 5
  dims (`open_4 > 240k`, `closed_5 < 240k`,
  `window_k_scores[5] < 1024`, `open_extension_factor = 0`
  unprobed). Optionally warm-start from C1 instead of HEAD-seed
  (HEAD-seed at trial #0 in C2 produced the worst result, -41.89
  Elo). Resumable study at `/tmp/phase_28c/2/study.db` — Optuna
  can extend with new sampler / bounds. Convergence early-stop
  (design.md §3 rule) cheap to wire.
- **28D-2-B — Opening-diversity library + harness wiring**:
  now relevant for A/B vs new `.bestref`. Deferred B-1.3 + B-1.4
  from Phase 28B C-DEFERRED. Replace the `NotImplementedError`
  in `run_match` / `run_match_parallel`, add `opening_moves`
  field to `GameConfig`, round-robin in `build_game_configs`,
  opening-replay loop in `play_one_game` after `reset`. Append
  12-20 HeXO opening positions to `benches/fixtures/positions.json`
  tagged `screen` / `validate`. Pure Python, ~150 LOC + ~10-20
  fixtures, bench-quick must be NPS-neutral. Unblocks all future
  diversity A/Bs.
- **28D-2-C — Tempo proxy investigation**: deferred from Phase
  28B retro (I1 § 3 cited TT p. 11 "tempo is the most important
  currency" as strongest PDF evidence for any deferred item).
  Structurally different from value tuning — requires detector
  revival or proxy invention.
- **28D-2-D — External arena (SealBot)**: PRIORITY per Outcome C
  dispatch column. Cross-engine independent signal confirms
  cumulative work is real strength, not within-engine harness
  artifact.
- **28D-2-E — Promote-harness commit-bug fix**: high-priority
  follow-up logged from Phase 28D-1 D1-RUN. Trivial fix in
  `hammerhead/hammerhead/promote.py` (reorder `-m` before `--`).
  Bundle with `SPEC_BENCHMARKS.md § [promote]` section addition.
- **NOT NEEDED**: 1600g promote-match (Outcome C already cleared
  at 800g). **DEFER**: search-side tuning revival (harness floor
  unchanged by new `.bestref`).

## Phase 26 candidates (deferred follow-ups)

Carried forward — items still open after Phase 25.

- **Per-line `LineContribution` cache** (Phase 24 candidate #2):
  **🚧 promoted to Phase 27 (in progress).** Layer 1 (~31 % of
  engine, ~27 % cacheable per HOTSPOTS Phase 26.5 / I-HOTPATH) re-
  scans every populated line on every leaf eval. Cache
  per-`(axis, line_id)` Layer-1 contribution on `Board` (`Box<[i32]>`
  of `3 * LINE_ID_RANGE`, sentinel-marked dirty via `i32::MIN`),
  invalidate the ≤3 lines a placed stone touches. Expected NPS gain
  +10–15 % real (Amdahl unconstrained +24–28 %). See
  `specs/SPEC_EVAL.md § LineContribution Cache` and
  `specs/SPEC_ENGINE.md § LineContribution Cache on Board`.
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
- **`would_make_six` / `creates_s0` run-scan cost** (Phase 24
  candidate #1, Phase 25 STEP 1.1 — attempted, reverted): the ordering
  predicates are ~20 % of engine self-time. Phase 25's bit-parallel
  `u64` run scan + line-lookup cache **regressed −15/−16 % NPS** —
  the existing unrolled `get()` loop is faster. Phase 25.5 R-02
  attempts the orthogonal *structural-fusion* lever: collapse the
  three independent passes (`would_make_six(side)`,
  `would_make_six(opp)`, `creates_s0(side)`) inside `bucket_value`
  into a single 3-axis fused probe (`AxisProbe`). Behaviour-identical
  refactor — reference node counts preserved. Algorithmic rewrite of
  `run_*` itself remains open (different lever).
- **`threats::compute` run-scan / dedup cost** (Phase 24 candidate #3,
  Phase 25 STEP 1.2 — attempted, reverted): per-player piece iteration
  was flat — the cost is the `walk_linear_runs` / `run_endpoints`
  linear scan, not the history filter. Still open; needs the run-scan
  itself addressed.
- **`for_each_in_range` proximity walk** (Phase 24 candidate #4,
  Phase 25 STEP 1.3 — attempted, reverted): offset tables regressed
  −10/−11 %. The proximity walk is ~18 % of engine but the coord
  derivation is not the cost — the flat-array refcount stores are.
  Still open; a different angle (the search-internal proximity-skip
  above) is the live candidate.
- **Move-ordering bucket-quality refinement** — a strength change
  (reshapes the tree, changes node counts) — for a strength-focused
  phase, `make vs`-gated; not a perf candidate.
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
  (Phase 14 HOTSPOTS #4): the original Phase 15 follow-up text
  claimed an `axis_run_cache` in `OrderingContext`; that cache was
  reverted and never landed. The actual resolution is Phase 25.5 R-02
  — the `AxisProbe` helper fuses the three independent axis passes
  (`would_make_six(side)`, `would_make_six(opp)`, `creates_s0(side)`)
  inside `bucket_value` into one 3-axis loop, halving `line()` slot
  loads and tripling-down on `run_*` reuse per move.

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
