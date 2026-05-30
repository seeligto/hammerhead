# Hammerhead — Architecture Spec

## Goal

Minimax engine. Beat SealBot. Fast. Clean.

## Stack

- Rust core. No alloc in hot path where possible.
- PyO3 bindings. Zero-copy where possible.
- Python wrapper. Game integration, benchmarks, eval testing.

## Crate Layout

```
hexo.toml                   single source of truth for engine tuning (SPEC_CONFIG.md)

hammerhead-engine/          Rust crate
├── Cargo.toml
├── pyproject.toml          maturin build config
├── build.rs                reads ../hexo.toml (+ the NNUE net_file), codegens src/config_generated.rs
├── nets/                   committed trained NNUE nets (peraxis_aug.json) + provenance
├── src/
│   ├── lib.rs              crate root, pub use only
│   ├── config.rs           include!-s codegen'd consts from hexo.toml
│   ├── coords.rs           axial coord type, hex math
│   ├── board.rs            piece storage, place/undo, hash, threat/eval caches
│   ├── proximity.rs        proximity counts, sparse cell sets, Board proximity helpers
│   ├── axis_bitmap.rs      per-axis sparse line bitmaps
│   ├── moves.rs            move gen, legality, candidate cells
│   ├── win.rs              6-in-row detection
│   ├── threats.rs          WSC threat classification
│   ├── line_contrib.rs     per-(axis,line_id) Layer-1 contribution cache (Phase 27)
│   ├── eval.rs             static eval (NNUE leaf eval; hand-built 3-layer fallback)
│   ├── nnue.rs             outcome-net leaf eval + incremental accumulator + int16 quant
│   ├── zobrist.rs          hash keys, incremental update
│   ├── tt.rs               transposition table
│   ├── ordering.rs         move ordering for alpha-beta
│   ├── search.rs           minimax + alpha-beta + iter deepening
│   ├── engine.rs           Engine handle — owns board/tt/ordering, place/undo/best_move
│   └── pybind.rs           PyO3 wrapper, no logic
└── tests/                  per-module integration tests

hammerhead/                 Python package
├── pyproject.toml
├── hammerhead/
│   ├── __init__.py
│   ├── config.py           reads ../hexo.toml via tomllib (SPEC_CONFIG.md)
│   ├── exceptions.py       HammerheadError exception family
│   ├── types.py            shared type aliases
│   ├── bot.py              high-level Bot class
│   ├── game.py             game state convenience
│   ├── benchmark.py        benchmark suite — measurement functions
│   ├── promote.py          promotion harness — match drivers + data model
│   ├── promote_sprt.py     promotion harness — Wilson/Elo/SPRT statistics
│   ├── promote_worktree.py promotion harness — .bestref + worktree management
│   ├── cli.py              CLI entrypoint — argparse + dispatch + play/selfplay/bot
│   ├── cli_bench.py        CLI — bench subcommand handlers
│   └── cli_match.py        CLI — match/promote/vs subcommand handlers
└── tests/
```

## Module Responsibilities

One job per file. If file does 2 things, split.

| Module | Job |
|---|---|
| `config` | re-exports codegen'd consts from `hexo.toml` — no magic numbers elsewhere |
| `coords` | axial coord type, hex distance, 3 axis vectors |
| `board` | piece storage, place/undo, occupancy, candidate set, threat/eval caches |
| `proximity` | proximity counts, sparse cell sets, `Board` proximity-maintenance helpers |
| `axis_bitmap` | per-axis sparse line bitmaps shared by win/threats/eval |
| `moves` | generate legal candidates, depth-limited radius |
| `win` | detect 6-in-row after placement (O(1)) |
| `threats` | classify shapes per WSC tuples |
| `line_contrib` | per-`(axis, line_id)` Layer-1 contribution cache (Phase 27) — sentinel-marked lazy memoisation, invalidated on `Board::place`/`undo` |
| `eval` | compute static score from threat counts |
| `zobrist` | incremental position hash |
| `tt` | TT lookup/store, depth/bound/flag |
| `ordering` | rank moves for alpha-beta pruning |
| `search` | minimax driver, iter deepening, time mgmt |
| `engine` | `Engine` handle — owns board/tt/ordering, place/undo/best_move surface |
| `pybind` | thin PyO3 layer, no game logic |

## Data Flow

```
Python Bot
    │
    ▼
PyO3 boundary (pybind.rs)
    │
    ▼
search.rs  ──uses──▶  ordering.rs, tt.rs
    │
    ▼
eval.rs  ──uses──▶  threats.rs, win.rs
    │
    ▼
board.rs  ──uses──▶  coords.rs, zobrist.rs, moves.rs
```

