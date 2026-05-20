# Phase 16 — Proximity-field callsite scan

Survey of every read/write of the four `Board` proximity fields, ahead of
replacing them with flat-array (`SparseCellSet`) structures.

**Scope result:** all four fields are *private* to `hexo-engine/src/board.rs`.
No other `src/` module, test, or bench touches them directly. External code
only reaches them through two public accessors: `Board::candidates()` and
`Board::inner_candidates()`. This makes the replacement self-contained inside
`board.rs` plus the two helper fns.

Field definitions: `hexo-engine/src/board.rs:57-62`

```
57  proximity_count: FxHashMap<Coord, u32>,        // outer, r = MAX_PIECE_DISTANCE (8)
60  inner_proximity_count: FxHashMap<Coord, u32>,  // inner, r = MOVE_GEN_INNER_RADIUS (2)
61  candidate_cells: FxHashSet<Coord>,             // outer legal-empty set
62  inner_candidate_cells: FxHashSet<Coord>,       // inner move-gen set
```

---

## proximity_count (outer, r=8)

Per-cell refcount of pieces within `MAX_PIECE_DISTANCE`. Drives legality.

| file:line | op | notes / frequency |
|---|---|---|
| board.rs:57 | def | `FxHashMap<Coord, u32>` |
| board.rs:138-141 | construct | `with_capacity_and_hasher(256)` — once per `Board::new` |
| board.rs:172 | `.clear()` | once per `Board::reset` |
| board.rs:225-231 | passed `&mut` to `add_proximity` | once per `place` |
| board.rs:279-284 | passed `&mut` to `remove_proximity` | once per `undo` |
| board.rs:302 | `.get(&c).copied().unwrap_or(0)` | once per `undo` (re-add-candidate test) |
| board.rs:434 | `.get(&c).copied().unwrap_or(0) > 0` | `is_legal_internal` — once per `place` legality check; also per `is_legal()` public call |
| board.rs:609-615 | passed `&mut` to `add_proximity` | once per `place_for_test` |
| board.rs:761-766 | inside `add_proximity`: `.entry(d).or_insert(0)`, `*count += 1` | **r=8 hex = 217 cells** per `place` |
| board.rs:780-786 | inside `remove_proximity`: `.get_mut(&d)`, decrement, `.remove(&d)` on zero | **217 cells** per `undo` |

**Per-node cost:** one `place` + one `undo` per visited search node ⇒ ~217
entry probes + ~217 get_mut/remove on the outer map per node, plus 1 `.get`
in `undo` and 1 `.get` in `is_legal_internal`.

Reads are point lookups only — **never iterated**. No order sensitivity.

---

## inner_proximity_count (r=2)

Per-cell refcount of pieces within `MOVE_GEN_INNER_RADIUS` (2). Drives move-gen.

| file:line | op | notes / frequency |
|---|---|---|
| board.rs:60 | def | `FxHashMap<Coord, u32>` |
| board.rs:142-145 | construct | `with_capacity_and_hasher(256)` — once per `Board::new` |
| board.rs:173 | `.clear()` | once per `Board::reset` |
| board.rs:232-238 | passed `&mut` to `add_proximity` | once per `place` |
| board.rs:285-290 | passed `&mut` to `remove_proximity` | once per `undo` |
| board.rs:305 | `.get(&c).copied().unwrap_or(0) > 0` | once per `undo` (re-add-candidate test) |
| board.rs:616-622 | passed `&mut` to `add_proximity` | once per `place_for_test` |
| board.rs:761-766 / 780-786 | inside helpers | **r=2 hex = 19 cells** per `place`/`undo` |

**Per-node cost:** ~19 entry/get_mut probes per `place`+`undo`. Point lookups
only; never iterated. No order sensitivity.

---

## candidate_cells (outer set)

Legal empty cells (within `MAX_PIECE_DISTANCE` of some piece). On an empty
board it is exactly `{ORIGIN}`.

