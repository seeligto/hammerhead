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
    proximity_count: FxHashMap<Coord, u32>,
    candidate_cells: FxHashSet<Coord>,
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

## Move Generation (`moves.rs`)

Search uses ply = 1 stone. Branching halved vs. full-turn search.

```rust
pub fn generate(board: &Board, radius: i16) -> SmallVec<[Coord; 64]>;
```

Strategy:
- Default radius: 2 (immediate combat)
- Extended: 4 (configurable, handles colony spam)
- Full legality radius: 8 (for explicit legality checks)
- Cap top ~30 moves after ordering

For colony detection: scan piece clusters every N plies. If opponent makes far colony, expand candidates locally near it.

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

## Zobrist (`zobrist.rs`)

128-bit hash. Reduces collision probability to negligible across deep search.

Each `(q, r, player)` → random `u128`. XOR-update on place/undo.

Strategy: bounded preallocated window + lazy fallback.

- Window: `q, r ∈ [-WINDOW, WINDOW]`. Default `WINDOW = 127` →
  255 × 255 × 2 × 16 bytes ≈ 2 MB. Allocated once, seeded from
  fixed constant (deterministic hashes across runs for reproducibility).
- Outside window: `FxHashMap<(Coord, Player), u128>`, populated lazily with
  a second PRNG stream so values are stable per process.

API:

```rust
pub struct ZobristTable { /* opaque */ }

impl ZobristTable {
    pub fn new() -> Self;
    /// Hash key for (coord, player). Cheap for in-window coords (array load).
    pub fn key(&mut self, c: Coord, p: Player) -> u128;
}
```

`Board` holds `hash: u128`. XORs `table.key(c, p)` on place; XORs same key on
undo (XOR is its own inverse).

TT will derive its bucket index from `hash & ((1 << TT_INDEX_BITS) - 1)` and
store the full 128-bit hash for collision verification.

## Transposition Table (`tt.rs`)

```rust
pub struct TTEntry {
    pub hash: u128,
    pub depth: i8,
    pub score: i32,
    pub flag: TTFlag,
    pub best_move: Coord,
}

pub enum TTFlag { Exact, LowerBound, UpperBound }
```

Fixed-size array. Bucket index is `(hash as u64) & MASK`. Full `u128` stored
for collision verification on probe. Replace-by-depth (prefer deeper entries).

Size: 2^24 entries × ~24 bytes = ~400 MB. Configurable.

## Search (`search.rs`)

```rust
pub fn search(board: &mut Board, cfg: SearchConfig) -> SearchResult {
    let mut best = None;
    for depth in 1..=cfg.max_depth {
        let score = pvs(board, depth, -INF, INF, cfg);
        if cfg.deadline_passed() { break; }
        best = Some((score, tt_best_move));
    }
    best.unwrap()
}
```

Features:
- Iterative deepening
- Principal variation search (PVS) with null-window probes
- Aspiration windows after depth 4
- Null move pruning (HeXO non-zugzwang per Nash arg)
- Late move reductions (LMR) for late ordered moves at high depth
- Quiescence: only threat moves (open-3+, open-4, closed-5)
- Check extensions: extend on opponent-creates-S0-threat

### Eval sign

Positive = X advantage. Negative = O advantage. Search uses negamax form with `to_move_sign`.

### Time management

- `cfg.time_ms` is total budget for `best_move` call
- Check deadline every N nodes (e.g. 4096)
- Soft-fail iterative deepening on deadline

## Ordering (`ordering.rs`)

Priority queue:

1. TT best move
2. Winning move (creates 6-in-row)
3. Blocks opponent winning move
4. Creates open-4 / closed-5
5. Blocks opponent open-4 / closed-5
6. Creates open-3 / rhombus
7. Killer moves at this depth
8. History heuristic score
9. Proximity to last move (Chebyshev hex dist)

Implement as bitmask + score tuple for stable sort.
