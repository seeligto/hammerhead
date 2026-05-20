# Hammerhead

Rust core + PyO3 bindings + Python interface. Minimax engine for
[HeXO](https://hexo.did.science/), the 2-stones-per-turn hexagonal
6-in-a-row game. Goal: beat
[SealBot](https://github.com/Ramora0/SealBot).

## Using Hammerhead from Python

Install — builds the Rust engine, then the `hammerhead` SDK:

```bash
make build
```

Drive the engine in-process:

```python
from hammerhead import Bot

bot = Bot(time_per_stone_ms=500)
bot.play((0, 0))                 # X opens at the origin
while not bot.is_game_over:
    bot.play(bot.suggest())      # engine picks the next stone
print("winner:", bot.winner)
```

Moves are axial `(q, r)` coordinates. One `Bot` drives one game; it is
stateful and single-threaded. Full reference, error handling, and worked
examples: [`docs/sdk.md`](docs/sdk.md).

## Goal

Minimax + alpha-beta. WSC threat eval. Fast NPS. Win.

## Toolchain

- Rust 1.85+ (edition 2024)
- Python 3.11+
- pyo3 0.28
- maturin 1.13+

## Build

```bash
make build         # maturin develop --release + pip install -e hammerhead
make test          # cargo test + pytest
make check         # lint + test
```

Manual equivalent:

```bash
pip install maturin
cd hammerhead-engine
maturin develop --release
pip install -e ../hammerhead
pytest hammerhead/tests
```

## Development

```bash
make rebuild       # clean + build (after pulling)
make fmt           # cargo fmt
make lint          # clippy with pedantic
```

## CLI

After `make build`:

```bash
hammerhead play                       # human vs bot REPL
hammerhead selfplay -n 10             # bot vs bot, 10 games
hammerhead bench --time-ms 1000       # NPS smoke
hammerhead bot                        # subprocess protocol (Phase 11 harness)
hammerhead match A_CMD B_CMD          # generic two-binary match
hammerhead promote [--dry-run]        # current vs .bestref worktree
```

The `hammerhead bot` subcommand exposes a line-oriented stdin/stdout protocol
used by the promotion harness. See `specs/SPEC_API.md` for the command
list.

## Validation (Phase 11)

`make vs N_GAMES=200` runs the current build against the worktree
checked out at `.bestref` and reports win-rate, Wilson 95% CI, SPRT LLR
(or raw/Wilson verdict), and an Elo estimate. It does not advance
`.bestref`. `make promote` runs the same match and advances `.bestref`
to `HEAD` if the verdict is `PROMOTE`.

Tuning lives in `hexo.toml § [promote]`. The worktree at `.worktree-best/`
and its per-worktree venv `.venv-best/` are bootstrapped automatically
by `scripts/setup_worktree.sh`; if `.bestref` is missing it's
initialized to `HEAD`, so the first `make vs` runs current-vs-current.

See `specs/SPEC_ROADMAP.md` § Phase 11 for the harness specification.

## Config

All engine tuning in `hexo.toml`. One file. Rust codegen + Python tomllib.
See [SPEC_CONFIG](specs/SPEC_CONFIG.md).

## Layout

- `hexo.toml` source of truth for engine tuning
- `hammerhead-engine/` Rust core, PyO3 bindings
- `hammerhead/` Python wrapper, bot, CLI, benchmarks
- `specs/` source-of-truth specs

## Specs

Source of truth lives in `specs/`. Read in this order:

1. [SPEC_ARCHITECTURE](specs/SPEC_ARCHITECTURE.md) — what's where, how things fit
2. [SPEC_CONFIG](specs/SPEC_CONFIG.md) — `hexo.toml`, the tuning surface
3. [SPEC_ENGINE](specs/SPEC_ENGINE.md) — Rust internals
4. [SPEC_EVAL](specs/SPEC_EVAL.md) — WSC threat theory, eval layers
5. [SPEC_API](specs/SPEC_API.md) — Python surface, subprocess protocol
6. [SPEC_ROADMAP](specs/SPEC_ROADMAP.md) — phase plan, resolved decisions

## External references

- Play: https://hexo.did.science/
- SealBot: https://github.com/Ramora0/SealBot
- Sandbox: https://meaf.us/
- Explorer: https://explore.htttx.io/

## License

TBD.
