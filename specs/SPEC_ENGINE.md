# Hammerhead Engine — Internals Spec

## Tuning constants

All numeric tuning lives in `hexo.toml` (workspace root) and is exposed via
`crate::config::*`. See [SPEC_CONFIG](SPEC_CONFIG.md). When a value appears in
this spec (e.g. radius 2, time check every 4096 nodes), the canonical source is
`hexo.toml` — do not duplicate.

## Coordinates (`coords.rs`)

Axial coords. Drop `s` (implicit: `s = -q - r`).

```rust
pub struct Coord { pub q: i16, pub r: i16 }

pub const AXIS_Q: (i16, i16) = (1, 0);   // horizontal
pub const AXIS_R: (i16, i16) = (0, 1);   // diag 1
pub const AXIS_S: (i16, i16) = (1, -1);  // diag 2

pub const AXES: [(i16, i16); 3] = [AXIS_Q, AXIS_R, AXIS_S];

pub fn hex_distance(a: Coord, b: Coord) -> i16 {
    let dq = a.q - b.q;
    let dr = a.r - b.r;
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}
```

## Board (`board.rs`)

```rust
pub struct Board {
    proximity: ProximityCounts,        // flat r8 (legality) + r2 (move-gen) counts
    candidates: SparseCellSet,         // r8 legal empty cells
    inner_candidates: SparseCellSet,   // r2 move-gen cells
    history: Vec<Coord>,
    hash: u128,             // 128-bit Zobrist; TT bucket = (hash as u64) & MASK
    ply: u32,
    zobrist: ZobristTable,
    axes: AxisBitmaps,
    winner: Option<Player>,
    line_contrib: RefCell<Box<[i32]>>, // Phase 27: per-(axis,line_id) Layer-1
                                       // contribution cache, sentinel-marked
                                       // dirty (i32::MIN)
}

#[repr(u8)]
pub enum Player { X = 0, O = 1 }
```

Board does **not** store a `pieces` map (removed in Phase 13 — see Phase 12
flamegraph evidence). `piece_at(coord)` is served by probing the axis
bitmaps: first the unified Q-axis occupancy bitmap short-circuits the
empty case in one probe, then a single per-player Q-axis probe
disambiguates. Single-axis probe is sufficient because every placed
stone is registered in **all three** axis bitmaps; the Q-axis is chosen
by convention. `is_empty_cell` is the negation of `AxisBitmaps::is_occupied`
(one bitmap probe). `piece_count` returns `history.len()`.

`pieces()` iteration walks the move-history `Vec` in insertion order.
The player for each entry comes from a parallel `history_players:
Vec<Player>` field, **not** from `player_at_ply(idx)`: the test-only
`Board::place_for_test` allows placing for an arbitrary player
regardless of parity, so deriving the player from `player_at_ply`
would round-trip the wrong player through `undo`. `history_players[i]`
is the player actually placed at `history[i]`, kept in lockstep by
`place` / `place_for_test` / `undo`.

Callers that depended on the prior `FxHashMap` randomized order must
either tolerate either order or sort explicitly; Phase 13 verified all
existing callers are order-insensitive (see
`subagents/scans/phase13-piece-at-callsites.md`).

Two proximity refcounts are maintained in parallel. The outer (`r8`) one
defines legality. The inner (`r2`, value from `MOVE_GEN_INNER_RADIUS`) backs
move generation at default search radius without a per-node scan over
every legal cell.

Accessors:

```rust
#[inline] pub fn axes(&self) -> &AxisBitmaps;
#[inline] pub fn winner(&self) -> Option<Player>;
```

`winner` is set by `place` when the just-placed move makes a 6-in-row, and
cleared by `undo` whenever the undone move was the winning one. See "Win
Detection" below.

### Parity rules

- ply 0 → X plays (single stone, first move only)
- ply 1, 2 → O plays
- ply 3, 4 → X plays
- ply 5, 6 → O plays
- general: `player_at_ply(p) = if p == 0 { X } else { if ((p-1) / 2) % 2 == 0 { O } else { X } }`

### Operations

```rust
fn place(&mut self, c: Coord) -> Result<()>;   // updates hash, candidates, history
fn undo(&mut self) -> Result<()>;              // pops last
fn to_move(&self) -> Player;
fn is_legal(&self, c: Coord) -> bool;
fn is_empty_cell(&self, c: Coord) -> bool;
fn hash(&self) -> u128;
```

### Legality

A cell `c` is legal iff:

- It is empty (`is_empty_cell(c)` — bitmap probe, no `pieces` map), AND
- One of:
  - `ply == 0` and `c == (0, 0)` (forced first move at origin), OR
  - `ply >= 1` and `min(hex_dist(c, p) for p in pieces) <= MAX_PIECE_DISTANCE`
    (default 8, from `hexo.toml`).

Framing: the legal region is the **union of `r8` hexes** centred on each
existing piece. Placing a new piece at `c` extends the region by the `r8` hex
around `c`. Example: with stones at (0,0) and (8,0), legal cells span up to
(16,0).

### Candidate maintenance

Proximity storage uses a **bounded-key flat structure** derived from the
zobrist window (Phase 16 — replaced the `FxHashMap` / `FxHashSet`
quartet). The Phase 15 flamegraph showed
`for_each_in_range<board::add_proximity / remove_proximity>` at the #2
user-space position: each `place` walks the r=8 neighbourhood (~217
cells) and probed hashbrown 4× (outer count, inner count, outer set,
inner set). Flat arrays cut the per-cell cost from ~4 probes to ~4
array indexes.

The structures live in `src/proximity.rs`:

```
PROX_HALF        = ZOBRIST_WINDOW + MAX_PIECE_DISTANCE   // 135 at W=127
PROX_COORD_RANGE = 2 * PROX_HALF + 1                     // 271
PROX_FIELD_SIZE  = PROX_COORD_RANGE * PROX_COORD_RANGE   // ~73k cells

struct ProximityCounts {
    outer: Box<[u8]>,   // r=8 legality refcount, len PROX_FIELD_SIZE
    inner: Box<[u8]>,   // r=2 move-gen refcount, len PROX_FIELD_SIZE
}
```

`prox_idx(c) = (c.q + PROX_HALF) * PROX_COORD_RANGE + (c.r +
PROX_HALF)`. The `MAX_PIECE_DISTANCE` term in `PROX_HALF` is required:
`add_proximity` touches empty cells up to `MAX_PIECE_DISTANCE` beyond a
placed piece, and a piece at the zobrist-window edge would push those
cells past `2 * ZOBRIST_WINDOW + 1`. `u8` is sufficient: a cell's
count is the number of pieces within range, bounded by
`hex_area(8) ≈ 217 < 255`. `add_proximity` bumps via `saturating_add`;
a `debug_assert!` on `== 255` flags a pathological position. A `0`
count means "no piece in range" — there is no absent/present
distinction, so the old `remove_proximity` panic-on-missing invariant
is gone (a `debug_assert` covers underflow).

