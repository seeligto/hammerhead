# Opening Book Construction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Analyze `data/*.json` human Hexo corpus and emit a canonical opening book + opening tree + theory index + coverage report.

**Architecture:** Pure-Python offline pipeline. Stage 1 normalises each game position via the 12-element hex dihedral group (D6), hashes via deterministic 64-bit Zobrist, walks each game ply-by-ply emitting `(position_hash, canonical_move)` records. Stage 2 aggregates per-position move stats (overall and high-ELO subset), builds the parent-child opening tree to depth 16, matches named openings, flags blunder candidates, and writes binary `opening_book.bin` + JSON `opening_tree.json` + `theory_index.json`. ELO-weighted winrate uses 3× weight on `elo >= 1300` games.

**Tech Stack:** Python 3.10+ stdlib only (no new deps); pytest for tests; struct module for binary record format; hexagonal coordinates in axial form `(q, r)` with `s = -q - r` (matches `hexo-engine/src/coords.rs`).

---

## File Structure

All new code lives under `scripts/openbook/`. Caller-supplied data and outputs go under `data/analysis/`.

```
scripts/
  build_opening_book.py            thin entry point: `python -m openbook` or direct
  openbook/
    __init__.py                    empty marker
    symmetry.py                    12 hex transforms (D6) + canonical form selection
    zobrist.py                     64-bit Zobrist keys (lazy dict, deterministic seed)
    walker.py                      iterate game JSON → (state, played_move) tuples
    aggregator.py                  per-position move stats (overall + high-ELO)
    tree.py                        parent-child graph + KL divergence over distributions
    theory.py                      named opening matcher (HeXOpedia patterns)
    blunders.py                    blunder candidate flagging
    turn_struct.py                 stone-1/stone-2 distance and conditional stats
    locality.py                    per-stone axis-locality histograms
    io_book.py                     binary writer + JSON writers
    report.py                      markdown coverage report
    main.py                        CLI driver wiring all modules
    tests/
      __init__.py                  empty
      conftest.py                  prepend `scripts/` to sys.path
      test_symmetry.py
      test_zobrist.py
      test_walker.py
      test_aggregator.py
      test_tree.py
      test_io_book.py
      test_locality.py
      test_turn_struct.py
      test_blunders.py
      test_theory.py
data/analysis/
  hexopedia_patterns.json          caller-supplied pattern list (placeholder shipped)
  opening_book.bin                 OUTPUT: sorted binary records
  opening_tree.json                OUTPUT: graph for visualization
  theory_index.json                OUTPUT: hash → opening_name
  REPORT_BOOK.md                   OUTPUT: coverage curve + summary
```

### Hot-path note

The engine's hot-path discipline (FxHashMap, `#[inline]`, no alloc in search) does **not** apply to this offline analysis tool. Use idiomatic Python: built-in `dict`, list, dataclasses, `Counter`. The pipeline runs ~once per corpus refresh; readability beats microsecond optimisation.

### Pytest discovery

`make test` runs `pytest` in `hexo/` directory. New tests live under `scripts/openbook/tests/` and are run via `pytest scripts/openbook/` from repo root. `conftest.py` adds `scripts/` to `sys.path` so `import openbook.<module>` resolves.

### Source-tree gitignore policy

**The openbook code is local-only.** It follows the precedent of `scripts/analyze_human_games.py` (already in `.gitignore`): not part of the engine, kept out of the repo. The first task adds `scripts/openbook/` and `scripts/build_opening_book.py` to `.gitignore`. That `.gitignore` change is the **only** commit produced by this plan. All output artefacts (`data/analysis/*`) are already covered by the existing `data/` ignore rule.

### Binary record format (locked)

```
struct format: '<QhhHHIh'   little-endian, no padding
fields:
  hash        u64    canonical position Zobrist (8 bytes)
  move_q      i16    canonical played-stone q (2 bytes)
  move_r      i16    canonical played-stone r (2 bytes)
  weight      u16    encoded ELO-weighted weight, see below (2 bytes)
  winrate     u16    fixed-point: round(winrate * 65535) (2 bytes)
  n_games     u32    raw count of games passing through (4 bytes)
  engine_score i16   placeholder, written as 0 (2 bytes)
total: 22 bytes/record
```

`weight` = `min(65535, round(n_low_elo + 3 * n_high_elo))` where `high_elo = max_player_elo_in_game >= 1300`.

Records are sorted by `(hash, -weight, move_q, move_r)` so binary-search lookups by hash return moves heaviest-first.

---

## Self-Review Pre-Notes

Spec requirements mapped to tasks:
1. Canonical position hash (12 transforms, Zobrist with halfmove + side) → Tasks 1, 2, 4
2. Per-position move stats (winrate, ELO, time) → Task 4
3. Opening tree to depth 16 + KL divergence theory junctions → Task 6
4. Named opening detection (HeXOpedia patterns) → Task 7
5. Blunder candidates (best move winrate < 0.4 & n >= 20) → Task 8
6. Turn structure (stone-1/stone-2 distance + conditional) → Task 9
7. Axis locality (own/opp within radius 2 on each axis) → Task 10
8. Output formats (bin/json) + ELO bucketing (≥1300) + 3× weight → Tasks 4, 11
9. Coverage curve at depths 4/8/12/16 → Task 12
10. CLI driver + Makefile target → Task 13

---

## Task 0: Bootstrap directory + package skeleton + gitignore

**Files:**
- Modify: `.gitignore` — add `scripts/openbook/` and `scripts/build_opening_book.py`
- Create: `scripts/openbook/__init__.py` (empty)
- Create: `scripts/openbook/tests/__init__.py` (empty)
- Create: `scripts/openbook/tests/conftest.py`
- Create: `data/analysis/hexopedia_patterns.json` (placeholder; lives under already-ignored `data/`)

- [ ] **Step 1: Update `.gitignore`**

Append under the existing "local-only analysis scripts" section:

```
scripts/openbook/
scripts/build_opening_book.py
```

- [ ] **Step 2: Create `scripts/openbook/__init__.py`**

```python
"""Opening book construction pipeline. Offline analysis only — no engine deps."""
```

- [ ] **Step 3: Create `scripts/openbook/tests/__init__.py`** (empty file)

- [ ] **Step 4: Create `scripts/openbook/tests/conftest.py`**

```python
"""Make `import openbook.<mod>` resolve when pytest is run from repo root."""
import sys
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[2]
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))
```

- [ ] **Step 5: Create placeholder pattern file `data/analysis/hexopedia_patterns.json`**

```json
{
  "_comment": "Caller supplies named opening patterns extracted from refs/The HeXOpedia.pdf. Each pattern: name + stones list in canonical (q,r) form. Empty list ships by default; theory_index.json will be empty until populated.",
  "patterns": []
}
```

- [ ] **Step 6: Smoke-test the import path**

Run: `cd /home/timmy/Work/hexo_minimax && python -c "import sys; sys.path.insert(0, 'scripts'); import openbook; print('ok')"`
Expected: `ok`

- [ ] **Step 7: Verify openbook is now ignored**

Run: `cd /home/timmy/Work/hexo_minimax && git status --ignored scripts/openbook scripts/build_opening_book.py 2>&1 | head -20`
Expected: lines show `scripts/openbook/` is ignored (no untracked files reported under it).

- [ ] **Step 8: Commit `.gitignore` only**

```bash
cd /home/timmy/Work/hexo_minimax
git add .gitignore
git commit -m "gitignore: openbook scripts (local-only analysis tool)"
```

---

## Task 1: Hex D6 symmetry transforms

**Files:**
- Create: `scripts/openbook/symmetry.py`
- Create: `scripts/openbook/tests/test_symmetry.py`

The hex point group D6 has 12 elements: 6 rotations × {identity, reflection}.
In cube coords `(q, r, s)` with `s = -q - r`:
- Rotate 60° CW: `(q, r, s) → (-r, -s, -q)`
- Reflect across q-axis: `(q, r, s) → (q, s, r)` (swap r, s)

In axial `(q, r)`, written using only `q, r`:
- Identity: `(q, r) → (q, r)`
- R60:     `(q, r) → (-r, q + r)`
- R120:    `(q, r) → (-q - r, q)`
- R180:    `(q, r) → (-q, -r)`
- R240:    `(q, r) → (r, -q - r)`
- R300:    `(q, r) → (q + r, -q)`
- ReflectQ (swap r↔s):       `(q, r) → (q, -q - r)`
- ReflectQ ∘ R60:             `(q, r) → (q + r, -r)`
- ReflectQ ∘ R120:            `(q, r) → (r, q)`
- ReflectQ ∘ R180:            `(q, r) → (-q, q + r)`
- ReflectQ ∘ R240:            `(q, r) → (-q - r, r)`
- ReflectQ ∘ R300:            `(q, r) → (-r, -q)`

Canonical form: for a list of `(cell, player)` pairs, apply each of the 12 transforms, normalise each into a sorted tuple of `((q, r), player_byte)`, choose lex-smallest. Return both the canonical pair list AND the transform index that produced it (so the same transform can be applied to the move-to-play).

- [ ] **Step 1: Write failing tests for the 12 transforms**

