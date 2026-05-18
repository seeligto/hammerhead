# HeXO Bot — Implementation Roadmap

Save as `specs/SPEC_ROADMAP.md`.

## Status

| Phase | Module(s) | State |
|---|---|---|
| 1 | `coords`, `board`, `zobrist` | ✅ done |
| 2 | `win` + line bitmaps | next |
| 3 | `moves` | next |
| 4 | `threats` | pending |
| 5 | `eval` | pending |
| 6 | `tt` | pending |
| 7 | `ordering` | pending |
| 8 | `search` | pending |
| 9 | `pybind` + Python `Bot` | pending |

Order is fixed. Each phase depends on the previous.

## Core decisions (locked)

### Two-stone turn → minimax form

`Board::to_move()` returns the player whose turn it is on the *current* ply
(already handles double-stone turns via the `(p-1)/2 % 2` parity rule).
Search uses **minimax form**, not negamax:

```rust
fn search(board, depth, alpha, beta) -> i32 {
    if depth == 0 || terminal { return eval(board); }
    match board.to_move() {
        Player::X => maximize(board, depth, alpha, beta),
        Player::O => minimize(board, depth, alpha, beta),
    }
}
```

Within-turn continuation is automatic: when X plays stone 1 of their turn,
`to_move()` still returns X for stone 2. Search recurses into another
`maximize` call. No sign flip needed mid-turn.

**Why not negamax?** Negamax assumes side flips every ply. Two-stone turns
break that assumption. Mixed-flip negamax is possible but error-prone (one
mis-placed minus and search is silently wrong). Minimax form is verbose but
literal.

**Eval sign convention**: positive = X advantage, negative = O advantage.
Same as current spec. Never side-relative.

### Engine API granularity

`Engine::best_move()` returns **one stone**, not a pair. The Python `Bot`
calls it twice per turn:

```python
def play(self) -> tuple[tuple[int,int], tuple[int,int]]:
    a = self.engine.best_move(time_ms=t)
    self.engine.place(a)
    b = self.engine.best_move(time_ms=t)
    self.engine.place(b)
    return a, b
```

Or for an external integration that wants atomic turns, wrap as:

```python
def play_turn(self) -> list[tuple[int,int]]:
    moves = []
    moves.append(self.engine.best_move(...))
    self.engine.place(moves[-1])
    if not self.engine.winner():
        moves.append(self.engine.best_move(...))
        self.engine.place(moves[-1])
    return moves
```

Benefits:
- Engine never knows about turns. Pure stone-level search.
- TT entries from stone 1's search are immediately useful for stone 2 (same
  position, deeper PV).
- Time budget can be split asymmetrically (e.g. 30 % on stone 1, 70 % on
  stone 2 when stone 1 is forced).

### Bitmap representation: per-axis line bitmaps

Infinite hex board → cannot use a fixed 2D bitboard. Instead, **per-axis
sparse line bitmaps**.

For each of 3 axes × 2 players = 6 collections, store
`FxHashMap<LineId, LineBitmap>`. Each `LineBitmap` is a packed `Vec<u64>`
with a `base_pos: i16` anchor.

| Axis | Line ID | Position along line |
|---|---|---|
| Q (horizontal) | `r` | `q` |
| R (diagonal 1) | `q` | `r` |
| S (diagonal 2) | `q + r` | `q` |

Operations:
- `set(c, player)`: locate or insert line, set bit
- `clear(c, player)`: clear bit (used by `undo`)
- `count_run_through(c, axis, player) -> u8`: walk ±5 bits, count consecutive
- `window6(c, axis, player) -> u8`: extract 6-bit window for eval

Win detection: `count_run_through(c, axis, mover) >= 6` for any of 3 axes.

Eval layer 1 (window scan) uses the same line bitmaps: slide the 6-bit window
along each line, popcount own bits, look up score table.

### Augmentation / rotation

Hex grid has D6 symmetry (6 rotations + 6 reflections = 12 transforms).
Per-axis line bitmaps make rotation almost free: a 60° rotation **permutes
the three axes**.

Concretely, if we define rotation `R60: (q, r) → (-r, q + r)`:
- Axis Q lines → Axis S lines (with line IDs remapped)
- Axis R lines → Axis Q lines
- Axis S lines → Axis R lines

For minimax search we **do not** canonicalize the hash by rotation — it
breaks incremental update. Instead, expose a separate `canonical_hash()`
helper that computes the min over 12 transforms. Useful for:
- Opening book lookup
- Self-play data augmentation
- Cross-rotation transposition detection in offline analysis

Defer canonical_hash impl until self-play data export phase (post-MVP).

The bitmap design supports it cheaply when we get there.

### Performance targets

| Op | Target | Comment |
|---|---|---|
| `place` | < 500 ns | board update + bitmap set + zobrist xor |
| `undo` | < 500 ns | symmetric |
| `is_winning_move` | < 100 ns | bitmap line walk |
| `generate(r=2)` | < 5 μs | < 50 candidates typical |
| `eval` (layer 1 only) | < 5 μs | incremental window cache |
| `search` NPS | > 200 k nps | release build, full eval, single-thread |

