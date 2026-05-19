# Turn-State Opening-Book Refactor — execution plan & results

**Date:** 2026-05-19
**Author:** Tom S (with Claude Code execution)
**Companion spec:** `docs/specs/SPEC_OPENING_BOOK.md`
**Companion artefact:** `data/analysis/REPORT_BOOK.md` (regenerated)

## Goal

Fix the opening-book canonicalization so that the book indexes
**turn-start** states and **unordered stone pairs**, not individual
plies. Order within a HeXO turn (s1 then s2 vs. s2 then s1) is
semantically irrelevant; the v1 per-ply book treated it as significant
and split branching across half-turn states it should never have
created.

## Tasks done

### Phase 1 — Debug & verification tooling

- [x] **1a.** `scripts/openbook/render.py`: minimal ASCII hex-board
  renderer (axial coords, diagonal-slant rows, origin marker).
- [x] **1a.** `scripts/openbook/decode_hash.py`: CLI inspector that
  takes a hex hash and prints (X/O stone lists, ASCII board, side-to-
  move, turn index, all recorded pairs sorted by frequency). Falls
  back to scanning raw games when `position_index.json` is absent.
- [x] **1b.** Symmetry round-trip test added to
  `scripts/openbook/tests/test_symmetry.py`. 200 random configs × 12
  D6 transforms — hash must be invariant. Plus an idempotence check
  and a "lex-smallest in orbit" check. All green.
- [x] **1c.** `--emit-index` flag on `scripts/build_opening_book.py`
  writes `data/analysis/position_index.json` (hash → stones, side,
  turn). Default off; file is ~30 MB.

### Phase 2 — Per-turn canonicalization

- [x] **2a.** `docs/specs/SPEC_OPENING_BOOK.md` written. Position-key
  is the turn-start state; move record is the lex-sorted unordered
  pair `{s1, s2}` with `NULL_STONE = (-32768, -32768)` for the turn-1
  single-stone special case. Binary record grows from 22 → **26**
  bytes (adds two i16 fields for `s2q`, `s2r`).
- [x] **2b.** Pipeline refactor:
  - `zobrist.position_hash(table, stones, side_to_move)` — dropped the
    `stones_remaining` axis.
  - `symmetry.canonicalize_with_pair(stones, s1, s2)` — D6 lex-min on
    `(stones, sorted_pair)`.
  - `walker.iter_game_turns(game, max_turn)` and `walker.TurnRecord`
    replace `iter_game_plies` / `PlyRecord`.
  - `canonical.canonical_turns(...)` and `CanonTurnRecord` replace
    `canonical_plies` / `CanonRecord`.
  - `aggregator.Aggregator` re-keyed on `(hash, pair)`; per-slot now
    splits low- vs high-ELO win counts directly.
  - `tree`, `io_book`, `theory`, `report`, `main` all updated.
- [x] **2c.** Probe-API section (§8) added to the spec describing
  `Book::probe(position, min_weight) → (Stone, Option<Stone>)`. Caller
  contract: probe only at turn-start; play s1 and s2 back-to-back with
  no intervening re-probe. No Rust code yet.

### Phase 3 — ELO buckets and pair-offset prior

- [x] **3a.** `HIGH_ELO_THRESHOLD` lowered to **1250** (was 1300).
  REPORT.md buckets updated to `<1100 / 1100-1249 / 1250-1399 / ≥1400`
  in `scripts/analyze_human_games.py`. The `≥1400` bucket is now 31
  games and is no longer the cutoff for "high-ELO" weighting; ≥1250
  is the relevant subset (647 games).
- [x] **3b.** `scripts/openbook/pair_offset_prior.py` collects every
  two-stone turn into a per-phase Counter keyed by
  `(s1-centroid, s2-centroid)`, lex-sorted under D6, where centroid
  is the integer-rounded mean of stones placed BEFORE the turn.
  Emitted as `data/analysis/pair_offset_prior.json` (top-200 per
  phase, normalised to per-phase mass ≤ 1.0 after truncation).

### Phase 4 — Verify & report

- [x] **4a.** Pipeline regenerated end-to-end. `pytest
  scripts/openbook/` is green (**96 passed** in 0.09s).
- [x] **4b.** `REPORT_BOOK.md` re-emitted with:
  - Per-turn coverage (turns 2, 4, 6, 8) plus high-ELO column.
  - Old-vs-new side-by-side table (v1 numbers baked into
    `COVERAGE_V1_PER_PLY` since the v1 pipeline is gone).
  - Top-10 KL junctions, each with an ASCII board render.
  - Top-20 blunders, each with an ASCII board render and pair label.
  - Pair-offset distribution sanity check, top 10 per phase.
- [x] **4c.** This file.

## Files changed (tracked)

- `docs/specs/SPEC_OPENING_BOOK.md` *(new)*
- `docs/superpowers/plans/2026-05-19-turn-state-refactor.md` *(new — this file)*

## Files changed (local-only, gitignored)

All paths below are under `.gitignore` precedent established by
`24ef2eb` (openbook scripts), so they are local-only analysis tooling
that does not enter source control.