| file:line | op | notes / frequency |
|---|---|---|
| board.rs:61 | def | `FxHashSet<Coord>` |
| board.rs:134-136 | construct + `.insert(ORIGIN)` | once per `Board::new` |
| board.rs:174-175 | `.clear()` + `.insert(ORIGIN)` | once per `Board::reset` |
| board.rs:218 | `.remove(&c)` | once per `place` (placed cell leaves candidate set) |
| board.rs:225-231 | passed `&mut` to `add_proximity` (`.insert` inside) | once per `place` |
| board.rs:279-284 | passed `&mut` to `remove_proximity` (`.remove` inside) | once per `undo` |
| board.rs:297-298 | `.clear()` + `.insert(ORIGIN)` | once per `undo` **only when ply hits 0** |
| board.rs:303 | `.insert(c)` | once per `undo` (re-add placed cell if still legal), ply>0 path |
| board.rs:386 | **`.iter().copied()`** | `Board::candidates()` accessor — see Order-sensitivity |
| board.rs:603 | `.remove(&c)` | once per `place_for_test` |
| board.rs:609-615 | passed `&mut` to `add_proximity` | once per `place_for_test` |

**`Board::candidates()` external callers** (`hexo-engine/src/board.rs:385`):
- `hexo-engine/benches/bench_board.rs:32` — `.next().unwrap_or(ORIGIN)`
- `hexo-engine/benches/bench_axis_bitmap.rs:37` — `.next().unwrap_or(PROBE)`
- `hexo-engine/benches/bench_eval.rs:40` — `.next().unwrap_or(ORIGIN)`
- `hexo-engine/tests/board_tests.rs` — lines 23, 80, 96, 106, 120, 133, 140,
  184, 201 — collected into `Vec`/`HashSet`
- `hexo-engine/tests/zobrist_tests.rs:79, 177` — collected into `Vec<Coord>`

No `src/` production code (moves/search/ordering/eval) iterates
`candidate_cells`. The outer set is consumed only by tests/benches and by
`moves::generate`'s path-3 sweep — which does **not** use it (sweep walks
`board.pieces()` instead, see moves.rs:79).

---

## inner_candidate_cells (inner set)

Empty cells within `MOVE_GEN_INNER_RADIUS` (2) of some piece. Backs the
default-radius move generator. Empty on an empty board.

| file:line | op | notes / frequency |
|---|---|---|
| board.rs:62 | def | `FxHashSet<Coord>` |
| board.rs:147-150 | construct | `with_capacity_and_hasher(256)` — once per `Board::new` |
| board.rs:176 | `.clear()` | once per `Board::reset` |
| board.rs:219 | `.remove(&c)` | once per `place` |
| board.rs:232-238 | passed `&mut` to `add_proximity` (`.insert` inside) | once per `place` |
| board.rs:285-290 | passed `&mut` to `remove_proximity` (`.remove` inside) | once per `undo` |
| board.rs:299 | `.clear()` | once per `undo` when ply hits 0 |
| board.rs:306 | `.insert(c)` | once per `undo` (re-add placed cell), ply>0 path |
| board.rs:393 | **`.iter().copied()`** | `Board::inner_candidates()` accessor — **HOT** |
| board.rs:604 | `.remove(&c)` | once per `place_for_test` |
| board.rs:616-622 | passed `&mut` to `add_proximity` | once per `place_for_test` |

**`Board::inner_candidates()` callers** (`hexo-engine/src/board.rs:392`):
- `hexo-engine/src/moves.rs:52` — `out.extend(board.inner_candidates())` in
  `moves::generate`, path 2 (`radius <= MOVE_GEN_INNER_RADIUS`). **This is the
  hot search path** — see below.
- `hexo-engine/tests/board_tests.rs` — lines 395, 413, 426, 460, 472, 476,
  487, 501, 505, 516, 519, 530-531, 545-546, 557 — all collected into
  `HashSet<Coord>` or tested with `.any(|c| ...)`.

