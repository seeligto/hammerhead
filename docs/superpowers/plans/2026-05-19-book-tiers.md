# Opening Book Cleanup + Engine Deepening + Tier System

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Trim the per-turn opening book from 78k noisy records to a tiered, deepened, high-signal artifact that the future Rust probe can rely on, while keeping the full analysis tree for offline study.

**Architecture:** Three phases. **A** is offline data cleanup (spec, decoder, build flags, two new file outputs). **B** runs the engine deeper than production on a curated target set and stores results to `data/analysis/engine_eval.json`. **C** introduces a tier classifier that fuses human winrate + engine eval + sample count into `TIER_1..4 / DROP`, then re-emits the book at a new 28-byte record format split by tier. **D** rewrites `REPORT_BOOK.md`. No Rust code touched. No `hexo.toml` touched.

**Tech Stack:** Python 3 (`scripts/openbook/*`), `struct` for binary records, existing `hexo_engine.Engine` Python binding for the deepening sweep.

**Constraints (from CLAUDE.md + task brief):**
- Caveman commits. < 72 char subject. **Never** `Co-Authored-By: Claude` or any Claude attribution.
- Atomic commits — one logical change per commit.
- `make check` green at every phase boundary (we run `make test` since this is all Python; `make check` would also re-lint Rust, which is fine but unchanged).
- Rust engine code untouched. `hexo.toml` untouched. `refs/` untouched.

---

## File Structure

**Modified:**
- `docs/specs/SPEC_OPENING_BOOK.md` — typo, format bump, new sections
- `scripts/openbook/blunders.py` — emit both ELO bands
- `scripts/openbook/decode_hash.py` — show both ELO columns
- `scripts/openbook/aggregator.py` — track position-conditional winrate inputs
- `scripts/openbook/canonical.py` — expose `is_wide_opening` flag on record
- `scripts/openbook/tree.py` — store pos_n_games_through / pos_x_wins
- `scripts/openbook/io_book.py` — 28-byte record format; tier-aware writer; wide split
- `scripts/openbook/report.py` — wide vs tight, pos-conditional histogram, tier breakdown, engine-vs-human, top safe / top trap
- `scripts/openbook/main.py` — wire it all up; CLI flags
- `scripts/build_opening_book.py` — pass through `--prune-singletons`, `--engine-eval`
- `scripts/openbook/tests/test_blunders.py` — new band-emission tests
- `scripts/openbook/tests/test_aggregator.py` — position-conditional winrate test
- `scripts/openbook/tests/test_io_book.py` — 28-byte format tests
- `data/analysis/REPORT_BOOK.md` — regenerated end of Phase D
- `data/analysis/opening_book.bin` — regenerated, smaller, tiered
- `data/analysis/opening_tree.json` — regenerated with new fields

**New:**
- `scripts/openbook/tier.py` — tier classifier
- `scripts/openbook/tier_config.py` — thresholds
- `scripts/openbook/deepen.py` — engine deepening runner
- `scripts/openbook/tests/test_tier.py`
- `scripts/openbook/tests/test_deepen.py`
- `data/analysis/deepening_targets.json`
- `data/analysis/engine_eval.json`
- `data/analysis/wide_openings.bin`  (intermediate, Phase A)
- `data/analysis/trap_inventory.bin` (Phase C; replaces wide_openings.bin)

---

## Phase A — Cleanup

### Task A1: Fix SPEC byte-count typo

**Files:**
- Modify: `docs/specs/SPEC_OPENING_BOOK.md` (the `## 5 — Binary record layout` heading area)

- [ ] **Step 1: Read the current heading**

The current spec text (verified by `head -n 80 docs/specs/SPEC_OPENING_BOOK.md | grep -n bytes`) shows:

```
## 5 — Binary record layout (revised)

```
struct format: '<Qhhhh H H I h'   little-endian, no padding (24 bytes)
```

The bracketed `(24 bytes)` is the typo; the math three lines below the fence shows `Total: 8 + 8 + 2 + 2 + 4 + 2 = **26 bytes/record**`.

- [ ] **Step 2: Replace the typo**

Change the fence comment from `(24 bytes)` to `(26 bytes)`. Do not touch any other line.

- [ ] **Step 3: Commit**

```bash
git add docs/specs/SPEC_OPENING_BOOK.md
git commit -m "spec: openbook record header is 26 bytes not 24"
```

---

### Task A2: Blunder finder emits both ELO bands; decoder shows both; section is renamed

**Files:**
- Modify: `scripts/openbook/blunders.py`
- Modify: `scripts/openbook/decode_hash.py`
- Modify: `scripts/openbook/report.py`
- Modify: `scripts/openbook/main.py`
- Modify: `scripts/openbook/tests/test_blunders.py`

The current `blunder_candidates(stats)` returns 4-tuples `(hash, pair, winrate, n_games)` where `winrate` and `n_games` are the **all-ELO** aggregates (from `s["winrate"]` and `s["n_games"]`). The user reports that this disagrees with `book-best`, which uses `winrate_high_elo` and `n_high_elo_games`. Fix: return a 6-tuple, callers updated, tests pinned.

- [ ] **Step 1: Write the failing test**

Append to `scripts/openbook/tests/test_blunders.py`:

```python
def test_blunder_emits_both_elo_bands():
    """A blunder candidate must carry both all-ELO and high-ELO winrates
    so REPORT_BOOK and decode_hash can show them side by side."""
    stats = {
        0xE: {
            P1: {
                "n_games": 25,
                "n_high_elo_games": 5,
                "winrate": 0.30,
                "winrate_high_elo": 0.20,
                "weight": 30,
            },
            P2: {
                "n_games": 10,
                "n_high_elo_games": 0,
                "winrate": 0.40,
                "winrate_high_elo": 0.0,
                "weight": 10,
            },
        }
    }
    out = blunder_candidates(stats)
    assert len(out) == 1
    rec = out[0]
    assert rec.hash == 0xE
    assert rec.pair == P1
    assert abs(rec.all_elo_winrate - 0.30) < 1e-9
    assert rec.all_elo_n_games == 35
    assert abs(rec.high_elo_winrate - 0.20) < 1e-9
    assert rec.high_elo_n_games == 5


def test_blunder_no_high_elo_data():
    stats = {
        0xF: {
            P1: {
                "n_games": 25,
                "n_high_elo_games": 0,
                "winrate": 0.25,
                "winrate_high_elo": 0.0,
                "weight": 25,
            },
        }
    }
    out = blunder_candidates(stats)
    assert len(out) == 1
    rec = out[0]
    assert rec.high_elo_n_games == 0
    # winrate_high_elo is None (not 0.0) when no high-ELO games seen;
    # this lets the report formatter print '—' rather than '0.0%'.
    assert rec.high_elo_winrate is None
```

The existing tests in this file currently destructure 4-tuples — update them to use the new dataclass interface in the same edit. The new shape:

```python
@dataclass(frozen=True)
class BlunderCandidate:
    hash: int
    pair: tuple[tuple[int, int], tuple[int, int]]
    all_elo_winrate: float
    all_elo_n_games: int
    high_elo_winrate: float | None
    high_elo_n_games: int
```

Migrate `test_position_with_best_winrate_below_threshold_flagged` and `test_best_pair_picked_by_weight` to assert attributes by name (`rec.hash`, `rec.pair`, `rec.all_elo_winrate`, `rec.all_elo_n_games`) — these two tests rely on the 4-tuple shape today.

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cd /home/timmy/Work/hexo_minimax
.venv/bin/pytest scripts/openbook/tests/test_blunders.py -v
```

Expected: failures because `blunder_candidates` still returns 4-tuples.

- [ ] **Step 3: Replace `scripts/openbook/blunders.py`**

```python
"""Blunder-candidate flagging: positions whose heaviest human move loses."""
from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class BlunderCandidate:
    hash: int
    pair: tuple
    all_elo_winrate: float
    all_elo_n_games: int
    high_elo_winrate: float | None
    high_elo_n_games: int


def blunder_candidates(
    stats: dict[int, dict[tuple, dict[str, Any]]],
    min_n_games: int = 20,
    max_best_winrate: float = 0.4,
) -> list[BlunderCandidate]:
    out: list[BlunderCandidate] = []
    for h, by_move in stats.items():
        total_n = sum(s["n_games"] for s in by_move.values())
        if total_n < min_n_games:
            continue
        best_move, best_s = max(
            by_move.items(), key=lambda kv: kv[1]["weight"],
        )
        if best_s["winrate"] >= max_best_winrate:
            continue
        n_he = int(best_s.get("n_high_elo_games", 0))
        wr_he = (
            float(best_s["winrate_high_elo"]) if n_he > 0 else None
        )
        out.append(BlunderCandidate(
            hash=h,
            pair=best_move,
            all_elo_winrate=float(best_s["winrate"]),
            all_elo_n_games=total_n,
            high_elo_winrate=wr_he,
            high_elo_n_games=n_he,
        ))
    out.sort(key=lambda r: r.all_elo_winrate)
    return out
```

- [ ] **Step 4: Update every caller**

Search for callers:

```bash
grep -rn 'blunder_candidates\|blunder_decoded\|_decode_blunder_block' scripts/openbook/
```

Hits to patch:
- `scripts/openbook/main.py` — `_decode_blunder_block(blunders, …)` currently destructures `for h, pair, wr, n in blunders:` → change to `for rec in blunders:` and use `rec.hash`, `rec.pair`, `rec.all_elo_winrate`, etc. The decoded block must now print TWO lines:

```python
def _decode_blunder_block(
    blunders, hash_to_canon: dict[int, dict],
) -> list[str]:
    out: list[str] = []
    for rec in blunders:
        out.extend(_decode_block_for_hash(rec.hash, hash_to_canon))
        out.append(
            f"Best human pair: **{_format_pair(rec.pair)}**"
        )
        out.append(
            f"- all-ELO winrate **{rec.all_elo_winrate*100:.1f}%** "
            f"(n_games **{rec.all_elo_n_games}**)"
        )
        if rec.high_elo_winrate is None:
            out.append(
                "- high-ELO winrate **—** (n_games **0**)"
            )
        else:
            out.append(
                f"- high-ELO winrate **{rec.high_elo_winrate*100:.1f}%** "
                f"(n_games **{rec.high_elo_n_games}**)"
            )
        out.append("")
    return out
```

- `scripts/openbook/report.py` — `_format_pair(...)` call site in the legacy table path (the `else` branch when `blunder_decoded` is empty). Change the destructuring there too:

```python
if blunder_decoded:
    L.extend(blunder_decoded)
else:
    L.append("| hash | best human pair | all-ELO wr | all-ELO n | "
             "high-ELO wr | high-ELO n |")
    L.append("|---|---|---|---|---|---|")
    for rec in blunder_candidates[:20]:
        wr_he = (
            "—" if rec.high_elo_winrate is None
            else f"{rec.high_elo_winrate*100:.1f}%"
        )
        L.append(
            f"| `0x{rec.hash:016x}` | {_format_pair(rec.pair)} | "
            f"{rec.all_elo_winrate*100:.1f}% | {rec.all_elo_n_games} | "
            f"{wr_he} | {rec.high_elo_n_games} |"
        )
```