```python
# scripts/openbook/tests/test_symmetry.py
from openbook.symmetry import (
    TRANSFORMS, apply_transform, canonicalize, canonicalize_with_move,
)


def test_there_are_exactly_twelve_transforms():
    assert len(TRANSFORMS) == 12


def test_identity_is_first_transform():
    assert apply_transform(0, (3, -2)) == (3, -2)
    assert apply_transform(0, (0, 0)) == (0, 0)


def test_r60_rotates_unit_q_to_minus_r_axis():
    # (1, 0) under 60° CW rotation → (0, 1)
    assert apply_transform(1, (1, 0)) == (0, 1)


def test_r180_negates_both_coords():
    assert apply_transform(3, (4, -1)) == (-4, 1)


def test_each_transform_preserves_origin():
    for t in range(12):
        assert apply_transform(t, (0, 0)) == (0, 0)


def test_each_transform_preserves_hex_distance_from_origin():
    # hex_distance((q,r), origin) = (|q| + |r| + |q+r|) / 2
    def hd(c):
        q, r = c
        return (abs(q) + abs(r) + abs(q + r)) // 2

    for t in range(12):
        for c in [(1, 0), (0, 1), (2, -1), (3, -3), (4, 1), (-2, 5)]:
            assert hd(apply_transform(t, c)) == hd(c)


def test_each_transform_is_an_involution_of_its_inverse():
    # Composing all 12 should be a group: applying any transform to all 12
    # outputs yields the same set, just permuted.
    sample = [(1, 0), (2, -3), (-1, 4)]
    out = []
    for t in range(12):
        out.append(tuple(sorted(apply_transform(t, c) for c in sample)))
    # All 12 outputs must be distinct (no two transforms collapse on this sample).
    assert len(set(out)) == 12


def test_canonicalize_returns_lex_smallest_orbit_member():
    # Single stone at (1, 0). The 12 transforms produce all 6 unit hexes
    # (each appearing twice due to reflection). Lex-smallest among
    # {(1,0), (0,1), (-1,1), (-1,0), (0,-1), (1,-1)} is (-1, 0).
    stones = [((1, 0), 0)]
    canon, t_idx = canonicalize(stones)
    assert canon == (((-1, 0), 0),)


def test_canonicalize_sorts_within_canonical_form():
    stones = [((2, 0), 0), ((1, 1), 1), ((-1, 0), 0)]
    canon, _ = canonicalize(stones)
    # canon is a sorted tuple
    assert list(canon) == sorted(canon)


def test_canonicalize_with_move_applies_same_transform_to_move():
    # Pick a configuration whose canonical transform is non-trivial.
    stones = [((3, -2), 0)]  # under transform 3 (R180), this becomes (-3, 2)
    move = (4, -1)
    canon_stones, canon_move, _ = canonicalize_with_move(stones, move)
    # canonical_move must match: applying the chosen transform to move
    # produces canon_move.
    from openbook.symmetry import apply_transform as _apply
    # Re-derive: among the 12 results, find which produced canon_stones.
    # canonicalize_with_move must give a self-consistent answer.
    for t in range(12):
        if tuple(sorted(((_apply(t, c), p) for (c, p) in stones))) == canon_stones:
            assert canon_move == _apply(t, move)
            return
    raise AssertionError("no transform matched canonical form")
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `cd /home/timmy/Work/hexo_minimax && pytest scripts/openbook/tests/test_symmetry.py -v`
Expected: ImportError (module not yet created).

- [ ] **Step 3: Implement `scripts/openbook/symmetry.py`**

```python
"""Hex D6 (12-element) point-group transforms in axial coords.

Cube coords (q, r, s) with s = -q - r. We express each transform purely in
(q, r). TRANSFORMS[t] is a function (q, r) -> (q', r').
"""
from __future__ import annotations

from typing import Callable, Sequence

Cell = tuple[int, int]
StonePair = tuple[Cell, int]


def _id(q: int, r: int) -> Cell:
    return (q, r)


def _r60(q: int, r: int) -> Cell:
    return (-r, q + r)


def _r120(q: int, r: int) -> Cell:
    return (-q - r, q)


def _r180(q: int, r: int) -> Cell:
    return (-q, -r)


def _r240(q: int, r: int) -> Cell:
    return (r, -q - r)


def _r300(q: int, r: int) -> Cell:
    return (q + r, -q)


def _ref(q: int, r: int) -> Cell:
    return (q, -q - r)


def _ref_r60(q: int, r: int) -> Cell:
    return (q + r, -r)


def _ref_r120(q: int, r: int) -> Cell:
    return (r, q)


def _ref_r180(q: int, r: int) -> Cell:
    return (-q, q + r)


def _ref_r240(q: int, r: int) -> Cell:
    return (-q - r, r)


def _ref_r300(q: int, r: int) -> Cell:
    return (-r, -q)


TRANSFORMS: tuple[Callable[[int, int], Cell], ...] = (
    _id, _r60, _r120, _r180, _r240, _r300,
    _ref, _ref_r60, _ref_r120, _ref_r180, _ref_r240, _ref_r300,
)


def apply_transform(t: int, c: Cell) -> Cell:
    return TRANSFORMS[t](c[0], c[1])


def canonicalize(
    stones: Sequence[StonePair],
) -> tuple[tuple[StonePair, ...], int]:
    """Return (canonical_sorted_stones, transform_index)."""
    best: tuple[StonePair, ...] | None = None
    best_t = 0
    for t in range(12):
        fn = TRANSFORMS[t]
        transformed = tuple(sorted(
            ((fn(c[0], c[1]), p) for (c, p) in stones)
        ))
        if best is None or transformed < best:
            best = transformed
            best_t = t
    assert best is not None
    return best, best_t


def canonicalize_with_move(
    stones: Sequence[StonePair],
    move: Cell,
) -> tuple[tuple[StonePair, ...], Cell, int]:
    """Canonicalize the position, return canonical-frame move as well."""
    canon, t = canonicalize(stones)
    return canon, TRANSFORMS[t](move[0], move[1]), t
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_symmetry.py -v`
Expected: all 9 tests PASS. (openbook source is gitignored — no commit.)

---

## Task 2: 64-bit Zobrist hashing

**Files:**
- Create: `scripts/openbook/zobrist.py`
- Create: `scripts/openbook/tests/test_zobrist.py`

64-bit Zobrist over canonical `((q, r), player)` pairs plus side-to-move and stones-remaining (1 or 2). Lazy dict per `(cell, player)` keyed off a deterministic `random.Random(seed)`. Side-to-move and stones-remaining are fixed pre-drawn constants.

Player encoding: `0 = X` (first player to place), `1 = O`.

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_zobrist.py
from openbook.zobrist import position_hash, ZobristTable


def test_hash_is_64_bit():
    t = ZobristTable()
    h = position_hash(t, stones=[], side_to_move=0, stones_remaining=1)
    assert 0 <= h < (1 << 64)


def test_empty_position_hash_only_uses_side_and_remaining():
    t = ZobristTable()
    h1 = position_hash(t, stones=[], side_to_move=0, stones_remaining=1)
    h2 = position_hash(t, stones=[], side_to_move=0, stones_remaining=1)
    assert h1 == h2
    h3 = position_hash(t, stones=[], side_to_move=1, stones_remaining=1)
    assert h1 != h3
    h4 = position_hash(t, stones=[], side_to_move=0, stones_remaining=2)
    assert h1 != h4


def test_hash_is_order_invariant_for_stones():
    t = ZobristTable()
    s1 = [((0, 0), 0), ((1, 0), 1), ((2, -1), 0)]
    s2 = [((2, -1), 0), ((0, 0), 0), ((1, 0), 1)]
    h1 = position_hash(t, stones=s1, side_to_move=0, stones_remaining=1)
    h2 = position_hash(t, stones=s2, side_to_move=0, stones_remaining=1)
    assert h1 == h2


def test_adding_a_stone_changes_the_hash():
    t = ZobristTable()
    h0 = position_hash(t, stones=[((0, 0), 0)], side_to_move=1, stones_remaining=2)
    h1 = position_hash(
        t,
        stones=[((0, 0), 0), ((1, 0), 1)],
        side_to_move=1,
        stones_remaining=1,
    )
    assert h0 != h1


def test_table_is_deterministic_across_constructions():
    t1 = ZobristTable()
    t2 = ZobristTable()
    h1 = position_hash(
        t1, stones=[((3, -2), 0)], side_to_move=0, stones_remaining=2,
    )
    h2 = position_hash(
        t2, stones=[((3, -2), 0)], side_to_move=0, stones_remaining=2,
    )
    assert h1 == h2


def test_player_label_swap_changes_hash():
    t = ZobristTable()
    h0 = position_hash(
        t, stones=[((0, 0), 0), ((1, 0), 1)], side_to_move=0, stones_remaining=1,
    )
    h1 = position_hash(
        t, stones=[((0, 0), 1), ((1, 0), 0)], side_to_move=0, stones_remaining=1,
    )
    assert h0 != h1
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_zobrist.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/zobrist.py`**

```python
"""Deterministic 64-bit Zobrist over canonical positions.

A lazy dict caches per-(cell, player) keys drawn from a fixed-seed
random.Random. Side-to-move and stones-remaining flags are pre-drawn
constants so they are stable across runs.

The hash is computed by XOR over:
  - every (cell, player) key in the stone list
  - SIDE_KEY[side_to_move]
  - REMAINING_KEY[stones_remaining]
"""
from __future__ import annotations

import random
from typing import Iterable

Cell = tuple[int, int]
StonePair = tuple[Cell, int]

SEED = 0xC0DE_FEED_5EED_C0DE
MASK64 = (1 << 64) - 1


def _draw_u64(rng: random.Random) -> int:
    return rng.getrandbits(64)


class ZobristTable:
    """64-bit Zobrist key table with lazy per-(cell, player) draws."""

    def __init__(self) -> None:
        self._rng = random.Random(SEED)
        # Pre-draw fixed-position keys for side and stones-remaining
        # before any cell keys, so the cell stream is deterministic
        # regardless of which cells are first queried.
        self.side_keys: tuple[int, int] = (
            _draw_u64(self._rng),
            _draw_u64(self._rng),
        )
        # Stones remaining is 1 or 2; index 0 unused. 3 entries simplifies.
        self.remaining_keys: tuple[int, int, int] = (
            0,
            _draw_u64(self._rng),
            _draw_u64(self._rng),
        )
        self._cell_keys: dict[tuple[Cell, int], int] = {}

    def cell_key(self, c: Cell, player: int) -> int:
        key = (c, player)
        k = self._cell_keys.get(key)
        if k is None:
            k = _draw_u64(self._rng)
            self._cell_keys[key] = k
        return k


def position_hash(
    table: ZobristTable,
    stones: Iterable[StonePair],
    side_to_move: int,
    stones_remaining: int,
) -> int:
    assert side_to_move in (0, 1)
    assert stones_remaining in (1, 2)
    h = table.side_keys[side_to_move]
    h ^= table.remaining_keys[stones_remaining]
    for cell, player in stones:
        h ^= table.cell_key(cell, player)
    return h & MASK64
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_zobrist.py -v`
Expected: 6 PASS. (gitignored — no commit.)

---

## Task 3: Game walker — ply state iterator

**Files:**
- Create: `scripts/openbook/walker.py`
- Create: `scripts/openbook/tests/test_walker.py`

For each game, walk plies 1..32 and yield records describing the position BEFORE the move is played, plus the move played and meta (player ELO, opponent ELO, time spent, etc.).

State per ply (before the move):
- `stones: list[((q,r), player_byte)]`
- `side_to_move: 0 or 1`  — whose turn (player byte of upcoming move)
- `stones_remaining: 1 or 2`
- `played_move: (q, r)`
- `ply_index: int` (1-indexed)
- `mover_elo: int | None`
- `opponent_elo: int | None`
- `mover_won_game: bool`
- `game_length_remaining: int` (stones from this ply onwards, inclusive)
- `time_spent_ms: int | None` (ms between this and previous timestamp)

Player byte rule:
- The first played stone (ply 1, always `(0,0)`) is by **player 0** (X).
- Map the JSON `playerId` of ply 1 → byte 0; the other player → byte 1. Consistent across the game.

Stones-remaining / side rule per ply:
- Ply 1: side=0, remaining=1 (single-stone first turn).
- Ply 2: opponent's first stone of their 2-stone turn, side=1, remaining=2.
- Ply 3: opponent's second stone, side=1, remaining=1.
- Ply 4: player 0's first stone of next turn, side=0, remaining=2.
- General formula for ply k ≥ 2: turn index `t = (k - 2) // 2`; side = `1 - (t % 2)` (alternating, starting from 1); remaining = `1 if (k - 2) % 2 == 1 else 2`.

