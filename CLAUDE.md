# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repo shape

Two-package workspace, one shared config file:

- `hexo-engine/` — Rust crate (edition 2024, rust 1.85+). Core engine + PyO3 bindings. Compiles to a Python extension module named `hexo_engine`.
- `hexo/` — Python package (3.11+, needs stdlib `tomllib`). Thin wrapper over `hexo_engine`: `Bot`, CLI, benchmarks, notation, config view.
- `hexo.toml` — single source of truth for engine *tuning* (eval weights, search defaults, board constants). See "Config invariant" below.
- `specs/SPEC_*.md` — authoritative design docs. Treat as source of truth for behavior; the code is currently scaffolded and mostly `todo!()`. PDFs in `specs/` are background design references (HeXO theory, WSC threats, radius theory).

The Python package's `hexo_engine` dependency is satisfied by `maturin develop` installing the Rust build into the active venv — it is not on PyPI.

## Build & test

First-time setup (run inside the project venv at `.venv/`):

```bash
pip install maturin
cd hexo-engine && maturin develop --release   # builds Rust, installs hexo_engine into venv
pip install -e ../hexo                        # installs Python wrapper editable
```

After Rust source or `hexo.toml` changes, re-run `maturin develop --release` in `hexo-engine/`. `build.rs` declares `rerun-if-changed=../hexo.toml`, so editing the config triggers a Rust rebuild automatically.

| Task | Command |
|---|---|
| Rust unit + integration tests | `cargo test` in `hexo-engine/` |
| Single Rust test | `cargo test --test win_tests` or `cargo test <name_substring>` |
| Python tests | `pytest hexo/tests` from repo root |
| Single Python test | `pytest hexo/tests/test_smoke.py::test_imports` |
| Criterion benches | `cargo bench` in `hexo-engine/` (criterion is a dev-dep) |
| CLI | `hexo play` / `hexo selfplay -n 100` / `hexo bench` / `hexo analyze <bsn>` (after `pip install -e ../hexo`) |

`Cargo.lock` is gitignored — this is intentional for a library crate.

## Config invariant (important)

All engine *tuning* parameters live in `hexo.toml`. Both languages must read the same file:

- Rust: `hexo-engine/build.rs` parses `../hexo.toml` at compile time and emits `$OUT_DIR/config_generated.rs` containing `pub const` definitions. `src/config.rs` is nothing but `include!(...)`. Reference values as `crate::config::OPEN_5_SCORE`, `crate::config::DEFAULT_TT_SIZE_MB`, etc.
- Python: `hexo/hexo/config.py` walks parents from `__file__` (or honors `$HEXO_CONFIG`) and parses via `tomllib`, exposing frozen dataclasses at `from hexo.config import CONFIG`.

Rules:
- **No magic numbers.** Any tuning constant referenced from Rust or Python must originate in `hexo.toml`. Duplicating a value in code = drift = bug.
- Adding a new constant means: (1) add the key to `hexo.toml`, (2) add a matching `emit_*` call in `build.rs`, (3) add the field to the relevant dataclass in `hexo/hexo/config.py`. See `specs/SPEC_CONFIG.md`.
- **Build-system metadata stays out of `hexo.toml`** — dep versions, edition, rust-version, profile flags, Python deps live in the respective `Cargo.toml` / `pyproject.toml` because Cargo and PEP 621 cannot reference external TOML for those.

## Architecture conventions

From `specs/SPEC_ARCHITECTURE.md`:

- **One job per file.** If a Rust module does two things, split it. Module responsibility table is in the spec; honor it when adding code.
- **`pybind.rs` is a thin shim.** No game logic in the PyO3 layer — all logic lives in pure-Rust modules so it can be tested with `cargo test` without a Python interpreter.
- **No allocation in the search hot path** where avoidable. Pre-allocated move buffers (`SmallVec`), incremental threat/window caches, incremental Zobrist hash updates, fixed-size TT indexed by `hash & MASK`.
- **GIL handling:** long searches in `pybind.rs` should release the GIL via `py.allow_threads(|| ...)`.
- **Engine eval sign:** positive = X advantage, negative = O advantage. Search uses negamax with a `to_move_sign` multiplier.
- **`lib.rs` is `pub use` only** — re-exports the public surface; no logic.

## Spec → code mapping

When implementing or modifying behavior, the spec is authoritative. Start with the relevant doc, not the (mostly stub) code:

- `specs/SPEC_ARCHITECTURE.md` — module layout, data flow, build flags
- `specs/SPEC_ENGINE.md` — board/coords/win/zobrist/TT/search internals, parity rule for whose move it is at a given ply
- `specs/SPEC_EVAL.md` — three eval layers (window scan, WSC shape detection, fork/mate-via-multi-threat), incremental maintenance rules
- `specs/SPEC_API.md` — exact PyO3 surface (`Engine` class methods, errors)
- `specs/SPEC_CONFIG.md` — config flow detailed above
