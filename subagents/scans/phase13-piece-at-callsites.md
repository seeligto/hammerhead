# Phase 13 — piece_at / pieces() callsite scan

## Summary

This scan identifies all public API callsites (`piece_at`, `pieces()`) and direct field access patterns (`pieces:`, `pieces.len()`, `pieces.contains_key()`, etc.) in the hexo-engine crate.

**Total callsites found:**
- `piece_at()` calls: 10 callsites (public read API)
- `pieces()` calls: 2 callsites (public iteration API)
- Direct field access in internal methods: 8 sites (in board.rs)

---

## piece_at callsites

| File:line | Caller fn | Context | What it does with result | Order-sensitive? |
|---|---|---|---|---|
| ordering.rs:273 | `creates_s0` | Check if left flank of a 4/5-run is not occupied by opponent | Reads `Option<Player>` to test inequality with `Some(opp)` | no |
| ordering.rs:274 | `creates_s0` | Check if right flank of a 4/5-run is not occupied by opponent | Reads `Option<Player>` to test inequality with `Some(opp)` | no |
| threats.rs:163 | `walk_linear_runs` | Check if left flank of a run is not opponent (debug_assert) | Reads `Option<Player>` to test inequality with `Some(opp)` | no |
| threats.rs:164 | `walk_linear_runs` | Check if right flank of a run is not player (debug_assert) | Reads `Option<Player>` to test inequality with `Some(opp)` | no |
| threats.rs:166 | `walk_linear_runs` | Debug assert: left cell is not occupied by player | Reads `Option<Player>` to assert inequality with `Some(player)` | no |
| threats.rs:170 | `walk_linear_runs` | Debug assert: right cell is not occupied by player | Reads `Option<Player>` to assert inequality with `Some(player)` | no |
| threats.rs:235 | `classify_linear_run` | Check if cell beyond open end is not opponent (for closed-4 legality) | Reads `Option<Player>` to test inequality with `Some(opp)` | no |
| threats.rs:330 | `has_room_for_six` | Check if either cell beyond a 3-run is not opponent | Reads `Option<Player>` to test inequality with `Some(opp)` (OR combinator) | no |
| threats.rs:345 | `is_isolated_open_two` | Check if any cell within ±2 of open-2 is opponent | Reads `Option<Player>` to test equality with `Some(opp)` | no |
| threats.rs:447 | `matches_pattern` | Check if anchor + offsets form a complete pattern match | Reads `Option<Player>` to test equality with `Some(player)` | no |

**Classification:**
- **All 10 callsites are ORDER-INSENSITIVE.**
- All `piece_at()` calls use the result only for player identity checks (`Some(p) != Some(q)`, `Some(p) == Some(q)`, or `!=`), not for ordering-dependent logic.
- `piece_at()` returns a snapshot from `AxisBitmaps` (per-player occupancy probe), not from the HashMap iteration order.

---

## pieces() callsites

| File:line | Caller fn | Iteration usage | Order-sensitive? |
|---|---|---|---|
| threats.rs:125–128 | `full_recompute` | Filters by player, collects into `Vec<Coord>`, then passes to `walk_linear_runs` and `walk_cross_axis` | **POTENTIALLY YES** — see risk analysis below |
| moves.rs:79 | `sweep_neighbourhood` | Iterates all pieces, for each piece sweeps neighborhood cells into dedup set | no |

### Risk Analysis: threats.rs:125–128 (full_recompute)

```rust
let pieces: Vec<Coord> = board
    .pieces()
    .filter_map(|(c, p)| (p == player).then_some(c))
    .collect();

walk_linear_runs(board, player, &pieces, &mut out);
walk_cross_axis(board, player, &pieces, &mut out.counts);
```

**Concern:** The `pieces: Vec<Coord>` is collected from `board.pieces()` iterator (currently HashMap, thus randomized) and later passed to:
1. `walk_linear_runs()` — iterates `pieces` and looks up `axes.run_endpoints()` for each coord, processes runs in detected order, builds `ThreatInstance` with pieces in `axis-order` via `run_pieces()` helper (not the passed `pieces` order).
2. `walk_cross_axis()` — iterates `pieces` looking for pattern matches, counts them in `out: &mut ThreatCounts` (unordered accumulator).