(This matches CLAUDE.md: per-stone, parity-flipped, halfmove handled.)

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_walker.py
from openbook.walker import iter_game_plies, player_byte_map


SAMPLE_GAME = {
    "id": "g",
    "players": [
        {"playerId": "A", "elo": 1000},
        {"playerId": "B", "elo": 1200},
    ],
    "gameResult": {"winningPlayerId": "A", "reason": "six-in-a-row"},
    "moves": [
        {"moveNumber": 2, "playerId": "A", "x": 0, "y": 0, "timestamp": 1000},
        {"moveNumber": 3, "playerId": "B", "x": 2, "y": -2, "timestamp": 1500},
        {"moveNumber": 4, "playerId": "B", "x": -3, "y": 3, "timestamp": 1700},
        {"moveNumber": 5, "playerId": "A", "x": 0, "y": 1, "timestamp": 2400},
        {"moveNumber": 6, "playerId": "A", "x": 0, "y": 2, "timestamp": 2900},
    ],
}


def test_player_byte_map_assigns_first_mover_zero():
    pbm = player_byte_map(SAMPLE_GAME)
    assert pbm["A"] == 0
    assert pbm["B"] == 1


def test_iter_game_plies_emits_one_record_per_move():
    recs = list(iter_game_plies(SAMPLE_GAME, max_ply=32))
    assert len(recs) == 5


def test_first_ply_state_is_empty_origin_move():
    rec = list(iter_game_plies(SAMPLE_GAME, max_ply=32))[0]
    assert rec.stones == ()
    assert rec.side_to_move == 0
    assert rec.stones_remaining == 1
    assert rec.played_move == (0, 0)
    assert rec.ply_index == 1


def test_second_ply_state_has_one_stone():
    rec = list(iter_game_plies(SAMPLE_GAME, max_ply=32))[1]
    assert rec.stones == (((0, 0), 0),)
    assert rec.side_to_move == 1
    assert rec.stones_remaining == 2
    assert rec.played_move == (2, -2)
    assert rec.ply_index == 2


def test_third_ply_state_has_two_stones_remaining_one():
    rec = list(iter_game_plies(SAMPLE_GAME, max_ply=32))[2]
    assert rec.side_to_move == 1
    assert rec.stones_remaining == 1
    assert rec.played_move == (-3, 3)


def test_fourth_ply_switches_side_back():
    rec = list(iter_game_plies(SAMPLE_GAME, max_ply=32))[3]
    assert rec.side_to_move == 0
    assert rec.stones_remaining == 2


def test_mover_won_is_true_when_played_by_winner():
    recs = list(iter_game_plies(SAMPLE_GAME, max_ply=32))
    # Ply 1, 4, 5 are A's moves; A won.
    assert recs[0].mover_won is True
    assert recs[3].mover_won is True
    assert recs[4].mover_won is True
    # Ply 2, 3 are B's moves; B lost.
    assert recs[1].mover_won is False
    assert recs[2].mover_won is False


def test_mover_and_opponent_elo_assigned():
    recs = list(iter_game_plies(SAMPLE_GAME, max_ply=32))
    assert recs[0].mover_elo == 1000
    assert recs[0].opponent_elo == 1200
    assert recs[1].mover_elo == 1200
    assert recs[1].opponent_elo == 1000


def test_time_spent_ms_first_ply_is_none():
    recs = list(iter_game_plies(SAMPLE_GAME, max_ply=32))
    assert recs[0].time_spent_ms is None
    assert recs[1].time_spent_ms == 500


def test_game_length_remaining_decrements():
    recs = list(iter_game_plies(SAMPLE_GAME, max_ply=32))
    assert recs[0].game_length_remaining == 5
    assert recs[-1].game_length_remaining == 1


def test_max_ply_truncates():
    recs = list(iter_game_plies(SAMPLE_GAME, max_ply=2))
    assert len(recs) == 2
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_walker.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/walker.py`**

```python
"""Walk each JSON game ply-by-ply, yielding pre-move state records."""
from __future__ import annotations

from dataclasses import dataclass
from typing import Iterator

Cell = tuple[int, int]
StonePair = tuple[Cell, int]


@dataclass(frozen=True)
class PlyRecord:
    ply_index: int
    stones: tuple[StonePair, ...]
    side_to_move: int
    stones_remaining: int
    played_move: Cell
    mover_byte: int
    mover_elo: int | None
    opponent_elo: int | None
    mover_won: bool
    game_length_remaining: int
    time_spent_ms: int | None


def player_byte_map(game: dict) -> dict[str, int]:
    """First playerId in moves list = byte 0 (X). Other = byte 1 (O)."""
    moves = game.get("moves") or []
    if not moves:
        return {}
    first_pid = moves[0].get("playerId")
    other_pid = None
    for p in game.get("players", []):
        pid = p.get("playerId")
        if pid and pid != first_pid:
            other_pid = pid
            break
    out: dict[str, int] = {}
    if first_pid is not None:
        out[first_pid] = 0
    if other_pid is not None:
        out[other_pid] = 1
    return out


def _side_and_remaining(ply_index: int) -> tuple[int, int]:
    """ply_index is 1-indexed. Returns (side_to_move, stones_remaining)."""
    if ply_index == 1:
        return 0, 1
    # plies 2,3 = side 1; plies 4,5 = side 0; plies 6,7 = side 1; ...
    turn_idx = (ply_index - 2) // 2
    side = 1 - (turn_idx % 2)
    remaining = 2 if (ply_index - 2) % 2 == 0 else 1
    return side, remaining


def iter_game_plies(game: dict, max_ply: int = 32) -> Iterator[PlyRecord]:
    moves = game.get("moves") or []
    if not moves:
        return
    pbm = player_byte_map(game)
    elo_by_pid: dict[str, int | None] = {
        p.get("playerId"): p.get("elo") for p in game.get("players", [])
    }
    winner = (game.get("gameResult") or {}).get("winningPlayerId")
    stones: list[StonePair] = []
    n_total = len(moves)
    for i, m in enumerate(moves):
        ply = i + 1
        if ply > max_ply:
            break
        played = (m["x"], m["y"])
        mover_pid = m.get("playerId")
        if mover_pid not in pbm:
            return
        mover_byte = pbm[mover_pid]
        side, remaining = _side_and_remaining(ply)
        # Sanity: side must equal mover_byte; if mismatched (corrupt game),
        # skip the rest.
        if side != mover_byte:
            return
        opp_pid = next((p for p in pbm if p != mover_pid), None)
        ts_prev = moves[i - 1].get("timestamp") if i > 0 else None
        ts_cur = m.get("timestamp")
        time_spent = None
        if i > 0 and ts_prev is not None and ts_cur is not None:
            dt = ts_cur - ts_prev
            if 0 <= dt <= 10 * 60 * 1000:
                time_spent = dt
        yield PlyRecord(
            ply_index=ply,
            stones=tuple(stones),
            side_to_move=side,
            stones_remaining=remaining,
            played_move=played,
            mover_byte=mover_byte,
            mover_elo=elo_by_pid.get(mover_pid),
            opponent_elo=elo_by_pid.get(opp_pid) if opp_pid else None,
            mover_won=(winner == mover_pid) if winner else False,
            game_length_remaining=n_total - i,
            time_spent_ms=time_spent,
        )
        stones.append((played, mover_byte))
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_walker.py -v`
Expected: 11 PASS. (gitignored — no commit.)

---

## Task 4: Per-position move stats aggregator

**Files:**
- Create: `scripts/openbook/aggregator.py`
- Create: `scripts/openbook/tests/test_aggregator.py`

Aggregate over all `PlyRecord`s into a `dict[position_hash, dict[move_canonical, MoveStats]]`. Track raw counts AND high-ELO-only counts (game has `max(player_elo) >= 1300`).

MoveStats fields (final, after `.summarize()`):
- `n_games: int`
- `n_high_elo_games: int`
- `n_wins_for_mover: int`
- `n_wins_for_mover_high_elo: int`
- `sum_elo_mover: int`
- `sum_elo_opp: int`
- `sum_game_length_from_here: int`
- `times_ms: list[int]`  (drop None; cap list length at 256 per move to bound memory; use reservoir if exceeded — but for first cut just keep all)
- `winrate: float`
- `winrate_high_elo: float`
- `avg_elo_mover: float`
- `avg_elo_opp: float`
- `avg_game_length_from_here: float`
- `avg_time_ms: float`
- `p50_time_ms: float`
- `weight: int`  (n_games + 3 * n_high_elo_games; clamped to u16)

High-ELO threshold: a game qualifies if `max(both player ELOs) >= 1300`.

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_aggregator.py
from openbook.aggregator import Aggregator, HIGH_ELO_THRESHOLD


def _make_record(
    pos_hash, move, mover_won, mover_elo, opp_elo, time_ms,
    game_length_remaining=5,
):
    # Lightweight namespace stand-in for PlyRecord
    class R:
        pass
    r = R()
    r.position_hash = pos_hash
    r.canonical_move = move
    r.mover_won = mover_won
    r.mover_elo = mover_elo
    r.opponent_elo = opp_elo
    r.time_spent_ms = time_ms
    r.game_length_remaining = game_length_remaining
    r.is_high_elo = (max(mover_elo or 0, opp_elo or 0) >= HIGH_ELO_THRESHOLD)
    return r


def test_high_elo_threshold_is_1300():
    assert HIGH_ELO_THRESHOLD == 1300


def test_single_record_winrate_is_1_if_mover_won():
    a = Aggregator()
    a.add(_make_record(0x1, (1, 0), True, 1000, 1100, 200))
    stats = a.finalize()
    s = stats[0x1][(1, 0)]
    assert s["n_games"] == 1
    assert s["winrate"] == 1.0


def test_winrate_averages_across_games():
    a = Aggregator()
    a.add(_make_record(0xA, (0, 1), True, 1000, 1100, 200))
    a.add(_make_record(0xA, (0, 1), False, 1000, 1100, 300))
    s = a.finalize()[0xA][(0, 1)]
    assert s["n_games"] == 2
    assert s["winrate"] == 0.5


def test_high_elo_tracked_separately():
    a = Aggregator()
    a.add(_make_record(0x5, (2, 0), True, 900, 950, 150))
    a.add(_make_record(0x5, (2, 0), True, 1400, 1350, 250))
    s = a.finalize()[0x5][(2, 0)]
    assert s["n_games"] == 2
    assert s["n_high_elo_games"] == 1
    assert s["winrate_high_elo"] == 1.0


def test_weight_is_n_plus_3x_high_elo():
    a = Aggregator()
    a.add(_make_record(0x9, (1, 1), True, 1500, 1400, 100))  # high
    a.add(_make_record(0x9, (1, 1), False, 900, 1000, 100))  # low
    s = a.finalize()[0x9][(1, 1)]
    assert s["weight"] == 1 + 3 * 1


def test_p50_time_is_median():
    a = Aggregator()
    for t in [100, 200, 300, 400, 500]:
        a.add(_make_record(0x7, (0, 0), True, 1000, 1000, t))
    s = a.finalize()[0x7][(0, 0)]
    assert s["p50_time_ms"] == 300


def test_avg_elo_mover_is_mean():
    a = Aggregator()
    a.add(_make_record(0x3, (1, 0), True, 1000, 1100, 200))
    a.add(_make_record(0x3, (1, 0), True, 1200, 1300, 300))
    s = a.finalize()[0x3][(1, 0)]
    assert s["avg_elo_mover"] == 1100


def test_avg_game_length_from_here():
    a = Aggregator()
    a.add(_make_record(0xC, (0, 0), True, 1000, 1000, 200,
                       game_length_remaining=10))
    a.add(_make_record(0xC, (0, 0), True, 1000, 1000, 200,
                       game_length_remaining=20))
    s = a.finalize()[0xC][(0, 0)]
    assert s["avg_game_length_from_here"] == 15
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_aggregator.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/aggregator.py`**