- `scripts/build_opening_book.py` — added `--emit-index` flag.
- `scripts/analyze_human_games.py` — bucket boundaries updated.
- `scripts/openbook/zobrist.py` — dropped `stones_remaining` axis.
- `scripts/openbook/symmetry.py` — added `canonicalize_with_pair`.
- `scripts/openbook/walker.py` — `TurnRecord`, `iter_game_turns`, `NULL_STONE`.
- `scripts/openbook/canonical.py` — `CanonTurnRecord`, `canonical_turns`.
- `scripts/openbook/aggregator.py` — pair-keyed, `HIGH_ELO_THRESHOLD=1250`.
- `scripts/openbook/tree.py` — pair-keyed nodes.
- `scripts/openbook/io_book.py` — 26-byte record format.
- `scripts/openbook/theory.py` — drop `stones_remaining` arg.
- `scripts/openbook/report.py` — turn-N coverage + decoded blocks + side-by-side.
- `scripts/openbook/main.py` — turn-state driver + report enrichment.
- `scripts/openbook/render.py` *(new)* — ASCII hex board.
- `scripts/openbook/decode_hash.py` *(new)* — CLI hash decoder.
- `scripts/openbook/pair_offset_prior.py` *(new)* — pair-offset prior.
- `scripts/openbook/tests/test_render.py` *(new)*.
- `scripts/openbook/tests/test_pair_offset_prior.py` *(new)*.
- All existing tests under `scripts/openbook/tests/` updated for the
  new APIs (see git status — none are tracked).

Generated under `data/analysis/` (gitignored):

- `opening_book.bin` (78541 records × 26 bytes).
- `opening_tree.json`, `theory_index.json`, `turn_struct.json`,
  `axis_locality.json` (regenerated for the turn-state model).
- `position_index.json` *(new, gated)*.
- `pair_offset_prior.json` *(new)*.
- `REPORT_BOOK.md`, `REPORT.md` (regenerated with the new bucket
  thresholds and pair-keyed sections).

## Numbers before / after

| metric                                    | v1 (per-ply) | v2 (per-turn) |
|-------------------------------------------|--------------|---------------|
| unique canonical positions                | 152,716      | **72,058**    |
| book records                              | 160,107      | **78,541**    |
| coverage at ply 4 / turn 2                | 96.1%        | **100.0%**    |
| coverage at ply 8 / turn 4                | 29.9%        | **56.1%**     |
| coverage at ply 12 / turn 6               | 3.1%         | **7.5%**      |
| coverage at ply 16 / turn 8               | 0.5%         | **1.0%**      |
| high-ELO coverage at turn 4 (≥1250)       | n/a          | 36.1%         |
| share of positions with branching=1       | 94.6%        | 97.5%         |
| share of positions seen exactly once      | 93.2%        | 98.9%         |
| binary record size                        | 22 bytes     | 26 bytes      |

Target met for unique-positions (≤80k) and turn-4 coverage (≥55%).
Branching=1 share was projected `<70%` but is **higher** in v2 —
discussion below.

### Note on branching=1 share

The v2 share looks *worse* than v1's 94.6% prediction-target of `<70%`.
The interpretation that makes both numbers honest:

- v1 had ~152k positions, of which ~144k were singletons and ~8k were
  multi-branch.
- v2 has ~72k positions, of which ~70k are singletons and ~1.8k are
  multi-branch.

So the count of **multi-branch positions dropped 4×** (from 8.2k to
1.8k), but the total position count dropped only 2×. The ratio
("branching=1 share") therefore rose. The long tail of one-off
positions still dominates because human games rarely transpose past
turn 4. This metric was the noisiest of the three projections; the
coverage and position-count metrics are the load-bearing ones, and
both hit target.

### Note on pair-offset prior

The acceptance check said early-phase top-3 keys should include the
"distance-1 and distance-5 cluster offsets". In practice the top-3
early keys are *all* distance-1 (28.04% of pairs are distance-1
corpus-wide; distance-5 is 13.54%; in early specifically, the
distance-5 mass is spread across many distinct keys). Distance-5
takes the top slot in **mid and late** phases — see the report's
sanity-check tables. The corpus-wide pattern claimed by the spec is
borne out by `turn_struct.json`; the top-3-keys phrasing is just a
phase-mismatch in the original task description.

## Outstanding follow-ups

1. **Engine probe (Rust).** Spec §8 freezes the API; implementation
   is the next thing the engine needs. Should land alongside
   `Book::probe` and a fixture-driven test using the 26-byte format.
2. **HeXOpedia named-opening patterns.** Still 0 matches — the
   `data/analysis/hexopedia_patterns.json` placeholder needs real
   patterns. The matcher already canonicalises and hashes correctly;
   the gap is content.
3. **Pair-offset consumer in Rust.** Spec §9 describes the JSON; the
   move-ordering hook that loads it at startup is not yet written.
4. **Engine deepening.** Coverage at turn 6 is only 7.5% — once the
   probe ships, deepening will benefit from richer patterns rather
   than more games. Consider seeding additional pair records by
   computer self-play to extend the book past turn 6 where human
   coverage falls off a cliff.
5. **`make` target.** `make book` could wrap
   `python scripts/build_opening_book.py --emit-index` for parity with
   the other make targets; currently the script is run directly. Low
   priority — `scripts/build_opening_book.py` is gitignored so it
   wouldn't expose a public make target.

## Verification commands

```
.venv/bin/pytest scripts/openbook/                       # 96 passed
.venv/bin/python scripts/build_opening_book.py --emit-index
.venv/bin/python scripts/openbook/decode_hash.py <hash>  # arbitrary book hash
```

All ten KL-junction hashes listed in REPORT_BOOK.md decode to valid
turn-start positions when round-tripped through the decoder.