Candidate iteration uses `SparseCellSet` (same file):

```
struct SparseCellSet {
    members: Vec<Coord>,   // live cells, swap-perturbed insertion order
    slot:    Box<[u32]>,   // slot[prox_idx(c)] = members-position + 1;
                           // 0 = absent (the +1 bias frees 0 as sentinel)
}
```

`insert` is O(1) (push + slot store). `remove` is O(1): `slot` locates
the member, `swap_remove` pops it, and the swapped survivor's slot is
patched. `contains` is one `slot` probe. `iter` walks the contiguous
`members` `Vec` — cache-friendly, no hashing. `swap_remove` perturbs
iteration order; every caller is order-insensitive **for correctness**
(verified by the STEP 2.1 callsite scan), though the change of order
does shift alpha-beta tie-break decisions — see note below.

`candidates` holds the *current* legal empty cells; `inner_candidates`
the r=2 move-gen cells. Maintained incrementally:

- `place(c)`: for every `d` in the `r8` hex around `c`, bump
  `proximity.outer[prox_idx(d)]`; if it rose from 0 and `d` is empty,
  `candidates.insert(d)`. Same at r=2 into `proximity.inner` /
  `inner_candidates`. Remove `c` itself from both sets. After
  proximity / hash / history updates: `axes.set(c, player)`, then
  `winner = Some(player)` iff `is_winning_move(self, c, player)`.
- `undo(c)`: reverse. Before any other rollback: `axes.clear(c,
  player)` and clear `winner` if the undone move was the winning one.
  Then decrement the counts; remove from the matching set when a
  count hits 0. Re-insert `c` into each set whose remaining count > 0.
- `ply == 0` special case: candidates = `{(0, 0)}` when board empty;
  `inner_candidates` cleared (origin re-eligible via outer logic).

Memory cost at default `ZOBRIST_WINDOW = 127`: two `u8` count fields
(~73 KB each) and two `SparseCellSet` (`slot` ~290 KB each) — ~730 KB
per `Board`. Negligible vs the 64 MB TT, and there is exactly one live
`Board` (search uses make/undo, never clone).

> **Phase 16 node-count note (superseded by Phase 25.5 R-05).** The
> flat `inner_candidates` iterates in a different order than the old
> `FxHashSet`. `moves::generate` feeds that order into `order_moves`.
> Originally the priority key was a `u32` that left ties between
> equally-rated moves, and the stable sort + `MOVE_GEN_CAP` truncation
> broke those ties by generation order — so the proximity rework
> shifted which equally-rated move was searched first, changing
> alpha-beta node counts. This was not a strength change (verified
> by `make vs`); the Phase 16 reference baseline was refreshed once.
>
> **Phase 25.5 R-05** makes the priority key a total order by packing
> the move's `(q, r)` into the low 32 bits of a `u64` key (see
> "Ordering — encoding" below), and replaces the full stable sort
> with `select_nth_unstable_by` + sort-prefix. Tie order is now
> determined by Coord pack, not generation order, so node counts
> drift again — also documented as not a strength change. The
> baseline was refreshed a second time at R-05.

### place_for_search / undo_for_search (Sprint 3B)

Search-internal `place` / `undo` variants that skip the r=8 outer-halo
maintenance. The inner r=2 walk runs unchanged (search reads
`inner_candidates` only). The candidate audit (Sprint 3A,
`/tmp/sprint_3/A_place_for_search_design.md`) confirmed three search-
internal call sites — all `board.is_legal(m)` — and zero search/eval/
threats/movegen callers of the outer `candidates: SparseCellSet`.

**Contract**: during a search rooted at top-level position `P0`,
`place_for_search` / `undo_for_search` leave `proximity.outer` and
`candidates` frozen at `P0`'s state. Search descent is balanced —
every `place_for_search` is matched by exactly one `undo_for_search`
before returning to caller — so outer state at search exit ≡ outer
state at search entry, still valid for `P0`. **No `resync_outer_proximity`
is required at the search→top-level boundary.** `Engine::best_move`'s
subsequent top-level `place()` of the chosen move continues outer
maintenance via the normal API.

`is_legal_during_search(c)` is the search-safe legality query: empty-
cell check via `axes` plus an O(history) `hex_distance` walk. Called
~3 / node (TT + 2 killers); the per-call cost is bounded by piece
count (~50 in mid-game). `is_legal` (public, `proximity.outer`-backed)
is unchanged for top-level callers and tests.

Sprint 2C's first attempt re-used the normal `place()` body's
`candidates.remove(c)` without the symmetric re-add in
`undo_for_search`, progressively corrupting `candidates`.
Sprint 3B's split skips all outer-state ops uniformly — including
`candidates.remove` — preventing the leak by construction.

Predicted +10-15 % NPS based on Sprint 2C measurement (+37 % NPS
before the broken-contract revert; Sprint 3B's clean implementation
lands somewhere between, accounting for the small `is_legal_during_search`
overhead). Validation requires both reference fixture byte-identity AND
a new `chimera-positions` test set that exercises `is_legal_during_search`
plus the post-search `candidates()` oracle.

## Move Generation (`moves.rs`)

Per-stone generation. Search calls this once per ply (not once per turn) —
the two stones of a HeXO turn each get their own ordering and pruning.

```rust
pub fn generate(board: &Board, radius: i16, out: &mut Vec<Coord>);
```

`out` is caller-owned; `generate` clears it first and pushes the resulting
candidates. The Phase 25.5 `SearchScratch` allocator threads per-ply
`Vec<Coord>` slots through `pvs_node` / `quiescence_node` so the heap
allocation amortises to one per slot for the whole search. Earlier revisions
returned an inline-`SmallVec<[Coord; 32]>` on the assumption that
`MOVE_GEN_CAP = 24` covered the common case, but empirical measurement of
`Board::inner_candidates()` showed 36–52 cells from the early midgame
onward — the inline cap spilled to the heap every node beyond the opening.

### Algorithm

1. **Empty board**: return `{ORIGIN}`. Caller must place at `(0,0)`.
2. **`radius <= MOVE_GEN_INNER_RADIUS`**: copy `inner_candidate_cells`,
   `O(|inner|)`. No scanning.