In `write_report(...)`, also pick the section heading:

```python
any_high = any(
    b.high_elo_n_games > 0 for b in blunder_candidates[:20]
)
section_title = (
    "## Top blunders (cross-ELO)" if any_high
    else "## Top low-ELO traps"
)
L.append(section_title)
```

- `scripts/openbook/decode_hash.py` — currently does NOT show blunder info inline. Add an optional `--blunders` flag and a small printer. Simpler: scan the in-memory tree node already loaded and, if the heaviest pair matches the blunder criterion in BOTH bands, print:

Add at the very end of `decode(...)` (right before `return 0`), after the moves table:

```python
    # When this hash is a blunder candidate, show side-by-side ELO winrates
    # for the heaviest pair. Re-uses tree_json counts.
    moves = node.get("moves", {})
    if moves:
        # Pick the highest-total pair.
        def _row(key):
            counts = moves[key]
            high = int(counts.get("high", 0))
            low = int(counts.get("low", 0))
            return key, high, low
        rows = [_row(k) for k in moves]
        rows.sort(key=lambda r: -(r[1] + r[2]))
        key, hi, lo = rows[0]
        total = hi + lo
        all_wr = "—"
        he_wr = "—"
        # Tree carries wins per band when --emit-index is on (Phase A5
        # adds these fields). Until then we just print sample sizes.
        print()
        print("Heaviest pair sample sizes:")
        print(f"  all-ELO n_games: {total}")
        print(f"  high-ELO n_games: {hi}")
```

(We will revisit the actual winrate display in Phase A5 once the tree has the wins-per-band fields; for now just print sample sizes so the decoder no longer pretends to know the winrate without saying which band.)

- [ ] **Step 5: Run the tests**

```bash
.venv/bin/pytest scripts/openbook/tests/ -v
```

Expected: PASS — including the two new tests and the rewritten existing ones. **Don't move on** if other tests broke from the destructuring change.

- [ ] **Step 6: Commit**

```bash
git add scripts/openbook/blunders.py \
        scripts/openbook/main.py \
        scripts/openbook/report.py \
        scripts/openbook/decode_hash.py \
        scripts/openbook/tests/test_blunders.py
git commit -m "blunders: emit all-ELO and high-ELO bands together"
```

---

### Task A3: Singleton pruning flag for production book

**Files:**
- Modify: `scripts/openbook/io_book.py` — accept `min_n_games` arg
- Modify: `scripts/openbook/main.py` — read `prune_singletons` kwarg, pipe through, print stats
- Modify: `scripts/build_opening_book.py` — `--prune-singletons` flag, default ON
- Modify: `docs/specs/SPEC_OPENING_BOOK.md` — §5 note about prune semantics
- Modify: `scripts/openbook/tests/test_io_book.py` — new test
- Create: nothing
- Test: `scripts/openbook/tests/test_io_book.py::test_write_book_bin_respects_min_n_games`

- [ ] **Step 1: Write the failing test**

Append to `scripts/openbook/tests/test_io_book.py`:

```python
def test_write_book_bin_respects_min_n_games(tmp_path: Path):
    stats = {
        0x1: {
            ((0, 0), NULL_STONE): {  # singleton — must be pruned
                "n_games": 1,
                "weight": 1,
                "winrate": 0.5,
                "winrate_high_elo": 0.5,
            },
            ((1, 0), (0, 1)): {       # recurring — must survive
                "n_games": 5,
                "weight": 5,
                "winrate": 0.6,
                "winrate_high_elo": 0.6,
            },
        }
    }
    path = tmp_path / "b.bin"
    n = write_book_bin(stats, path, min_n_games=2)
    assert n == 1
    out = read_book_bin(path)
    assert len(out) == 1
    assert out[0]["s1"] == (1, 0)


def test_write_book_bin_keeps_singletons_when_no_threshold(tmp_path: Path):
    stats = {
        0x1: {((0, 0), NULL_STONE): {
            "n_games": 1, "weight": 1, "winrate": 0.5, "winrate_high_elo": 0.5,
        }}
    }
    path = tmp_path / "b.bin"
    n = write_book_bin(stats, path, min_n_games=1)
    assert n == 1
```

- [ ] **Step 2: Run the tests; expect failure**

```bash
.venv/bin/pytest scripts/openbook/tests/test_io_book.py::test_write_book_bin_respects_min_n_games -v
```

Expected: failure (`write_book_bin` takes no `min_n_games` kwarg yet).

- [ ] **Step 3: Update `write_book_bin`**

In `scripts/openbook/io_book.py`, change the signature and gate emission:

```python
def write_book_bin(
    stats: dict[int, dict[tuple[tuple[int, int], tuple[int, int]], dict[str, Any]]],
    path: Path,
    *,
    min_n_games: int = 1,
) -> int:
    records: list[tuple] = []
    n_total = 0
    n_pruned = 0
    for h, by_pair in stats.items():
        for (s1, s2), s in by_pair.items():
            n_total += 1
            if int(s["n_games"]) < min_n_games:
                n_pruned += 1
                continue
            weight = max(0, min(65535, int(s["weight"])))
            winrate = _encode_winrate(s.get("winrate_high_elo", s["winrate"]))
            n_games = max(0, min((1 << 32) - 1, int(s["n_games"])))
            engine = int(s.get("engine_score", 0))
            records.append((
                h,
                _clamp_i16(s1[0]), _clamp_i16(s1[1]),
                _clamp_i16(s2[0]), _clamp_i16(s2[1]),
                weight, winrate, n_games,
                _clamp_i16(engine),
            ))
    records.sort(key=lambda r: (r[0], -r[5], r[1], r[2], r[3], r[4]))
    with open(path, "wb") as fh:
        for rec in records:
            fh.write(struct.pack(RECORD_FORMAT, *rec))
    return len(records)
```

(No `n_pruned` return — main.py recomputes the pruned count by diffing length before/after; keeping the signature simple.)

- [ ] **Step 4: Run the test; expect pass**

```bash
.venv/bin/pytest scripts/openbook/tests/test_io_book.py::test_write_book_bin_respects_min_n_games -v
```

Expected: PASS.

- [ ] **Step 5: Wire the flag through `main.run`**

Modify `scripts/openbook/main.py`:

```python
def run(emit_index: bool = False, prune_singletons: bool = True) -> int:
    ...
    print("Writing outputs...", file=sys.stderr)
    min_n = 2 if prune_singletons else 1
    n_total_records = sum(len(by_pair) for by_pair in stats.values())
    n_written = write_book_bin(
        stats, OUT / "opening_book.bin", min_n_games=min_n,
    )
    n_pruned = n_total_records - n_written
    print(
        f"  book.bin: wrote {n_written}/{n_total_records} records "
        f"(pruned {n_pruned} below n_games={min_n})",
        file=sys.stderr,
    )
```

- [ ] **Step 6: Add the CLI flag**

Modify `scripts/build_opening_book.py`:

```python
if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--emit-index",
        action="store_true",
        help="emit data/analysis/position_index.json (large)",
    )
    ap.add_argument(
        "--prune-singletons",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="drop records with n_games=1 from book.bin (default ON; "
             "use --no-prune-singletons for the full dump). "
             "opening_tree.json is unaffected.",
    )
    args = ap.parse_args()
    sys.exit(run(
        emit_index=args.emit_index,
        prune_singletons=args.prune_singletons,
    ))
```

- [ ] **Step 7: Update the spec**

In `docs/specs/SPEC_OPENING_BOOK.md`, between `## 5 — Binary record layout (revised)` and `## 6 — Weighting`, add this paragraph:

```markdown
**Production vs. analysis artefacts.** `data/analysis/opening_book.bin`
is the *pruned* production artefact: records with `n_games < 2` are
dropped at write time (default; toggle with
`--no-prune-singletons` on the build CLI). The full enumeration —
including every singleton — is kept in `data/analysis/opening_tree.json`
so offline analysis can still walk the long tail. The pruned book is
what the Rust probe binary-searches; the full tree is for tooling only.
```

- [ ] **Step 8: Rebuild book once and sanity-check size**

```bash
cd /home/timmy/Work/hexo_minimax
.venv/bin/python scripts/build_opening_book.py
```

Expected: `book.bin: wrote ~8500/78541 records` line in stderr. The exact survivor count depends on the corpus; **anything between 6000 and 15000 is fine**. Anything outside that band means singleton accounting is wrong — stop and dig before continuing.

- [ ] **Step 9: Commit**

```bash
git add scripts/openbook/io_book.py \
        scripts/openbook/main.py \
        scripts/build_opening_book.py \
        scripts/openbook/tests/test_io_book.py \
        docs/specs/SPEC_OPENING_BOOK.md
git commit -m "book: prune singletons by default; full tree unchanged"
```

Do **not** stage `data/analysis/opening_book.bin` — `.gitignore` already excludes it.

---

### Task A4: Wide-opening separation

Per the task brief: a record is *wide* if **any** stone (either player) in the position-state has hex-distance from origin > 5, **and** the turn index is ≤ 6. Wide records get split off into `data/analysis/wide_openings.bin`. Same 26-byte format. The split happens **before** singleton pruning (so wide singletons are also dropped from `wide_openings.bin` when `--prune-singletons` is on; this matches the production-artefact intent).

**Files:**
- Modify: `scripts/openbook/canonical.py` — compute and surface `is_wide` per record
- Modify: `scripts/openbook/aggregator.py` — propagate `is_wide` per (hash, pair); since the same canonical hash can never appear in both wide and tight (the position-state is shared) the wide flag is a per-hash attribute, but we still store it on the pair-stat for ergonomic emission
- Modify: `scripts/openbook/main.py` — write a second bin
- Modify: `scripts/openbook/io_book.py` — accept an iterable of records (alternative to stats dict) or expose a split helper
- Modify: `scripts/openbook/report.py` — split table
- Modify: `scripts/openbook/tests/test_canonical.py` (new test)
- Modify: `scripts/openbook/tests/test_io_book.py` (new test)

- [ ] **Step 1: Helper for hex-distance from origin in `canonical.py`**

At the top of `scripts/openbook/canonical.py` (after the imports, before `CanonTurnRecord`):

```python
def _hex_dist_from_origin(c: Cell) -> int:
    dq, dr = c[0], c[1]
    return (abs(dq) + abs(dr) + abs(dq + dr)) // 2


WIDE_OPENING_TURN_MAX = 6
WIDE_OPENING_DIST_THRESHOLD = 5


def is_wide_position(stones, turn_index: int) -> bool:
    if turn_index > WIDE_OPENING_TURN_MAX:
        return False
    return any(
        _hex_dist_from_origin(c) > WIDE_OPENING_DIST_THRESHOLD
        for c, _ in stones
    )
```

- [ ] **Step 2: Add `is_wide` field to `CanonTurnRecord`**