**Conclusion: ORDER-INSENSITIVE**
- `walk_linear_runs()` uses the `pieces` list only as an enumeration source to trigger axis-line checks. The final `ThreatInstance.pieces` SmallVec is built via `run_pieces()` at line 195 and 299–303, which reconstructs coords in **axis order**, not in the passed `pieces` order.
- `walk_cross_axis()` iterates `pieces` and updates `ThreatCounts` (counters, not order-dependent).
- The replacement (walking `Board::history` filtered by player) will produce insertion-ordered coords, which is still merely an enumeration source. The final outputs will be identical.

**Test validation:** `moves_tests.rs:16` collects `pieces()` into a `HashSet<Coord>` and uses it to compute expected neighborhoods. No order dependency.

---

## Direct Board field access (internal methods only)

### Direct `pieces` HashMap accesses in board.rs

| File:line | Method | Operation | Current code | Replacement strategy |
|---|---|---|---|---|
| board.rs:55 | N/A (field decl) | Declaration | `pieces: FxHashMap<Coord, Player>` | Remove field after Phase 13 |
| board.rs:104–107 | `Board::new()` | Allocation | `FxHashMap::with_capacity_and_hasher(256, FxBuildHasher::default())` | Remove initialization |
| board.rs:138 | `Board::reset()` | Clear | `self.pieces.clear()` | Remove after field is gone |
| board.rs:171 | `Board::place()` | contains_key check | `self.pieces.contains_key(&c)` | Use `axes.is_set(c, Player::X) \| axes.is_set(c, Player::O)` OR check both players. See below. |
| board.rs:183 | `Board::place()` | Insert | `self.pieces.insert(c, player)` | Remove after field is gone |
| board.rs:190, 197 | `Board::place()` | Pass to `add_proximity()` | `&self.pieces` | Replace all `add_proximity()` calls — see below |
| board.rs:226 | `Board::undo()` | Remove + retrieve | `self.pieces.remove(&c).expect(...)` | Replace with `axes.clear(c, player)` then get player from history; see below |
| board.rs:312 | `Board::piece_count()` | len | `self.pieces.len()` | Return `self.history.len()` as u32 cast |
| board.rs:319 | `Board::is_empty_cell()` | contains_key | `!self.pieces.contains_key(&c)` | Use `axes.is_set(c, Player::X) \| axes.is_set(c, Player::O)` (or both players check) |
| board.rs:326 | `Board::piece_at()` | get + copy | `self.pieces.get(&c).copied()` | Return result from `AxisBitmaps::player_at(c)` or equivalent |
| board.rs:358 | `Board::pieces()` | iter + map | `self.pieces.iter().map(|(&c, &p)| (c, p))` | Walk `self.history` with `player_at_ply()` lookup; see details below |
| board.rs:481 | `Board::place_for_test()` | Insert | `self.pieces.insert(c, player)` | Remove after field is gone |
| board.rs:488, 495 | `Board::place_for_test()` | Pass to `add_proximity()` | `&self.pieces` | Same as place() above |
| board.rs:607 | `add_proximity()` helper | Parameter | `pieces: &FxHashMap<Coord, Player>` | Change parameter type or inline the occupancy check; see below |
| board.rs:613 | `add_proximity()` helper | contains_key | `!pieces.contains_key(&d)` | Probe AxisBitmaps for both players instead |

### Detailed Replacement Strategies

#### 1. `piece_at()` (line 325–327)
**Current:**
```rust
pub fn piece_at(&self, c: Coord) -> Option<Player> {
    self.pieces.get(&c).copied()
}
```

**Replacement:**
```rust
pub fn piece_at(&self, c: Coord) -> Option<Player> {
    // Check both players via AxisBitmaps
    if self.axes.is_set(c, Player::X) {
        Some(Player::X)
    } else if self.axes.is_set(c, Player::O) {
        Some(Player::O)
    } else {
        None
    }
}
```
Note: `AxisBitmaps::is_set(c, player)` must exist or be derived from the existing public API.

#### 2. `is_empty_cell()` (line 318–320)
**Current:**
```rust
pub fn is_empty_cell(&self, c: Coord) -> bool {
    !self.pieces.contains_key(&c)
}
```