```python
"""Per-position move-stat aggregator with high-ELO sub-tracking."""
from __future__ import annotations

import statistics
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Any

HIGH_ELO_THRESHOLD = 1300


@dataclass
class _MoveAccum:
    n_games: int = 0
    n_high_elo_games: int = 0
    n_wins: int = 0
    n_wins_high_elo: int = 0
    sum_elo_mover: int = 0
    sum_elo_opp: int = 0
    n_elo_mover: int = 0
    n_elo_opp: int = 0
    sum_game_length: int = 0
    times_ms: list[int] = field(default_factory=list)


class Aggregator:
    def __init__(self) -> None:
        self._table: dict[int, dict[tuple[int, int], _MoveAccum]] = defaultdict(
            lambda: defaultdict(_MoveAccum)
        )

    def add(self, rec: Any) -> None:
        slot = self._table[rec.position_hash][rec.canonical_move]
        slot.n_games += 1
        if rec.mover_won:
            slot.n_wins += 1
        is_high = getattr(rec, "is_high_elo", None)
        if is_high is None:
            is_high = (
                max(rec.mover_elo or 0, rec.opponent_elo or 0)
                >= HIGH_ELO_THRESHOLD
            )
        if is_high:
            slot.n_high_elo_games += 1
            if rec.mover_won:
                slot.n_wins_high_elo += 1
        if rec.mover_elo is not None:
            slot.sum_elo_mover += rec.mover_elo
            slot.n_elo_mover += 1
        if rec.opponent_elo is not None:
            slot.sum_elo_opp += rec.opponent_elo
            slot.n_elo_opp += 1
        slot.sum_game_length += rec.game_length_remaining
        if rec.time_spent_ms is not None:
            slot.times_ms.append(rec.time_spent_ms)

    def finalize(self) -> dict[int, dict[tuple[int, int], dict[str, Any]]]:
        out: dict[int, dict[tuple[int, int], dict[str, Any]]] = {}
        for h, by_move in self._table.items():
            out[h] = {}
            for mv, a in by_move.items():
                winrate = a.n_wins / a.n_games if a.n_games else 0.0
                winrate_he = (
                    a.n_wins_high_elo / a.n_high_elo_games
                    if a.n_high_elo_games
                    else 0.0
                )
                avg_elo_m = (
                    a.sum_elo_mover / a.n_elo_mover if a.n_elo_mover else 0.0
                )
                avg_elo_o = (
                    a.sum_elo_opp / a.n_elo_opp if a.n_elo_opp else 0.0
                )
                avg_len = (
                    a.sum_game_length / a.n_games if a.n_games else 0.0
                )
                avg_time = statistics.fmean(a.times_ms) if a.times_ms else 0.0
                p50_time = statistics.median(a.times_ms) if a.times_ms else 0.0
                weight = min(65535, a.n_games + 3 * a.n_high_elo_games)
                out[h][mv] = {
                    "n_games": a.n_games,
                    "n_high_elo_games": a.n_high_elo_games,
                    "n_wins_for_mover": a.n_wins,
                    "n_wins_for_mover_high_elo": a.n_wins_high_elo,
                    "winrate": winrate,
                    "winrate_high_elo": winrate_he,
                    "avg_elo_mover": avg_elo_m,
                    "avg_elo_opp": avg_elo_o,
                    "avg_game_length_from_here": avg_len,
                    "avg_time_ms": avg_time,
                    "p50_time_ms": p50_time,
                    "weight": weight,
                }
        return out
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_aggregator.py -v`
Expected: 8 PASS. (gitignored — no commit.)

---

## Task 5: Integrate symmetry + Zobrist into the walker pipeline

**Files:**
- Modify: `scripts/openbook/walker.py` — add a helper to produce canonical records (do not change existing `PlyRecord`/`iter_game_plies`).
- Create: `scripts/openbook/canonical.py` — orchestrates walker + symmetry + zobrist.
- Create: `scripts/openbook/tests/test_canonical.py`

`canonical_plies(game, table, max_ply)` yields enriched records that add:
- `position_hash: int` (canonical Zobrist 64-bit)
- `canonical_move: (q, r)` (move transformed by the same symmetry chosen for canonicalization)
- `is_high_elo: bool`

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_canonical.py
from openbook.canonical import canonical_plies
from openbook.zobrist import ZobristTable


SAMPLE_GAME = {
    "players": [
        {"playerId": "A", "elo": 1500},
        {"playerId": "B", "elo": 1100},
    ],
    "gameResult": {"winningPlayerId": "A", "reason": "six-in-a-row"},
    "moves": [
        {"moveNumber": 2, "playerId": "A", "x": 0, "y": 0, "timestamp": 1000},
        {"moveNumber": 3, "playerId": "B", "x": 2, "y": -2, "timestamp": 1500},
        {"moveNumber": 4, "playerId": "B", "x": -3, "y": 3, "timestamp": 1700},
        {"moveNumber": 5, "playerId": "A", "x": 0, "y": 1, "timestamp": 2400},
    ],
}


def test_canonical_plies_emits_one_per_walker_record():
    t = ZobristTable()
    recs = list(canonical_plies(SAMPLE_GAME, t, max_ply=32))
    assert len(recs) == 4


def test_each_canonical_record_has_64bit_hash():
    t = ZobristTable()
    for r in canonical_plies(SAMPLE_GAME, t, max_ply=32):
        assert 0 <= r.position_hash < (1 << 64)


def test_is_high_elo_uses_max_player_elo_1300():
    t = ZobristTable()
    recs = list(canonical_plies(SAMPLE_GAME, t, max_ply=32))
    # Game has 1500/1100 → max=1500 ≥ 1300 → True for all plies.
    assert all(r.is_high_elo for r in recs)


def test_low_elo_game_marked_not_high_elo():
    g = dict(SAMPLE_GAME)
    g["players"] = [{"playerId": "A", "elo": 1200},
                    {"playerId": "B", "elo": 1100}]
    t = ZobristTable()
    recs = list(canonical_plies(g, t, max_ply=32))
    assert not any(r.is_high_elo for r in recs)


def test_canonical_form_groups_symmetric_positions():
    # Two single-stone positions that differ only by rotation must hash equal.
    g1 = {
        "players": [{"playerId": "A", "elo": 1000},
                    {"playerId": "B", "elo": 1000}],
        "gameResult": {"winningPlayerId": "A"},
        "moves": [
            {"moveNumber": 2, "playerId": "A", "x": 0, "y": 0,
             "timestamp": 1000},
            {"moveNumber": 3, "playerId": "B", "x": 1, "y": 0,
             "timestamp": 1100},
        ],
    }
    g2 = {
        "players": [{"playerId": "A", "elo": 1000},
                    {"playerId": "B", "elo": 1000}],
        "gameResult": {"winningPlayerId": "A"},
        "moves": [
            {"moveNumber": 2, "playerId": "A", "x": 0, "y": 0,
             "timestamp": 1000},
            {"moveNumber": 3, "playerId": "B", "x": 0, "y": 1,
             "timestamp": 1100},
        ],
    }
    t = ZobristTable()
    r1 = list(canonical_plies(g1, t, max_ply=32))
    r2 = list(canonical_plies(g2, t, max_ply=32))
    # Position-after-stone-1 (before stone-2) is identical: just origin.
    # Hashes at ply index 2 must match.
    assert r1[1].position_hash == r2[1].position_hash
    # Move played at ply 2 should canonicalize to the same cell — both
    # (1,0) and (0,1) are in the same D6 orbit when the only stone is origin.
    assert r1[1].canonical_move == r2[1].canonical_move


def test_first_ply_canonical_move_is_origin():
    t = ZobristTable()
    rec = list(canonical_plies(SAMPLE_GAME, t, max_ply=32))[0]
    assert rec.canonical_move == (0, 0)
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_canonical.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/canonical.py`**

```python
"""Wire walker → symmetry → zobrist. Emit canonical-frame ply records."""
from __future__ import annotations

from dataclasses import dataclass
from typing import Iterator

from openbook.aggregator import HIGH_ELO_THRESHOLD
from openbook.symmetry import canonicalize_with_move
from openbook.walker import PlyRecord, iter_game_plies
from openbook.zobrist import ZobristTable, position_hash


@dataclass(frozen=True)
class CanonRecord:
    base: PlyRecord
    position_hash: int
    canonical_move: tuple[int, int]
    is_high_elo: bool

    # Pass-throughs so Aggregator can read attributes directly
    @property
    def ply_index(self) -> int:
        return self.base.ply_index

    @property
    def mover_won(self) -> bool:
        return self.base.mover_won

    @property
    def mover_elo(self) -> int | None:
        return self.base.mover_elo

    @property
    def opponent_elo(self) -> int | None:
        return self.base.opponent_elo

    @property
    def time_spent_ms(self) -> int | None:
        return self.base.time_spent_ms

    @property
    def game_length_remaining(self) -> int:
        return self.base.game_length_remaining


def _game_is_high_elo(game: dict) -> bool:
    elos = [p.get("elo") or 0 for p in game.get("players", [])]
    return bool(elos) and max(elos) >= HIGH_ELO_THRESHOLD