```python
@dataclass(frozen=True)
class CanonTurnRecord:
    base: TurnRecord
    position_hash: int
    canonical_pair: tuple[Cell, Cell]
    canon_stones: tuple[StonePair, ...] = field(default_factory=tuple)
    is_high_elo: bool = False
    is_wide: bool = False
```

And in `canonical_turns(...)`, compute it after `canon_stones` is known:

```python
        wide = is_wide_position(canon_stones, rec.turn_index)
        yield CanonTurnRecord(
            base=rec,
            position_hash=h,
            canonical_pair=pair,
            canon_stones=canon_stones,
            is_high_elo=is_high,
            is_wide=wide,
        )
```

- [ ] **Step 3: Write the failing test**

Append to `scripts/openbook/tests/test_canonical.py`:

```python
def test_is_wide_position_true_when_stone_far_from_origin_and_turn_le_6():
    from openbook.canonical import is_wide_position
    stones = [((0, 0), 0), ((6, 0), 1)]   # second stone at hex-dist 6
    assert is_wide_position(stones, turn_index=2) is True


def test_is_wide_position_false_when_all_stones_close():
    from openbook.canonical import is_wide_position
    stones = [((0, 0), 0), ((1, 0), 1), ((-1, 0), 1)]
    assert is_wide_position(stones, turn_index=2) is False


def test_is_wide_position_false_after_turn_6():
    from openbook.canonical import is_wide_position
    stones = [((0, 0), 0), ((6, 0), 1)]
    assert is_wide_position(stones, turn_index=7) is False
```

- [ ] **Step 4: Run test; expect pass after Step 1 implementation**

```bash
.venv/bin/pytest scripts/openbook/tests/test_canonical.py::test_is_wide_position_true_when_stone_far_from_origin_and_turn_le_6 -v
```

Expected: PASS.

- [ ] **Step 5: Surface `is_wide` via aggregator**

`Aggregator.add(rec)` should remember which hashes were ever observed as wide. Add to the class:

```python
class Aggregator:
    def __init__(self) -> None:
        self._table: dict[int, dict[PairKey, _PairAccum]] = defaultdict(
            lambda: defaultdict(_PairAccum)
        )
        self._wide_hashes: set[int] = set()

    def add(self, rec: Any) -> None:
        ...
        if getattr(rec, "is_wide", False):
            self._wide_hashes.add(rec.position_hash)
        ...

    @property
    def wide_hashes(self) -> set[int]:
        return self._wide_hashes
```

(`is_wide` is a property of the **position**, not the pair, so a single set suffices.)

- [ ] **Step 6: Split stats into wide / tight in `main.run`**

After `stats = agg.finalize()`:

```python
    wide_hashes = agg.wide_hashes
    wide_stats = {h: stats[h] for h in stats if h in wide_hashes}
    tight_stats = {h: stats[h] for h in stats if h not in wide_hashes}
```

Use `tight_stats` for the production book write; write `wide_stats` to a second bin. Adjust the existing write block:

```python
    min_n = 2 if prune_singletons else 1
    n_total_tight = sum(len(by) for by in tight_stats.values())
    n_total_wide = sum(len(by) for by in wide_stats.values())
    n_tight = write_book_bin(
        tight_stats, OUT / "opening_book.bin", min_n_games=min_n,
    )
    n_wide = write_book_bin(
        wide_stats, OUT / "wide_openings.bin", min_n_games=min_n,
    )
    print(
        f"  book.bin (tight):    wrote {n_tight}/{n_total_tight} "
        f"(pruned {n_total_tight - n_tight})",
        file=sys.stderr,
    )
    print(
        f"  wide_openings.bin:   wrote {n_wide}/{n_total_wide} "
        f"(pruned {n_total_wide - n_wide})",
        file=sys.stderr,
    )
```

`n_written` from the original code is replaced by `n_tight + n_wide` if anything else references it.

- [ ] **Step 7: Surface wide/tight count in REPORT**

In `scripts/openbook/main.py`, pass wide/tight counts into the report. Modify the `write_report(...)` call:

```python
    write_report(
        path=OUT / "REPORT_BOOK.md",
        ...
        n_tight_records=n_tight,
        n_wide_records=n_wide,
        n_tight_positions=len(tight_stats),
        n_wide_positions=len(wide_stats),
        ...
    )
```

In `scripts/openbook/report.py::write_report(...)`, accept the new kwargs (default `None`) and emit:

```python
    if n_tight_records is not None and n_wide_records is not None:
        L.append("## Wide vs tight openings (turn ≤ 6, any stone > hex-dist 5)")
        L.append("")
        L.append("| split | positions | records |")
        L.append("|---|---|---|")
        L.append(f"| tight | {n_tight_positions} | {n_tight_records} |")
        L.append(f"| wide  | {n_wide_positions} | {n_wide_records} |")
        L.append("")
```

Insert this section right above the existing `## Coverage curve (turn-based)` header.

- [ ] **Step 8: io_book test for round-trip across both files**

Append to `scripts/openbook/tests/test_io_book.py`:

```python
def test_wide_and_tight_round_trip_independently(tmp_path: Path):
    """When the build emits two bins, each is a valid book on its own."""
    stats_tight = {0x1: {((0, 0), NULL_STONE): {
        "n_games": 5, "weight": 5, "winrate": 0.6, "winrate_high_elo": 0.6,
    }}}
    stats_wide = {0x2: {((6, 0), (0, 0)): {
        "n_games": 3, "weight": 3, "winrate": 0.4, "winrate_high_elo": 0.4,
    }}}
    write_book_bin(stats_tight, tmp_path / "tight.bin", min_n_games=1)
    write_book_bin(stats_wide,  tmp_path / "wide.bin",  min_n_games=1)
    t = read_book_bin(tmp_path / "tight.bin")
    w = read_book_bin(tmp_path / "wide.bin")
    assert len(t) == 1 and len(w) == 1
    assert t[0]["hash"] == 0x1
    assert w[0]["hash"] == 0x2
```

- [ ] **Step 9: Run full test suite**

```bash
.venv/bin/pytest scripts/openbook/tests/ -v
```

Expected: PASS. Sanity-check failures: if `test_io_book` ever fails because the writer expects a kwarg-only `min_n_games`, double-check the call sites in `main.py`.

- [ ] **Step 10: Rebuild book and inspect**

```bash
.venv/bin/python scripts/build_opening_book.py
ls -la data/analysis/wide_openings.bin
```

Expected: `wide_openings.bin` exists and is non-empty (the corpus has at least a handful of long-range openings).

- [ ] **Step 11: Commit**

```bash
git add scripts/openbook/canonical.py \
        scripts/openbook/aggregator.py \
        scripts/openbook/main.py \
        scripts/openbook/report.py \
        scripts/openbook/tests/test_canonical.py \
        scripts/openbook/tests/test_io_book.py
git commit -m "book: split wide openings into wide_openings.bin"
```

---

### Task A5: Position-conditional winrate

The tree already counts moves per position-band. Add three extra integers per position node:

- `pos_n_games_through` — number of game-walks that pass through this position
- `pos_x_eventual_wins` — of those, how many were eventually won by X
- `pos_o_eventual_wins` — likewise for O

Computed by adding *one* count per game-walk per visited position (already what the walker yields). Store on the tree node, write into `opening_tree.json`, and add a histogram to the report.

**Files:**
- Modify: `scripts/openbook/tree.py` — extra counters
- Modify: `scripts/openbook/canonical.py` — `winner_byte` on the canon record (already present via `mover_byte` + `mover_won` — derive winner instead)
- Modify: `scripts/openbook/io_book.py::write_tree_json` — emit new fields
- Modify: `scripts/openbook/report.py` — histogram
- Modify: `scripts/openbook/main.py` — pass through
- Modify: `scripts/openbook/tests/test_aggregator.py` — actually this lives on `tree`, so the test moves to `test_tree.py`
- Modify: `scripts/openbook/tests/test_tree.py` — add `test_position_conditional_winrate`

- [ ] **Step 1: Add fields to `_Node`**

In `scripts/openbook/tree.py`:

```python
@dataclass
class _Node:
    moves_high: dict[PairKey, int] = field(
        default_factory=lambda: defaultdict(int)
    )
    moves_low: dict[PairKey, int] = field(
        default_factory=lambda: defaultdict(int)
    )
    n_games_through: int = 0
    x_eventual_wins: int = 0
    o_eventual_wins: int = 0
```

And in `Tree`:

```python
    def observe_game(self, h: int, eventual_winner_byte: int | None) -> None:
        """Increment n_games_through and the appropriate winner counter."""
        n = self.nodes[h]
        n.n_games_through += 1
        if eventual_winner_byte == 0:
            n.x_eventual_wins += 1
        elif eventual_winner_byte == 1:
            n.o_eventual_wins += 1

    def x_winrate(self, h: int) -> float:
        n = self.nodes.get(h)
        if n is None or n.n_games_through == 0:
            return 0.0
        return n.x_eventual_wins / n.n_games_through
```

- [ ] **Step 2: Compute `eventual_winner_byte` on the canon record**

The walker already knows `mover_byte` and `mover_won`. Derive the eventual winner: if `mover_won`, winner = mover; else if a winner exists, winner = other byte; else `None` (draw / undecided).

Add to `CanonTurnRecord` (or compute on the fly in `build_tree`). Doing it inline avoids touching the dataclass:

In `scripts/openbook/tree.py::build_tree`:

```python
def build_tree(records: Iterable, max_depth: int) -> Tree:
    t = Tree()
    for r in records:
        if r.turn_index > max_depth:
            continue
        t.add(r.position_hash, r.canonical_pair, r.is_high_elo)
        # The eventual winner is unambiguous from mover_byte + mover_won.
        # mover_byte ∈ {0,1}; mover_won = bool. Draws / undecided games
        # have base.mover_won = False AND no explicit win condition; the
        # walker doesn't distinguish them, but for now treat anything
        # without mover_won as "other player wins" — when the corpus has
        # genuine draws we'll revisit.
        mover_byte = r.base.mover_byte
        winner_byte = mover_byte if r.base.mover_won else (1 - mover_byte)
        t.observe_game(r.position_hash, winner_byte)
    return t
```

**Caveat:** this overcounts draws as opponent-wins. The current corpus is human play and lacks an explicit draw byte; the walker treats no-winner as `mover_won=False`. For the histogram-bin distribution this is acceptable signal-vs-noise. Document it in the report section.

- [ ] **Step 3: Write the failing test**

Append to `scripts/openbook/tests/test_tree.py`:

```python
def test_position_conditional_winrate():
    """Tree records per-position eventual winner counts."""
    from openbook.tree import Tree
    t = Tree()
    # X plays the same pair from hash 0xAA twice — once wins, once loses.
    pair_x = ((0, 0), (1, 0))
    t.add(0xAA, pair_x, is_high=True)
    t.observe_game(0xAA, eventual_winner_byte=0)
    t.add(0xAA, pair_x, is_high=False)
    t.observe_game(0xAA, eventual_winner_byte=1)
    n = t.nodes[0xAA]
    assert n.n_games_through == 2
    assert n.x_eventual_wins == 1
    assert n.o_eventual_wins == 1
    assert t.x_winrate(0xAA) == 0.5
```

