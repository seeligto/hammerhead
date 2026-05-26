# Hammerhead API Spec

Authoritative surface for the project's Python-facing APIs.

The **public** surface is the `hammerhead` SDK package (`hammerhead.Bot`
and friends) — the supported way to embed the engine. Everything else —
the `hammerhead_engine` PyO3 module, the `hammerhead` CLI, the
line-based subprocess protocol — is **internal**: used by the CLI,
benchmarks, and the Phase 11 promotion harness, and not covered by any
stability guarantee.

## Public SDK (`hammerhead`)

In-process, single-game, stateful. Import everything from the package
root:

```python
from hammerhead import (
    Bot, SearchStats,
    Move, Player, MATE_SCORE,
    HammerheadError, IllegalMoveError, GameOverError, NotationError,
)
```

Full reference with worked examples: `docs/sdk.md`.

### Types and constants

- `Move` — a stone as an axial hex coordinate tuple `(q, r)`. The origin
  `(0, 0)` is X's mandatory opening cell.
- `Player` — the literal string `"X"` or `"O"`.
- `MATE_SCORE` — `int`, the score magnitude of a forced win. A decisive
  position evaluates near `±(MATE_SCORE - ply)`. Sourced from
  `hexo.toml § [engine.eval] mate_score`.
- `SearchStats` — frozen dataclass returned by
  `Bot.suggest(return_stats=True)` (see below).

### `Bot`

```python
class Bot:
    def __init__(
        self,
        time_per_stone_ms: int | None = None,   # default: config (1000 ms)
        tt_size_mb: int | None = None,          # default: config (64 MB)
    ) -> None: ...

    # state mutation
    def reset(self) -> None: ...
    def play(self, move: Move) -> None: ...
    def undo(self) -> None: ...

    # read-only state (properties)
    to_move: Player                 # "X" / "O"
    ply: int
    stone_in_turn: int              # 0 = turn start, 1 = owes 2nd stone
    is_game_over: bool
    winner: Player | None
    history: list[Move]
    time_per_stone_ms: int
    tt_size_mb: int

    # engine queries (no mutation)
    def suggest(
        self,
        time_ms: int | None = None,
        depth: int | None = None,
        return_stats: bool = False,
    ) -> Move | tuple[Move, SearchStats]: ...
    def evaluate(self) -> int: ...
    def principal_variation(self, max_depth: int = 16) -> list[Move]: ...

    # configuration
    def set_time_per_stone(self, ms: int) -> None: ...
```

- One `Bot` represents one game in progress. Stateful: `play` advances
  the position, the queries inspect it without mutating, `undo` rewinds
  one stone, `reset` starts over.
- Search is **per stone**. A HeXO turn is two stones for the same side;
  X's opening turn is a single stone. `stone_in_turn` disambiguates.
- `suggest` does not place the move — apply it with `play`.
- `suggest` accepts `time_ms` (per-stone budget), `depth` (fixed-depth
  target), or both. Passing `depth=N` alone lifts the time bound; passing
  both lets the search abort on whichever bound hits first. With neither
  argument, the construction-time `time_per_stone_ms` is used. Set
  `return_stats=True` to receive `(Move, SearchStats)` instead of `Move`
  — default `False` is backwards-compatible.
- The engine is X-positive: `evaluate` returns positive for an X
  advantage regardless of side to move.
- `Bot` is **not thread-safe**. One instance per game; do not share
  across threads.
- The engine is deterministic — there is no random seed.

### `SearchStats`

Frozen dataclass returned by `Bot.suggest(return_stats=True)`:

```python
@dataclass(frozen=True, slots=True)
class SearchStats:
    max_depth_reached: int   # deepest ID iteration that completed
    nodes: int               # recursive + qsearch nodes visited
    nps: float               # nodes / (time_ms / 1000); 0.0 if time_ms == 0
    time_ms: float           # actual search wall-clock
    score: int               # X-positive evaluation of the chosen move
```