def canonical_plies(
    game: dict, table: ZobristTable, max_ply: int = 32,
) -> Iterator[CanonRecord]:
    is_high = _game_is_high_elo(game)
    for rec in iter_game_plies(game, max_ply=max_ply):
        canon_stones, canon_move, _ = canonicalize_with_move(
            list(rec.stones), rec.played_move,
        )
        h = position_hash(
            table,
            canon_stones,
            side_to_move=rec.side_to_move,
            stones_remaining=rec.stones_remaining,
        )
        yield CanonRecord(
            base=rec,
            position_hash=h,
            canonical_move=canon_move,
            is_high_elo=is_high,
        )
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_canonical.py -v`
Expected: 6 PASS. (gitignored — no commit.)

---

## Task 6: Opening tree + KL-divergence theory junctions

**Files:**
- Create: `scripts/openbook/tree.py`
- Create: `scripts/openbook/tests/test_tree.py`

Build a parent-child position graph (DAG, not strictly tree, due to transpositions) to depth 16. Each edge stores frequency and branching factor at parent.

KL divergence at each node (between high-ELO and low-ELO move distributions):
- Let `P` = high-ELO move probabilities, `Q` = low-ELO move probabilities (over the union of moves seen at the node).
- Smooth with Laplace add-1: `P_i = (n_high_i + 1) / (sum(n_high) + k)`, same for Q with `k` = number of unique moves at node.
- `KL(P || Q) = sum_i P_i * log(P_i / Q_i)`.
- Rank nodes by KL descending; top-N are theory junctions. Require `sum(n_high) >= 10` and `sum(n_low) >= 10` to be considered.

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_tree.py
import math

from openbook.tree import build_tree, kl_divergence, theory_junctions


def test_kl_divergence_is_zero_for_identical_distributions():
    # Same counts → KL = 0 after smoothing.
    high = {"a": 5, "b": 5}
    low = {"a": 5, "b": 5}
    assert kl_divergence(high, low) == 0.0


def test_kl_divergence_is_positive_when_distributions_differ():
    high = {"a": 10, "b": 0}
    low = {"a": 5, "b": 5}
    kl = kl_divergence(high, low)
    assert kl > 0.0


def test_build_tree_records_edges_from_canonical_records():
    # Two games sharing the first move sequence
    recs = [
        _rec(0, 0xAA, (0, 0), 0xBB),  # ply 1: hash AA, move (0,0) → child BB
        _rec(1, 0xBB, (1, 0), 0xCC),
        _rec(0, 0xAA, (0, 0), 0xBB),  # second game, same first move
        _rec(1, 0xBB, (2, 0), 0xDD),  # diverges at ply 2
    ]
    tree = build_tree(recs, max_depth=16)
    # Root node AA has one edge: (0,0) with weight 2.
    assert tree.edge_count(0xAA, (0, 0)) == 2
    assert tree.edge_count(0xBB, (1, 0)) == 1
    assert tree.edge_count(0xBB, (2, 0)) == 1


def test_branching_factor_counts_unique_moves_per_node():
    recs = [
        _rec(0, 0x1, (0, 0), 0x2),
        _rec(0, 0x1, (1, 0), 0x3),
        _rec(0, 0x1, (2, 0), 0x4),
    ]
    tree = build_tree(recs, max_depth=16)
    assert tree.branching_factor(0x1) == 3


def test_max_depth_caps_tree_growth():
    recs = [_rec(i, i, (0, 0), i + 1) for i in range(20)]
    # ply_index uses base-1 in PlyRecord; here we pass i as ply_index.
    # max_depth=5 means only plies 1..5 contribute.
    tree = build_tree(recs, max_depth=5)
    # Plies > 5 not included.
    for i in range(6, 20):
        assert tree.branching_factor(i) == 0


def test_theory_junctions_returns_high_kl_nodes():
    # Build a node A with strong split: high-ELO prefers move x, low-ELO y.
    recs = []
    # High-ELO games choose (1,0) at hash 0xA1
    for _ in range(15):
        recs.append(_rec(0, 0xA1, (1, 0), 0xB, is_high=True))
    # Low-ELO games choose (0,1) at the same hash
    for _ in range(15):
        recs.append(_rec(0, 0xA1, (0, 1), 0xC, is_high=False))
    # And a control node 0xA2 with matching distributions
    for _ in range(15):
        recs.append(_rec(0, 0xA2, (1, 0), 0xD, is_high=True))
    for _ in range(15):
        recs.append(_rec(0, 0xA2, (1, 0), 0xE, is_high=False))
    tree = build_tree(recs, max_depth=16)
    junctions = theory_junctions(tree, top_n=5, min_each=10)
    # 0xA1 must outrank 0xA2 (KL is positive for A1, ~0 for A2).
    hashes = [h for h, _ in junctions]
    assert 0xA1 in hashes
    if 0xA2 in hashes:
        assert hashes.index(0xA1) < hashes.index(0xA2)


# Helpers


class _R:
    pass


def _rec(idx, h, mv, _child_unused=None, is_high=False):
    r = _R()
    r.ply_index = idx
    r.position_hash = h
    r.canonical_move = mv
    r.is_high_elo = is_high
    return r
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_tree.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/tree.py`**

```python
"""Opening tree + KL-divergence theory junctions.

The tree is a DAG indexed by position_hash. We do not materialise child
hashes here (transpositions make that ambiguous without re-running the
canonical pipeline); we track per-node move counts. The DAG structure is
recovered later by the writer if needed.
"""
from __future__ import annotations

import math
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Iterable


Cell = tuple[int, int]


@dataclass
class _Node:
    moves_high: dict[Cell, int] = field(default_factory=lambda: defaultdict(int))
    moves_low: dict[Cell, int] = field(default_factory=lambda: defaultdict(int))

    def total(self) -> int:
        return (
            sum(self.moves_high.values()) + sum(self.moves_low.values())
        )


class Tree:
    def __init__(self) -> None:
        self.nodes: dict[int, _Node] = defaultdict(_Node)

    def add(self, h: int, mv: Cell, is_high: bool) -> None:
        n = self.nodes[h]
        if is_high:
            n.moves_high[mv] += 1
        else:
            n.moves_low[mv] += 1

    def edge_count(self, h: int, mv: Cell) -> int:
        n = self.nodes.get(h)
        if n is None:
            return 0
        return n.moves_high.get(mv, 0) + n.moves_low.get(mv, 0)

    def branching_factor(self, h: int) -> int:
        n = self.nodes.get(h)
        if n is None:
            return 0
        return len(set(n.moves_high) | set(n.moves_low))


def build_tree(records: Iterable, max_depth: int) -> Tree:
    t = Tree()
    for r in records:
        if r.ply_index > max_depth:
            continue
        t.add(r.position_hash, r.canonical_move, r.is_high_elo)
    return t


def kl_divergence(
    high_counts: dict[Cell, int], low_counts: dict[Cell, int],
) -> float:
    """KL(P || Q) with Laplace add-1 smoothing over the union of moves."""
    keys = set(high_counts) | set(low_counts)
    if not keys:
        return 0.0
    k = len(keys)
    sh = sum(high_counts.values()) + k
    sl = sum(low_counts.values()) + k
    kl = 0.0
    for m in keys:
        p = (high_counts.get(m, 0) + 1) / sh
        q = (low_counts.get(m, 0) + 1) / sl
        kl += p * math.log(p / q)
    return kl


def theory_junctions(
    tree: Tree, top_n: int = 20, min_each: int = 10,
) -> list[tuple[int, float]]:
    scored: list[tuple[int, float]] = []
    for h, node in tree.nodes.items():
        sh = sum(node.moves_high.values())
        sl = sum(node.moves_low.values())
        if sh < min_each or sl < min_each:
            continue
        kl = kl_divergence(node.moves_high, node.moves_low)
        scored.append((h, kl))
    scored.sort(key=lambda x: -x[1])
    return scored[:top_n]
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_tree.py -v`
Expected: 6 PASS. (gitignored — no commit.)

---

## Task 7: Named opening matcher

**Files:**
- Create: `scripts/openbook/theory.py`
- Create: `scripts/openbook/tests/test_theory.py`

Pattern file shape (`data/analysis/hexopedia_patterns.json`):
```json
{
  "patterns": [
    {
      "name": "Star Opening",
      "ply_at_most": 4,
      "stones": [[[0, 0], 0], [[1, 0], 1]],
      "side_to_move": 1,
      "stones_remaining": 1
    }
  ]
}
```

Pattern matching: a pattern matches a position if, AFTER canonicalising both pattern stones AND position stones, they have the same canonical hash AND same side/remaining. The matcher pre-computes pattern canonical hashes once at load time. Lookup against position hashes is O(1).

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_theory.py
from openbook.theory import TheoryIndex
from openbook.zobrist import ZobristTable


def test_empty_pattern_list_yields_empty_index():
    t = ZobristTable()
    idx = TheoryIndex.from_patterns({"patterns": []}, t)
    assert idx.matches(0x1234) is None


def test_pattern_canonicalises_then_hashes():
    # Position: just stone (1,0) by X. Canonical = (-1, 0).
    t = ZobristTable()
    patterns = {
        "patterns": [
            {
                "name": "OneStone",
                "stones": [[[1, 0], 0]],
                "side_to_move": 1,
                "stones_remaining": 2,
            }
        ]
    }
    idx = TheoryIndex.from_patterns(patterns, t)
    # The pattern's canonical hash should be computable via known primitives:
    from openbook.symmetry import canonicalize
    from openbook.zobrist import position_hash
    canon, _ = canonicalize([((1, 0), 0)])
    h = position_hash(t, canon, side_to_move=1, stones_remaining=2)
    assert idx.matches(h) == "OneStone"


def test_unmatched_hash_returns_none():
    t = ZobristTable()
    patterns = {
        "patterns": [
            {
                "name": "X",
                "stones": [[[0, 0], 0]],
                "side_to_move": 1,
                "stones_remaining": 2,
            }
        ]
    }
    idx = TheoryIndex.from_patterns(patterns, t)
    assert idx.matches(0xDEADBEEF) is None


def test_export_dict_has_hex_keys():
    t = ZobristTable()
    patterns = {
        "patterns": [
            {
                "name": "Y",
                "stones": [[[0, 0], 0]],
                "side_to_move": 1,
                "stones_remaining": 2,
            }
        ]
    }
    idx = TheoryIndex.from_patterns(patterns, t)
    d = idx.export()
    assert all(isinstance(k, str) and k.startswith("0x") for k in d)
    assert "Y" in d.values()
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_theory.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/theory.py`**

```python
"""Named-opening detector: canonical-hash → opening_name."""
from __future__ import annotations

from openbook.symmetry import canonicalize
from openbook.zobrist import ZobristTable, position_hash


