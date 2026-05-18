# HeXO Engine — Internals Spec

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
    hash: u64,
    ply: u32,                      // total stones placed
    history: Vec<Coord>,
    candidate_cells: FxHashSet<Coord>,  // empties near pieces
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
fn is_empty(&self, c: Coord) -> bool;
fn hash(&self) -> u64;
```

### Legality

- empty cell
- if `ply == 0`: must be origin `(0, 0)`
- else: `min hex_dist to any piece ≤ 8`

Maintain candidate set incrementally:
- on `place(c)`: insert all empties within radius 2 around `c`
- on `undo`: remove cells no longer adjacent to any piece (use refcount or rebuild lazy)

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

Each `(q, r, player)` → random `u64`. XOR-update on place/undo.

Option A (bounded preallocation):
- Window: q,r in [-127, 127]. 255² × 2 players × 8 bytes ≈ 1 MB. Init once. Outside window: hash on demand.

Option B (lazy):
- `FxHashMap<(Coord, Player), u64>`. Populate on first access. Slower but unbounded.

Recommendation: Option A with fallback to B for far cells.

## Transposition Table (`tt.rs`)

```rust
pub struct TTEntry {
    pub hash: u64,
    pub depth: i8,
    pub score: i32,
    pub flag: TTFlag,
    pub best_move: Coord,
}

pub enum TTFlag { Exact, LowerBound, UpperBound }
```

Fixed-size array. Index by `hash & MASK`. Replace-by-depth (prefer deeper entries).

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
