# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Style

Caveman mode. No articles. No filler. No pleasantries.
Short sentences. Code speaks for itself. Explain only when asked.

## Commits

Atomic. Descriptive. Simple. Short (< 72 char subject).
**Never include `Co-Authored-By: Claude` or any Claude Code attribution.**
Each commit does one logical thing.

Examples (good):
- `threats: add ThreatSet defense_cells extraction`
- `tt: implement two-bucket replacement policy`
- `zobrist: add Z_HALFMOVE constant + parity transitions`

## Repo shape

Two-package workspace, one shared config file:

- `hexo-engine/` — Rust crate (edition 2024, rust 1.85+). Core engine + PyO3 bindings. Compiles to a Python extension module named `hexo_engine`.
- `hexo/` — Python package (3.11+, needs stdlib `tomllib`). Thin wrapper over `hexo_engine`: `Bot`, CLI, benchmarks, notation, config view.
- `hexo.toml` — single source of truth for engine *tuning* (eval weights, search defaults, board constants). See "Config invariant" below.
- `specs/SPEC_*.md` — authoritative design docs. Treat as source of truth for behavior; the code is currently scaffolded and mostly `todo!()`.

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
- **Engine eval sign:** positive = X advantage, negative = O advantage. Search is **minimax form**, not negamax; eval is X-positive globally.
- **`lib.rs` is `pub use` only** — re-exports the public surface; no logic.

## Hot path discipline

- `FxHashMap` / `FxHashSet` only. Never `std::collections::HashMap`.
- `#[inline]` on accessors and short helpers in hot loops.
- `#[cold]` on init / growth branches.
- No `unwrap` in library code. `Result` + `thiserror`.
- No `unsafe` without profiling evidence + invariant comment.
- All numeric tuning lives in `hexo.toml`. Magic numbers in code = bug.
- No alloc in search inner loop. `SmallVec` for short collections.

## Linting

Must pass:

```
cargo clippy --all-targets -- -D warnings -W clippy::all -W clippy::pedantic -A clippy::module_name_repetitions
```

## Architectural decisions (locked)

- Search is **per-stone**, not per-turn.
- **Minimax form**, not negamax. Eval is X-positive globally.
- `Board::to_move()` parity rule handles the same-player-twice case.
- `Engine::best_move()` returns one stone. Python `Bot` calls twice per turn.
- Zobrist is 128-bit. **Includes a `Z_HALFMOVE` parity flag** to disambiguate
  "whose second stone is next" — addressed in Phase 6.
- Per-axis sparse line bitmaps (`axis_bitmap.rs`) shared by win/threats/eval.
- TT is two-bucket (depth-preferred + always-replace), generation-aged.
- Null-move pruning **skipped in v1**. Two-stone turn parity is fragile;
  revisit post-baseline.
- Threat-only quiescence with hard cap (8 plies).
- LMR at depth ≥ 3, move index ≥ 6, R = 1. Disabled for TT / killer / S0.
- Time split: stone 1 = 60% of turn budget, stone 2 = 40% (configurable).

## Verification gate

Every phase ends green:

```
make check    # cargo clippy + cargo test --release + pytest
```

## Specs are source of truth

Before writing any code, read the relevant `specs/SPEC_*.md`. If a
spec is ambiguous, flag it — do not improvise. If a spec needs
updating, update the spec first, in its own commit, with a clear
rationale.

When following a Phase prompt: STEP 0 always updates specs. Do not
skip it.

## Phase plan

See `specs/SPEC_ROADMAP.md`. Do not skip phases. Do not implement
out of order without flagging the deviation. Each phase has a prompt
in `prompts/PHASE_N_PROMPT.md`.

When a prompt has a STEP 0 (spec & config updates), do it first, in
its own commit.

## Reporting

End of each phase: produce the "When Done" report from the prompt
verbatim. Do not paraphrase. Do not omit ambiguities encountered.

## Spec → code mapping

When implementing or modifying behavior, the spec is authoritative. Start with the relevant doc, not the (mostly stub) code:

- `specs/SPEC_ARCHITECTURE.md` — module layout, data flow, build flags
- `specs/SPEC_ENGINE.md` — board/coords/win/zobrist/TT/search internals, parity rule for whose move it is at a given ply
- `specs/SPEC_EVAL.md` — three eval layers (window scan, WSC shape detection, fork/mate-via-multi-threat), incremental maintenance rules
- `specs/SPEC_API.md` — exact PyO3 surface (`Engine` class methods, errors)
- `specs/SPEC_CONFIG.md` — config flow detailed above