- [ ] **Step 4: Run; expect pass**

```bash
.venv/bin/pytest scripts/openbook/tests/test_tree.py::test_position_conditional_winrate -v
```

Expected: PASS.

- [ ] **Step 5: Emit new fields in `write_tree_json`**

In `scripts/openbook/io_book.py::write_tree_json`:

```python
def write_tree_json(tree, path: Path) -> None:
    doc: dict[str, Any] = {}
    for h, node in tree.nodes.items():
        moves_doc: dict[str, dict[str, int]] = {}
        all_pairs = set(node.moves_high) | set(node.moves_low)
        for pair in all_pairs:
            s1, s2 = pair
            key = f"{s1[0]},{s1[1]}|{s2[0]},{s2[1]}"
            moves_doc[key] = {
                "high": int(node.moves_high.get(pair, 0)),
                "low": int(node.moves_low.get(pair, 0)),
            }
        n_through = int(node.n_games_through)
        x_wins = int(node.x_eventual_wins)
        o_wins = int(node.o_eventual_wins)
        x_winrate = (x_wins / n_through) if n_through else 0.0
        doc[f"0x{h:016x}"] = {
            "branching": len(all_pairs),
            "moves": moves_doc,
            "pos_n_games_through": n_through,
            "pos_x_eventual_wins": x_wins,
            "pos_o_eventual_wins": o_wins,
            "pos_x_winrate": x_winrate,
        }
    Path(path).write_text(json.dumps(doc, indent=2, sort_keys=True))
```

- [ ] **Step 6: Histogram section in report**

In `scripts/openbook/main.py`, build the histogram before calling `write_report`:

```python
    pos_winrate_histogram = _build_pos_winrate_histogram(
        tree, min_through=10,
    )
```

Add `_build_pos_winrate_histogram` near the other underscore helpers in `main.py`:

```python
def _build_pos_winrate_histogram(tree, min_through: int) -> list[str]:
    bins = [0] * 10  # 0.0-0.1, 0.1-0.2, ..., 0.9-1.0
    sample = 0
    for h, node in tree.nodes.items():
        if node.n_games_through < min_through:
            continue
        wr = node.x_eventual_wins / node.n_games_through
        idx = min(9, int(wr * 10))
        bins[idx] += 1
        sample += 1
    if sample == 0:
        return ["_(no positions with n_games_through ≥ "
                f"{min_through})_"]
    lines = [
        "| pos_x_winrate bucket | # positions | bar |",
        "|---|---|---|",
    ]
    max_bar = max(bins)
    for i, count in enumerate(bins):
        lo = i / 10
        hi = (i + 1) / 10
        bar = "█" * round(40 * count / max_bar) if max_bar else ""
        lines.append(f"| {lo:.1f}–{hi:.1f} | {count} | `{bar}` |")
    lines.append("")
    lines.append(
        f"_n = {sample} positions with `pos_n_games_through ≥ "
        f"{min_through}`. Draws (rare in corpus) get credited to "
        f"the opponent of last mover._"
    )
    return lines
```

And pipe `pos_winrate_histogram` into `write_report`. In `scripts/openbook/report.py`, after the wide-vs-tight block, before the coverage section:

```python
    if pos_winrate_histogram:
        L.append("## Position-conditional winrate distribution")
        L.append("")
        L.extend(pos_winrate_histogram)
        L.append("")
```

- [ ] **Step 7: Run all tests + rebuild book**

```bash
.venv/bin/pytest scripts/openbook/tests/ -v
.venv/bin/python scripts/build_opening_book.py
```

Expected: green tests, report includes the histogram.

- [ ] **Step 8: Commit**

```bash
git add scripts/openbook/tree.py \
        scripts/openbook/io_book.py \
        scripts/openbook/main.py \
        scripts/openbook/report.py \
        scripts/openbook/tests/test_tree.py
git commit -m "tree: track position-conditional X winrate; histogram in report"
```

---

## Phase B — Engine deepening

### Task B1: Deepening-target selection

Generate a curated list of 200-300 hashes worth running the engine deeper on.

**Files:**
- Create: `scripts/openbook/targets.py`
- Create: `scripts/openbook/tests/test_targets.py`
- Modify: `scripts/openbook/main.py` — emit `data/analysis/deepening_targets.json` at end of run
- Modify: `scripts/build_opening_book.py` — already covered; no flag needed (targets always emitted)

- [ ] **Step 1: Write the failing test**

Create `scripts/openbook/tests/test_targets.py`:

```python
from openbook.targets import build_targets


def test_build_targets_dedupes_across_pools():
    """If the same hash appears in TOP_TRAFFIC and BLUNDERS, only one
    entry comes out — and it carries both source labels."""
    # Fake stats: hash 0x1 has 50 games (TOP_TRAFFIC) and is the heaviest
    # pair, so blunder_candidates picks it as a low-winrate trap too.
    # Fake tree: hash 0x1 has KL above the 0.3 cutoff.
    targets = build_targets(
        top_traffic_hashes=[0x1, 0x2, 0x3],
        blunder_hashes=[0x1, 0x4],
        kl_junction_hashes=[0x1, 0x5],
        position_data={
            h: {"stones": [], "side_to_move": 0, "turn_index": 2}
            for h in {0x1, 0x2, 0x3, 0x4, 0x5}
        },
    )
    assert {t["hash"] for t in targets} == {0x1, 0x2, 0x3, 0x4, 0x5}
    h1 = next(t for t in targets if t["hash"] == 0x1)
    assert set(h1["sources"]) == {"TOP_TRAFFIC", "BLUNDERS", "KL_JUNCTIONS"}


def test_build_targets_orders_by_hash():
    """Determinism: sort by hash so the JSON output is stable across
    runs of the build."""
    targets = build_targets(
        top_traffic_hashes=[0x3, 0x1, 0x2],
        blunder_hashes=[],
        kl_junction_hashes=[],
        position_data={
            h: {"stones": [], "side_to_move": 0, "turn_index": 2}
            for h in {0x1, 0x2, 0x3}
        },
    )
    assert [t["hash"] for t in targets] == [0x1, 0x2, 0x3]
```

- [ ] **Step 2: Run; expect failure**

```bash
.venv/bin/pytest scripts/openbook/tests/test_targets.py -v
```

Expected: `ModuleNotFoundError: openbook.targets`.

- [ ] **Step 3: Create `scripts/openbook/targets.py`**

```python
"""Deepening target selection.

Three input pools — TOP_TRAFFIC, BLUNDERS, KL_JUNCTIONS — are deduplicated
into a single ordered list of positions. Each entry carries the union of
the pool labels that named it.
"""
from __future__ import annotations

from typing import Iterable


def build_targets(
    *,
    top_traffic_hashes: Iterable[int],
    blunder_hashes: Iterable[int],
    kl_junction_hashes: Iterable[int],
    position_data: dict[int, dict],
) -> list[dict]:
    sources: dict[int, set[str]] = {}
    for h in top_traffic_hashes:
        sources.setdefault(h, set()).add("TOP_TRAFFIC")
    for h in blunder_hashes:
        sources.setdefault(h, set()).add("BLUNDERS")
    for h in kl_junction_hashes:
        sources.setdefault(h, set()).add("KL_JUNCTIONS")
    out: list[dict] = []
    for h in sorted(sources):
        pd = position_data.get(h)
        if pd is None:
            continue
        out.append({
            "hash": h,
            "sources": sorted(sources[h]),
            "stones": pd["stones"],
            "side_to_move": int(pd["side_to_move"]),
            "turn_index": int(pd["turn_index"]),
        })
    return out
```

- [ ] **Step 4: Run; expect pass**

```bash
.venv/bin/pytest scripts/openbook/tests/test_targets.py -v
```

Expected: PASS.

- [ ] **Step 5: Wire into `main.run`**

Right before the existing `if emit_index:` block in `main.py`, compute and write targets:

```python
    print("Selecting deepening targets...", file=sys.stderr)
    top_traffic = sorted(
        stats,
        key=lambda h: -sum(s["n_games"] for s in stats[h].values()),
    )[:200]
    blunder_hashes = [b.hash for b in blunders]
    kl_min = 0.3
    kl_hashes = [h for h, kl in junctions_full if kl >= kl_min]
    targets = build_targets(
        top_traffic_hashes=top_traffic,
        blunder_hashes=blunder_hashes,
        kl_junction_hashes=kl_hashes,
        position_data=hash_to_canon,
    )
    targets_doc = {
        "schema_version": 1,
        "n_targets": len(targets),
        "targets": [
            {
                "hash": f"0x{t['hash']:016x}",
                "sources": t["sources"],
                "stones": t["stones"],
                "side_to_move": t["side_to_move"],
                "turn_index": t["turn_index"],
            }
            for t in targets
        ],
    }
    (OUT / "deepening_targets.json").write_text(
        json.dumps(targets_doc, indent=2, sort_keys=True)
    )
    print(f"  wrote {len(targets)} targets to deepening_targets.json",
          file=sys.stderr)
```

There is a subtlety: `junctions = theory_junctions(tree, top_n=20)` is currently capped at 20 in `main.py`. We need the full list for KL ≥ 0.3 filtering. Add a second uncapped call:

```python
    junctions = theory_junctions(tree, top_n=20)              # for report
    junctions_full = theory_junctions(tree, top_n=10**9)      # for targets
```

And import:

```python
from openbook.targets import build_targets
```

- [ ] **Step 6: Rebuild + inspect**

```bash
.venv/bin/python scripts/build_opening_book.py
.venv/bin/python -c "import json; d = json.load(open('data/analysis/deepening_targets.json')); print(d['n_targets']); print(d['targets'][0])"
```

Expected: roughly 200-300 targets, first entry has hash, sources, stones, side_to_move, turn_index.

- [ ] **Step 7: Commit**

```bash
git add scripts/openbook/targets.py \
        scripts/openbook/main.py \
        scripts/openbook/tests/test_targets.py
git commit -m "targets: emit deepening_targets.json (top traffic + blunders + KL)"
```

---

### Task B2: Engine deepening runner

`scripts/openbook/deepen.py` loads each target into a fresh `Bot`, walks it to the position by replaying moves in turn order, searches at `production_depth + depth_bonus`, saves the best pair + score + nodes + time.

Design note: replaying moves from origin works for early-game positions (turn ≤ 6). For each target the position state is `stones` from canonical frame; we reconstruct a legal play order by alternating turns (1 X, 2 O, 2 X, ...). Within a turn, stone order is irrelevant for engine state. We assert no win occurs en route (very rare for turn ≤ 6 positions); if a win is detected, log and skip.

**Files:**
- Create: `scripts/openbook/deepen.py`
- Create: `scripts/openbook/tests/test_deepen.py`

- [ ] **Step 1: Write the failing tests**

Create `scripts/openbook/tests/test_deepen.py`:

```python
import pytest

