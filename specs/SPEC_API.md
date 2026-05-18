# HeXO API Spec

## PyO3 Surface

Minimal. Only outward exposure. All game logic lives in Rust.

```python
from hexo_engine import Engine, Player

eng = Engine()
eng.place((0, 0))                       # X first stone
eng.place((0, 1))                       # O stone 1
eng.place((1, 0))                       # O stone 2
move = eng.best_move(time_ms=1000)      # (q, r)
move = eng.best_move(depth=8)           # fixed depth
score = eng.eval()                      # static eval, X-positive
state = eng.state()                     # dict snapshot
eng.undo()
eng.reset()
eng.load_bsn("base64string")
bsn = eng.dump_bsn()
```

### Engine class

```python
class Engine:
    def __init__(self, tt_size_mb: int = 256) -> None: ...
    def place(self, pos: tuple[int, int]) -> None: ...
    def undo(self) -> None: ...
    def best_move(
        self,
        time_ms: int | None = None,
        depth: int | None = None,
    ) -> tuple[int, int]: ...
    def eval(self) -> int: ...
    def state(self) -> dict: ...
    def reset(self) -> None: ...
    def load_bsn(self, s: str) -> None: ...
    def dump_bsn(self) -> str: ...
    def to_move(self) -> Player: ...
    def is_legal(self, pos: tuple[int, int]) -> bool: ...
    def winner(self) -> Player | None: ...
    def ply(self) -> int: ...
```

### Errors

- `IllegalMove`: cell occupied, too far, or game ended
- `InvalidNotation`: BSN parse failure
- `NoSearchBudget`: best_move called without time_ms or depth

## Rust Side (`pybind.rs`)

Thin wrapper. No logic.

```rust
use pyo3::prelude::*;

#[pyclass]
pub struct Engine {
    board: Board,
    search: SearchConfig,
    tt: TranspositionTable,
}

#[pymethods]
impl Engine {
    #[new]
    #[pyo3(signature = (tt_size_mb = 256))]
    fn new(tt_size_mb: usize) -> Self { ... }
    
    fn place(&mut self, pos: (i16, i16)) -> PyResult<()> { ... }
    fn undo(&mut self) -> PyResult<()> { ... }
    
    #[pyo3(signature = (time_ms = None, depth = None))]
    fn best_move(
        &mut self,
        time_ms: Option<u64>,
        depth: Option<i8>,
    ) -> PyResult<(i16, i16)> { ... }
    
    fn eval(&self) -> i32 { ... }
    fn state<'py>(&self, py: Python<'py>) -> PyResult<&'py PyDict> { ... }
    fn reset(&mut self) { ... }
    fn load_bsn(&mut self, s: &str) -> PyResult<()> { ... }
    fn dump_bsn(&self) -> String { ... }
    fn to_move(&self) -> u8 { ... }
    fn is_legal(&self, pos: (i16, i16)) -> bool { ... }
    fn winner(&self) -> Option<u8> { ... }
    fn ply(&self) -> u32 { ... }
}

#[pymodule]
fn hexo_engine(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Engine>()?;
    Ok(())
}
```

GIL handling: long search releases GIL via `py.allow_threads(|| ...)`.

## Python Wrapper (`hexo/bot.py`)

High-level convenience over raw Engine.

```python
from dataclasses import dataclass
from hexo_engine import Engine

@dataclass
class BotConfig:
    time_per_move_ms: int = 1000
    max_depth: int | None = None
    tt_size_mb: int = 256

class Bot:
    def __init__(self, cfg: BotConfig = BotConfig()):
        self.engine = Engine(tt_size_mb=cfg.tt_size_mb)
        self.cfg = cfg
    
    def play(self) -> tuple[int, int]:
        return self.engine.best_move(
            time_ms=self.cfg.time_per_move_ms,
            depth=self.cfg.max_depth,
        )
    
    def observe(self, move: tuple[int, int]) -> None:
        self.engine.place(move)
    
    def reset(self) -> None:
        self.engine.reset()
```

## Notation (`hexo/notation.py`)

Pure Python. Parse / dump game records.

```python
def parse_bsn(s: str) -> list[tuple[int, int]]: ...
def dump_bsn(moves: list[tuple[int, int]]) -> str: ...

def parse_bke(s: str) -> list[tuple[int, int]]: ...
def dump_bke(moves: list[tuple[int, int]]) -> str: ...

# HXN is binary, more complex
def parse_hxn(data: bytes) -> "GameRecord": ...
def dump_hxn(record: "GameRecord") -> bytes: ...
```

## Benchmark Harness (`hexo/benchmark.py`)

```python
def match(bot_a: Bot, bot_b: Bot, max_plies: int = 200) -> MatchResult: ...
def tournament(bots: list[Bot], rounds: int) -> Standings: ...
def vs_sealbot(bot: Bot, num_games: int) -> WinRate: ...
def perft(engine: Engine, depth: int) -> int: ...   # move-gen sanity
```

## CLI (`hexo/cli.py`)

```bash
hexo play              # interactive REPL vs bot
hexo selfplay -n 100   # bot vs bot
hexo bench             # NPS benchmark
hexo analyze <bsn>     # show eval + best line
```

## Build

```
pip install maturin
cd hexo-engine
maturin develop --release
pip install -e ../hexo
```

## Integration Path (future)

- WebSocket client for hexo.did.science live play
- SealBot match harness (HTTP or socket)
- Self-play data export for ML tuning
- Web UI via simple Flask/FastAPI wrapper

## Versioning

Engine version in `Cargo.toml`. Expose via `hexo_engine.__version__`.

BSN/HXN: tag with format version on dump. Reject unknown versions on parse.
