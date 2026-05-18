# HeXO Minimax Bot

Rust core + PyO3 bindings + Python interface. Minimax engine for
[HeXO](https://hexo.did.science/), the 2-stones-per-turn hexagonal
6-in-a-row game. Goal: beat
[SealBot](https://github.com/Ramora0/SealBot).

## Goal

Minimax + alpha-beta. WSC threat eval. Fast NPS. Win.

## Toolchain

- Rust 1.85+ (edition 2024)
- Python 3.11+
- pyo3 0.28
- maturin 1.13+

## Build

```bash
make build         # maturin develop --release + pip install -e hexo
make test          # cargo test + pytest
make check         # lint + test
```

Manual equivalent:

```bash
pip install maturin
cd hexo-engine
maturin develop --release
pip install -e ../hexo
pytest hexo/tests
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
hexo play                       # human vs bot REPL
hexo selfplay -n 10             # bot vs bot, 10 games
hexo bench --time-ms 1000       # NPS smoke
hexo bot                        # subprocess protocol (Phase 10 harness)
```

The `hexo bot` subcommand exposes a line-oriented stdin/stdout protocol
used by the promotion harness. See `specs/SPEC_API.md` for the command
list.

## Validation (Phase 10, planned)

`make vs N_GAMES=200` will run the current build against the last
validated `best` and report win-rate statistics. `make promote` will
advance `.bestref` if the configured threshold is met.

See `specs/SPEC_ROADMAP.md` § Phase 10 for the harness specification.

## Config

All engine tuning in `hexo.toml`. One file. Rust codegen + Python tomllib.
See [SPEC_CONFIG](specs/SPEC_CONFIG.md).

## Layout

- `hexo.toml` source of truth for engine tuning
- `hexo-engine/` Rust core, PyO3 bindings
- `hexo/` Python wrapper, bot, CLI, benchmarks
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
