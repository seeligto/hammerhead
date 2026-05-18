# HeXO Engine — Internals Spec

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
    pieces: FxHashMap<Coord, Player>,
    proximity_count: FxHashMap<Coord, u32>,        // for legality (radius 8)
    inner_proximity_count: FxHashMap<Coord, u32>,  // for move-gen (radius 2)
    candidate_cells: FxHashSet<Coord>,
    inner_candidate_cells: FxHashSet<Coord>,
    history: Vec<Coord>,
    hash: u128,             // 128-bit Zobrist; TT bucket = (hash as u64) & MASK
    ply: u32,
    zobrist: ZobristTable,
    axes: AxisBitmaps,
    winner: Option<Player>,
}

#[repr(u8)]
pub enum Player { X = 0, O = 1 }
```

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

- It is empty (`c` not in `pieces`), AND
- One of:
  - `ply == 0` and `c == (0, 0)` (forced first move at origin), OR
  - `ply >= 1` and `min(hex_dist(c, p) for p in pieces) <= MAX_PIECE_DISTANCE`
    (default 8, from `hexo.toml`).

Framing: the legal region is the **union of `r8` hexes** centred on each
existing piece. Placing a new piece at `c` extends the region by the `r8` hex
around `c`. Example: with stones at (0,0) and (8,0), legal cells span up to
(16,0).

### Candidate maintenance

`candidate_cells` holds the *current* legal empty cells. Maintained
incrementally:

- `proximity_count: FxHashMap<Coord, u32>` — for each cell within `r8` of any
  piece, count how many pieces are within `r8`. Cell is in candidate set iff
  `proximity_count > 0` AND cell is empty.
- `place(c)`: for every `d` in the `r8` hex around `c`, increment
  `proximity_count[d]`. If `proximity_count[d]` rose from 0 and `d` is empty,
  insert into candidates. Remove `c` itself from candidates. After proximity
  / hash / history updates: `axes.set(c, player)`, then set
  `winner = Some(player)` iff `is_winning_move(self, c, player)`.
- `undo(c)`: reverse. Before any other rollback: `axes.clear(c, player)`
  and clear `winner` if the undone move was the winning one. Then decrement
  counts; remove from candidates when count hits 0. Re-insert `c` if its
  remaining proximity count > 0 (other pieces still in range).
- `ply == 0` special case: candidates = `{(0, 0)}` when board empty.

The inner refcount (`inner_proximity_count` / `inner_candidate_cells`) is
maintained with exactly the same algorithm at radius
`MOVE_GEN_INNER_RADIUS`:

```
place(c):
    (existing r8 steps...)
    for each d in for_each_in_range(c, INNER_RADIUS):
        increment inner_proximity_count[d]
        if rose from 0 and d not in pieces: add to inner_candidate_cells
    Remove c from inner_candidate_cells.

undo():
    (existing r8 rollback...)
    for each d in for_each_in_range(c, INNER_RADIUS):
        decrement inner_proximity_count[d]
        if zero: remove from inner_candidate_cells (and from the count map)
    If c is still inner-adjacent to surviving pieces, re-add to
    inner_candidate_cells.
    If pieces now empty: clear inner_candidate_cells (origin re-eligible
    via the outer candidate logic; `place(ORIGIN)` will repopulate inner).
```

## Move Generation (`moves.rs`)

Per-stone generation. Search calls this once per ply (not once per turn) —
the two stones of a HeXO turn each get their own ordering and pruning.

```rust
pub fn generate(board: &Board, radius: i16) -> SmallVec<[Coord; MOVE_GEN_CAP_INLINE]>;
```

`MOVE_GEN_CAP_INLINE = 32` is the SmallVec inline capacity — slightly above
the typical `MOVE_GEN_CAP` of 30 so the SmallVec stays on-stack for the
common case.

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
fn gen_in_outer_band(board: &Board, radius: i16, out: &mut MoveList) {
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

The `seen` hashset is recreated per call. Reusing it via a thread-local or
search-scoped scratch buffer is a future optimisation.

### Ordering hook

`generate` returns moves in **insertion order**, not ordered. Phase 7
(`ordering`) is responsible for ranking and applying `MOVE_GEN_CAP`.
`generate` never truncates — capping arbitrary first-N would throw away
strong moves.

### Hot path notes

- No allocation on the inner-radius path beyond the returned `SmallVec`.
- Outer path: one `FxHashSet` allocation per call, pre-reserved with a
  rough estimate of `piece_count * 8`.
- `SmallVec` inlines up to 32 items. Typical inner-radius candidate sets
  fit comfortably.

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
    /// [axis][player] -> map of line_id -> bitmap
    lines: [[FxHashMap<i16, LineBitmap>; 2]; 3],
}
```

