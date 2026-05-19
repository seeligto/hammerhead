# SPEC_OPENING_BOOK — opening book canonicalization, format, and probe API

Status: ACTIVE. Supersedes the per-ply layout described in
`docs/superpowers/plans/2026-05-19-opening-book.md`. That earlier layout
is **DEPRECATED**; old `data/analysis/opening_book.bin` files must be
regenerated, not converted.

Audience: openbook tooling under `scripts/openbook/` (Python, offline)
and a future Rust probe under `hexo-engine/src/book.rs` (not yet built —
this spec freezes the API surface).

## 1 — Game-state recap

HeXO turn structure:

- **Turn 1.** X places one stone at the origin `(0, 0)`. No choice.
- **Turn N ≥ 2.** The mover places *two* stones in the same turn. The
  two stones are played back-to-back. *Order within a turn is
  semantically irrelevant*: the post-turn position depends only on
  the unordered pair `{s1, s2}`.

Win = any six-in-a-row along one of the three hex axes.
Legal-cell rule: a cell is legal if empty AND within hex-distance 8 of
some existing stone (any colour).

Axial coordinates `(q, r)` with `s = -q - r`.

## 2 — Position-key definition

A book *position-key* is the canonical hash of a **turn-start state**.
A turn-start state is the board immediately before the mover places
their first stone of the current turn. Equivalently, *zero* stones
have been placed for the side-to-move so far in this turn.

The book never indexes mid-turn states (after stone-1, before stone-2).
That was the per-ply v1 mistake: it bloated the position table with
half-turns whose continuations were uninformative, and split the natural
turn-level branching factor.

Inputs to the canonical hash:

- `X_stones`: lex-sorted `(q, r)` tuples of player-0 stones, after the
  chosen D6 transform.
- `O_stones`: lex-sorted `(q, r)` tuples of player-1 stones, after the
  same transform.
- `side_to_move`: 0 = X, 1 = O.

There is no `stones_remaining` axis — by construction the key only
exists for the moment when `stones_remaining == 2` (or `== 1` for the
opening Turn 1).

## 3 — Symmetry group

The hex board has the dihedral group D6 (12 elements): 6 rotations
(0°, 60°, …, 300°) combined with `{identity, reflection}`. The 12
transforms are enumerated in `scripts/openbook/symmetry.py` as
`TRANSFORMS[0..11]`.

The **canonical form** of a turn-start state is the lex-smallest
`(sorted X_stones, sorted O_stones)` across all 12 D6 images of the
input. Tie-break (vanishingly rare) by the smaller transform index.

Round-trip invariant (verified by
`scripts/openbook/tests/test_symmetry.py`): for any state `S` and any
`T ∈ D6`, `canonical_hash(S) == canonical_hash(T(S))`.

## 4 — Move record

A move record represents the unordered pair of stones the mover plays
this turn. After applying the same D6 transform that canonicalised the
position:

- The pair `{s1, s2}` is **lex-sorted on `(q, r)`** so that
  `s1 ≤ s2`. We call this *canonical pair order*.
- For **Turn 1** there is only one stone; it is placed at `s1 = (0, 0)`
  by rule. `s2` is the sentinel `NULL_STONE = (i16::MIN, i16::MIN)`
  meaning "no second stone".

`NULL_STONE` is required so the record layout is fixed-width.

## 5 — Binary record layout (revised)

```
struct format: '<Qhhhh H H I h'   little-endian, no padding (24 bytes)
fields:
  hash         u64    canonical position hash (turn-start)
  s1q, s1r     i16,i16  first stone of pair (canonical lex-min)
  s2q, s2r     i16,i16  second stone, or NULL_STONE = (i16::MIN, i16::MIN)
  weight       u16    scaled composite weight (see §6)
  winrate      u16    winrate * 65535 (high-ELO subset)
  n_games      u32    total games seen at this (pos, pair)
  engine_score i16    engine eval placeholder, default 0
```

Total: 8 + 8 + 2 + 2 + 4 + 2 = **26 bytes/record**. (`<Qhhhh H H I h`
packs to 26 with no padding under `struct`.)

`NULL_STONE` constant: `(i16::MIN, i16::MIN) = (-32768, -32768)`. A
record with this `s2` value is interpreted as the opening Turn 1 single
stone. A non-NULL `s2` records both stones of a two-stone turn.

Records are sorted by `(hash, -weight, s1q, s1r, s2q, s2r)` so a binary
search by hash returns the heaviest pair first.

`Records.sort` key is exactly the same shape as in the deprecated layout
plus the two extra `(s2q, s2r)` fields appended.

## 6 — Weighting

Per (position, pair):

- `n_games_low`  = games at this pair with both players' ELO < 1250.
- `n_games_high` = games at this pair with at least one player ≥ 1250.

`weight = min(65535, n_games_low + 3 * n_games_high)`.

`winrate` is computed from the **high-ELO subset** only. If no
high-ELO games were observed at this pair, `winrate = 0` and the
caller should rely on `weight` alone for ordering.

The ELO bucket boundaries are normative (§9 of `REPORT.md`):
`<1100`, `1100–1249`, `1250–1399`, `≥1400`. The "high-ELO subset" is
`≥1250` (i.e. the union of the top two buckets).

## 7 — Aggregation, tie-breaking, and turn-1 special case

