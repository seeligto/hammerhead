# HeXO API Spec

Authoritative surface for the Rust `hexo_engine` PyO3 module, the Python
`hexo.bot` wrapper, the `hexo` CLI subcommands, and the line-based
subprocess protocol consumed by the Phase 11 promotion harness.

## PyO3 module (`hexo_engine`)

```python
from hexo_engine import Engine

eng = Engine(tt_size_mb=64)
eng.place((0, 0))                    # X first stone
eng.place((1, 0))                    # O stone 1
eng.place((-1, 0))                   # O stone 2
move = eng.best_move(time_ms=1000)   # (q, r) — search splits the budget
pv   = eng.find_pv(depth=4)          # best line walked off the TT
score = eng.cached_eval()
eng.undo()
eng.reset()
eng.clear_tt()
```

### `Engine`

```python
class Engine:
    def __init__(self, tt_size_mb: int = 64) -> None: ...
    def place(self, pos: tuple[int, int]) -> None: ...
    def undo(self) -> None: ...
    def best_move(
        self,
        time_ms: int | None = None,
        depth: int | None = None,
    ) -> tuple[int, int]: ...
    def find_pv(self, depth: int) -> list[tuple[int, int]]: ...
    def cached_eval(self) -> int: ...
    def to_move(self) -> int: ...      # 0 = X, 1 = O
    def winner(self) -> int | None: ...
    def ply(self) -> int: ...
    def halfmove(self) -> int: ...     # 0 = next stone starts a turn, 1 = same side's 2nd
    def hash(self) -> int: ...         # 128-bit Zobrist
    def reset(self) -> None: ...
    def clear_tt(self) -> None: ...
```

- `place` uses the side stored on the board. No player argument.
- `best_move` must be called with at least one of `time_ms` or `depth`
  set. Internally splits the per-turn budget by `time_stone1_pct`.
- `find_pv(depth)` walks the TT from the current position, returning at
  most `depth` legal moves. The walk is **best-effort**: it stops at the
  first TT miss or illegal move, so the returned list may be shorter
  than `depth`. The board is restored to its starting state before
  return.
- `clear_tt()` wipes the transposition table only. Ordering history
  (killers / butterfly history) is preserved — TT scales with positions
  seen, history is per-game move-quality memory and survives a clear.
- Errors raised: `ValueError` on illegal `place`, illegal `undo`, or
  `best_move` called with neither budget set. PyO3's automatic
  panic-to-`PanicException` machinery handles unexpected engine panics;
  `pybind.rs` itself raises only `ValueError`.

### Rust shim (`pybind.rs`)

Thin wrapper. No game logic. Releases the GIL for every `best_move`
call via `py.detach` (PyO3 0.28 — the post-`allow_threads` API). Errors
map to `PyValueError`.

```rust
#[pyclass(name = "Engine")]
pub struct PyEngine {
    inner: crate::search::Engine,
}
```

## Python `Bot` (`hexo.bot`)

High-level convenience over one `Engine`.

```python
@dataclass(frozen=True, slots=True)
class BotConfig:
    time_per_move_ms: int = CONFIG.bot.default_time_per_move_ms
    max_depth: int | None = None
    tt_size_mb: int = CONFIG.bot.default_tt_size_mb

class Bot:
    def __init__(self, cfg: BotConfig = BotConfig()) -> None: ...
    def reset(self) -> None: ...
    def play_stone(self) -> tuple[int, int]:
        """Search one stone, place it, return its coord."""
    def play_turn(self) -> list[tuple[int, int]]:
        """Play 1 or 2 stones (X's first turn = 1, else 2). Return placed."""
    def observe(self, move: tuple[int, int]) -> None:
        """Apply an externally-played stone to the local engine."""
    def winner(self) -> int | None: ...
    def halfmove(self) -> int: ...
    def to_move(self) -> int: ...
```

The "play 1 or 2" decision is driven by the engine's `halfmove`: after
the first stone, if `halfmove == 1` the same side continues.

## Subprocess protocol (`hexo bot`)

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
continues unless the offending command was `quit`. Unknown commands
return `error: unknown command <CMD>`.

Startup: the process emits `hexo bot ready` to stdout once and flushes
before reading the first command, so clients can synchronize on it.

## CLI (`hexo`)

```bash
hexo play                            # human vs bot REPL
hexo selfplay -n N                   # bot vs bot, log winners
hexo bench [--time-ms T]             # NPS smoke
hexo analyze <bsn>                   # placeholder (BSN parsing is Phase 12+)
hexo bot [--tt-size-mb MB]           # subprocess protocol (above)
hexo match CURRENT BEST              # generic two-binary match (Phase 11)
hexo promote [--dry-run]             # current vs .bestref worktree (Phase 11)
```

The match commands accept ``--n N --time-ms T --test sprt|wilson|raw``.
Exit codes: ``0`` if the final verdict is ``PROMOTE``; ``1`` otherwise
(``REJECT`` or ``INCONCLUSIVE``). ``hexo promote`` rewrites and commits
``.bestref`` atomically on ``PROMOTE`` unless ``--dry-run`` is set; on
commit failure the file is rolled back to its prior contents.

## Build

```
make build    # maturin develop --release + pip install -e ../hexo
make test     # cargo test --release + pytest
make check    # lint + test
```

## Integration path (future)

- WebSocket client for live play on hexo.did.science
- SealBot harness (HTTP or socket)
- Self-play data export for ML tuning
- Web UI (Flask / FastAPI shim)

## Versioning

Engine version in `hexo-engine/Cargo.toml`. Re-exported as
`hexo_engine.__version__`.

BSN / HXN / BKE parsers (`hexo.notation`) are deferred to a later
phase. Until they land, `hexo analyze` is a stub.