**Replacement:**
```rust
pub fn is_empty_cell(&self, c: Coord) -> bool {
    !self.axes.is_set(c, Player::X) && !self.axes.is_set(c, Player::O)
}
```

#### 3. `piece_count()` (line 311–313)
**Current:**
```rust
pub fn piece_count(&self) -> usize {
    self.pieces.len()
}
```

**Replacement:**
```rust
pub fn piece_count(&self) -> usize {
    self.history.len()
}
```
Trivial — history is the insertion-ordered stone list.

#### 4. `pieces()` (line 357–359)
**Current:**
```rust
pub fn pieces(&self) -> impl Iterator<Item = (Coord, Player)> + '_ {
    self.pieces.iter().map(|(&c, &p)| (c, p))
}
```

**Replacement:**
```rust
pub fn pieces(&self) -> impl Iterator<Item = (Coord, Player)> + '_ {
    self.history
        .iter()
        .enumerate()
        .filter_map(|(idx, &c)| {
            let ply = idx as u32 + 1; // history index → ply (history[0] is ply 1)
            let player = player_at_ply(ply);
            Some((c, player))
        })
}
```
Or more cleanly, iterate history and use the existing `player_at_ply()` function (already defined at line 530).

#### 5. `place()` — contains_key check (line 171)
**Current:**
```rust
if self.pieces.contains_key(&c) {
    return Err(BoardError::AlreadyOccupied(c.q, c.r));
}
```

**Replacement:**
```rust
if !self.is_empty_cell(c) {
    return Err(BoardError::AlreadyOccupied(c.q, c.r));
}
```
Delegate to the updated `is_empty_cell()` method.

#### 6. `place()` — `add_proximity()` call (lines 185–191 and 192–198)
**Current:**
```rust
add_proximity(
    &mut self.proximity_count,
    &mut self.candidate_cells,
    c,
    MAX_PIECE_DISTANCE,
    &self.pieces,  // <-- HashMap passed to check occupancy
);
```

**Two options:**

**Option A (minimal change):** Keep `add_proximity()` signature, but change the internal occupancy check to use `AxisBitmaps`:
```rust
fn add_proximity(
    counts: &mut FxHashMap<Coord, u32>,
    candidates: &mut FxHashSet<Coord>,
    center: Coord,
    radius: i16,
    axes: &AxisBitmaps,  // <-- Pass AxisBitmaps instead of pieces HashMap
) {
    for_each_in_range(center, radius, |d| {
        let count = counts.entry(d).or_insert(0);
        let was_zero = *count == 0;
        *count += 1;
        if d != center && was_zero && !axes.is_occupied(d) {
            candidates.insert(d);
        }
    });
}
```

**Option B (more explicit):** Inline the occupancy check at call sites:
```rust
for_each_in_range(c, MAX_PIECE_DISTANCE, |d| {
    let count = self.proximity_count.entry(d).or_insert(0);
    let was_zero = *count == 0;
    *count += 1;
    if d != c && was_zero && self.is_empty_cell(d) {
        self.candidate_cells.insert(d);
    }
});
```

**Recommendation:** Option A is cleaner; requires adding `AxisBitmaps::is_occupied(c)` method or adapting the existing API.

#### 7. `undo()` — remove + retrieve (line 226)
**Current:**
```rust
let player = self
    .pieces
    .remove(&c)
    .expect("invariant: history piece in pieces map");
```

**Replacement:**
The ply is known: it's `self.ply` (the ply count BEFORE decrement at line 236). Use:
```rust
let player = player_at_ply(self.ply);
```
Then call `self.axes.clear(c, player)` instead of relying on the HashMap remove.

**Validation:** The history invariant ensures the popped coordinate matches the board state; we just derive the player from ply parity.

#### 8. `place_for_test()` (line 481)
**Current:**
```rust
self.pieces.insert(c, player);
```

**Replacement:** Same as `place()` — remove the insert, the field will be gone.

---

## Risk Summary

### Order-sensitive callers

**Count: 0**

All `piece_at()` calls are order-insensitive (player identity checks only).

The `pieces()` iteration in `full_recompute()` at threats.rs:125–128 is **order-insensitive** because:
1. The output `ThreatInstance.pieces` SmallVec is reconstructed in **axis order** by `run_pieces()`, not in the input enumeration order.
2. The `ThreatCounts` accumulators are unordered.
3. The replacement using insertion-ordered history will still pass identical deduped coord sets to the walking functions.