from openbook.deepen import (
    reconstruct_play_order,
    DeepeningError,
)


def test_reconstruct_play_order_turn_1_only():
    stones = [((0, 0), 0)]
    order = reconstruct_play_order(stones, side_to_move=1, turn_index=2)
    assert order == [((0, 0), 0)]


def test_reconstruct_play_order_after_turn_2():
    # Turn 1: X at (0,0). Turn 2: O plays 2 stones. side_to_move now = X.
    stones = [((0, 0), 0), ((1, 0), 1), ((0, 1), 1)]
    order = reconstruct_play_order(stones, side_to_move=0, turn_index=3)
    # X first, then both O stones in lex order.
    assert order[0] == ((0, 0), 0)
    assert set(order[1:]) == {((1, 0), 1), ((0, 1), 1)}


def test_reconstruct_play_order_after_turn_3():
    # X has 3 stones total (1 from turn 1 + 2 from turn 3); O has 2.
    stones = [
        ((0, 0), 0), ((2, 0), 0), ((0, 2), 0),
        ((1, 0), 1), ((0, 1), 1),
    ]
    order = reconstruct_play_order(stones, side_to_move=1, turn_index=4)
    assert order[0] == ((0, 0), 0)
    assert {s[1] for s in order[1:3]} == {1}    # O's 2 stones (turn 2)
    assert {s[1] for s in order[3:5]} == {0}    # X's other 2 stones (turn 3)


def test_reconstruct_rejects_inconsistent_counts():
    # Turn 4 should have X=3 (1 + 2), O=2. Give X=4 and it must fail.
    stones = [((q, 0), 0) for q in range(4)] + [((0, 1), 1)]
    with pytest.raises(DeepeningError):
        reconstruct_play_order(
            stones, side_to_move=1, turn_index=4,
        )
```

- [ ] **Step 2: Run; expect failure**

```bash
.venv/bin/pytest scripts/openbook/tests/test_deepen.py -v
```

Expected: `ModuleNotFoundError`.

- [ ] **Step 3: Implement `scripts/openbook/deepen.py`**

```python
#!/usr/bin/env python3
"""Run engine deeper than production on a curated target set.

CLI:
  python scripts/openbook/deepen.py \
      --targets data/analysis/deepening_targets.json \
      --depth-bonus 4 \
      --time-per-position 10000 \
      --output data/analysis/engine_eval.json

For each target, reconstruct a legal play order from the canonical
stones, walk a fresh Bot to that state, then search deeper than
production. Capture best pair, engine_score, depth_reached, nodes, time.
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path
from typing import Iterable

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "hexo"))   # ensure hexo package importable


class DeepeningError(RuntimeError):
    pass


Cell = tuple[int, int]
StonePair = tuple[Cell, int]


def reconstruct_play_order(
    stones: Iterable[StonePair],
    side_to_move: int,
    turn_index: int,
) -> list[StonePair]:
    """Return stones in a legal turn-by-turn play order.

    HeXO turn structure: turn 1 = 1 X stone at origin; turn N≥2 = 2
    stones for the side-to-move-of-turn-N. We don't know within-turn
    order from canonical state — pick any (within-turn order does not
    affect post-turn engine state).
    """
    stones = list(stones)
    x_stones = sorted(c for c, p in stones if p == 0)
    o_stones = sorted(c for c, p in stones if p == 1)
    # Expected stone counts derived from (turn_index, side_to_move).
    # After turn T, X has stones-after = 1 + 2*((T-1)//2) when T is odd
    # (i.e. X just moved), 1 + 2*(T//2 - 1) = T - 1 when T is even, etc.
    # Simpler: simulate forward to turn_index-1 (the post-turn count
    # going INTO turn turn_index).
    expected_x = 1 if turn_index >= 2 else 0
    expected_o = 0
    cur_side = 0  # X moves first
    if turn_index == 1:
        expected_x = 0
    for t in range(2, turn_index):
        if t == 2:
            cur_side = 1
        if cur_side == 0:
            expected_x += 2
        else:
            expected_o += 2
        cur_side = 1 - cur_side
    if turn_index == 1:
        expected_x = 0
    elif turn_index == 2:
        expected_x = 1
    if len(x_stones) != expected_x or len(o_stones) != expected_o:
        raise DeepeningError(
            f"stone counts mismatch: have X={len(x_stones)} O={len(o_stones)}, "
            f"expected X={expected_x} O={expected_o} at turn={turn_index}"
        )
    # Walk turn-by-turn, draining the lex-sorted pools.
    order: list[StonePair] = []
    if expected_x >= 1:
        # Turn 1: X plays origin if present, else the lex-smallest X.
        if (0, 0) in x_stones:
            order.append(((0, 0), 0))
            x_stones.remove((0, 0))
        else:
            order.append((x_stones.pop(0), 0))
    side = 1
    for _ in range(2, turn_index):
        pool = o_stones if side == 1 else x_stones
        if len(pool) < 2:
            raise DeepeningError(
                f"not enough side-{side} stones to fill turn"
            )
        order.append((pool.pop(0), side))
        order.append((pool.pop(0), side))
        side = 1 - side
    return order


def run_one(target: dict, depth: int, time_ms: int):
    from hexo.bot import Bot, BotConfig

    h = int(target["hash"], 16) if isinstance(target["hash"], str) \
        else int(target["hash"])
    stones = [
        ((int(c[0]), int(c[1])), int(p))
        for c, p in target["stones"]
    ]
    side_to_move = int(target["side_to_move"])
    turn_index = int(target["turn_index"])
    play_order = reconstruct_play_order(
        stones, side_to_move, turn_index,
    )
    bot = Bot(BotConfig(time_per_move_ms=time_ms, max_depth=depth))
    for (q, r), _player in play_order:
        bot.engine.place((int(q), int(r)))
    # Sanity check.
    if bot.engine.to_move() != side_to_move:
        raise DeepeningError(
            f"engine to_move={bot.engine.to_move()} but target wants "
            f"{side_to_move} at hash 0x{h:016x}"
        )
    if bot.engine.winner() is not None:
        raise DeepeningError(
            f"position already terminal at hash 0x{h:016x}; skip"
        )

    t0 = time.monotonic()
    q1, r1, score, depth_reached, nodes, _t_ms = \
        bot.engine.bench_best_move(time_ms=time_ms, depth=depth)
    # Place s1 and search again for s2 if halfmove == 1.
    bot.engine.place((q1, r1))
    s2: tuple[int, int] | None = None
    if bot.engine.halfmove() == 1 and bot.engine.winner() is None:
        q2, r2, _s2_score, _d2, n2, _t2 = bot.engine.bench_best_move(
            time_ms=time_ms, depth=depth,
        )
        s2 = (q2, r2)
        nodes += n2
    total_ms = int((time.monotonic() - t0) * 1000)
    return {
        "best_pair": [[int(q1), int(r1)],
                      [int(s2[0]) if s2 else -32768,
                       int(s2[1]) if s2 else -32768]],
        "engine_score": int(score),
        "depth_reached": int(depth_reached),
        "nodes": int(nodes),
        "time_ms": int(total_ms),
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--targets", required=True, type=Path)
    ap.add_argument("--depth-bonus", type=int, default=4)
    ap.add_argument("--time-per-position", type=int, default=10000)
    ap.add_argument("--output", required=True, type=Path)
    ap.add_argument("--production-depth", type=int, default=10,
                    help="depth the production engine uses (informational; "
                         "actual search depth = this + depth_bonus)")
    ap.add_argument("--limit", type=int, default=None,
                    help="cap number of targets (for smoke testing)")
    args = ap.parse_args(argv)
    doc = json.loads(args.targets.read_text())
    targets = doc["targets"]
    if args.limit is not None:
        targets = targets[: args.limit]
    depth = args.production_depth + args.depth_bonus
    results: dict[str, dict] = {}
    skipped: list[dict] = []
    for i, t in enumerate(targets):
        if i % 10 == 0:
            print(
                f"[{i}/{len(targets)}] hash={t['hash']} "
                f"sources={','.join(t['sources'])}",
                file=sys.stderr,
            )
        try:
            r = run_one(t, depth=depth, time_ms=args.time_per_position)
            results[t["hash"]] = r
        except DeepeningError as e:
            skipped.append({"hash": t["hash"], "reason": str(e)})
            print(
                f"  skipping {t['hash']}: {e}", file=sys.stderr,
            )
    out_doc = {
        "schema_version": 1,
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "production_depth": args.production_depth,
        "deepening_depth": depth,
        "n_targets": len(targets),
        "n_evaluated": len(results),
        "n_skipped": len(skipped),
        "skipped": skipped,
        "results": results,
    }
    args.output.write_text(json.dumps(out_doc, indent=2, sort_keys=True))
    print(
        f"wrote {len(results)} / {len(targets)} results to {args.output}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Run tests**

```bash
.venv/bin/pytest scripts/openbook/tests/test_deepen.py -v
```

Expected: PASS for the four unit tests of `reconstruct_play_order`.

- [ ] **Step 5: Smoke-run deepen on first 5 targets**

```bash
cd /home/timmy/Work/hexo_minimax
.venv/bin/python scripts/openbook/deepen.py \
    --targets data/analysis/deepening_targets.json \
    --depth-bonus 4 \
    --time-per-position 2000 \
    --limit 5 \
    --output /tmp/engine_eval_smoke.json
.venv/bin/python -c "import json; d=json.load(open('/tmp/engine_eval_smoke.json')); print('evaluated:', d['n_evaluated'], 'skipped:', d['n_skipped'])"
```

Expected: ≥3 of 5 evaluated. If `n_skipped == 5`, something is wrong with `reconstruct_play_order` for real corpus data — debug before continuing.

- [ ] **Step 6: Full deepening sweep**

Allot ~40 minutes wall-clock. Use `time` to gauge:

```bash
time .venv/bin/python scripts/openbook/deepen.py \
    --targets data/analysis/deepening_targets.json \
    --depth-bonus 4 \
    --time-per-position 10000 \
    --output data/analysis/engine_eval.json
```

Expected on completion: `engine_eval.json` exists, `n_evaluated ≥ 150` (per the acceptance criterion). If fewer than 150, dig into the skip reasons in the JSON.

- [ ] **Step 7: Commit code (NOT the JSON — that's a data artefact, gitignored)**

```bash
git add scripts/openbook/deepen.py \
        scripts/openbook/tests/test_deepen.py
git commit -m "deepen: engine sweep at production_depth + 4 on targets"
```

If anything in `hexo.bot` turns out to be insufficient (e.g. no way to set a position directly), document it at `docs/superpowers/plans/2026-05-19-deepening-todo.md` with the exact missing piece — do NOT modify Rust code in this round.

---

### Task B3: Engine vs human disagreement report

**Files:**
- Modify: `scripts/openbook/main.py` — load `engine_eval.json` if present and pass through
- Modify: `scripts/openbook/report.py` — new section
- Modify: `scripts/openbook/tests/test_report.py` — assertion test on the section

- [ ] **Step 1: Write the test**

Append to `scripts/openbook/tests/test_report.py`:

```python
def test_disagreement_report_flags_disagree_and_improve(tmp_path):
    from openbook.report import build_disagreement_block

    # human best pair = (0,1)+(1,0) with high-ELO winrate 0.6
    # engine best pair = (1,1)+(0,0) with engine_score = -50
    #   → DISAGREE: human wins but engine says losing
    # engine recommends (2,0)+... not in human play → IMPROVE
    human_data = {
        0x1: {
            "best_pair": ((0, 1), (1, 0)),
            "high_elo_winrate": 0.6,
            "high_elo_n_games": 12,
        },
    }
    engine_data = {
        0x1: {
            "best_pair": [[1, 1], [0, 0]],
            "engine_score": -50,
            "depth_reached": 14,
        },
    }
    hash_to_canon = {0x1: {"stones": [], "side_to_move": 0, "turn_index": 3}}
    block = build_disagreement_block(
        human_data=human_data,
        engine_data=engine_data,
        hash_to_canon=hash_to_canon,
    )
    text = "\n".join(block)
    assert "DISAGREE" in text
    assert "0x0000000000000001" in text
```

- [ ] **Step 2: Run; expect failure**

```bash
.venv/bin/pytest scripts/openbook/tests/test_report.py::test_disagreement_report_flags_disagree_and_improve -v
```

Expected: `ImportError` on `build_disagreement_block`.

- [ ] **Step 3: Implement `build_disagreement_block` in `report.py`**

```python
def build_disagreement_block(
    *,
    human_data: dict[int, dict],
    engine_data: dict[int, dict],
    hash_to_canon: dict[int, dict],
) -> list[str]:
    lines: list[str] = [
        "## Engine vs human disagreement",
        "",
        "DISAGREE = human's heaviest pair wins ≥50% of high-ELO games "
        "but the engine evaluates that pair as losing (engine_score < 0).",
        "",
        "IMPROVE = engine recommends a pair humans have never played in "
        "the (n_games ≥ 2) tier.",
        "",
        "| hash | turn | flag | human pair | h.high_wr | engine pair | engine_score |",
        "|---|---|---|---|---|---|---|",
    ]
    rows: list[tuple] = []
    for h, h_info in human_data.items():
        e_info = engine_data.get(h)
        if e_info is None:
            continue
        h_pair = h_info["best_pair"]
        e_pair_raw = e_info["best_pair"]
        e_pair = (
            tuple(e_pair_raw[0]),
            tuple(e_pair_raw[1]),
        )
        hwr = h_info.get("high_elo_winrate")
        es = int(e_info["engine_score"])
        flag = None
        if (
            tuple(h_pair[0]) != e_pair[0] or tuple(h_pair[1]) != e_pair[1]
        ):
            if hwr is not None and hwr > 0.5 and es < 0:
                flag = "DISAGREE"
        if flag is None and h_info.get("engine_thinks_engine_pair_unseen"):
            flag = "IMPROVE"
        if flag is None:
            # Not interesting — skip the row.
            continue
        turn = hash_to_canon.get(h, {}).get("turn_index", "?")
        rows.append((
            abs(es), h, turn, flag, h_pair, hwr, e_pair, es,
        ))
    # Sort by disagreement strength (|engine_score| desc).
    rows.sort(key=lambda r: -r[0])
    for _, h, turn, flag, h_pair, hwr, e_pair, es in rows[:30]:
        wr_str = (
            "—" if hwr is None else f"{hwr*100:.1f}%"
        )
        lines.append(
            f"| `0x{h:016x}` | {turn} | **{flag}** | "
            f"{_format_pair(h_pair)} | {wr_str} | "
            f"{_format_pair(e_pair)} | {es:+d} |"
        )
    lines.append("")
    return lines
```

- [ ] **Step 4: Run test; expect pass**

```bash
.venv/bin/pytest scripts/openbook/tests/test_report.py::test_disagreement_report_flags_disagree_and_improve -v
```

- [ ] **Step 5: Wire into `main.run`**

In `scripts/openbook/main.py`, after the targets-write block, before `write_report(...)`:

```python
    engine_eval_path = OUT / "engine_eval.json"
    engine_data: dict[int, dict] = {}
    if engine_eval_path.exists():
        ed = json.loads(engine_eval_path.read_text())
        for k, v in ed.get("results", {}).items():
            engine_data[int(k, 16)] = v

    human_best: dict[int, dict] = {}
    for h, by_pair in stats.items():
        if not by_pair:
            continue
        best_pair, best_s = max(
            by_pair.items(), key=lambda kv: kv[1]["weight"],
        )
        hwr = (
            best_s["winrate_high_elo"]
            if best_s.get("n_high_elo_games", 0) > 0 else None
        )
        human_best[h] = {
            "best_pair": best_pair,
            "high_elo_winrate": hwr,
            "high_elo_n_games": best_s.get("n_high_elo_games", 0),
        }

    # IMPROVE detection: engine's best pair never played in human stats.
    for h, ed in engine_data.items():
        if h not in stats:
            continue
        played_pairs = set(stats[h].keys())
        ep = ed["best_pair"]
        e_pair = (tuple(ep[0]), tuple(ep[1]))
        if e_pair not in played_pairs and int(ed["engine_score"]) > 0:
            human_best.setdefault(h, {"best_pair": next(iter(played_pairs)),
                                      "high_elo_winrate": None,
                                      "high_elo_n_games": 0,
                                      })["engine_thinks_engine_pair_unseen"] = True

    disagreement_block = build_disagreement_block(
        human_data=human_best,
        engine_data=engine_data,
        hash_to_canon=hash_to_canon,
    ) if engine_data else None
```

Pass to `write_report(...)`:

```python
        disagreement_block=disagreement_block,
```

And in `write_report`, accept `disagreement_block: list[str] | None = None` and emit it right after the blunder section:

```python
    if disagreement_block:
        L.append("")
        L.extend(disagreement_block)
```

- [ ] **Step 6: Rebuild and inspect**

```bash
.venv/bin/python scripts/build_opening_book.py
grep -A 2 'Engine vs human' data/analysis/REPORT_BOOK.md | head -20
```

Expected: at least one `**DISAGREE**` row in the report. Acceptance criterion from the brief: "At least one DISAGREE flag triggered (otherwise something is fishy with engine or with humans)". If zero rows, audit `engine_score` signs vs. side-to-move conventions.

- [ ] **Step 7: Commit**

```bash
git add scripts/openbook/main.py \
        scripts/openbook/report.py \
        scripts/openbook/tests/test_report.py
git commit -m "report: engine vs human disagreement section"
```

---

## Phase C — Tier system

### Task C1: Composite tier classifier

**Files:**
- Create: `scripts/openbook/tier.py`
- Create: `scripts/openbook/tier_config.py`
- Create: `scripts/openbook/tests/test_tier.py`

- [ ] **Step 1: Write the failing tests**

Create `scripts/openbook/tests/test_tier.py`:

```python
from openbook.tier import classify, Tier


def test_classify_tier_1_safe():
    rec = {
        "n_games": 30,
        "winrate_high_elo": 0.62,
        "n_high_elo_games": 10,
        "pair": ((0, 1), (1, 0)),
    }
    eng = {
        "engine_score": 40,
        "best_pair": [[0, 1], [1, 0]],
    }
    assert classify(rec, eng) == Tier.TIER_1_SAFE


def test_classify_tier_2_expert():
    rec = {
        "n_games": 3,
        "winrate_high_elo": 0.65,
        "n_high_elo_games": 3,
        "pair": ((0, 1), (1, 0)),
    }
    eng = {
        "engine_score": 30,
        "best_pair": [[0, 1], [1, 0]],
    }
    assert classify(rec, eng) == Tier.TIER_2_EXPERT


def test_classify_tier_3_engine_only():
    rec = {
        "n_games": 0,
        "winrate_high_elo": 0.0,
        "n_high_elo_games": 0,
        "pair": ((2, 0), (0, 2)),
    }
    eng = {
        "engine_score": 80,
        "best_pair": [[2, 0], [0, 2]],
    }
    assert classify(rec, eng) == Tier.TIER_3_ENGINE_ONLY


def test_classify_tier_4_trap():
    rec = {
        "n_games": 15,
        "winrate_high_elo": 0.25,
        "n_high_elo_games": 10,
        "pair": ((-1, 0), (0, -1)),
    }
    eng = None
    assert classify(rec, eng) == Tier.TIER_4_TRAP


def test_classify_tier_drop_for_uninformative():
    rec = {
        "n_games": 3,
        "winrate_high_elo": 0.5,
        "n_high_elo_games": 1,
        "pair": ((0, 1), (1, 0)),
    }
    eng = None
    assert classify(rec, eng) == Tier.TIER_DROP
```

- [ ] **Step 2: Run; expect failure**

```bash
.venv/bin/pytest scripts/openbook/tests/test_tier.py -v
```

Expected: `ImportError`.

- [ ] **Step 3: Create `scripts/openbook/tier_config.py`**

```python
"""Tunable thresholds for tier classification.