3. **`MOVE_GEN_INNER_RADIUS < radius <= MAX_PIECE_DISTANCE`**: forward
   sweep — for each piece, walk its `r`-hex neighbourhood and union empty
   cells via an `FxHashSet` scratch.
4. **`radius > MAX_PIECE_DISTANCE`**: clamp to `MAX_PIECE_DISTANCE`, then
   same as case 3.

### Filtering by radius > INNER

Two implementation options were considered:

**A. Distance test per candidate.** Iterate `candidate_cells`. For each,
scan pieces; if any piece within `radius`, keep. Complexity
`O(|cand| × |pieces|)`. For 100 pieces × 200 candidates that's 20k
distance computations.

**B. Forward sweep.** For each piece, walk its `r`-hex neighbourhood and
union into a fresh `FxHashSet`. Complexity
`O(|pieces| × hex_area(r))`. For 100 pieces × 61 cells (r=4) that's 6.1k
ops with tight cache behaviour.

We pick **B** — fewer ops, no random probes into a large `candidate_cells`
set, and the scratch hashset is small and short-lived.

Concrete shape:

```rust
fn gen_in_outer_band(board: &Board, radius: i16, out: &mut Vec<Coord>) {
    let mut seen = FxHashSet::default();
    for (piece, _) in board.pieces() {
        for_each_in_range(piece, radius, |d| {
            if d == piece { return; }
            if board.is_empty_cell(d) && seen.insert(d) {
                out.push(d);
            }
        });
    }
}
```

The `seen` hashset is recreated per call; the candidate `out` buffer is
the search driver's per-ply slot from `SearchScratch` (Phase 25.5), so
its allocation survives across nodes.

### Ordering hook

`generate` returns moves in **insertion order**, not ordered. Phase 7
(`ordering`) is responsible for ranking and applying `MOVE_GEN_CAP`.
`generate` never truncates — capping arbitrary first-N would throw away
strong moves.

### Hot path notes

- Zero allocation on either path after first-search warmup; per-ply slots
  in `SearchScratch` retain capacity.
- Outer path still builds a fresh `FxHashSet` per call (out of scope for
  Phase 25.5 — small, short-lived).
- The candidate-move buffer is `Vec<Coord>` (the Phase 25.5 R-04
  `MoveList` alias was removed once no external callers were found).
  Search uses the boxed per-ply `Vec<Coord>` slots in `SearchScratch`
  directly; `ordering::order_moves` takes `&mut Vec<Coord>`.

## Axis Bitmaps (`axis_bitmap.rs`)

Sparse, per-axis, per-player line bitmaps. Shared infrastructure for win
detection, window-scan eval (Layer 1), and shape detection (Layer 2).

### Indexing

Three axes. For each axis, a hex cell `(q, r)` maps to a `(line_id, pos)`
pair:

| Axis | line_id | pos |
|---|---|---|
| Q (horizontal) | `r` | `q` |
| R (diagonal 1) | `q` | `r` |
| S (diagonal 2) | `q + r` | `q` |

All values fit in `i16`. The chosen mapping makes adjacent cells on the same
line have adjacent `pos` values, so they pack into consecutive bits.

### Data structure

```rust
pub struct LineBitmap {
    /// Packed bits. bit i corresponds to position `base_pos + i`.
    words: SmallVec<[u64; 4]>,
    base_pos: i16,
}

pub struct AxisBitmaps {
    /// [axis][player] -> fixed-length flat array of optional line bitmaps,
    /// indexed by `(line_id - LINE_ID_OFFSET)` where
    /// `LINE_ID_OFFSET = -2 * ZOBRIST_WINDOW` and the array length is
    /// `LINE_ID_RANGE = 4 * ZOBRIST_WINDOW + 1`. The 2× factor covers the
    /// S-axis line_id `q + r`, which reaches `±2 * ZOBRIST_WINDOW` even
    /// when each coord component stays inside the per-coordinate zobrist
    /// window of `±ZOBRIST_WINDOW`.
    lines: [[Box<[Option<LineBitmap>]>; 2]; 3],
    /// [axis][player] -> list of every `line_id` ever touched by `set`
    /// (insertion order, never removed). Backs `line_ids()` so the eval
    /// hot path enumerates populated lines in O(populated_lines) instead
    /// of scanning the full `LINE_ID_RANGE`-long flat array. Mirrors the
    /// prior `FxHashMap` semantics of "keys persist after the line's bits
    /// are cleared". The `SmallVec` inline size of 32 holds a typical
    /// midgame's per-axis line count on-stack.
    populated_ids: [[SmallVec<[i16; 32]>; 2]; 3],
    /// [axis] -> unified per-axis occupancy bitmap (no player dimension).
    /// Set whenever either player places at the cell; cleared on any
    /// `clear` (HeXO permits at most one stone per cell, so the other
    /// player cannot own it). Backs `is_occupied(c)` as a single per-axis
    /// probe — the hot path inside `Board::add_proximity`'s
    /// neighbour-occupancy check fires ~470 times per place, so a single
    /// bitmap load beats two per-player probes by ~6 % NPS.
    occupied: [Box<[Option<LineBitmap>]>; 3],
}
```

`SmallVec<[u64; 4]>` keeps most short lines inline (256 bits, covers ±128
positions inline). Long lines spill to heap. No allocation in the common
case once a line is established.

**Storage rationale (Phase 13)**: line IDs are bounded by
`±2 * ZOBRIST_WINDOW` (default 127 → 509 entries per axis-player; the
2× factor accommodates axis-S `line_id = q + r`). The Phase 12
flamegraph showed hashbrown probes inside `AxisBitmaps::line` /
`is_set` / window scans consuming ~500 M samples — the largest user-space
cost after the bench-harness TT allocator artifact. Replacing the
`FxHashMap<i16, LineBitmap>` with a fixed flat array reduces every probe
to a single bounds-checked array load. Out-of-range line IDs are a bug
(`debug_assert!`); the zobrist window already bounds per-coord values.

Slots are lazily initialized: `None` until the first `set` on that line.
`clear` does **not** deallocate (keeps the `Some(empty)` slot for re-use;
reduces allocator churn during search). Memory cost per axis-player is
`LINE_ID_RANGE * size_of::<Option<LineBitmap>>()` (≈ 24 KB at default
window), total ≈ 150 KB — negligible compared to the 64 MB TT.

Rotation property unchanged (axis permutation Q → S → R → Q; the array
backing has no effect on the symmetry exploitation).

### Operations