Numbers are estimates. Benchmark on phase 8 completion. Adjust if needed.

## Phase sketches

### Phase 2 — `win` + line bitmaps (next)

See dedicated prompt. New module `axis_bitmap.rs` shared by `win` and (later)
`threats`/`eval`.

### Phase 3 — `moves` (next)

See dedicated prompt. Adds second proximity refcount for radius 2.

### Phase 4 — `threats`

Inputs: `Board`, `axis_bitmap` data, last-placed cell.
Outputs: `ThreatCounts` (WSC tuples per spec), per player.

Algorithm:
1. For each piece of player P near last move (radius 5): walk lines on 3
   axes, classify endpoints (open/closed), match shape table.
2. Cross-axis shapes (rhombus, triangle, bone): cluster scan within radius 2
   of last move.
3. Cache `ThreatCounts` per player on board. Invalidate on each `place(c)`,
   recompute only within radius 5 of `c`.

Forks (mate-via-multi-threat) are computed by `eval`, not `threats`. The
threats module just produces counts and a list of S0 threat positions with
defense sets.

### Phase 5 — `eval`

Layer 1: window scan (already supported by axis bitmaps — popcount per
window).
Layer 2: shape bonus (sum `ThreatCounts × weight` from `hexo.toml`).
Layer 3: fork detection (defense-set vertex cover ≥ 3 ⇒ mate).
Tempo: small advisory term.

Eval result cached per board state. Invalidated on `place`/`undo`. Cache is
incremental for layer 1 (only affected windows recompute); layers 2 and 3
recompute locally near last move.

Defer **radius-theory colony discounting** (HeXO Radius Theory PDF) to a
sub-phase after baseline eval works against SealBot. Pure additive feature.

### Phase 6 — `tt`

Fixed-size flat array. Power-of-two size from `hexo.toml`. Index by low
`TT_INDEX_BITS` of `u128 hash`. Full `u128` stored in entry for collision
verification.

Replacement: depth-preferred. Two-bucket scheme (always-replace +
depth-preferred) is a possible v2 enhancement.

Aging: increment generation per root search. Older entries replaceable.

### Phase 7 — `ordering`

Stable priority bucket sort per spec. Buckets:
1. TT best move
2. Wins (creates 6-in-row)
3. Defensive wins (blocks opponent's would-be 6-in-row)
4. Creates S0 threat (open-4 / closed-5)
5. Blocks opponent S0 threat
6. Creates S1 threat (open-3 / rhombus / arch / bone)
7. Killer moves at this ply
8. History heuristic
9. Proximity to last move (Chebyshev tie-break)

Implementation: `u32 score = (bucket << 24) | history_score`. Sort
descending.

### Phase 8 — `search`

Features in roughly this build order:
1. Plain alpha-beta (minimax form) + iterative deepening + TT
2. PVS (null-window + re-search)
3. Aspiration windows
4. Killer moves + history heuristic (feed ordering)
5. Late move reductions (LMR) at high depth, late ordered moves
6. Quiescence search: extend only threat moves (S0+) until quiet
7. Check extensions: opponent creates S0 → extend depth
8. **Null move pruning: SKIP for v1.** Strategy-stealing argument suggests
   it's theoretically sound, but two-stone turns make implementation fragile.
   Revisit post-baseline.

Time management: deadline check every N nodes (`config::DEADLINE_CHECK_NODES`,
default 4096). Soft-fail iterative deepening.

### Phase 9 — `pybind` + Python `Bot`

PyO3 surface per `SPEC_API.md`. GIL released around `best_move` via
`py.allow_threads`. Python `Bot` wraps `Engine` with `BotConfig` defaults.

Add `Bot.play_turn()` for atomic 2-stone turns. Add `Engine.find_pv()` to
return the principal variation as a list of `(q, r)` for analysis tools.

## Out of scope for v1

- Null-move pruning (revisit)
- MCTS / hybrid (purely alpha-beta first)
- Neural net eval (purely hand-crafted threat eval first)
- Multi-threaded search (lazy-SMP later)
- Opening book (collect self-play games first)
- Endgame tables (game is theoretically a draw with perfect play, so n/a)
- WebSocket / SealBot live integration (separate workstream)

## File summary of work ahead

New Rust modules to create:
- `hexo-engine/src/axis_bitmap.rs` — shared by win/threats/eval

Modules getting real impl (currently stubs):
- `win.rs` (phase 2)
- `moves.rs` (phase 3)
- `threats.rs` (phase 4)
- `eval.rs` (phase 5)
- `tt.rs` (phase 6)
- `ordering.rs` (phase 7)
- `search.rs` (phase 8)
- `pybind.rs` (phase 9)

Python files getting real impl:
- `hexo/hexo/bot.py` (phase 9)
- `hexo/hexo/game.py` (phase 9)
- `hexo/hexo/notation.py` (phase 9, optional — could stay stub)

## References

- Connect-6 + Alpha-Beta-TSS: Wu et al., NCKU group
- CodeCup 2020 Gomoku winner write-up: rotated bitboards + threat board
- Yixin engine architecture: TT + PVS + LMR + threat-aware quiescence
- Stockfish: general alpha-beta best practices