Edit here, not in tier.py — keep all knobs in one place.
"""

TIER_1_MIN_N_GAMES = 5
TIER_1_MIN_HIGH_ELO_WR = 0.55
TIER_1_MIN_ENGINE_SCORE = 0      # engine_score > 0

TIER_2_MIN_HIGH_ELO_WR = 0.60
# Tier 2 has no n_games floor; expert-validated by winrate alone.

TIER_3_MIN_ENGINE_SCORE = 0      # engine_score > 0
# Tier 3 requires engine best_pair == rec.pair AND n_games == 0.

TIER_4_MIN_N_GAMES = 10
TIER_4_MAX_HIGH_ELO_WR = 0.30
```

- [ ] **Step 4: Create `scripts/openbook/tier.py`**

```python
"""Tier classification — composite of human winrate, engine eval, sample size."""
from __future__ import annotations

from enum import IntEnum

from openbook import tier_config as TC


class Tier(IntEnum):
    TIER_DROP = 0
    TIER_1_SAFE = 1
    TIER_2_EXPERT = 2
    TIER_3_ENGINE_ONLY = 3
    TIER_4_TRAP = 4


def _engine_agrees(rec: dict, eng: dict | None) -> bool:
    """Engine and humans agree on direction (both winning or both losing)."""
    if eng is None:
        return False
    n_he = rec.get("n_high_elo_games", 0)
    if n_he == 0:
        return False
    hw = rec["winrate_high_elo"]
    es = int(eng["engine_score"])
    return (hw > 0.5) == (es > 0)


def _engine_best_matches(rec: dict, eng: dict | None) -> bool:
    if eng is None:
        return False
    bp = eng["best_pair"]
    e_pair = (tuple(bp[0]), tuple(bp[1]))
    return tuple(rec["pair"]) == e_pair


def classify(rec: dict, eng: dict | None) -> Tier:
    n = int(rec["n_games"])
    n_he = int(rec.get("n_high_elo_games", 0))
    hw = float(rec["winrate_high_elo"]) if n_he > 0 else None
    es = (
        int(eng["engine_score"]) if eng is not None else None
    )
    agree = _engine_agrees(rec, eng)

    # TIER 1: highest confidence — humans win, engine agrees, sample large.
    if (
        n >= TC.TIER_1_MIN_N_GAMES and hw is not None
        and hw > TC.TIER_1_MIN_HIGH_ELO_WR
        and es is not None and es > TC.TIER_1_MIN_ENGINE_SCORE
        and agree
    ):
        return Tier.TIER_1_SAFE

    # TIER 2: expert-only — high-ELO winrate strong, engine agrees.
    if (
        hw is not None and hw > TC.TIER_2_MIN_HIGH_ELO_WR
        and agree
    ):
        return Tier.TIER_2_EXPERT

    # TIER 3: engine sees a winning line humans never tried.
    if (
        es is not None and es > TC.TIER_3_MIN_ENGINE_SCORE
        and n == 0 and _engine_best_matches(rec, eng)
    ):
        return Tier.TIER_3_ENGINE_ONLY

    # TIER 4: human trap — heavily played, demonstrably losing.
    if (
        n >= TC.TIER_4_MIN_N_GAMES and hw is not None
        and hw < TC.TIER_4_MAX_HIGH_ELO_WR
    ):
        return Tier.TIER_4_TRAP

    return Tier.TIER_DROP
```

- [ ] **Step 5: Run tests; expect pass**

```bash
.venv/bin/pytest scripts/openbook/tests/test_tier.py -v
```

Expected: PASS for all 5 cases.

- [ ] **Step 6: Commit**

```bash
git add scripts/openbook/tier.py \
        scripts/openbook/tier_config.py \
        scripts/openbook/tests/test_tier.py
git commit -m "tier: composite classifier (SAFE / EXPERT / ENGINE / TRAP)"
```

---

### Task C2: 28-byte record format with tier + flags

**Files:**
- Modify: `scripts/openbook/io_book.py` — new `RECORD_FORMAT`, `RECORD_SIZE`, encoder, reader, accept `tier_data` arg
- Modify: `scripts/openbook/tests/test_io_book.py` — 28-byte tests + flag tests
- Modify: `docs/specs/SPEC_OPENING_BOOK.md` — §5 layout + flag bit description, v2.1 note

- [ ] **Step 1: Write the failing tests**

Append to `scripts/openbook/tests/test_io_book.py`:

```python
def test_record_format_is_28_bytes():
    """Phase C: tier + flags bring record from 26 → 28 bytes."""
    assert RECORD_SIZE == 28
    assert struct.calcsize(RECORD_FORMAT) == 28


def test_round_trip_carries_tier_and_flags(tmp_path: Path):
    stats = {
        0xA1: {((0, 0), NULL_STONE): {
            "n_games": 10,
            "weight": 13,
            "winrate": 0.6,
            "winrate_high_elo": 0.65,
            "engine_score": 42,
            "tier": 1,
            "flags": 0b0000_0011,   # engine_agrees + wide
        }}
    }
    write_book_bin(stats, tmp_path / "b.bin", min_n_games=1)
    out = read_book_bin(tmp_path / "b.bin")
    assert len(out) == 1
    assert out[0]["tier"] == 1
    assert out[0]["flags"] == 0b0000_0011
    assert out[0]["engine_score"] == 42


def test_default_tier_and_flags_are_zero(tmp_path: Path):
    """If caller doesn't set tier/flags, writer defaults them to 0 so the
    pre-Phase-C call sites keep working unchanged."""
    stats = {
        0x1: {((0, 0), NULL_STONE): {
            "n_games": 5, "weight": 5, "winrate": 0.5,
            "winrate_high_elo": 0.5,
        }}
    }
    write_book_bin(stats, tmp_path / "b.bin", min_n_games=1)
    out = read_book_bin(tmp_path / "b.bin")
    assert out[0]["tier"] == 0
    assert out[0]["flags"] == 0
```

- [ ] **Step 2: Run; expect failure**

```bash
.venv/bin/pytest scripts/openbook/tests/test_io_book.py::test_record_format_is_28_bytes -v
```

- [ ] **Step 3: Update `io_book.py`**

```python
RECORD_FORMAT = "<QhhhhHHIhBB"
RECORD_SIZE = struct.calcsize(RECORD_FORMAT)
assert RECORD_SIZE == 28, f"expected 28-byte record, got {RECORD_SIZE}"

FLAG_ENGINE_AGREES   = 1 << 0
FLAG_WIDE_OPENING    = 1 << 1
FLAG_KL_JUNCTION     = 1 << 2
FLAG_BLUNDER_CANDIDATE = 1 << 3
```

Update `write_book_bin`:

```python
def write_book_bin(
    stats,
    path,
    *,
    min_n_games: int = 1,
) -> int:
    records: list[tuple] = []
    for h, by_pair in stats.items():
        for (s1, s2), s in by_pair.items():
            if int(s["n_games"]) < min_n_games:
                continue
            weight = max(0, min(65535, int(s["weight"])))
            winrate = _encode_winrate(s.get("winrate_high_elo", s["winrate"]))
            n_games = max(0, min((1 << 32) - 1, int(s["n_games"])))
            engine = int(s.get("engine_score", 0))
            tier = max(0, min(255, int(s.get("tier", 0))))
            flags = max(0, min(255, int(s.get("flags", 0))))
            records.append((
                h,
                _clamp_i16(s1[0]), _clamp_i16(s1[1]),
                _clamp_i16(s2[0]), _clamp_i16(s2[1]),
                weight, winrate, n_games,
                _clamp_i16(engine), tier, flags,
            ))
    records.sort(key=lambda r: (r[0], -r[5], r[1], r[2], r[3], r[4]))
    with open(path, "wb") as fh:
        for rec in records:
            fh.write(struct.pack(RECORD_FORMAT, *rec))
    return len(records)
```

Update `read_book_bin`:

```python
def read_book_bin(path: Path) -> list[dict[str, Any]]:
    out = []
    data = Path(path).read_bytes()
    for i in range(0, len(data), RECORD_SIZE):
        chunk = data[i : i + RECORD_SIZE]
        if len(chunk) < RECORD_SIZE:
            break
        (h, s1q, s1r, s2q, s2r, weight, winrate, n_games, engine,
         tier, flags) = struct.unpack(RECORD_FORMAT, chunk)
        s2: tuple[int, int] | None = (s2q, s2r)
        if (s2q, s2r) == NULL_STONE:
            s2 = None
        out.append({
            "hash": h,
            "s1": (s1q, s1r),
            "s2": s2,
            "weight": weight,
            "winrate": winrate / 65535.0,
            "n_games": n_games,
            "engine_score": engine,
            "tier": tier,
            "flags": flags,
        })
    return out
```

- [ ] **Step 4: Run tests; expect pass**

```bash
.venv/bin/pytest scripts/openbook/tests/test_io_book.py -v
```

Expected: PASS for the new tests. The existing tests that hard-asserted `RECORD_SIZE == 26` need to be removed (only `test_record_format_is_26_bytes` exists — drop it and rely on the new `test_record_format_is_28_bytes`).

- [ ] **Step 5: Update spec**

In `docs/specs/SPEC_OPENING_BOOK.md`, replace the `## 5 — Binary record layout (revised)` block with the v2.1 version:

```markdown
## 5 — Binary record layout (v2.1, 28 bytes)

```
struct format: '<Q hh hh H H I h B B'   little-endian, no padding (28 bytes)
fields:
  hash         u64    canonical position hash (turn-start)
  s1q, s1r     i16,i16  first stone of pair (canonical lex-min)
  s2q, s2r     i16,i16  second stone, or NULL_STONE = (i16::MIN, i16::MIN)
  weight       u16    scaled composite weight (see §6)
  winrate      u16    high-ELO winrate * 65535
  n_games      u32    total games seen at this (pos, pair)
  engine_score i16    engine eval at deepening depth (0 if not deepened)
  tier         u8     1=SAFE, 2=EXPERT, 3=ENGINE_ONLY, 4=TRAP, 0=DROP
  flags        u8     bitfield:
                       bit 0 = engine_agrees_with_humans
                       bit 1 = wide_opening (turn ≤ 6, stone hex-dist > 5)
                       bit 2 = kl_junction (KL between ELO bands ≥ 0.3)
                       bit 3 = blunder_candidate
                       bits 4-7 reserved
```

Total: 8 + 8 + 2 + 2 + 4 + 2 + 1 + 1 = **28 bytes/record**.

**v2 → v2.1 migration:** the v2 26-byte format is regenerable, not
converted. The Rust probe (when built) reads only v2.1.
```

Insert just below the spec's existing **Production vs. analysis artefacts** paragraph (from A3): a sentence noting `trap_inventory.bin` is the same v2.1 28-byte format restricted to TIER_4 entries — defer this if you'd rather wait until C3.

- [ ] **Step 6: Commit**

```bash
git add scripts/openbook/io_book.py \
        scripts/openbook/tests/test_io_book.py \
        docs/specs/SPEC_OPENING_BOOK.md
git commit -m "io: 28-byte record format (tier + flags)"
```

---

### Task C3: Final emission — tier-split book and trap inventory

**Files:**
- Modify: `scripts/openbook/main.py` — classify, set tier+flags on stats, split into book.bin + trap_inventory.bin
- Modify: `scripts/openbook/io_book.py` — accept a `tier_filter: set[int] | None` arg on `write_book_bin` (or do the filtering in main; either fine — choose main to keep io_book pure)
- Modify: `scripts/openbook/report.py` — tier counts table, top 10 SAFE, top 10 TRAP sections

- [ ] **Step 1: Classify and tag stats in `main.run`**

After `stats = agg.finalize()` and after engine_data is loaded (move the engine_data load earlier in the function if needed), add:

```python
    from openbook.tier import classify, Tier
    from openbook.io_book import (
        FLAG_ENGINE_AGREES, FLAG_WIDE_OPENING,
        FLAG_KL_JUNCTION, FLAG_BLUNDER_CANDIDATE,
    )

    blunder_hashes_set = {b.hash for b in blunders}
    kl_hashes_set = {h for h, kl in junctions_full if kl >= 0.3}

    tier_counts = {t: 0 for t in Tier}
    for h, by_pair in stats.items():
        for pair, s in by_pair.items():
            eng = engine_data.get(h)
            rec_view = {
                "n_games": s["n_games"],
                "n_high_elo_games": s.get("n_high_elo_games", 0),
                "winrate_high_elo": s["winrate_high_elo"],
                "pair": pair,
            }
            tier = classify(rec_view, eng)
            s["tier"] = int(tier)

            # flags
            f = 0
            if eng is not None and rec_view["n_high_elo_games"] > 0:
                if (rec_view["winrate_high_elo"] > 0.5) == (int(eng["engine_score"]) > 0):
                    f |= FLAG_ENGINE_AGREES
            if h in wide_hashes:
                f |= FLAG_WIDE_OPENING
            if h in kl_hashes_set:
                f |= FLAG_KL_JUNCTION
            if h in blunder_hashes_set:
                f |= FLAG_BLUNDER_CANDIDATE
            s["flags"] = f
            # Plumb engine_score through to the writer.
            if eng is not None:
                s["engine_score"] = int(eng["engine_score"])

            tier_counts[tier] += 1
```

- [ ] **Step 2: Split into two stats dicts by tier**

```python
    safe_tiers = {Tier.TIER_1_SAFE, Tier.TIER_2_EXPERT, Tier.TIER_3_ENGINE_ONLY}
    book_stats: dict = {}
    trap_stats: dict = {}
    for h, by_pair in stats.items():
        for pair, s in by_pair.items():
            t = Tier(s["tier"])
            target = (
                book_stats if t in safe_tiers
                else trap_stats if t == Tier.TIER_4_TRAP
                else None  # TIER_DROP — discarded entirely
            )
            if target is None:
                continue
            target.setdefault(h, {})[pair] = s
```

- [ ] **Step 3: Replace the wide_openings.bin write with trap_inventory.bin**

Delete the existing `wide_openings.bin` write block from A4 (the wide info now lives in the `flags` field) and replace with:

```python
    n_book = write_book_bin(
        book_stats, OUT / "opening_book.bin", min_n_games=min_n,
    )
    n_traps = write_book_bin(
        trap_stats, OUT / "trap_inventory.bin", min_n_games=min_n,
    )
    print(f"  book.bin (tiers 1-3):    {n_book} records", file=sys.stderr)
    print(f"  trap_inventory.bin (4):  {n_traps} records", file=sys.stderr)
    for t in Tier:
        print(f"    tier {t.name}: {tier_counts[t]}", file=sys.stderr)
```

Also: remove the stale `data/analysis/wide_openings.bin` from the repo afterward (it was gitignored anyway, but delete locally for tidiness):

```bash
rm -f data/analysis/wide_openings.bin
```

- [ ] **Step 4: Tier breakdown in the report**

Pass to `write_report`:

```python
        tier_counts=tier_counts,
```

In `report.py`:

```python
    if tier_counts is not None:
        L.append("## Tier breakdown")
        L.append("")
        L.append("| tier | count |")
        L.append("|---|---|")
        for t_name, count in [
            ("TIER_1_SAFE", tier_counts.get(1, 0)),
            ("TIER_2_EXPERT", tier_counts.get(2, 0)),
            ("TIER_3_ENGINE_ONLY", tier_counts.get(3, 0)),
            ("TIER_4_TRAP", tier_counts.get(4, 0)),
            ("TIER_DROP", tier_counts.get(0, 0)),
        ]:
            L.append(f"| {t_name} | {count} |")
        L.append("")
```

(`tier_counts.get(t, 0)` works whether the key is `Tier(...)` or an int — cast on call if needed.)

- [ ] **Step 5: Top 10 SAFE + Top 10 TRAP decoded blocks**

In `main.py`:

```python
    safe_records = []
    trap_records = []
    for h, by_pair in stats.items():
        for pair, s in by_pair.items():
            tier = Tier(s["tier"])
            if tier == Tier.TIER_1_SAFE:
                safe_records.append((h, pair, s))
            elif tier == Tier.TIER_4_TRAP:
                trap_records.append((h, pair, s))

    safe_records.sort(key=lambda r: -r[2].get("weight", 0))
    trap_records.sort(key=lambda r: r[2]["winrate_high_elo"])

    safe_decoded = _decode_tier_block(
        safe_records[:10], hash_to_canon, label="SAFE",
    )
    trap_decoded = _decode_tier_block(
        trap_records[:10], hash_to_canon, label="TRAP",
    )
```

Add `_decode_tier_block` to `main.py`:

```python
def _decode_tier_block(
    records, hash_to_canon, label: str,
) -> list[str]:
    out: list[str] = []
    for h, pair, s in records:
        out.extend(_decode_block_for_hash(h, hash_to_canon))
        wr_he = s.get("winrate_high_elo", 0.0)
        n_he = s.get("n_high_elo_games", 0)
        es = s.get("engine_score", 0)
        out.append(
            f"{label} pair: **{_format_pair(pair)}** — "
            f"high-ELO wr **{wr_he*100:.1f}%** "
            f"(n_high_elo={n_he}), engine_score **{es:+d}**"
        )
        out.append("")
    return out
```

Pass through to `write_report`:

```python
        safe_decoded=safe_decoded,
        trap_decoded=trap_decoded,
```

In `report.py`, after the disagreement block:

```python
    if safe_decoded:
        L.append("## Top 10 SAFE positions (tier 1)")
        L.append("")
        L.extend(safe_decoded)
    if trap_decoded:
        L.append("## Top 10 TRAP positions (tier 4)")
        L.append("")
        L.extend(trap_decoded)
```

- [ ] **Step 6: Run full pipeline + tests**

```bash
.venv/bin/pytest scripts/openbook/tests/ -v
.venv/bin/python scripts/build_opening_book.py
ls -la data/analysis/opening_book.bin data/analysis/trap_inventory.bin
```

Expected: tests green; `book.bin` is much smaller than the A3 ~8.5k (probably 1k-3k records since classification is strict); `trap_inventory.bin` has dozens-to-hundreds of records.

- [ ] **Step 7: Read-back invariant check**

```bash
.venv/bin/python -c "
from pathlib import Path
from openbook.io_book import read_book_bin
import sys
sys.path.insert(0, 'scripts')
b = read_book_bin(Path('data/analysis/opening_book.bin'))
t = read_book_bin(Path('data/analysis/trap_inventory.bin'))
assert all(r['tier'] in {1,2,3} for r in b), 'book.bin tier leak'
assert all(r['tier'] == 4 for r in t), 'trap_inventory.bin tier leak'
print(f'book.bin: {len(b)} records, trap_inventory.bin: {len(t)} records')
"
```

Expected: clean print; no AssertionError.

- [ ] **Step 8: Commit**

```bash
git add scripts/openbook/main.py \
        scripts/openbook/report.py
git commit -m "book: split tier 1-3 vs tier 4 into book.bin + trap_inventory.bin"
```

---

## Phase D — Final report

### Task D1: Regenerate REPORT_BOOK.md with the 10-section structure

The report assembler `write_report` already accepts most of the new kwargs by the end of Phase C. This task just reorders the output and adds the wide-vs-tight split that A4 created, plus the section index.

**Files:**
- Modify: `scripts/openbook/report.py` — section ordering
- Modify: `scripts/openbook/main.py` — collect all the kwargs

- [ ] **Step 1: Section order**

Rewrite the body of `write_report(...)` in `scripts/openbook/report.py` so the emitted section order matches:

1. Header + topline counts
2. **Coverage curve (turn-based)** with old/new side-by-side
3. **Tier breakdown**
4. **Engine vs human disagreement**
5. **Top 10 SAFE positions** (tier 1)
6. **Top 10 TRAP positions** (tier 4)
7. **Top theory junctions** (existing KL section)
8. **Position-conditional winrate distribution**
9. **Wide vs tight openings**
10. **Pair-offset prior sanity check** (existing)
11. **Output files**

Move the existing `_format_pair` legacy table out of the blunders code path entirely (we've replaced it with the engine-disagreement table). Keep the top-blunders table behind `if blunder_decoded:` as a fallback for runs that don't yet have engine_eval.

- [ ] **Step 2: Output files index**

Replace the existing `## Output files` block:

```python
    L.append("## Output files")
    L.append("")
    L.append("- `data/analysis/opening_book.bin` — pruned, tiered (1-3), 28-byte v2.1")
    L.append("- `data/analysis/trap_inventory.bin` — tier 4 only")
    L.append("- `data/analysis/opening_tree.json` — full analysis dump")
    L.append("- `data/analysis/theory_index.json`")
    L.append("- `data/analysis/pair_offset_prior.json`")
    L.append("- `data/analysis/deepening_targets.json`")
    L.append("- `data/analysis/engine_eval.json`")
    L.append("")
```

- [ ] **Step 3: Regenerate the report**

```bash
.venv/bin/python scripts/build_opening_book.py
head -50 data/analysis/REPORT_BOOK.md
```

Expected: section headers in the order above, no orphan or duplicate sections.

- [ ] **Step 4: Final test + acceptance check**

```bash
make check
```

Expected: clippy green (no Rust changed), Rust tests green (no Rust changed), pytest green.

Acceptance gates from the brief:
- `book.bin` contains zero TIER_4 / TIER_DROP records — verified by the Step 7 readback in C3 (run it again now).
- `trap_inventory.bin` contains only TIER_4 — same check.
- `engine_eval.json` exists with ≥150 deepened positions — verified by `n_evaluated` field.
- At least one DISAGREE flag in the report — `grep DISAGREE data/analysis/REPORT_BOOK.md`.

- [ ] **Step 5: Commit**

```bash
git add scripts/openbook/report.py \
        scripts/openbook/main.py
git commit -m "report: full v2.1 structure (tiers, engine disagree, safe/trap)"
```

---

## Done condition

All of these must be true:

- [ ] `docs/specs/SPEC_OPENING_BOOK.md` describes the 28-byte v2.1 layout.
- [ ] `data/analysis/opening_book.bin` contains only TIER_1, TIER_2, TIER_3.
- [ ] `data/analysis/trap_inventory.bin` exists and is TIER_4 only.
- [ ] `data/analysis/engine_eval.json` exists with `n_evaluated ≥ 150`.
- [ ] `data/analysis/REPORT_BOOK.md` follows the 10-section order from Phase D.
- [ ] `make check` is green.
- [ ] No commit message contains "Claude" or `Co-Authored-By`.
- [ ] No file under `hexo-engine/`, `hexo.toml`, or `refs/` has changed.
- [ ] This plan file (`docs/superpowers/plans/2026-05-19-book-tiers.md`) committed.

---

## Self-review notes

1. **Spec coverage:** Phase A covers all 5 brief items (A1-A5); Phase B covers B1-B3; Phase C covers C1-C3; Phase D covers the final report rewrite. Every brief item maps to a task.
2. **No placeholders.** Each step has either an exact code change or an exact command + expected output.
3. **Type consistency.** `Tier` enum is referenced consistently in C1, C2 (as int via `s["tier"]`), and C3 (as `Tier(s["tier"])`). `BlunderCandidate` dataclass is used in A2 onward (no 4-tuple sneaking back). `RECORD_FORMAT` shifts from `<QhhhhHHIh` (26B) to `<QhhhhHHIhBB` (28B) once between A3 and C2 — A3 and A4 still use the 26B format, C2 switches to 28B in a single commit.
4. **Caveman commits.** Every commit subject is < 72 chars and follows the existing project style. None include Claude attribution.
5. **Rust untouched.** Every step modifies only Python under `scripts/openbook/` and `scripts/`, plus `docs/specs/SPEC_OPENING_BOOK.md`. `hexo-engine/` is read-only here.