```rust
impl AxisBitmaps {
    pub fn new() -> Self;
    pub fn set(&mut self, c: Coord, player: Player);
    pub fn clear(&mut self, c: Coord, player: Player);

    /// Length of the longest contiguous run of `player`'s stones through `c`
    /// on `axis`. Returns 0 if `c` is not occupied by `player` on that line.
    /// Walks at most ±5 positions; bounded O(1).
    pub fn run_length_through(&self, c: Coord, axis: Axis, player: Player) -> u8;
}
```

### Axis enum

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum Axis { Q = 0, R = 1, S = 2 }

impl Axis {
    #[inline] pub const fn all() -> [Axis; 3] { [Axis::Q, Axis::R, Axis::S] }
    #[inline] pub const fn line_id(self, c: Coord) -> i16 {
        match self { Axis::Q => c.r, Axis::R => c.q, Axis::S => c.q + c.r }
    }
    #[inline] pub const fn pos(self, c: Coord) -> i16 {
        match self { Axis::Q => c.q, Axis::R => c.r, Axis::S => c.q }
    }
}
```

### Growth policy

`LineBitmap::set(pos)`:
- If `pos` falls within `[base_pos, base_pos + 64 * words.len())`: set bit.
- Below `base_pos`: prepend words, adjust `base_pos`. Grow by at least 64
  bits to amortize.
- Above range: append words.

Pre-grow on first insert: initialize with one 64-bit word centered around
`pos`, e.g. `base_pos = pos - 32` so first set sits in bit 32. This leaves
room on both sides for typical local expansion before realloc.

### Augmentation note

Hex grid 60° rotation `(q, r) → (-r, q + r)` permutes axes cyclically:
Q → S → R → Q. Reflections analogous. Per-axis bitmap storage means rotated
positions reuse identical bit patterns; only the axis index changes. This
is exploited later for canonical-hash and self-play augmentation. No work
required here — design is rotation-friendly by construction.

## Threats (`threats.rs`)

Detects WSC S0-shape patterns. Produces counts + S0 threat instances
with defense cells. Cached on board, recomputed by a linear-run scan
on the first read after a mutation.

### Types

```rust
pub struct ThreatCounts {
    pub open_5: u8, pub closed_5: u8,
    pub open_4: u8, pub closed_4: u8,
}

#[repr(u8)]
pub enum ThreatKind { OpenFive, ClosedFive, OpenFour, ClosedFour }

pub struct ThreatInstance {
    pub kind: ThreatKind,
    pub pieces: SmallVec<[Coord; 5]>,
    pub defense_cells: SmallVec<[Coord; 4]>,
}

pub struct ThreatSet {
    pub counts: ThreatCounts,
    pub s0_instances: Vec<ThreatInstance>,
}
```

### Operations

```rust
pub fn compute(board: &Board, player: Player) -> ThreatSet;
```

Always a full recompute by linear-run scan. (Phases 14–15 carried
`centers` / `prior` parameters for an incremental reconcile path;
Phase 17 made the full scan the only live path and Phase 22 removed
the vestigial parameters.)

### Cache on Board

`Board` gains:
```
threats_x: RefCell<ThreatSet>,
threats_o: RefCell<ThreatSet>,
threats_dirty: Cell<bool>,
```

Public accessor:
```
pub fn threats(&self, player: Player) -> Ref<ThreatSet>;
```

Invariants:

- `threats_dirty == false` ⟹ `threats_x` / `threats_o` hold the
  current cached threats. The hot path returns the cached `Ref` with
  a single `RefCell::borrow`.
- `threats_dirty == true` ⟹ the cached threats are stale; the next
  `threats()` read triggers a full recompute and clears the flag.
- Every `place` / `undo` sets `threats_dirty`. The `Cell<bool>` flag
  short-circuits the cache lookup hot path: clean reads do a single
  `RefCell::borrow` and a direct return.
- Initial state after `Board::new` / `Board::reset`: both caches hold
  `ThreatSet::default()` (empty), dirty flag `false`.

### LineContribution Cache on Board (Phase 27)

`Board` also gains:
```
line_contrib: RefCell<Box<[i32]>>,  // len = 3 * LINE_ID_RANGE = 1527
```

Memoises per-`(axis, line_id)` Layer-1 contribution (the value
returned by `eval::scan_line_8cell`).

- **Key**: linear `slot = (axis as usize) * LINE_ID_RANGE + line_id`.
  `axis_bitmap::Axis::line_id(c)` resolves the line for a coord.
- **Value**: single signed `i32`, X-positive. `WINDOW_SCORE_8`
  already folds X and O contributions into one signed scalar — no
  per-player split.
- **Dirty marker**: sentinel `i32::MIN`. No parallel dirty bitmap.
- **Init / reset**: `Board::new` allocates filled with sentinel.
  `Board::reset` wipes via `fill(i32::MIN)`.
- **Invalidation hook**: every `Board::place` / `Board::undo` /
  `Board::place_for_test` marks the ≤3 slots touched by the placed
  stone dirty. The set is exactly `(Q_line_id = c.r, R_line_id =
  c.q, S_line_id = c.q + c.r)` — hardcoded by the `Axis::all() ==
  [Q, R, S]` loop in `AxisBitmaps::set` / `AxisBitmaps::clear`. The
  three mutation sites share a private helper to prevent drift.
- **Recompute path**: lazy. On Layer-1 read, slots equal to
  `i32::MIN` trigger a call to `scan_line_8cell` and the result is
  stored. Clean reads return the cached value directly.
- **Containment**: `Board` exposes no `pub` axis field and no
  `axes_mut()` accessor — all axis-bitmap mutation funnels through
  the place/undo path, so the invalidation hook covers every
  mutation. Per-`Board` ownership; no thread-locals, no global
  state.

## Win Detection (`win.rs`)

After each `place(c)` by `player`, the move wins iff any of the 3 axes has
a run of length ≥ 6 through `c` of `player`'s stones.

```rust
pub fn is_winning_move(board: &Board, c: Coord, player: Player) -> bool {
    Axis::all().iter().any(|&axis| {
        board.axes().run_length_through(c, axis, player) >= 6
    })
}
```

`run_length_through` is bounded O(1): it walks at most 5 positions backward
and 5 forward from `c` on the line bitmap. Bit access is a single word load
+ shift + mask.

### Where it's called

- `Board::place(c)` updates a cached `winner: Option<Player>` field after
  setting the axis bitmap, by calling `is_winning_move(self, c, player)`.
- `Board::winner()` returns the cached value. O(1).

This caching means search can call `winner()` on every node without
re-scan.

### Edge case: overlines

HeXO win is "≥ 6 in a row", not "exactly 6". Per spec, 7+ in a row still
wins. `run_length_through` returns the *actual* length; the check is
`>= 6`.

## Zobrist hashing (`zobrist.rs`)

128-bit hash. One key per `(Coord, Player)`. Plus two constants:
- `Z_TURN_X`: XORed into hash whenever the side-to-move is X (regardless
  of halfmove). Toggles at every turn boundary — twice per full
  O→X→O cycle.
- `Z_HALFMOVE`: XORed into hash whenever the current stone is the
  **second of a 2-stone turn** (halfmove == 1).

The two contributions are orthogonal: all four `(side, halfmove)`
combinations (X,0), (X,1), (O,0), (O,1) hash to a distinct
parity overlay even when occupancy is identical.

Halfmove flag definition:
- `halfmove = 0`: side-to-move is about to place stone 1 of their turn.
- `halfmove = 1`: same side-to-move (or X on first stone of game) is
  about to place stone 2 of their turn.

Special case: X's very first move places only 1 stone. After that move,
halfmove returns to 0 and side flips to O. The flag tracks "current
stone is second-of-pair", which is **false** for X's opening singleton.

Hash invariants:
- After every `place(c, p)` or `undo(c, p)`, hash reflects the new
  state: occupied cells XOR'd + side-to-move + halfmove.
- Hash is unique up to true positional equivalence; positions identical
  in occupancy but differing in (side-to-move OR halfmove) hash
  differently.

Why: per-stone search recursion enters the same occupancy from
different halfmove states (e.g. stone-2-of-X vs stone-1-of-O after X
plays one). Without `Z_HALFMOVE`, both states alias to the same TT
slot and the engine reads a score evaluated for the wrong side-to-move.

Strategy: bounded preallocated window + lazy fallback.

- Window: `q, r ∈ [-WINDOW, WINDOW]`. Default `WINDOW = 127` →
  255 × 255 × 2 × 16 bytes ≈ 2 MB. Allocated once, seeded from
  fixed constant (deterministic hashes across runs for reproducibility).
- Outside window: `FxHashMap<(Coord, Player), u128>`, populated lazily with
  a second PRNG stream so values are stable per process.

The two parity constants live in reserved seed slots so existing
per-cell key values are byte-identical to pre-halfmove builds.

API:

```rust
pub const Z_TURN_X: u128;
pub const Z_HALFMOVE: u128;