Computed once per search; not cumulative. `nps` is computed in the SDK
from the underlying `(nodes, time_ms)` pair — the Rust layer returns
the two raw fields, the derived ratio lives in Python.

### Exceptions

```python
class HammerheadError(Exception): ...        # base — never raised directly
class IllegalMoveError(HammerheadError): ... # occupied / out-of-range cell
class GameOverError(HammerheadError): ...    # play / suggest after a win
class NotationError(HammerheadError): ...    # string passed where Move expected
```

| Method | Raises |
|--------|--------|
| `__init__` | `ValueError` (non-positive `time_per_stone_ms` / `tt_size_mb`) |
| `play` | `IllegalMoveError`, `GameOverError`, `NotationError`, `TypeError` |
| `undo` | `IndexError` (empty history) |
| `suggest` | `GameOverError`, `ValueError` (non-positive `time_ms` or `depth`) |
| `principal_variation` | `ValueError` (negative `max_depth`) |
| `set_time_per_stone` | `ValueError` (non-positive `ms`) |

`IndexError` / `TypeError` are ordinary programming errors and stay
outside the `HammerheadError` family.

### Deferred surface

Planned, not yet implemented — documented here so the surface is honest:

- **String move notation.** `play` accepts only `Move` tuples; passing a
  `str` raises `NotationError`. BKE / BSN / HXN string parsing is not
  planned for v1.
