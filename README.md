# HeXO Minimax Bot

Beat SealBot. Rust engine. Python wrapper.

## Goal

Minimax + alpha-beta. WSC threat eval. Fast NPS. Win.

## Toolchain

- Rust 1.85+ (edition 2024)
- Python 3.11+
- pyo3 0.28
- maturin 1.13+

## Build

```
pip install maturin
cd hexo-engine
maturin develop --release
pip install -e ../hexo
pytest hexo/tests
```

## Config

All engine tuning in `hexo.toml`. One file. Rust codegen + Python tomllib.
See [SPEC_CONFIG](specs/SPEC_CONFIG.md).

## Layout

- `hexo.toml` source of truth for engine tuning
- `hexo-engine/` Rust core, PyO3 bindings
- `hexo/` Python wrapper, bot, CLI, benchmarks
- `specs/` source-of-truth specs

## Specs

- [SPEC_ARCHITECTURE](specs/SPEC_ARCHITECTURE.md)
- [SPEC_ENGINE](specs/SPEC_ENGINE.md)
- [SPEC_EVAL](specs/SPEC_EVAL.md)
- [SPEC_API](specs/SPEC_API.md)
- [SPEC_CONFIG](specs/SPEC_CONFIG.md)