pub struct ZobristTable { /* opaque */ }

impl ZobristTable {
    pub fn new() -> Self;
    /// Hash key for (coord, player). Cheap for in-window coords (array load).
    pub fn key(&mut self, c: Coord, p: Player) -> u128;
}
```

`Board` holds `hash: u128` and `halfmove: u8`. XORs `table.key(c, p)` on
place; XORs same key on undo (XOR is its own inverse). Z_TURN_X /
Z_HALFMOVE are XOR'd in/out on every parity transition.

## Transposition Table (`tt.rs`)

Two-bucket flat array. Each slot holds `[depth_preferred, always_replace]`.
The slot is a `#[repr(C, align(64))]` newtype `TTBucket` so each bucket
occupies exactly one 64-byte cache line (Phase 27.5 / R-14).

```rust
pub struct TTEntry {
    pub hash: u128,        // full key for collision verification
    pub best_move: Coord,  // ORIGIN sentinel if no best
    pub score: i32,
    pub depth: i8,         // -1 = empty
    pub flag: TTFlag,
    pub generation: u8,
}

#[repr(u8)]
pub enum TTFlag { Empty, Exact, LowerBound, UpperBound }

#[repr(C, align(64))]
pub struct TTBucket(TTEntry, TTEntry);  // 64 B, align 64

pub struct TranspositionTable {
    buckets: Box<[TTBucket]>,
    mask: usize,
    generation: u8,
}

impl TranspositionTable {
    pub fn new(size_mb: usize) -> Self;
    pub fn probe(&self, hash: u128) -> Option<&TTEntry>;
    pub fn store(&mut self, hash: u128, depth: i8, score: i32,
                 flag: TTFlag, best_move: Coord);
    pub fn new_generation(&mut self);
    pub fn clear(&mut self);
    pub fn stats(&self) -> TTStats;
}
```

Sizing:
- `slot_size = size_of::<TTBucket>()` = 64 bytes (cache-line sized,
  cache-line aligned).
- `n_slots = floor_pow2((size_mb * 1024 * 1024) / slot_size)`.
- `mask = n_slots - 1`.
- Multi-MB allocations land via mmap on glibc, so the bucket array
  base address is page-aligned and every `TTBucket` sits on its own
  cache line.

Index: `(hash as u64 as usize) & mask`. Verification: compare full u128.

Probe:
- Read both buckets at index. Return first one whose `hash == query`
  AND `flag != Empty`. Prefer `depth_preferred` over `always_replace`
  when both match.

Store:
- If new depth ≥ depth_preferred.depth OR depth_preferred.generation
  != current_generation: overwrite depth_preferred. Move displaced
  entry to always_replace (if it had higher depth than current
  always_replace entry, else discard).
- Else: overwrite always_replace.

Aging: `new_generation` increments `generation` (wrapping). Aged
entries are eligible for depth-preferred replacement regardless of
depth.

Stats: probe / hit / store counts; deferred to Phase 8 instrumentation.

## Search (`search.rs`)

Per-stone alpha-beta minimax with iterative deepening. X-positive eval.

### Driver

```rust
pub struct SearchConfig {
    pub max_depth: i8,
    pub time_ms: Option<u64>,
    pub deadline_check_nodes: u32,
    pub asp_window_initial: i32,
    pub asp_window_widen_factor: u32,
    pub lmr_min_depth: i8,
    pub lmr_min_move_index: u8,
    pub lmr_reduction: i8,
    pub qsearch_max_plies: u8,
    pub max_check_extensions: u8,
}

pub struct SearchResult {
    pub best_move: Coord,
    pub score: i32,
    pub depth_reached: i8,
    pub nodes: u64,
    pub time_ms: u64,
}

pub fn search_root(
    board: &mut Board,
    tt: &mut TranspositionTable,
    ordering: &mut OrderingState,
    cfg: &SearchConfig,
) -> SearchResult;
```

### Algorithm flow

1. Bump TT generation; **reset killer slots**; decay ordering history.
   Killers are per-ply hints tied to a specific search-tree shape, so
   they must not bleed across `best_move()` calls. History is decayed
   (not wiped) — it is the design's per-game move-quality memory.