Search never alloc per node. Pre-allocated move buffers, threat caches.

## Configuration

All engine tuning parameters (eval weights, search defaults) live in
`hexo.toml` at workspace root. Rust ingests via `build.rs` codegen; Python
reads via `tomllib`. See [SPEC_CONFIG](SPEC_CONFIG.md).

## Toolchain

- Rust 1.85+ (edition 2024)
- Python 3.11+ (tomllib in stdlib)
- pyo3 0.28
- maturin 1.13+

Note: Rust edition 2026 does not exist. The most recent stable edition is
2024 (Rust 1.85, Feb 2025). Cargo 1.94 accepts only 2015 / 2018 / 2021 / 2024.

## Build

```
pip install maturin
cd hammerhead-engine
maturin develop --release   # builds Rust, installs Python module
```

Release flags in `Cargo.toml`:
```toml
[profile.release]
opt-level     = 3
lto           = "fat"
codegen-units = 1
panic         = "abort"
strip         = "symbols"
incremental   = false
```

### `.cargo/config.toml` — `target-cpu=native`

Phase 14 added `.cargo/config.toml` with `rustflags = ["-C",
"target-cpu=native"]`. This binds the release binary's instruction
set to the host CPU's exact feature set (Zen 4 / AVX2 / AVX-512 on
the bench host). Accepted trade-off for development + bench because
all macro NPS / flamegraph numbers are captured locally.

**For distribution / CI release artifacts**: swap `target-cpu=native`
for an explicit feature gate, e.g.

```toml
rustflags = ["-C", "target-feature=+avx2,+bmi2,+fma"]
```

…or build per-arch artifacts. The PyO3 cdylib loaded into a Python
ABI3 wheel inherits the same constraint; portable wheels should use
the explicit-feature form.

### `make pgo` — profile-guided optimization

Canonical release path. `make pgo` runs four passes via
`scripts/pgo_build.sh`:

1. **Instrumented build** — `maturin develop --release` with
   `RUSTFLAGS=-Cprofile-generate=<dir>`. The `.so` installed into
   `.venv` is instrumented; every call writes `.profraw` events.
2. **Training** — `scripts/pgo_training.py` runs the engine on the
   canonical fixtures (`midgame_12`, `midgame_30`, `single_origin`)
   at depth 6, ~30 s total wall-clock. Coverage focuses on the search
   hot path that real games stress.
3. **Profile merge** — `llvm-profdata merge` consolidates the
   `.profraw` set into `merged.profdata`. Rustup's bundled
   `llvm-profdata` (matched to the active toolchain's LLVM version)
   is preferred over Arch's `/usr/bin/llvm-profdata`.
4. **Optimized build** — `maturin develop --release` with
   `RUSTFLAGS=-Cprofile-use=<merged.profdata>`. PGO-flavoured `.so`
   replaces the instrumented one in `.venv`.

`CARGO_TARGET_DIR=hammerhead-engine/target-pgo` is exported by the
script so neither the instrumented nor the optimized build pollutes
the main `target/` (used by `make build`, `make bench-iai`, etc.).

**Trigger:** re-run `make pgo` on any commit that touches a
search-hot file (`src/{search,ordering,eval,threats,axis_bitmap,tt,
board}.rs`). The merged profile is host-specific and stale once the
instrumented call graph drifts.

**Caveats:**
- Per-arch retrain. PGO data captures host-specific code-layout
  decisions; cross-arch distribution needs per-target retrain.
  Out of scope for v1.
- Requires `llvm-tools-preview` rustup component (see
  `rust-toolchain.toml`).
- Skip with `HEXO_SKIP_PGO=1` when iterating on unrelated flags.

Measured impact at Sprint 1 baseline (`ae539b7`): +5.4% NPS
midgame_12 @ 500 ms vs the same source without PGO. See
`analysis/baseline_ae539b7/verdict.md` for the full measurement.

### Optional features

- `tt_stats` — `Engine::tt_stats()` populates probe/hit/store/
  collision counters. Zero-cost when off (no fields, no code paths).
- `mimalloc` — swaps the global allocator for mimalloc. Useful when
  small per-iter allocations dominate; off by default — see Phase 14
  STEP 3 result.
- `simd_eval` — enables the AVX2 `encode_ternary` lane in
  `eval::simd`. Runtime feature detect; scalar fallback always
  available. Off by default until the 729-table identity test
  certifies correctness on the build host.

## Testing Strategy

- Unit tests per module
- Integration: known mate-in-N positions
- Regression: SealBot match harness (Python side)
- Perf: criterion benches for search nps