Aggregation walks each game *turn-by-turn*, not ply-by-ply:

```
for each turn t in [1 .. N]:
    state = board state immediately before turn t
    hash  = canonical_hash(state.X_stones, state.O_stones, side_to_move)
    if t == 1:
        pair = (s1=(0,0), s2=NULL_STONE)        # forced
    else:
        # canonicalise the played pair under the same D6 transform
        # that canonicalised state, then lex-sort
        (a, b) = canonicalise_pair(played, T)
        pair = (min(a, b), max(a, b))
    record (hash, pair) once per game
```

A position-pair pair is counted **once per game**, not twice (the
walker yields a per-turn record, not a per-ply record).

## 8 — Probe API (Rust, planned — not yet implemented)

```rust
pub struct Book {
    // memory-mapped or fully-loaded array of fixed-width records,
    // sorted by hash for binary search.
    records: ...,
}

pub enum ProbeMiss {
    NotInBook,
    BelowWeightThreshold,  // entry exists but weight < min_weight
}

impl Book {
    /// Probe the book at the current turn-start position.
    ///
    /// Returns:
    ///   - Ok((s1, Some(s2))) for a two-stone turn pair, or
    ///   - Ok((s1, None))     for the opening Turn 1 (s2 == NULL_STONE).
    ///
    /// Caller's contract: on Ok, play s1 immediately; for two-stone
    /// turns, store s2 and play it on the next stone-decision call
    /// without re-probing the book. This is correctness-critical: the
    /// canonical book position only exists at turn-start; mid-turn
    /// probing is undefined.
    pub fn probe(&self, position: &Position, min_weight: u16)
        -> Result<(Stone, Option<Stone>), ProbeMiss>;
}
```

Probe semantics:

- A probe is valid only when `stones_remaining_this_turn == 2`
  (the opening Turn 1 is a single-stone special case that the bot
  short-circuits without probing). The book never indexes mid-turn
  states, so a mid-turn probe is a caller bug.
- On `Ok((s1, Some(s2)))` the bot must play `s1` first, then `s2`,
  with **no further book probe** in between. The two stones together
  comprise the recorded pair; splitting that across two probes would
  reintroduce the per-ply path-dependence the refactor removed.
- The Rust side reads the binary book via `mmap` at startup. Records
  are sorted by hash; lookup is a binary search.

## 9 — Pair-offset prior (planned consumer)

`data/analysis/pair_offset_prior.json` (written by Phase 3 of the
refactor) is consumed by Rust move-ordering when scoring candidate
stone-2 choices given stone-1. The Rust engine loads it from JSON at
startup (offline / cold path); the hot path uses pre-converted internal
tables. This file is not a probe target — it is a prior over relative
positioning, used only when the book itself misses.

Schema:

```json
{
  "schema_version": 1,
  "n_pairs_total": <int>,
  "phase": {
    "early": { "(dq1,dr1)|(dq2,dr2)": weight, ... },
    "mid":   { ... },
    "late":  { ... }
  }
}
```

Where weight is normalised to sum to 1.0 per phase. Keys are
canonical-sorted offsets relative to the centroid of all stones at
the time the pair was played, with hex-symmetry applied so the 12
rotational equivalents collapse to a single key. Top-200 entries per
phase only (long tail is noise).

## 10 — Migration

The deprecated per-ply book layout has the following observable
problems on the current corpus (6749 games):

| metric                | per-ply v1 | per-turn v2 (target) |
|-----------------------|------------|----------------------|
| unique positions      | 152716     | ~50k–80k             |
| branching=1 share     | 94.6%      | <70%                 |
| singleton positions   | 93.2%      | <50%                 |
| ply-8 / turn-4 cov.   | 29.9%      | >55%                 |

The new pipeline emits the new format; the deprecated format is not
read by anything in v2. Existing files must be regenerated:

```
python scripts/build_opening_book.py             # default: turn-state
python scripts/build_opening_book.py --emit-index  # plus stones map
```

## 11 — Reverse index

`data/analysis/position_index.json` (optional, written when
`--emit-index` is set) maps `canonical_hash → { stones, side_to_move,
turn_index }`. Used by `scripts/openbook/decode_hash.py` for O(1) hash
inspection. Not consumed by the Rust engine.

Schema:

```json
{
  "schema_version": 1,
  "n_entries": <int>,
  "entries": {
    "0x49882ed124f2264f": {
      "stones":       [[[0,0], 0], [[-6,1], 1]],
      "side_to_move": 1,
      "turn_index":   2
    },
    ...
  }
}
```

`turn_index` is 1-based; turn 1 is X's single forced opening stone.

## 12 — Sentinels and constants

```
NULL_STONE       = (-32768, -32768)          # placeholder for absent s2
HIGH_ELO_FLOOR   = 1250                      # ≥ this counts as high-ELO
MAX_BOOK_DEPTH   = 16 turns (≈ 32 plies)     # walker stops past this
```

## 13 — Verification

Each refactor commit must keep these green:

- `pytest scripts/openbook/`
- `python scripts/openbook/decode_hash.py <hash>` round-trips for every
  hash listed in REPORT_BOOK.md § "Top KL junctions".
- `test_symmetry.py::test_canonical_hash_invariant_under_d6_for_200_random_configs`.