2. Compute `deadline = now + cfg.time_ms` (if `Some`).
3. For `depth = 1..=max_depth`:
   a. **Aspiration**: if `depth >= 4` and `prev_score` known, search the
      narrow window `[prev - delta, prev + delta]`. On a fail-low /
      fail-high, widen `delta *= factor` and re-search; on the second
      failure promote to full window. Each failed attempt counts time
      against the deadline. The loop is guaranteed to terminate because
      the full-window pass always returns in-window.
   b. Run `pvs_node(depth, alpha, beta)` at the root.
   c. If deadline elapsed during the iteration, discard the partial
      result **and restore killer state from the start of the aborted
      iteration**, then return the last completed iteration's result.
      The snapshot is taken once per ID depth (outside the aspiration
      loop), so failed aspiration attempts within a depth deliberately
      share killer state with the next attempt at that depth.
   d. Save `(depth, score, best_move)` as the current result.
4. Return the current result.

### Recursive nodes (`pvs_node`)

Minimax form (X maximizes, O minimizes). A single recursive `pvs_node`
function dispatches on `board.to_move()` per call — X-nodes maximize, O-
nodes minimize. We do NOT split into separate `pvs_max` / `pvs_min`
entry points: the duplicated bookkeeping is dead weight and the
per-node `maximize` flag is one branch in cold code. Per node:

1. Check deadline every `cfg.deadline_check_nodes` nodes via a
   thread-local counter; on timeout propagate `Err(Timeout)` up.
2. If `board.winner()` is `Some(p)`, return mate-distance score
   `±(MATE_SCORE - ply)` (sign by `p`).
3. Probe TT. If a hit at sufficient depth with a compatible bound:
   return its score.
4. If `depth == 0`: return `quiescence_*` (see below).
5. **Staged move iteration (Phase 26 R-01).** The move sequence is built
   on demand in three stages. A `tried: SmallVec<[Coord; 3]>` records the
   moves dispatched by stages 1 / 2 so that stage 3 can dedup. A single
   `move_idx` counter increments across all three stages — the PVS
   "first move" (`is_first = move_idx == 0`) and LMR's
   `move_idx >= lmr_min_move_index` predicates apply globally, so the
   stage-1 TT move always gets the PV-line full window when present.
   a. **Stage 1 — TT move.** If `tt_move = Some(m)` from the TT probe
      and `board.is_legal(m)` (defensive against stale entries from hash
      collisions / proximity contraction after undo), dispatch the move.
      Its bucket is read from `ordering::bucket_value(ctx, m)` — for a TT
      move this short-circuits to 10. Alloc-free: `moves::generate` and
      the ordering pass are NOT executed yet.
   b. **Stage 2 — Killer moves.** Walk `ord.killers[ply].slots()` in slot
      order (slot 0 = newest). For each `Some(m)` not in `tried` and
      satisfying `board.is_legal(m)`, dispatch the move using
      `ordering::bucket_value(ctx, m)` — typically 3 (killer fallback),
      but a killer that ALSO matches a higher-priority shape (win,
      block-win, S0-create…) gets the higher bucket so LMR / check
      extension behave identically to today's ordering-pass result.
      Alloc-free.
   c. **Stage 3 — Full generate + order.** If no β-cutoff yet, run
      `moves::generate(board, DEFAULT_MOVE_RADIUS)` and
      `ordering::order_moves_with_buckets(...)` exactly as before.
      Iterate the ordered list with `if tried.contains(&m) { continue; }`
      to skip stage-1/2 entries.

   When stage 1 or stage 2 produces a β-cutoff, the function returns
   without ever calling `moves::generate` — the TT-cutoff fast path
   eliminates the candidate-buffer fill + ordering work on the
   ~89-95% of nodes where the TT or a killer cuts.

6. Iterate the staged sequence:
   - **First move (move_idx == 0)**: full window `[alpha, beta]`.
   - **Subsequent (move_idx > 0)**: null-window probe `[alpha, alpha + 1]` at
     possibly-reduced depth. On `probe > alpha` re-search at full depth
     (and full window if `probe < beta`). On fail-high we also fall back
     to the full window.
   - **LMR**: if `depth >= lmr_min_depth`, `move_idx >= lmr_min_move_index`,
     and the move's ordering bucket is not in
     `{TT, win, block-win, stone1-defense, S0-create, S0-block, killer}`,
     search at `depth - 1 - lmr_reduction`. The PVS dance is therefore
     three-step:
     1. Reduced null-window probe (`probe_depth = new_depth - lmr`).
     2. On `probe > alpha`, re-search at FULL depth, STILL null window.
     3. On `widened > alpha && widened < beta`, re-search at full depth,
        FULL window.
     This saves a full-window pass when the full-depth null also fails
     low — a real win on hot tactical lines.
   - **Check extension**: if the placed move creates an S0 threat for the
     side that just moved and `extensions_left > 0`, search at the new
     depth (i.e. `+1` over the would-be `depth - 1`) and decrement
     `extensions_left`.
7. On `score >= beta` (max) / `score <= alpha` (min): record killer +
   history, break (β-cutoff).
8. Store a TT entry with flag `Exact`, `LowerBound`, or `UpperBound`
   depending on whether alpha was raised / beta was cut.

