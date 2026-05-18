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
}

#[repr(u8)]
pub enum Player { X = 0, O = 1 }
```

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
  insert into candidates. Remove `c` itself from candidates.
- `undo(c)`: reverse. Decrement counts; remove from candidates when count hits
  0. Re-insert `c` if its remaining proximity count > 0 (other pieces still in
  range).
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

## Win Detection (`win.rs`)

After each `place(c)`: scan 3 axes through `c`.

```rust
pub fn is_winning_move(board: &Board, c: Coord, p: Player) -> bool {
    for axis in AXES {
        if line_length_through(board, c, axis, p) >= 6 { return true; }
    }
    false
}
```

Walk back up to 5 along axis (stop at non-`p`), then forward, count consecutive `p`. O(1) bounded by 11 cells × 3 axes = 33 lookups.

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