- **`threats(side)`** — per-side threat-shape report. Needs new PyO3
  surface (the engine's `ThreatCounts` is not exposed today).
- **`board_ascii`** — ASCII board renderer. Needs a new engine accessor.
- **`set_tt_size(mb)`** — live transposition-table resize. The engine
  has no live-resize entry point yet.

## Internal: PyO3 module (`hammerhead_engine`)

The Rust extension the SDK wraps. Not a public surface — consume it via
`hammerhead.Bot`.

```python
class Engine:
    def __init__(self, tt_size_mb: int = 64) -> None: ...
    def place(self, pos: tuple[int, int]) -> None: ...
    def undo(self) -> None: ...
    def best_move(
        self, time_ms: int | None = None, depth: int | None = None,
    ) -> tuple[int, int]: ...
    def find_pv(self, depth: int) -> list[tuple[int, int]]: ...
    def cached_eval(self) -> int: ...
    def to_move(self) -> int: ...      # 0 = X, 1 = O
    def winner(self) -> int | None: ...
    def ply(self) -> int: ...
    def halfmove(self) -> int: ...     # 0 = turn start, 1 = same side's 2nd
    def hash(self) -> int: ...         # 128-bit Zobrist
    def reset(self) -> None: ...
    def clear_tt(self) -> None: ...
    # Runtime tuning surface (Sprint 4A — see § Tunable parameters)
    def search_params(self) -> dict: ...
    def set_search_params(self, params: dict) -> None: ...
    def reset_search_params(self) -> None: ...
```

- `place` uses the side stored on the board. No player argument.
- `best_move` must be called with at least one of `time_ms` or `depth`.
  `time_ms` is the **per-stone** budget — the engine consumes the
  whole value on this call and does not split. It does **not** place
  the move.
- `find_pv(depth)` walks the TT from the current position, returning at
  most `depth` legal moves. Best-effort: stops at the first TT miss; the
  board is restored before return.
- `clear_tt()` wipes the transposition table only; ordering history
  (killers / butterfly history) survives.
- Errors: `ValueError` on illegal `place`, illegal `undo`, or
  `best_move` with neither budget set. The SDK translates these into the
  `HammerheadError` family.
- Bench-only extras (`bench_best_move`, `tt_stats`) exist for the
  benchmark suite — see `specs/SPEC_BENCHMARKS.md`.

### Rust shim (`pybind.rs`)

Thin wrapper, no game logic. Releases the GIL for every `best_move` via
`py.detach` (PyO3 0.28). Errors map to `PyValueError`.

### Tunable parameters (runtime override, Sprint 4A)

`hexo.toml` remains the source of truth for **defaults**. Production
`Engine::new()` initialises `SearchConfig` from those defaults. Tuning
workflows can transiently override values without rebuild via:

```python
engine.set_search_params({"lmr_min_depth": 2,
                          "lmr_min_move_index": 8,
                          "lmr_reduction": 2})
result = engine.best_move(time_ms=500)
engine.reset_search_params()   # restore TOML defaults
current = engine.search_params()
```

Partial dict semantics mirror `set_eval_overrides`: missing keys retain
their *current* value, unknown keys raise `ValueError`. The override
persists across `reset()` and `clear_tt()` but **not** across engine
restart — a fresh `Engine()` reads TOML defaults.

Sprint 4A exposes the LMR triplet (`lmr_min_depth`,
`lmr_min_move_index`, `lmr_reduction`). Sprint 4C extends the dict to
aspiration + extension parameters.

Rule: `hexo.toml` is the source of truth for committed values. Runtime
override is a tune-loop knob, not a replacement for TOML. Any tuned
value that wins a Texel sweep must be committed to TOML to take effect
in production. Measured hot-path cost: < 0.1 % NPS (struct-field load
vs. compile-time const — verified Sprint 4A via iai-callgrind).

## Internal: subprocess protocol (`hammerhead bot`)

One command per line, one line per response. Used by the Phase 11
promotion harness. Coordinates are integers `q r`, space-separated.

| Command       | Response          | Notes                                |
|---------------|-------------------|--------------------------------------|
| `reset`       | `ok`              | Fresh game; TT retained.             |
| `place Q R`   | `ok`              | Place at `(Q, R)` for side-to-move.  |
| `best_move T` | `Q R`             | Search `T` ms. Engine splits budget. |
| `winner`      | `X` / `O` / `none`|                                      |
| `ply`         | `N`               | Stones placed so far.                |
| `halfmove`    | `0` / `1`         |                                      |
| `to_move`     | `X` / `O`         |                                      |
| `eval`        | `SCORE`           | Cached static eval. X-positive.      |
| `hash`        | `HEX`             | Lowercase, zero-padded to 32 chars.  |
| `quit`        | `bye`             | Process exits afterwards.            |

Errors are emitted as `error: <message>` on a single line. The session
continues unless the offending command was `quit`. Startup: the process
emits `hammerhead bot ready` to stdout once and flushes before reading.

## Internal: CLI (`hammerhead`)

```bash
hammerhead play                            # human vs bot REPL
hammerhead selfplay -n N                   # bot vs bot, log winners
hammerhead bench [...]                     # benchmark suite
hammerhead bot [--tt-size-mb MB]           # subprocess protocol (above)
hammerhead match CURRENT BEST              # generic two-binary match (Phase 11)
hammerhead promote [--dry-run]             # current vs .bestref worktree (Phase 11)
```

The match commands accept `--n N --time-ms T --test sprt|wilson|raw`.
Exit codes: `0` if the final verdict is `PROMOTE`; `1` otherwise.
`hammerhead promote` rewrites and commits `.bestref` atomically on
`PROMOTE` unless `--dry-run` is set.

## Build

```
make build    # maturin develop --release + pip install -e hammerhead
make test     # cargo test --release + pytest
make check    # lint + test
```

The SDK requires Python ≥ 3.11. `pip install -e 'hammerhead[dev]'` adds
`pytest` and `pdoc`.

## Versioning

Engine version in `hammerhead-engine/Cargo.toml`, re-exported as
`hammerhead_engine.__version__`. SDK version is `hammerhead.__version__`
(`0.1.0`).

## Integration path (future)

- WebSocket client for live play on hexo.did.science
- SealBot harness (HTTP or socket)
- Self-play data export for ML tuning