class TheoryIndex:
    def __init__(self, hash_to_name: dict[int, str]) -> None:
        self._map = hash_to_name

    @classmethod
    def from_patterns(cls, doc: dict, table: ZobristTable) -> "TheoryIndex":
        out: dict[int, str] = {}
        for p in doc.get("patterns", []):
            name = p["name"]
            stones = [(tuple(c), int(player)) for c, player in p["stones"]]
            canon, _ = canonicalize(stones)
            h = position_hash(
                table,
                canon,
                side_to_move=int(p["side_to_move"]),
                stones_remaining=int(p["stones_remaining"]),
            )
            out[h] = name
        return cls(out)

    def matches(self, position_hash_value: int) -> str | None:
        return self._map.get(position_hash_value)

    def export(self) -> dict[str, str]:
        return {f"0x{h:016x}": name for h, name in self._map.items()}
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_theory.py -v`
Expected: 4 PASS. (gitignored — no commit.)

---

## Task 8: Blunder candidate detector

**Files:**
- Create: `scripts/openbook/blunders.py`
- Create: `scripts/openbook/tests/test_blunders.py`

A "blunder candidate" position is one where the best human move (highest weight, or among well-played moves) has winrate < 0.4 AND total `n_games >= 20`. These positions need engine analysis priority — humans haven't found a good answer.

API: `blunder_candidates(stats_table, min_n_games=20, max_best_winrate=0.4)` returns `list[(position_hash, best_move, best_winrate, total_n)]`.

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_blunders.py
from openbook.blunders import blunder_candidates


def test_position_with_high_winrate_not_flagged():
    stats = {
        0xA: {
            (0, 0): {"n_games": 30, "winrate": 0.6, "weight": 30},
            (1, 0): {"n_games": 5, "winrate": 0.4, "weight": 5},
        }
    }
    assert blunder_candidates(stats) == []


def test_position_below_min_n_not_flagged():
    stats = {
        0xB: {
            (0, 0): {"n_games": 10, "winrate": 0.2, "weight": 10},
        }
    }
    assert blunder_candidates(stats) == []


def test_position_with_best_winrate_below_threshold_flagged():
    stats = {
        0xC: {
            (0, 0): {"n_games": 15, "winrate": 0.30, "weight": 15},
            (1, 0): {"n_games": 8, "winrate": 0.35, "weight": 8},
        }
    }
    # total = 23 ≥ 20, best (heaviest) move winrate = 0.30 < 0.4 → flagged.
    out = blunder_candidates(stats)
    assert len(out) == 1
    h, mv, wr, n = out[0]
    assert h == 0xC
    assert mv == (0, 0)
    assert abs(wr - 0.30) < 1e-9
    assert n == 23


def test_best_move_picked_by_weight():
    stats = {
        0xD: {
            (0, 0): {"n_games": 5, "winrate": 0.9, "weight": 5},
            (1, 0): {"n_games": 18, "winrate": 0.20, "weight": 18},
        }
    }
    out = blunder_candidates(stats)
    # total = 23 ≥ 20; best-by-weight is (1,0) with winrate 0.20.
    assert out[0][1] == (1, 0)
    assert abs(out[0][2] - 0.20) < 1e-9
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_blunders.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/blunders.py`**

```python
"""Blunder-candidate flagging: positions whose heaviest human move loses."""
from __future__ import annotations

from typing import Any


def blunder_candidates(
    stats: dict[int, dict[tuple[int, int], dict[str, Any]]],
    min_n_games: int = 20,
    max_best_winrate: float = 0.4,
) -> list[tuple[int, tuple[int, int], float, int]]:
    out: list[tuple[int, tuple[int, int], float, int]] = []
    for h, by_move in stats.items():
        total_n = sum(s["n_games"] for s in by_move.values())
        if total_n < min_n_games:
            continue
        best_move, best_s = max(
            by_move.items(), key=lambda kv: kv[1]["weight"],
        )
        if best_s["winrate"] < max_best_winrate:
            out.append((h, best_move, best_s["winrate"], total_n))
    out.sort(key=lambda x: x[2])  # most-likely-blunder first
    return out
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_blunders.py -v`
Expected: 4 PASS. (gitignored — no commit.)

---

## Task 9: Turn-structure stats (stone-1 vs stone-2)

**Files:**
- Create: `scripts/openbook/turn_struct.py`
- Create: `scripts/openbook/tests/test_turn_struct.py`

For each two-stone turn (skip the first single-stone move), record:
- Hex distance between stone 1 and stone 2 of the turn.
- Conditional `(stone1_offset_from_centroid) → stone2_offset` distribution.
  - Centroid of pre-turn stones; offsets measured from centroid; quantise to nearest hex cell.
  - For simplicity: just record `(stone1_relative_to_origin, stone2_relative_to_stone1)` distribution.

API: `TurnStructStats.update(game_moves, pbm)`; `.summarise() -> dict`.

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_turn_struct.py
from openbook.turn_struct import TurnStructStats


def test_two_stone_turn_distance_recorded():
    moves = [
        {"playerId": "A", "x": 0, "y": 0},
        {"playerId": "B", "x": 1, "y": 0},
        {"playerId": "B", "x": 4, "y": 0},  # distance 3 from prior
        {"playerId": "A", "x": 0, "y": 1},
        {"playerId": "A", "x": 0, "y": 3},  # distance 2
    ]
    s = TurnStructStats()
    s.update(moves)
    out = s.summarise()
    # Two two-stone turns observed.
    assert out["n_turns"] == 2
    assert sorted(out["distance_distribution"].keys()) == [2, 3]


def test_conditional_pair_recorded():
    moves = [
        {"playerId": "A", "x": 0, "y": 0},
        {"playerId": "B", "x": 1, "y": 0},
        {"playerId": "B", "x": 1, "y": -1},  # vector (0,-1) from stone1
    ]
    s = TurnStructStats()
    s.update(moves)
    out = s.summarise()
    pairs = out["pair_offsets"]
    # Stone1 relative to origin: (1, 0). Stone2 relative to stone1: (0, -1).
    assert pairs[((1, 0), (0, -1))] == 1


def test_skip_first_single_stone_move():
    moves = [
        {"playerId": "A", "x": 0, "y": 0},
    ]
    s = TurnStructStats()
    s.update(moves)
    assert s.summarise()["n_turns"] == 0
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_turn_struct.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/turn_struct.py`**

```python
"""Stats over the two-stone turn structure: distance, conditional pairs."""
from __future__ import annotations

from collections import Counter


def _hex_dist(a: tuple[int, int], b: tuple[int, int]) -> int:
    dq = a[0] - b[0]
    dr = a[1] - b[1]
    return (abs(dq) + abs(dr) + abs(dq + dr)) // 2


class TurnStructStats:
    def __init__(self) -> None:
        self._distances: Counter[int] = Counter()
        self._pairs: Counter[tuple[tuple[int, int], tuple[int, int]]] = Counter()
        self._n_turns = 0

    def update(self, moves: list[dict]) -> None:
        # Iterate moves starting from index 1 in pairs of two same-player moves.
        i = 1
        while i + 1 < len(moves):
            m1, m2 = moves[i], moves[i + 1]
            if m1.get("playerId") != m2.get("playerId"):
                # Out-of-pair due to corrupt game; advance one and continue.
                i += 1
                continue
            c1 = (m1["x"], m1["y"])
            c2 = (m2["x"], m2["y"])
            self._distances[_hex_dist(c1, c2)] += 1
            offset = (c2[0] - c1[0], c2[1] - c1[1])
            self._pairs[(c1, offset)] += 1
            self._n_turns += 1
            i += 2

    def summarise(self) -> dict:
        return {
            "n_turns": self._n_turns,
            "distance_distribution": dict(self._distances),
            "pair_offsets": dict(self._pairs),
        }
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_turn_struct.py -v`
Expected: 3 PASS. (gitignored — no commit.)

---

## Task 10: Axis locality stats

**Files:**
- Create: `scripts/openbook/locality.py`
- Create: `scripts/openbook/tests/test_locality.py`

For each placed stone, on each of the 3 hex axes, count own/opp stones within radius 2 along that axis. An axis-line is shared by stones with one of:
- axis 0 (q-axis, constant `r`): same `r`, distance along q.
- axis 1 (r-axis, constant `q`): same `q`, distance along r.
- axis 2 (s-axis, constant `s = -q-r`): same `q+r`, distance measured in q (or r — equivalent).

Bucket by phase (early/mid/late thirds of game). Output histograms per axis × phase × own-or-opp.

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_locality.py
from openbook.locality import LocalityStats


def test_no_axis_neighbours_when_alone():
    s = LocalityStats()
    s.observe_stone(
        placed=(0, 0), placed_player=0,
        prior_stones=[],
        phase="early",
    )
    out = s.summarise()
    assert out["per_axis"]["early"][0]["own"][0] == 1  # 1 obs at own count 0
    assert out["per_axis"]["early"][1]["own"][0] == 1
    assert out["per_axis"]["early"][2]["own"][0] == 1


def test_own_axis_neighbour_within_two():
    s = LocalityStats()
    s.observe_stone(
        placed=(0, 0), placed_player=0,
        prior_stones=[((2, 0), 0)],  # axis 0 (same r), distance 2 → counts
        phase="mid",
    )
    out = s.summarise()
    # On axis 0, own-count = 1.
    assert out["per_axis"]["mid"][0]["own"][1] == 1


def test_opp_distinct_from_own():
    s = LocalityStats()
    s.observe_stone(
        placed=(0, 0), placed_player=0,
        prior_stones=[((1, 0), 1)],  # axis 0 (same r), distance 1, opponent
        phase="late",
    )
    out = s.summarise()
    assert out["per_axis"]["late"][0]["opp"][1] == 1
    assert out["per_axis"]["late"][0]["own"][0] == 1


def test_axis_s_uses_constant_q_plus_r():
    s = LocalityStats()
    # (0, 0) and (1, -1): q+r = 0 same, distance along axis = 1.
    s.observe_stone(
        placed=(0, 0), placed_player=0,
        prior_stones=[((1, -1), 0)],
        phase="early",
    )
    out = s.summarise()
    # Axis 2 own count = 1 for this stone observation.
    assert out["per_axis"]["early"][2]["own"][1] == 1


def test_radius_cap_excludes_distance_three():
    s = LocalityStats()
    s.observe_stone(
        placed=(0, 0), placed_player=0,
        prior_stones=[((3, 0), 0)],  # axis 0, distance 3 → excluded
        phase="early",
    )
    out = s.summarise()
    # Axis 0 own count = 0 (no neighbour within radius 2).
    assert out["per_axis"]["early"][0]["own"][0] == 1
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_locality.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/locality.py`**

```python
"""Per-stone axis-locality stats: own/opp counts on each of 3 axes."""
from __future__ import annotations

from collections import Counter
from typing import Iterable

RADIUS = 2

# Each axis maps a coord to (line_id, position_along_line).
# Axis 0 (q-axis, constant r): line_id = r, pos = q.
# Axis 1 (r-axis, constant q): line_id = q, pos = r.
# Axis 2 (s-axis, constant q+r): line_id = q+r, pos = q.

def _axis_keys(c: tuple[int, int]) -> tuple[
    tuple[int, int], tuple[int, int], tuple[int, int],
]:
    q, r = c
    return ((r, q), (q, r), (q + r, q))


class LocalityStats:
    def __init__(self) -> None:
        # phase → axis → {"own"|"opp"} → Counter[count]
        self._h: dict[str, dict[int, dict[str, Counter[int]]]] = {
            ph: {ax: {"own": Counter(), "opp": Counter()} for ax in range(3)}
            for ph in ("early", "mid", "late")
        }

    def observe_stone(
        self,
        placed: tuple[int, int],
        placed_player: int,
        prior_stones: Iterable[tuple[tuple[int, int], int]],
        phase: str,
    ) -> None:
        if phase not in self._h:
            return
        placed_keys = _axis_keys(placed)
        own_counts = [0, 0, 0]
        opp_counts = [0, 0, 0]
        for c, player in prior_stones:
            keys = _axis_keys(c)
            for ax in range(3):
                if keys[ax][0] != placed_keys[ax][0]:
                    continue
                if abs(keys[ax][1] - placed_keys[ax][1]) > RADIUS:
                    continue
                if player == placed_player:
                    own_counts[ax] += 1
                else:
                    opp_counts[ax] += 1
        for ax in range(3):
            self._h[phase][ax]["own"][own_counts[ax]] += 1
            self._h[phase][ax]["opp"][opp_counts[ax]] += 1

    def summarise(self) -> dict:
        return {
            "per_axis": {
                ph: {
                    ax: {
                        "own": dict(self._h[ph][ax]["own"]),
                        "opp": dict(self._h[ph][ax]["opp"]),
                    }
                    for ax in range(3)
                }
                for ph in self._h
            }
        }
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_locality.py -v`
Expected: 5 PASS. (gitignored — no commit.)