`SmallVec<[u64; 4]>` keeps most short lines inline (256 bits, covers ±128
positions inline). Long lines spill to heap. No allocation in the common
case once a line is established.

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

    /// 6-bit window starting at position `pos` of `axis` line `line_id` for
    /// `player`. Used by eval window scan (Layer 1) later.
    pub fn window6(&self, axis: Axis, line_id: i16, pos: i16, player: Player) -> u8;
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

Detects WSC-shape patterns. Produces counts + S0 threat instances with
defense cells. Cached on board, recomputed incrementally.

### Types

```rust
pub struct ThreatCounts {
    pub open_5: u8, pub closed_5: u8,
    pub open_4: u8, pub closed_4: u8,
    pub open_3: u8, pub rhombus: u8, pub arch: u8,
    pub bone: u8, pub trapezoid: u8,
    pub open_2: u8, pub closed_3: u8, pub triangle: u8,
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
pub fn compute(
    board: &Board,
    player: Player,
    center: Option<Coord>,
    prior: Option<&ThreatSet>,
) -> ThreatSet;
```

`center = None` → full recompute (used on `Board::reset`).
`center = Some(c)` + `prior = Some(_)` → incremental: drop instances
within radius 5 of `c`, rescan, merge.

### Cache on Board

`Board` gains:
```
threats_x: RefCell<Option<ThreatSet>>,
threats_o: RefCell<Option<ThreatSet>>,
threats_dirty_center: Cell<Option<Coord>>,
```

Public accessor:
```
pub fn threats(&self, player: Player) -> Ref<ThreatSet>;
```

Lazy: recomputes on first read after dirty marking.

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

pub struct TranspositionTable {
    buckets: Box<[(TTEntry, TTEntry)]>,
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
- `slot_size = size_of::<(TTEntry, TTEntry)>()` (~64 bytes after
  padding).
- `n_slots = floor_pow2((size_mb * 1024 * 1024) / slot_size)`.
- `mask = n_slots - 1`.

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
    pub stone1_time_pct: f32,  // 0..=1, default 0.6
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

1. Bump TT generation; decay ordering history.
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
      result and return the last completed iteration's result.
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
5. Generate candidates via `moves::generate(board, DEFAULT_MOVE_RADIUS)`;
   order via `ordering::order_moves(...)`.
6. Iterate ordered moves:
   - **First move (i == 0)**: full window `[alpha, beta]`.
   - **Subsequent (i > 0)**: null-window probe `[alpha, alpha + 1]` at
     possibly-reduced depth. On `probe > alpha` re-search at full depth
     (and full window if `probe < beta`). On fail-high we also fall back
     to the full window.
   - **LMR**: if `depth >= lmr_min_depth`, `i >= lmr_min_move_index`,
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

### Quiescence (`quiescence_max` / `quiescence_min`)

Threat-only, hard-capped at `cfg.qsearch_max_plies`.

1. Check deadline; return early on terminal.
2. Stand-pat with `board.cached_eval()`. For `max`: if `static >= beta`
   return `beta`; else `alpha = max(alpha, static)`. Mirror for `min`.
3. If `q_ply >= cfg.qsearch_max_plies` return alpha.
4. Generate threat-only moves: a move is included iff it creates own S0,
   blocks an opponent S0, or makes a 6-in-row. Skip the rest.
5. If no threat moves remain, return alpha (the position is quiet).
6. Recurse normally with the threat-only move list.

### Stone-1 threat completion

When the just-placed move turned `halfmove` from 0 → 1 and created an S0,
the recursive call's `OrderingContext::stone1_s0_defense` carries the new
S0's `defense_cells` so bucket 7 ("complete the threat") kicks in.

### Time management

- `cfg.time_ms` is the budget for the current `search_root` call.
- The `Engine::best_move` wrapper splits the per-turn budget across the
  two stones: stone 1 = `time_stone1_pct * t`, stone 2 = the remainder.
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

Stable bucket sort over candidate moves. Buckets, highest priority first:

  1. TT best move
  2. Winning move (creates 6-in-row)
  3. Defensive win (blocks opponent would-be 6-in-row)
  4. Completes stone-1 S0 (only when current stone is `halfmove == 1`
     and stone-1 of this turn started an S0 threat; the search driver
     passes the defense cells of that threat as `stone1_s0_defense`)
  5. Creates S0 threat (open-4 / closed-5 / open-5)
  6. Blocks opponent S0 threat
  7. Creates S1 threat (open-3 / rhombus / arch / bone / trapezoid)
  8. Killer move at this ply (2 slots, OR over both)
  9. History heuristic
 10. Static delta-eval / proximity tie-break

Encoding: `u32 priority = (bucket << 24) | (history_score & 0x00FF_FFFF)`.
Buckets 1–8 occupy bucket values 10..3 respectively; bucket 9 has bucket
value 1; bucket 10 has bucket value 0. Higher `u32` = sorted earlier.

History values are clamped to `HISTORY_CUTOFF_MAX = 0x00FF_FFFF` (24 bits).

After sort, truncate to `MOVE_GEN_CAP` (default 24).

### State

`KillerSlot` holds at most `KILLER_SLOTS` (2) most-recent cutoff moves at a
ply. `OrderingState` owns `Box<[KillerSlot; MAX_PLY]>` (MAX_PLY = 128) and a
global `FxHashMap<(Coord, Player), u32>` history. The search driver calls:

- `record_cutoff(ply, m, p, depth)` on a β-cutoff: pushes `m` into the
  killer slot for `ply` (dedup), and increments
  `history[(m, p)] += depth² ` (saturating to `HISTORY_CUTOFF_MAX`).
- `decay_history()` once per root iteration: every entry is multiplied
  by `HISTORY_DECAY_NUM / HISTORY_DECAY_DEN` (default ½, integer-floor).

### Approximations (v1)

The exact `creates_s0` and `creates_s1` predicates would require a
make/undo + threat-recompute per candidate move — too expensive in the
inner loop. v1 uses cheap virtual-place axis-run probes:

- **creates_s0**: for each of the three axes, compute the run length
  `total = 1 + run_backward(pos) + run_forward(pos)` that would result
  from placing `side` at the empty cell `m`. Bucket fires when
  `total ∈ {4, 5}` AND at least one of the two cells immediately past
  the run's endpoints is not occupied by the opponent (i.e. some
  extension to 6 is reachable). Three axis-bitmap lookups, no `RefCell`
  borrow, catches new-S0-creators that the threats cache hasn't seen.
- **creates_s1**: the same virtual-place probe triggers when `total ≥ 3`
  on any axis. Catches open-3 directly and most rhombus / arch /
  trapezoid / bone extensions whose added stone is collinear with two
  existing stones. Pure non-collinear cross-axis shapes are
  bucket-7 noise per spec.

Both predicates are O(constant) in the hex neighbourhood of `m`.

### Win detection in ordering

`would_make_six` runs the same virtual-place axis-run probe and fires
on `total ≥ 6`. HeXO treats overlines as wins, so the threshold is `≥`,
not `==`.

### `stone1_s0_defense`

For stone 2 of a HeXO turn (`halfmove == 1`), the search driver passes
the defense cells of the S0 threat the same side created with stone 1.
Bucket 7 then matches "play one of the defense cells", which completes
the threat. For stone 1 of a turn the slice is empty and bucket 7 is
disabled.