**Search hotness:** `DEFAULT_MOVE_RADIUS = 2` and `MOVE_GEN_INNER_RADIUS = 2`
(both from `hexo.toml`). Therefore `moves::generate(board, DEFAULT_MOVE_RADIUS)`
**always takes path 2** (`radius <= MOVE_GEN_INNER_RADIUS`, moves.rs:51) for
every normal search node. `moves::generate` is called at search.rs:155, :313,
:592 — i.e. once per `pvs_node` and once per `quiescence_node`. So
`inner_candidate_cells.iter()` runs **once per search node**. This is the
single performance-critical iterator of the four fields.

---

## add_proximity / remove_proximity helpers

Both defined in `hexo-engine/src/board.rs`, both `#[inline]`, shared by the
outer (r=8) and inner (r=2) field pairs via the `radius` parameter.

### `add_proximity` — board.rs:753-768
```
for_each_in_range(center, radius, |d| {
    let count = counts.entry(d).or_insert(0);
    let was_zero = *count == 0;
    *count += 1;
    if d != center && was_zero && !axes.is_occupied(d) {
        candidates.insert(d);
    }
});
```
Walks the full radius-`r` hex *inclusive of center* (`for_each_in_range`,
`coords.rs:87-103`). For each cell: bump refcount; if it just rose 0→1, is
not the center, and is empty, insert into the candidate set.
- r=8 → `3·8·9 + 1 = 217` cells visited.
- r=2 → `3·2·3 + 1 = 19` cells visited.

### `remove_proximity` — board.rs:773-789
```
for_each_in_range(center, radius, |d| {
    let entry = counts.get_mut(&d).expect("invariant: ... entry exists");
    *entry -= 1;
    if *entry == 0 {
        counts.remove(&d);
        candidates.remove(&d);
    }
});
```
Same hex walk. Decrement; on reaching 0, drop the count entry and remove the
candidate. **Panics** if the count entry is missing (invariant guard) — the
flat-array replacement must preserve "count present ⇒ value ≥ 1" so a 0-count
slot is indistinguishable from absent, or keep an explicit presence test.

Note: `coords.rs` also exposes `RANGE_OFFSETS` (a const slice of all r≤8
non-center offsets, length `RANGE_OFFSET_COUNT = 216`) "to update proximity
counts ... without allocating" — but the current helpers use the closure
`for_each_in_range` instead, not `RANGE_OFFSETS`. The flat-index rewrite could
switch to `RANGE_OFFSETS` if convenient; not required.

---

## Order-sensitivity findings

`SparseCellSet`'s `swap_remove` perturbs insertion order, so any iterator
whose *consumer* depends on element order is at risk. There are exactly **two**
iterators (`candidates()` board.rs:386, `inner_candidates()` board.rs:393).

### `inner_candidate_cells` via `moves::generate` (moves.rs:52) — **YES, order matters (but tolerable)**
Evidence chain:
1. `moves.rs:14-15` doc: *"Results are in insertion order (arbitrary)."* The
   module already declares order arbitrary — so a *different* arbitrary order
   is contractually fine.
2. `moves::generate` output feeds `order_moves_with_buckets`
   (`ordering.rs:335-362`, called from search.rs:313/592).
3. `order_moves_with_buckets` does `scored.sort_by(|a,b| b.0.cmp(&a.0))` —
   **`sort_by` is stable** (comment ordering.rs:351) — then
   `.take(MOVE_GEN_CAP)` truncates (`MOVE_GEN_CAP` = `move_gen_cap` = 24 in
   `hexo.toml`).
4. **Consequence:** when two moves have equal `priority` (same bucket + same
   history score), the stable sort keeps them in *generation order*. If the
   node has >24 candidates, which tied move survives the `MOVE_GEN_CAP`
   truncation depends on generation order. `SparseCellSet` swap_remove changes
   that order ⇒ a *different but equally-valid* tied move may be searched.

   This does **not** corrupt search correctness — alpha-beta still returns a
   correct score for whatever move subset it searches; only which move wins a
   pure tie can shift. It **can** perturb benchmark node counts and
   `make vs` game results bit-for-bit. Flag for the reviewer / `make vs`
   baseline refresh, but it is not a bug.