---

## Task 11: Binary book + JSON writers

**Files:**
- Create: `scripts/openbook/io_book.py`
- Create: `scripts/openbook/tests/test_io_book.py`

`write_book_bin(stats, path)`:
- For each `(hash, move) → MoveStats`, emit one 22-byte record per move (one record per move per position; many records per position allowed).
- Sort by `(hash, -weight, move_q, move_r)`.
- Format: `<QhhHHIh`.

`write_tree_json(tree, path)`:
- Dump the node table: `{hex(hash): {"branching": k, "moves": {"q,r": {"high": n, "low": n}}}}`.

`write_theory_json(theory_index, path)`:
- Dump `{"0x...": "Name"}`.

`read_book_bin(path)` — used by tests only — returns the list back.

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_io_book.py
import json
import struct
from pathlib import Path

from openbook.io_book import (
    RECORD_FORMAT, RECORD_SIZE, read_book_bin, write_book_bin,
    write_theory_json, write_tree_json,
)


def test_record_format_is_22_bytes():
    assert RECORD_SIZE == 22
    assert struct.calcsize(RECORD_FORMAT) == 22


def test_book_round_trips(tmp_path: Path):
    stats = {
        0xAABB: {
            (1, 0): {
                "n_games": 10,
                "weight": 13,
                "winrate": 0.6,
            },
            (0, 1): {
                "n_games": 4,
                "weight": 5,
                "winrate": 0.25,
            },
        },
        0xCCDD: {
            (-1, 0): {
                "n_games": 7,
                "weight": 7,
                "winrate": 0.42,
            }
        },
    }
    path = tmp_path / "book.bin"
    n = write_book_bin(stats, path)
    assert n == 3
    out = read_book_bin(path)
    # 3 records, sorted by (hash, -weight)
    assert len(out) == 3
    # 0xAABB first; within it weight=13 before weight=5.
    assert out[0]["hash"] == 0xAABB
    assert out[0]["move"] == (1, 0)
    assert out[1]["hash"] == 0xAABB
    assert out[1]["move"] == (0, 1)
    assert out[2]["hash"] == 0xCCDD


def test_winrate_fixed_point_round_trip(tmp_path: Path):
    stats = {0x1: {(0, 0): {"n_games": 1, "weight": 1, "winrate": 0.5}}}
    write_book_bin(stats, tmp_path / "b.bin")
    out = read_book_bin(tmp_path / "b.bin")
    assert abs(out[0]["winrate"] - 0.5) < 1e-4


def test_engine_score_placeholder_is_zero(tmp_path: Path):
    stats = {0x1: {(0, 0): {"n_games": 1, "weight": 1, "winrate": 0.5}}}
    write_book_bin(stats, tmp_path / "b.bin")
    out = read_book_bin(tmp_path / "b.bin")
    assert out[0]["engine_score"] == 0


def test_tree_json_shape(tmp_path: Path):
    from openbook.tree import build_tree

    class R:
        def __init__(self, ply, h, mv, hi):
            self.ply_index = ply
            self.position_hash = h
            self.canonical_move = mv
            self.is_high_elo = hi

    recs = [
        R(0, 0xAA, (0, 0), True),
        R(0, 0xAA, (0, 0), False),
        R(0, 0xAA, (1, 0), False),
    ]
    tree = build_tree(recs, max_depth=16)
    path = tmp_path / "tree.json"
    write_tree_json(tree, path)
    out = json.loads(path.read_text())
    assert "0x00000000000000aa" in out
    node = out["0x00000000000000aa"]
    assert node["branching"] == 2
    assert node["moves"]["0,0"]["high"] == 1
    assert node["moves"]["0,0"]["low"] == 1


def test_theory_json_shape(tmp_path: Path):
    from openbook.theory import TheoryIndex

    idx = TheoryIndex({0xDEADBEEF: "MyOpening"})
    path = tmp_path / "theory.json"
    write_theory_json(idx, path)
    out = json.loads(path.read_text())
    assert out["0x00000000deadbeef"] == "MyOpening"
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_io_book.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/io_book.py`**

```python
"""Binary book + JSON writers."""
from __future__ import annotations

import json
import struct
from pathlib import Path
from typing import Any

RECORD_FORMAT = "<QhhHHIh"
RECORD_SIZE = struct.calcsize(RECORD_FORMAT)


def _encode_winrate(w: float) -> int:
    v = round(max(0.0, min(1.0, w)) * 65535)
    return max(0, min(65535, int(v)))


def write_book_bin(
    stats: dict[int, dict[tuple[int, int], dict[str, Any]]],
    path: Path,
) -> int:
    records: list[tuple] = []
    for h, by_move in stats.items():
        for (mq, mr), s in by_move.items():
            weight = max(0, min(65535, int(s["weight"])))
            winrate = _encode_winrate(s["winrate"])
            n_games = max(0, min((1 << 32) - 1, int(s["n_games"])))
            records.append((h, mq, mr, weight, winrate, n_games, 0))
    records.sort(key=lambda r: (r[0], -r[3], r[1], r[2]))
    with open(path, "wb") as fh:
        for rec in records:
            fh.write(struct.pack(RECORD_FORMAT, *rec))
    return len(records)


def read_book_bin(path: Path) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    data = Path(path).read_bytes()
    for i in range(0, len(data), RECORD_SIZE):
        chunk = data[i : i + RECORD_SIZE]
        if len(chunk) < RECORD_SIZE:
            break
        h, mq, mr, weight, winrate, n_games, engine_score = struct.unpack(
            RECORD_FORMAT, chunk,
        )
        out.append({
            "hash": h,
            "move": (mq, mr),
            "weight": weight,
            "winrate": winrate / 65535.0,
            "n_games": n_games,
            "engine_score": engine_score,
        })
    return out


def write_tree_json(tree, path: Path) -> None:
    doc: dict[str, Any] = {}
    for h, node in tree.nodes.items():
        moves_doc: dict[str, dict[str, int]] = {}
        all_moves = set(node.moves_high) | set(node.moves_low)
        for mv in all_moves:
            moves_doc[f"{mv[0]},{mv[1]}"] = {
                "high": int(node.moves_high.get(mv, 0)),
                "low": int(node.moves_low.get(mv, 0)),
            }
        doc[f"0x{h:016x}"] = {
            "branching": len(all_moves),
            "moves": moves_doc,
        }
    Path(path).write_text(json.dumps(doc, indent=2, sort_keys=True))


def write_theory_json(index, path: Path) -> None:
    Path(path).write_text(json.dumps(index.export(), indent=2, sort_keys=True))
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_io_book.py -v`
Expected: 6 PASS. (gitignored — no commit.)

---

## Task 12: Coverage report

**Files:**
- Create: `scripts/openbook/report.py`
- Create: `scripts/openbook/tests/test_report.py`

`coverage_at_depth(games, table, depth, book_hashes)` returns the fraction of games whose canonical position at ply == `depth` is in `book_hashes`.

`write_report(...)` emits `data/analysis/REPORT_BOOK.md`:
- Coverage curve at depths 4, 8, 12, 16.
- Summary counts (positions, records, blunder candidates, theory nodes).
- Top-10 theory junctions by KL.
- Top-20 blunder candidates.
- Pointer to `opening_book.bin`, `opening_tree.json`, `theory_index.json`.

- [ ] **Step 1: Write failing tests**

```python
# scripts/openbook/tests/test_report.py
from openbook.canonical import canonical_plies
from openbook.report import coverage_at_depth, write_report
from openbook.zobrist import ZobristTable


SAMPLE_GAME = {
    "players": [
        {"playerId": "A", "elo": 1000},
        {"playerId": "B", "elo": 1100},
    ],
    "gameResult": {"winningPlayerId": "A"},
    "moves": [
        {"moveNumber": 2, "playerId": "A", "x": 0, "y": 0, "timestamp": 1000},
        {"moveNumber": 3, "playerId": "B", "x": 1, "y": 0, "timestamp": 1100},
        {"moveNumber": 4, "playerId": "B", "x": -1, "y": 1, "timestamp": 1200},
        {"moveNumber": 5, "playerId": "A", "x": 0, "y": 1, "timestamp": 1300},
    ],
}


def test_coverage_zero_when_book_empty():
    t = ZobristTable()
    games = [SAMPLE_GAME]
    cov = coverage_at_depth(games, t, depth=2, book_hashes=set())
    assert cov == 0.0


def test_coverage_one_when_book_contains_depth_position():
    t = ZobristTable()
    # Compute the canonical hash of ply 2 ourselves.
    recs = list(canonical_plies(SAMPLE_GAME, t, max_ply=32))
    target_hash = recs[1].position_hash
    cov = coverage_at_depth(
        [SAMPLE_GAME], t, depth=2, book_hashes={target_hash},
    )
    assert cov == 1.0


def test_write_report_creates_file(tmp_path):
    p = tmp_path / "REPORT_BOOK.md"
    write_report(
        path=p,
        n_games=10,
        n_positions=5,
        n_records=12,
        coverage={4: 0.9, 8: 0.7, 12: 0.5, 16: 0.3},
        blunder_candidates=[],
        theory_junctions=[],
        n_theory=0,
    )
    text = p.read_text()
    assert "Coverage" in text
    assert "0.90" in text or "90" in text
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `pytest scripts/openbook/tests/test_report.py -v`
Expected: ImportError.

- [ ] **Step 3: Implement `scripts/openbook/report.py`**