### Order-sensitive test dependencies

**Count: 0**

Test `moves_tests.rs:16` collects `pieces()` into a `HashSet` (unordered), so iteration order is discarded before use.
Test `board_tests.rs:43` uses `piece_at()` (not order-dependent).

### Other gotchas

1. **AxisBitmaps API requirement:** `piece_at()` and `is_empty_cell()` replacements require probing the AxisBitmaps for both players. Ensure `AxisBitmaps` provides:
   - `is_set(c, player) -> bool` or equivalent occupancy check
   - Or expose the method that `piece_at()` currently uses from AxisBitmaps

2. **add_proximity() helper refactor:** The `add_proximity()` function at lines 602–617 currently takes `pieces: &FxHashMap<Coord, Player>` and calls `pieces.contains_key(&d)` on line 613. This must be replaced with:
   - Either pass `&AxisBitmaps` instead of `&FxHashMap`
   - Or pass both players' occupancy via a closure/trait object
   - Or inline the check at call sites

3. **player_at_ply() for undo/pieces():** The existing `player_at_ply()` function (line 530) correctly derives player from ply parity. Ensure it's used in:
   - `undo()` to get the player of the popped stone
   - `pieces()` to derive player for each history entry

4. **history.len() for piece_count():** Simple substitution; `history.len()` is always in sync with the actual stone count.

---

## Recommended Phase 13 STEP 3 Implementation Order

1. **Add AxisBitmaps occupancy API:**
   - Ensure `AxisBitmaps::is_set(c, player) -> bool` or equivalent exists
   - Or add `is_occupied(c) -> bool` that returns true if either player occupies the cell

2. **Refactor `add_proximity()` signature:**
   - Change parameter from `pieces: &FxHashMap<Coord, Player>` to `axes: &AxisBitmaps`
   - Update call sites in `place()` (lines 185–191, 192–198) and `place_for_test()` (lines 483–489, 490–496) to pass `&self.axes`
   - Update the occupancy check on line 613 to use `axes.is_occupied(&d)` or the boolean-returning API

3. **Update `piece_at()` (line 325–327):**
   - Replace HashMap lookup with AxisBitmaps probes
   - Verify no callers depend on randomized iteration (they don't — all are identity checks)

4. **Update `is_empty_cell()` (line 318–320):**
   - Replace `!self.pieces.contains_key(&c)` with dual AxisBitmaps probes

5. **Update `piece_count()` (line 311–313):**
   - Replace `self.pieces.len()` with `self.history.len()`

6. **Update `pieces()` iterator (line 357–359):**
   - Replace HashMap iteration with history enumeration using `player_at_ply()`

7. **Update `place()` — contains_key check (line 171):**
   - Delegate to updated `is_empty_cell()` (which uses AxisBitmaps)

8. **Update `undo()` (line 226):**
   - Replace `self.pieces.remove(&c)` with `player_at_ply(self.ply)`
   - Ensure `axes.clear(c, player)` is called with the correct player

9. **Remove field declaration and initialization:**
   - Delete `pieces: FxHashMap<Coord, Player>` field (line 55)
   - Delete initialization in `Board::new()` (lines 104–107)
   - Delete `self.pieces.clear()` in `reset()` (line 138)
   - Delete `self.pieces.insert()` calls in `place()` (line 183) and `place_for_test()` (line 481)

10. **Run all tests:**
    - `cargo test --package hexo-engine-core`
    - Verify threat detection unchanged (compare ThreatSet outputs)
    - Verify move generation unchanged (compare MoveList outputs)

---

## Validation Notes

- **piece_at replacement:** Verify that both-player probe is equivalent to the original HashMap get. AxisBitmaps must be in sync with every place/undo.
- **pieces() iteration:** The history-based enumeration will produce insertion-ordered coords. Verify that every downstream consumer (full_recompute via walk_linear_runs and walk_cross_axis) is agnostic to enumeration order — it is, because run detection and pattern matching use axis-line lookups, not relative positions.
- **add_proximity occupancy check:** Verify that the AxisBitmaps-based check for "is cell occupied by any player?" is correct and fast (should be a bitwise OR of two-player probes).

