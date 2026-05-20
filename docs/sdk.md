# Hammerhead Python SDK

`hammerhead` is the in-process Python SDK for the Hammerhead engine — a
minimax bot for [HeXO](https://hexo.did.science/), the two-stones-per-turn
hexagonal 6-in-a-row game.

One `Bot` drives one game: you advance the position with `play()`, ask
the engine for moves with `suggest()`, and query state through
properties. Everything runs in-process — no subprocess, no network.

---

## 1. Quickstart

```python
from hammerhead import Bot

bot = Bot(time_per_stone_ms=500)
bot.play((0, 0))                 # X opens at the origin

while not bot.is_game_over:
    move = bot.suggest()         # engine picks the next stone
    bot.play(move)               # apply it

print("winner:", bot.winner)
print("stones:", bot.ply)
```

`Bot` plays both sides here — it is just an engine handle, not a player
identity. To play *against* the engine, call `suggest()` for the side you
delegate and `play()` your own moves for the other side.

---

## 2. Installation

The SDK is a thin Python package over a Rust extension module
(`hammerhead_engine`), so the engine must be built first.

From the repository root:

```bash
make build        # builds the Rust engine + installs hammerhead editable
```

Manual equivalent:

```bash
pip install maturin
cd hammerhead-engine
maturin develop --release          # builds + installs hammerhead_engine
pip install -e ../hammerhead       # installs the hammerhead SDK
```

Python 3.11 or newer is required. To pull in the documentation and test
tooling as well:

```bash
pip install -e 'hammerhead[dev]'
```

Verify the install:

```python
import hammerhead
print(hammerhead.__version__)      # 0.1.0
```

---

## 3. API reference

Everything below is exported from the package root:

```python
from hammerhead import (
    Bot,
    Move, Player,
    HammerheadError, IllegalMoveError, GameOverError, NotationError,
)
```

### `Move` and `Player`

- `Move` — a stone as an axial hex coordinate tuple `(q, r)`. The origin
  `(0, 0)` is X's mandatory opening cell.
- `Player` — the literal string `"X"` or `"O"`.

### `Bot(time_per_stone_ms=None, tt_size_mb=None)`

Create a bot with an empty board.

| Argument | Meaning | Default |
|----------|---------|---------|
| `time_per_stone_ms` | Search budget per stone, in milliseconds. Used by every `suggest()` that does not override it. | configured default (1000 ms) |
| `tt_size_mb` | Transposition-table size, in mebibytes. | configured default (64 MB) |

Raises `ValueError` if either argument is not positive.

```python
bot = Bot(time_per_stone_ms=250, tt_size_mb=128)
```

### State mutation

#### `play(move: Move) -> None`

Apply one stone at axial coordinate `(q, r)`.

```python
bot = Bot()
bot.play((0, 0))       # X's opening
```

Raises:
- `IllegalMoveError` — cell occupied or out of range.
- `GameOverError` — the game has already been won.
- `NotationError` — a string was passed (string notation is not
  supported yet; see §5).
- `TypeError` — `move` is not a `(q, r)` pair.

#### `undo() -> None`

Undo the most recent stone — one stone, not one turn. Raises `IndexError`
if the history is empty.

```python
bot.play((0, 0))
bot.undo()             # back to an empty board
```

#### `reset() -> None`

Reset to an empty board. Clears history; keeps the configured time
budget and table size.

### Read-only state

| Property | Type | Meaning |
|----------|------|---------|
| `to_move` | `"X"` / `"O"` | Side that places the next stone. |
| `ply` | `int` | Total stones placed so far. |
| `stone_in_turn` | `0` / `1` | `0` = next stone opens a turn; `1` = the side owes its second stone. |
| `is_game_over` | `bool` | `True` once a side has won. |
| `winner` | `"X"` / `"O"` / `None` | Winning side, or `None` while undecided. |
| `history` | `list[Move]` | Stones played so far, in order (a fresh copy). |
| `time_per_stone_ms` | `int` | Current default per-stone budget. |

HeXO turns are two stones for the same side; X's opening turn is a single
stone. `stone_in_turn` tells you where you are within a turn:

```python
bot = Bot()
bot.play((0, 0))                 # X opening — a singleton turn
assert bot.to_move == "O"
assert bot.stone_in_turn == 0
bot.play(bot.suggest())          # O's first stone
assert bot.stone_in_turn == 1    # O still owes a stone
bot.play(bot.suggest())          # O's second stone
assert bot.stone_in_turn == 0
assert bot.to_move == "X"
```

### Engine queries

#### `suggest(time_ms: int | None = None) -> Move`

Return the engine's recommended next stone **without** mutating the
position. Pass `time_ms` to override the per-stone budget for this call
only. Raises `GameOverError` if the game is over, `ValueError` if
`time_ms` is not positive.

```python
move = bot.suggest()             # uses the configured budget
move = bot.suggest(time_ms=2000) # think harder, just this once
bot.play(move)
```

#### `evaluate() -> int`

Static evaluation of the current position. Positive favours X, negative
favours O — the engine is X-positive regardless of whose turn it is. A
decisive position scores near `±(MATE_SCORE - ply)`.

```python
score = bot.evaluate()
print("X is ahead" if score > 0 else "O is ahead")
```

#### `principal_variation(max_depth: int = 16) -> list[Move]`

The engine's predicted best line, walked off the transposition table.
Only meaningful after a search has populated the table — call
`suggest()` first. The walk stops at the first table miss, so the result
may be shorter than `max_depth` (and empty before any search). Raises
`ValueError` if `max_depth` is negative.

```python
bot.suggest()                    # populate the table
for stone in bot.principal_variation():
    print(stone)
```

### Configuration

#### `set_time_per_stone(ms: int) -> None`

Change the default per-stone budget mid-game. Takes effect on the next
`suggest()` that does not pass its own `time_ms`. Raises `ValueError` if
`ms` is not positive.

```python
bot.set_time_per_stone(5000)     # spend more time from now on
```

---

## 4. Threading & concurrency

`Bot` is **not thread-safe**. It wraps a stateful engine with internal
caches; concurrent calls corrupt that state.

- Use **one `Bot` per game**.
- Do **not** share a `Bot` across threads.
- For parallel games, create one `Bot` per game — ideally one per
  process, since each engine releases the GIL during search and several
  bots in one process still contend for CPU.

The engine is fully deterministic: the same move sequence with the same
time budgets reproduces the same results. There is no random seed.

---

## 5. Notation formats

HeXO games are commonly recorded in three text formats:

- **BKE** — Board Kifu Encoding: per-stone human notation like `A0`,
  `B1.1`.
- **BSN** — Board Sequence Notation: a compact full-game move list.
- **HXN** — HeXO Native: a binary game-record container.

**The SDK does not parse these yet.** Today every move crosses the API
as an axial `(q, r)` coordinate tuple (`Move`). Passing a string to
`play()` raises `NotationError`:

```python
from hammerhead import Bot, NotationError

bot = Bot()
try:
    bot.play("A0")
except NotationError as exc:
    print(exc)        # string notation is not supported yet ...
```

When the `hammerhead.notation` parsers ship, `play()` will additionally
accept BKE strings and the `Bot` will gain `to_notation()` /
`from_notation()`. Until then, work in coordinates.

### Axial coordinates

The board is addressed by two axes, `q` and `r`, centred on the origin
`(0, 0)`. X's opening stone must be `(0, 0)`; every other stone is any
empty cell within the engine's playable range.

---

## 6. Performance notes

- **Time budget.** `time_per_stone_ms` is the single biggest knob. 50 ms
  gives a fast, shallow move; 1000–5000 ms gives strong play. The budget
  is per *stone*, and a turn is two stones, so a turn costs roughly twice
  the per-stone budget.
- **`suggest(time_ms=...)`.** Override the budget for one call when a
  position deserves more thought — e.g. a critical midgame decision —
  without raising the default for the whole game.
- **Transposition table.** `tt_size_mb` defaults to 64 MB, which is
  ample for typical budgets. Raise it (128–256 MB) only for long
  searches (multi-second budgets) where the table would otherwise
  thrash. It costs memory, not correctness.
- **Determinism.** Identical inputs reproduce identical games, which
  makes runs reproducible — but note that a *time*-bounded search can
  reach different depths under different machine load. For bit-exact
  reproduction, replay a recorded move list rather than re-running
  `suggest()`.

---

## 7. Error handling

Every error the SDK raises on purpose derives from `HammerheadError`, so
one `except` clause can catch them all:

```python
from hammerhead import Bot, HammerheadError

bot = Bot()
try:
    bot.play((0, 0))
    bot.play((0, 0))             # occupied
except HammerheadError as exc:
    print("rejected:", exc)
```

| Exception | Fires when | Recovery |
|-----------|-----------|----------|
| `IllegalMoveError` | Cell is occupied or out of range. | Pick another cell; or call `suggest()` for a guaranteed-legal move. |
| `GameOverError` | `play()` or `suggest()` after a win. | Inspect `winner`; call `reset()` for a new game or `undo()` to reopen. |
| `NotationError` | A string is passed where a `(q, r)` tuple is expected. | Pass a coordinate tuple. |
| `HammerheadError` | Base class — never raised directly. | Use to catch the whole family. |

`undo()` on an empty history raises the built-in `IndexError`, and a
malformed move argument raises `TypeError` — these are ordinary
programming errors, not game errors, so they stay outside the
`HammerheadError` family.

---

## 8. Examples

### Play against the engine

```python
from hammerhead import Bot

bot = Bot(time_per_stone_ms=1000)
bot.play((0, 0))                         # you open as X

while not bot.is_game_over:
    if bot.to_move == "O":
        bot.play(bot.suggest())          # engine plays O
    else:
        # replace with real input in your UI
        bot.play(bot.suggest())          # engine plays X too, for demo
print("winner:", bot.winner)
```

### A match harness (two configs head to head)

```python
from hammerhead import Bot

def play_game(x_ms: int, o_ms: int) -> str | None:
    bot = Bot()
    bot.play((0, 0))
    while not bot.is_game_over and bot.ply < 400:
        bot.set_time_per_stone(x_ms if bot.to_move == "X" else o_ms)
        bot.play(bot.suggest())
    return bot.winner

print(play_game(x_ms=200, o_ms=1000))
```

### Save and load a game

The SDK has no file format yet, so persist the coordinate history
yourself — it is a plain list of `(q, r)` tuples.

```python
import json
from hammerhead import Bot

# save
bot = Bot(time_per_stone_ms=200)
bot.play((0, 0))
for _ in range(6):
    bot.play(bot.suggest())
saved = json.dumps(bot.history)

# load
restored = Bot(time_per_stone_ms=200)
for q, r in json.loads(saved):
    restored.play((q, r))
assert restored.history == bot.history
```

### Query the engine mid-game

```python
from hammerhead import Bot

bot = Bot(time_per_stone_ms=500)
bot.play((0, 0))
for _ in range(4):
    bot.play(bot.suggest())

print("eval:", bot.evaluate())                  # who is ahead
print("predicted line:", bot.principal_variation(max_depth=8))
print("to move:", bot.to_move, "ply:", bot.ply)
```