```python
"""Coverage curve + markdown report."""
from __future__ import annotations

from pathlib import Path
from typing import Iterable

from openbook.canonical import canonical_plies
from openbook.zobrist import ZobristTable


def coverage_at_depth(
    games: Iterable[dict],
    table: ZobristTable,
    depth: int,
    book_hashes: set[int],
) -> float:
    n_total = 0
    n_hit = 0
    for g in games:
        recs = list(canonical_plies(g, table, max_ply=depth))
        if len(recs) < depth:
            continue
        n_total += 1
        if recs[depth - 1].position_hash in book_hashes:
            n_hit += 1
    return n_hit / n_total if n_total else 0.0


def write_report(
    *,
    path: Path,
    n_games: int,
    n_positions: int,
    n_records: int,
    coverage: dict[int, float],
    blunder_candidates: list[tuple[int, tuple[int, int], float, int]],
    theory_junctions: list[tuple[int, float]],
    n_theory: int,
) -> None:
    L: list[str] = []
    L.append("# Hexo opening book — build report")
    L.append("")
    L.append(f"- Games analysed: **{n_games}**")
    L.append(f"- Unique canonical positions: **{n_positions}**")
    L.append(f"- Book records (position, move): **{n_records}**")
    L.append(f"- Named openings matched: **{n_theory}**")
    L.append(f"- Blunder candidates: **{len(blunder_candidates)}**")
    L.append("")
    L.append("## Coverage curve")
    L.append("")
    L.append("| depth | coverage |")
    L.append("|---|---|")
    for d in sorted(coverage):
        L.append(f"| {d} | {coverage[d]*100:.1f}% |")
    L.append("")
    target_hit = coverage.get(8, 0.0) >= 0.80
    L.append(f"Target: ≥80% coverage at ply 8 — "
             f"**{'PASS' if target_hit else 'FAIL'}** "
             f"({coverage.get(8, 0.0) * 100:.1f}%).")
    L.append("")
    L.append("## Top theory junctions (high KL between ELO bands)")
    L.append("")
    L.append("| hash | KL |")
    L.append("|---|---|")
    for h, kl in theory_junctions[:10]:
        L.append(f"| `0x{h:016x}` | {kl:.4f} |")
    L.append("")
    L.append("## Top blunder candidates")
    L.append("")
    L.append("| hash | best human move | winrate | n_games |")
    L.append("|---|---|---|---|")
    for h, mv, wr, n in blunder_candidates[:20]:
        L.append(f"| `0x{h:016x}` | ({mv[0]},{mv[1]}) | {wr*100:.1f}% | {n} |")
    L.append("")
    L.append("## Output files")
    L.append("")
    L.append("- `data/analysis/opening_book.bin`")
    L.append("- `data/analysis/opening_tree.json`")
    L.append("- `data/analysis/theory_index.json`")
    L.append("")
    Path(path).write_text("\n".join(L))
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `pytest scripts/openbook/tests/test_report.py -v`
Expected: 3 PASS. (gitignored — no commit.)

---

## Task 13: CLI driver + end-to-end run

**Files:**
- Create: `scripts/openbook/main.py` (gitignored)
- Create: `scripts/build_opening_book.py` (gitignored)

**Note:** No Makefile change. The openbook is local-only; matching the
existing `scripts/analyze_human_games.py` precedent (no make target).
Run it directly via `.venv/bin/python scripts/build_opening_book.py`.

`main.py` wires everything:
1. Load all `data/*.json` games (reuse `analyze_human_games.py` loader pattern: try/except on each file, count bad).
2. Build a single `ZobristTable`.
3. For each game, run `canonical_plies` to depth 16; feed records to `Aggregator`, `Tree`, `TurnStructStats`, `LocalityStats`.
4. Load `data/analysis/hexopedia_patterns.json` → `TheoryIndex`.
5. Finalize aggregator; run `blunder_candidates`; run `theory_junctions`.
6. Compute coverage at depths {4, 8, 12, 16} (book_hashes = positions present in finalized stats with at least 1 record).
7. Write `opening_book.bin`, `opening_tree.json`, `theory_index.json`, `REPORT_BOOK.md`.

`scripts/build_opening_book.py` is a 6-line driver:

```python
#!/usr/bin/env python3
"""Run the full opening-book pipeline.

Usage: python scripts/build_opening_book.py
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from openbook.main import run

if __name__ == "__main__":
    sys.exit(run())
```

- [ ] **Step 1: Create `scripts/openbook/main.py`**

```python
"""Opening-book pipeline driver."""
from __future__ import annotations

import json
import sys
from pathlib import Path

from openbook.aggregator import Aggregator
from openbook.blunders import blunder_candidates
from openbook.canonical import canonical_plies
from openbook.io_book import (
    write_book_bin, write_theory_json, write_tree_json,
)
from openbook.locality import LocalityStats
from openbook.report import coverage_at_depth, write_report
from openbook.theory import TheoryIndex
from openbook.tree import build_tree, theory_junctions
from openbook.turn_struct import TurnStructStats
from openbook.zobrist import ZobristTable

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "data"
OUT = DATA / "analysis"
PATTERNS = OUT / "hexopedia_patterns.json"
MAX_PLY = 32  # spec: walk plies 1..32
COVERAGE_DEPTHS = (4, 8, 12, 16)


def _load_games() -> tuple[list[dict], int]:
    games: list[dict] = []
    bad = 0
    for p in sorted(DATA.glob("*.json")):
        try:
            with p.open() as fh:
                g = json.load(fh)
            if g.get("moves"):
                games.append(g)
        except (OSError, json.JSONDecodeError):
            bad += 1
    return games, bad


def _phase_of(ply_index: int, total: int) -> str:
    if total <= 0:
        return "early"
    f = (ply_index - 1) / total
    if f < 1 / 3:
        return "early"
    if f < 2 / 3:
        return "mid"
    return "late"


def run() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    print("Loading games...", file=sys.stderr)
    games, bad = _load_games()
    print(f"  parsed {len(games)} games ({bad} unreadable)", file=sys.stderr)

    table = ZobristTable()
    agg = Aggregator()
    all_records: list = []
    turn_stats = TurnStructStats()
    locality = LocalityStats()

    print("Walking games...", file=sys.stderr)
    for g in games:
        moves = g.get("moves") or []
        turn_stats.update(moves)
        for rec in canonical_plies(g, table, max_ply=MAX_PLY):
            agg.add(rec)
            all_records.append(rec)
            # Locality observed against the pre-move stones in canonical
            # frame is intentional — same orbit.
            phase = _phase_of(rec.ply_index, len(moves))
            locality.observe_stone(
                placed=rec.canonical_move,
                placed_player=rec.base.mover_byte,
                prior_stones=list(rec.base.stones),
                phase=phase,
            )

    print("Finalising stats...", file=sys.stderr)
    stats = agg.finalize()
    n_positions = len(stats)
    n_records = sum(len(by_mv) for by_mv in stats.values())

    print("Building opening tree...", file=sys.stderr)
    tree = build_tree(all_records, max_depth=16)
    junctions = theory_junctions(tree, top_n=20)

    print("Detecting named openings...", file=sys.stderr)
    patterns_doc = (
        json.loads(PATTERNS.read_text()) if PATTERNS.exists() else {"patterns": []}
    )
    theory_idx = TheoryIndex.from_patterns(patterns_doc, table)
    n_theory = sum(1 for h in stats if theory_idx.matches(h) is not None)

    print("Flagging blunder candidates...", file=sys.stderr)
    blunders = blunder_candidates(stats)

    print("Computing coverage curve...", file=sys.stderr)
    book_hashes = set(stats)
    coverage = {
        d: coverage_at_depth(games, table, d, book_hashes)
        for d in COVERAGE_DEPTHS
    }

    print("Writing outputs...", file=sys.stderr)
    n_written = write_book_bin(stats, OUT / "opening_book.bin")
    write_tree_json(tree, OUT / "opening_tree.json")
    write_theory_json(theory_idx, OUT / "theory_index.json")
    write_report(
        path=OUT / "REPORT_BOOK.md",
        n_games=len(games),
        n_positions=n_positions,
        n_records=n_records,
        coverage=coverage,
        blunder_candidates=blunders,
        theory_junctions=junctions,
        n_theory=n_theory,
    )
    # Side stats for downstream consumers
    (OUT / "turn_struct.json").write_text(
        json.dumps(
            _stringify_keys(turn_stats.summarise()), indent=2, sort_keys=True,
        )
    )
    (OUT / "axis_locality.json").write_text(
        json.dumps(locality.summarise(), indent=2, sort_keys=True)
    )
    print(
        f"  wrote {n_written} records to {OUT/'opening_book.bin'}",
        file=sys.stderr,
    )
    print("Done.", file=sys.stderr)
    return 0


def _stringify_keys(obj):
    """JSON doesn't allow tuple keys; recursively stringify them."""
    if isinstance(obj, dict):
        return {
            _stringify_key(k): _stringify_keys(v) for k, v in obj.items()
        }
    if isinstance(obj, list):
        return [_stringify_keys(v) for v in obj]
    return obj


def _stringify_key(k):
    if isinstance(k, tuple):
        return "|".join(_stringify_key(x) for x in k)
    return str(k)
```

- [ ] **Step 2: Create `scripts/build_opening_book.py`**

```python
#!/usr/bin/env python3
"""Run the full opening-book pipeline.

Usage: python scripts/build_opening_book.py
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from openbook.main import run

if __name__ == "__main__":
    sys.exit(run())
```

- [ ] **Step 3: Run full openbook test suite**

Run: `cd /home/timmy/Work/hexo_minimax && .venv/bin/python -m pytest scripts/openbook/tests -v`
Expected: every previously-added test still passes (cumulative).

- [ ] **Step 4: Run the pipeline end-to-end**

Run: `cd /home/timmy/Work/hexo_minimax && .venv/bin/python scripts/build_opening_book.py`
Expected:
- prints loading/walking/finalising progress
- writes `data/analysis/opening_book.bin`, `opening_tree.json`, `theory_index.json`, `REPORT_BOOK.md`, `turn_struct.json`, `axis_locality.json`
- exits 0

- [ ] **Step 5: Manually inspect the report**

Run: `head -60 /home/timmy/Work/hexo_minimax/data/analysis/REPORT_BOOK.md`
Expected: human-readable markdown with coverage table; ≥80% coverage at ply 8 reported as PASS or FAIL.

No commits — all openbook source and outputs are gitignored.

---

## Self-Review Notes

- **Spec coverage:** All 8 numbered spec items have explicit tasks (1–10). ELO bucketing implemented in Task 4 via `n_high_elo_games` + `weight`. 3× weight on high-ELO done in `weight = n + 3*n_high`. Coverage curve at depths {4, 8, 12, 16} done in Task 12 + emitted in Task 13.
- **No placeholders:** Every code step has full code. The HeXOpedia pattern list ships as an empty stub (`{"patterns": []}`) — that is intentional, not a placeholder: the spec says "caller supplies pattern list," and the index handles empty input gracefully.
- **Type consistency:** `Cell = tuple[int, int]`, `StonePair = tuple[Cell, int]`, `position_hash: int`, `canonical_move: Cell` used identically across symmetry, zobrist, walker, canonical, aggregator, tree, theory, io_book, report. `Aggregator.finalize()` returns the exact dict shape consumed by `write_book_bin` and `blunder_candidates`.
- **Engine-score placeholder:** Always `0` in the binary record per spec.
- **Coverage target:** Computed and emitted; report does not fail the build if target missed (the spec says "target," not "gate").