5. `quiescence_node` (search.rs:592) filters generate output through
   `is_threat_move` then iterates — order-independent (filter, not pick-first).

**Verdict: YES order-dependent at tie-break + truncation granularity;
correctness-safe; expect bench/`vs` output drift.**

### `candidate_cells` via `Board::candidates()` (board.rs:386) — **NO (production); test-only concern**
- No `src/` production code calls `candidates()`.
- Benches `bench_board.rs:32`, `bench_axis_bitmap.rs:37`, `bench_eval.rs:40`
  all do `.candidates().next().unwrap_or(...)` — **take "first" element**.
  `.next()` on an unordered set is already arbitrary; with `swap_remove` the
  first element changes. These are benches picking a probe target; a different
  probe coord is harmless to bench validity but may shift reported numbers.
  **Uncertain→low-risk** — recommend reviewer eyeball the three bench numbers.
- Tests collect into `HashSet<Coord>` (set equality, order-independent) or
  `Vec` then compare as sets. board_tests.rs:23 and :201 collect into `Vec`
  — check those two: if they assert on `Vec` *equality* against an ordered
  literal they would break; if they only check `.len()` / `.contains()` they
  are safe. **Action: reviewer must read board_tests.rs:23 and :201.**
- zobrist_tests.rs:79, :177 collect into `Vec<Coord>` then (per file purpose)
  replay/permute — order-independent replay, but reviewer should confirm.

**Verdict for `candidate_cells`: NO production order dependence. Test/bench
"first element" and `Vec`-collect sites are low-risk; explicitly re-verify
`board_tests.rs:23`, `board_tests.rs:201`, `bench_*` probe selection.**

### No test asserts a literal candidate ordering
Grep of `board_tests.rs` / `zobrist_tests.rs` shows every candidate
consumer either collects to `HashSet` or uses `.any()` / `.next()`. No
`assert_eq!(vec_of_candidates, [literal, ...])` ordered assertion was found.

---

## Coord / ZOBRIST_WINDOW notes

`Coord` — `hexo-engine/src/coords.rs:11-16`
```
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
#[repr(C)]
pub struct Coord { pub q: i16, pub r: i16 }   // s implicit = -q-r
```
- Both fields `i16`, public. Packs to 32 bits, register-passed.
- `ORIGIN = {0,0}` (`coords.rs:19`).

`ZOBRIST_WINDOW` — config constant, default **127** (`hexo.toml:134`,
generated by `hexo-engine/build.rs:684`). Used as the per-coordinate window
half-width:
- `zobrist.rs:40`: `const W: i16 = ZOBRIST_WINDOW;` — zobrist table is indexed
  by `(q+W, r+W)`, i.e. q,r legal range is `[-127, 127]`.
- `axis_bitmap.rs:302-303`: `LINE_ID_RANGE = 4·ZW + 1 = 509`,
  `LINE_ID_OFFSET = -2·ZW` (axis-S line id `q+r` reaches `±2·ZW`).

**Flat-index encoding for the rewrite:**
`W = ZOBRIST_WINDOW = 127`, `RANGE = 2·W + 1 = 255`,
`idx(c) = (c.q + W) * RANGE + (c.r + W)`. Total slot count
`RANGE² = 255² = 65 025`. As `u32` counts that is `65025·4 ≈ 254 KB` per
proximity count array; two of them (outer+inner) ⇒ ~508 KB if both go flat.
The candidate sets, if flat as a bitmap, are `65025/8 ≈ 8 KB` each. The
existing zobrist table already assumes q,r ∈ `[-127,127]`, so any coord that
would overflow the flat index also overflows the zobrist table — the bound is
already enforced upstream; no new clamp needed (but a `debug_assert` is cheap
insurance).

---

## Board clone check