**TT prefetch (Sprint 1C).** Immediately after `board.place(m)` and
before the recursive `pvs_dance` / `quiescence_node` call, issue an
L1 prefetch (`_mm_prefetch::<_MM_HINT_T0>`) on the TT bucket the
child position will probe. The hash is `board.hash()` (post-place);
the bucket index is `(hash as u64 as usize) & tt.mask`. The
prefetch fires on x86_64 only; non-x86 builds use a no-op stub. It
is purely a timing hint — affects ordering of memory ops, never
correctness — and so cannot deadlock, race, or fault on a freed
pointer. Applies at all three post-place sites in `search.rs`: the
PVS main move loop, the qsearch TT-move attempt, and the qsearch
threat-move loop. Predicted +1.5-3 % NPS by hiding DRAM latency on
the child probe (see `analysis/baseline_ae539b7/verdict.md` #4).

### Quiescence (`quiescence_max` / `quiescence_min`)

Threat-only, hard-capped at `cfg.qsearch_max_plies`.

1. Check deadline; return early on terminal.
2. **TT probe (Phase 28F-3.4, gated by `qsearch_tt_enabled`).** If
   `cfg.qsearch_tt_enabled` is true, probe `tt.probe(board.hash())`
   BEFORE stand-pat. Save `tt_move = Some(entry.best_move)` for step 6.
   With `adjusted = score_from_tt(entry.score, ply)` and the explicit
   depth gate `entry.depth >= -1` (always true; the gate documents
   intent — see below for the depth convention):
   - `Exact` → return `adjusted`.
   - `LowerBound` and `adjusted >= beta` → return `adjusted`.
   - `UpperBound` and `adjusted <= alpha` → return `adjusted`.
   - else fall through to stand-pat with `tt_move` still saved.
3. Stand-pat with `board.cached_eval()`. For `max`: if `static >= beta`
   return `beta`; else `alpha = max(alpha, static)`. Mirror for `min`.
   **Stand-pat early-returns DO NOT store to TT.**
4. If `q_ply >= cfg.qsearch_max_plies` return alpha. **No TT store.**
5. Generate threat-only moves per `cfg.qsearch_filter_mode` (see below).
6. If `tt_move` is `Some(m)`, `m != ORIGIN`, `board.is_legal(m)`, and
   `is_threat_move(board, m, side, mode)` — try `m` FIRST, before
   iterating the generated list. If the TT move alone causes a beta
   cutoff, return without iterating the generated list. The generated
   loop skips `tt_move` to avoid double-search.
7. If no threat moves remain after step 5 (and no TT move was tried in
   step 6), return alpha (the position is quiet). **No TT store.**
8. Recurse normally with the threat-only move list.
9. **TT store (Phase 28F-3.4, gated by `qsearch_tt_enabled`).** If
   `cfg.qsearch_tt_enabled` is true AND at least one threat move was
   recursed (the TT move from step 6 counts), compute the bound flag
   from `(best_score, alpha_orig, beta_orig)` snapshotted at function
   entry (NOT from the in-flight `alpha`/`beta` refined by stand-pat
   or by recursive calls):
   - `best_score <= alpha_orig` → `UpperBound`.
   - `best_score >= beta_orig` → `LowerBound`.
   - else → `Exact`.
   Call `tt.store(board.hash(), -1, score_to_tt(best_score, ply),
   flag, qs_best_move)` where `qs_best_move` is the move that produced
   `best_score`, or `ORIGIN` if no move improved over the
   stand-pat-derived starting value. Stores fire on ANY search-derived
   termination — both function tail AND in-loop beta/alpha cutoffs —
   provided `searched_any_move == true` at the moment of store.
   Inline-cutoff stores are mandatory: qsearch returns early on
   `score >= beta` / `score <= alpha` and cannot fall through to a
   common tail; skipping cutoff stores would lose the strongest
   bounds qsearch derives. (Cf. Stockfish: qsearch stores on cutoff
   before returning.)

#### Qsearch TT depth convention (`-1`)

All qsearch TT entries store `depth = -1`. Rationale:

- `TTEntry::depth: i8` already supports negative values.
- Two-bucket replacement uses signed `depth >= a.depth`: a qsearch
  entry at `-1` will NEVER evict a main-search entry at `depth >= 0`
  from the depth-preferred bucket. Main search wins replacement
  structurally; qsearch entries land in the always-replace slot when
  the main-search entry is depth-preferred.
- `TTEntry::EMPTY.depth == -1` is disambiguated by `flag == Empty`;
  a stored qsearch entry has a non-Empty flag, so probes correctly
  distinguish "stored at depth -1" from "empty slot".
- Mate-distance re-anchoring (`score_to_tt` / `score_from_tt`) is
  applied identically to main-search entries.

#### What is NOT stored

- Stand-pat fail-high / fail-low early returns: these are eval-derived
  bounds, not search-derived. Storing them pollutes TT with redundant
  copies of `cached_eval`.
- `q_ply >= cap` early returns: the cutoff is the cap, not a real
  bound on the position.
- `threat_count == 0` early returns (no threats and no TT move
  recursed): static evaluation, no search.

#### Filter modes (`qsearch_filter_mode`)

`QsearchFilterMode` enum dispatched in the hot path. String → enum
parsed once at config load. Three modes:

| Mode | Predicate | Use case |
|------|-----------|----------|
| `current` | `would_make_six(self) \| creates_s0(self) \| blocks_opp_s0(self)` | Legacy. Speculative S0-creation drives BF up to ~4.7 in cluster decisive. |
| `resolution` | `would_make_six(self) \| blocks_opp_s0(self)` | Drops speculative S0 creation. Defensive resolution only. |
| `urgent` | `would_make_six(self) \| would_make_six(opp)` | Win or block immediate six only. Tightest filter. -22% cyc/node vs `current`. **Default since Phase 28F-3.3.** |

The `would_make_six(board, m, opp)` call in `urgent` exploits the fact
that `ordering::would_make_six` is a pure virtual-placement predicate
that reads the appropriate axis bitmap by `side` index without
mutating the board. Passing `opp` answers "would opp win if they
placed a stone at empty cell `m`?" — exactly the cell HH must block.

Phase 28F-3.3 selected `urgent` after 400g direct match vs `current`:
+50.7 Elo, Wilson CI [+16.4, +85.1]. External arena vs SB-perf:
+2.5pp (4.5% vs 2.0% baseline); below the strict +3pp gate but
internally definitive.

### Stone-1 threat completion

When the just-placed move turned `halfmove` from 0 → 1 and created an S0,
the recursive call's `OrderingContext::stone1_s0_defense` carries the new
S0's `defense_cells` so bucket 7 ("complete the threat") kicks in.

### Time management

- `cfg.time_ms` is the **per-stone** budget for the current `search_root`
  call.
- `Engine::best_move` is per-stone: the entire `time_ms` argument is
  consumed on the single `best_move` call. Callers issue one
  `best_move` per stone (two per turn for a normal turn). The engine
  performs no internal split — Phase 28E-0 dropped the historical
  60/40 stone-1/stone-2 split (every consumer was already passing
  per-stone, and the extra split halved effective utilisation).
- Deadline checked every `deadline_check_nodes` via a thread-local
  counter; on timeout the partial iteration is discarded and the last
  completed iteration's result is returned.

### Eval sign

Positive = X advantage. Negative = O advantage. **Minimax form**, not
negamax: the `pvs_node` function selects max/min by `board.to_move()`,
not by negation. Never sign-flip.

### TT mate-score adjustment

Mate scores `±(MATE_SCORE - ply)` are absolute-ply at the moment of
detection. Storing them verbatim in the TT lets a transposition reached
at a different ply return an off-by-N mate distance. On store we shift
mate-class scores by `±ply` (sign matches the side winning); on probe we
shift back relative to the current node's ply. Non-mate scores pass
through unchanged. The threshold for "mate-class" is
`|score| >= MATE_SCORE - MAX_PLY`.

## Ordering (`ordering.rs`)

Stable bucket sort over candidate moves. Buckets, highest priority first
(numbering as of Phase 17 — the creates-S1 bucket was removed):

  1. TT best move
  2. Winning move (creates 6-in-row)
  3. Defensive win (blocks opponent would-be 6-in-row)
  4. Completes stone-1 S0 (only when current stone is `halfmove == 1`
     and stone-1 of this turn started an S0 threat; the search driver
     passes the defense cells of that threat as `stone1_s0_defense`)
  5. Creates S0 threat (open-4 / closed-5 / open-5)
  6. Blocks opponent S0 threat
  7. Killer move at this ply (2 slots, OR over both)   — was 8
  8. History heuristic                                 — was 9
  9. Static delta-eval / proximity tie-break           — was 10

The bucket values above feed both the ordering pass and Phase 26 R-01's
**staged move iteration** (search.rs / SPEC § "Recursive nodes" step 5):
the staged path calls `bucket_value(...)` per dispatched move so LMR
exclusion and the S0-create check extension fire exactly as they would
have if the move had reached the ordered list. The TT bucket (10) and
killer bucket (3) are early-return paths in `bucket_value` that skip the
3-axis fused probe, so stage-1 / stage-2 bucket lookup is cheap.

Phase 17 disabled the old bucket 7, "Creates S1 threat" (the S1/S2
ablation A/B was net-negative — see `SPEC_EVAL.md § Layer 2 history`).
Phase 20 removed the `creates_s1` predicate with the rest of S1/S2
detection; a run-extending move falls through to the killer / history
buckets.

Encoding (Phase 25.5 R-05): the priority key is a `u64` total order
over candidate moves:

```
priority = (bucket << 56)
         | ((history_score & 0x00FF_FFFF) << 32)
         | qr_pack(m)

qr_pack(m) = ((m.q as u16) as u64) << 16 | ((m.r as u16) as u64)
```

Buckets 1–6 occupy bucket values 10..5 respectively; bucket 7 (killer)
has bucket value 3; bucket 8 (history) has bucket value 1; bucket 9
(static) has bucket value 0. Encoding values 4 (the removed creates-S1
bucket) and 2 are unused gaps. Higher `u64` = sorted earlier.

The low 32 bits pack `(q, r)` as `i16`-bitcast-to-`u16` halves, making
the key total: any two distinct legal `Coord`s have distinct keys.
This eliminates priority ties at the sort site and lets the truncation
use `select_nth_unstable_by` (O(n) partition) followed by a sort of
the kept prefix, instead of a full stable sort of all candidates. Tie
order is no longer "generation order" — it is fully determined by the
Coord pack, deterministic but unrelated to move generation.

History values are clamped to `HISTORY_CUTOFF_MAX = 0x00FF_FFFF` (24 bits).

After partial sort + truncation, only the top `MOVE_GEN_CAP` (default
24) moves survive into the move loop.

### State

`KillerSlot` holds at most `KILLER_SLOTS` (2) most-recent cutoff moves at a
ply. `OrderingState` owns `Box<[KillerSlot; MAX_PLY]>` (MAX_PLY = 128) and
the history table.

**Sprint 3C** replaces the prior global `FxHashMap<(Coord, Player), u32>`
with `HistoryTable` — a pair of flat `Box<[u32; PROX_FIELD_SIZE]>` arrays
(one per player) indexed by `prox_idx(coord)`. Memory cost: 2 × ~73k × 4
bytes ≈ 588 KB per `OrderingState`. Lookup is one array load, no hashing;
`record_cutoff`'s `slot` access is a single index. The Sprint 2 flamegraph
showed `record_cutoff` → hashbrown probes climbing into the top user-space
frames once the proximity rework had shrunk the heavier callers — the flat
table eliminates that path.

The search driver calls:

- `record_cutoff(ply, m, p, depth)` on a β-cutoff: pushes `m` into the
  killer slot for `ply` (dedup), and increments
  `history[p][prox_idx(m)] += depth² ` (saturating to `HISTORY_CUTOFF_MAX`).
- `decay_history()` once per `search_root` call, at search entry before
  the iterative-deepening loop begins: every entry is multiplied by
  `HISTORY_DECAY_NUM / HISTORY_DECAY_DEN` (default ½, integer-floor).
  History therefore decays per `best_move()` invocation (i.e. per stone),
  not between ID iterations within a single call. Decay walks both flat
  arrays (~146k cells); cost is comparable to the prior hashmap retain
  (similar live-entry count after a few iterations).

### Approximations (v1)

The exact `creates_s0` predicate would require a make/undo +
threat-recompute per candidate move — too expensive in the inner
loop. v1 uses a cheap virtual-place axis-run probe:

- **creates_s0**: for each of the three axes, compute the run length
  `total = 1 + run_backward(pos) + run_forward(pos)` that would result
  from placing `side` at the empty cell `m`. Bucket fires when
  `total ∈ {4, 5}` AND at least one of the two cells immediately past
  the run's endpoints is not occupied by the opponent (i.e. some
  extension to 6 is reachable). Three axis-bitmap lookups, no `RefCell`
  borrow, catches new-S0-creators that the threats cache hasn't seen.