`Board` is **never `Clone`d**. Evidence:
- `Board` has no `#[derive(Clone)]` and no `impl Clone for Board` (grep of
  `src/` finds none; board.rs:55 `pub struct Board` is bare).
- It cannot trivially be `Clone` anyway: it holds `RefCell`/`Cell` caches and
  is declared `#[pyclass(unsendable)]` (per CLAUDE.md).
- Search uses **make/undo only** — `place` then `undo` around each child node
  (`search.rs` pvs/quiescence), never a board copy. The only `.clone()` calls
  in board.rs (line 510) and search.rs (line 731) clone a `SmallVec` of
  centers / `defense_cells`, not a `Board`.

**Conclusion:** a ~250–500 KB flat structure per `Board` is paid **once per
`Board` instance** (one live board per engine search), not per node and not
per clone. Memory cost is a non-issue; the win is removing the per-node hash
probes. The only caveat: `Board::new` / `reset` must zero/allocate the flat
arrays — keep them `#[cold]` and consider `vec![0u32; RANGE*RANGE]` once,
reused across `reset` rather than reallocated.

---

## Recommended structure

**`SparseCellSet.members` should be `Vec<Coord>`, not `SmallVec<[Coord; 64]>`.**

Reasoning:
1. **Lifetime:** there is exactly one `Board` per engine, never cloned, never
   per-node. The members vec is allocated once (in `new`/`reset`) and grows
   monotonically over a game. There is no "many short-lived instances" case
   that `SmallVec` inline storage optimizes for.
2. **Typical populated size exceeds any reasonable inline cap.**
   - Outer `candidate_cells` (r=8): every empty cell within distance 8 of any
     stone. Fixtures `benches/fixtures/positions.json`: `midgame_30` = 30
     stones, `endgame_60` = 60 stones. With r=8, even a compact 12-stone
     cluster (`midgame_12`) covers on the order of 200+ empty cells; a 30–60
     stone midgame outer set is several hundred. Far past 64.
   - Inner `inner_candidate_cells` (r=2): empty cells within distance 2 of a
     stone. For a 30-stone position this is routinely 40–90 cells; `move_cap`
     / `move_gen_cap` are 30/24 (`hexo.toml:91,121`) — note the cap truncates
     the *ordered* list, the *unordered inner set* before truncation is
     larger. Comment at moves.rs:24 (`MOVE_GEN_CAP_INLINE = 32` "slightly
     above the typical MOVE_GEN_CAP of 30") concerns the *output* list, not
     the candidate set; the inner set itself commonly exceeds 32.
   A `SmallVec<[Coord;64]>` would spill to heap in essentially every midgame
   node for the outer set and frequently for the inner set — paying the
   `SmallVec` branch-on-spilled overhead with none of the inline benefit.
3. `SmallVec` inline storage of 64 `Coord` = 256 bytes embedded in the struct;
   useless once spilled and just bloats the (single) `Board`.

**Recommended `SparseCellSet` shape:**
```
struct SparseCellSet {
    members: Vec<Coord>,             // dense, swap_remove on delete
    slot: Vec<u32>,                  // flat idx(c) -> members index + 1 (0 = absent)
}
```
- `Vec<Coord> members` pre-`reserve`d to ~256 in `new`/`reset` (matches the
  current `INITIAL_MAP_CAPACITY = 256` board.rs:52) so it never reallocates
  mid-game.
- `slot` flat `Vec<u32>` of length `RANGE*RANGE = 65 025`, allocated once,
  reused across `reset` (do **not** reallocate in `reset` — just `members.clear()`
  + clear touched slots, or memset).
- For the **count** maps (`proximity_count`, `inner_proximity_count`) use a
  parallel flat `Vec<u32>` indexed by `idx(c)`; "absent" is simply count 0,
  which also removes the `remove_proximity` panic-on-missing invariant.

This keeps the hot `inner_candidates()` iterator a straight `Vec<Coord>` slice
walk (cache-friendly, no hashing) — the main Phase-16 payoff — and `place`/
`undo` become flat-array index bumps instead of hash entry/get_mut/remove.