The predicate is O(constant) in the hex neighbourhood of `m`.

### Fused 3-axis probe in `bucket_value` (Phase 25.5 R-02)

Buckets 9 (own win), 8 (block opp win), and 6 (own creates_s0) all
need run-length information for the empty cell `m` on the same three
axes. Phase 25.5 R-02 collapses these into a **single 3-axis fused
probe** instead of three independent passes.

The probe (`ordering::axis_probe`) returns an `AxisProbe { own_back,
own_fwd, opp_back, opp_fwd }` per axis: at most two
`AxisBitmaps::line` slot loads (own + opp) and at most four
`run_*` calls per axis. `bucket_value` walks the three axes once,
deriving `own_six = (1 + own_back + own_fwd) ≥ 6`,
`opp_six = (1 + opp_back + opp_fwd) ≥ 6`, and the own-side
`creates_s0` predicate from the same per-axis own counts.

Semantics are **unchanged** — the predicates are byte-identical to
the standalone `would_make_six` / `creates_s0` helpers (which are
retained for qsearch's `is_threat_move` frontier). Reference node
counts must remain pre-/post-byte-identical at every
`(fixture, depth)`.

### Win detection in ordering

`would_make_six` runs the same virtual-place axis-run probe and fires
on `total ≥ 6`. HeXO treats overlines as wins, so the threshold is `≥`,
not `==`. Inside `bucket_value`, the equivalent test reuses the fused
probe (above); the standalone `would_make_six` remains for qsearch.

### `stone1_s0_defense`

For stone 2 of a HeXO turn (`halfmove == 1`), the search driver passes
the defense cells of the S0 threat the same side created with stone 1.
Bucket 7 then matches "play one of the defense cells", which completes
the threat. For stone 1 of a turn the slice is empty and bucket 7 is
disabled.
