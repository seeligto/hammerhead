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
├── build.rs                reads ../hexo.toml, codegens src/config_generated.rs
├── src/
│   ├── lib.rs              crate root, pub use only
│   ├── config.rs           include!-s codegen'd consts from hexo.toml
│   ├── coords.rs           axial coord type, hex math
│   ├── board.rs            piece storage, place/undo, hash
│   ├── moves.rs            move gen, legality, candidate cells
│   ├── win.rs              6-in-row detection
│   ├── threats.rs          WSC threat classification
│   ├── eval.rs             static eval (sum of threat scores)
│   ├── zobrist.rs          hash keys, incremental update
│   ├── tt.rs               transposition table
│   ├── ordering.rs         move ordering for alpha-beta
│   ├── search.rs           minimax + alpha-beta + iter deepening
│   └── pybind.rs           PyO3 wrapper, no logic
└── tests/
    ├── coord_tests.rs
    ├── win_tests.rs
    ├── threat_tests.rs
    ├── eval_tests.rs
    └── search_tests.rs

hammerhead/                 Python package
├── pyproject.toml
├── hammerhead/
│   ├── __init__.py
│   ├── config.py           reads ../hexo.toml via tomllib (SPEC_CONFIG.md)
│   ├── bot.py              high-level Bot class
│   ├── game.py             game state convenience
│   ├── notation.py         BSN / BKE / HXN parsing
│   ├── benchmark.py        SealBot match harness, self-play
│   └── cli.py              command-line entrypoint
└── tests/
```

## Module Responsibilities

One job per file. If file does 2 things, split.

| Module | Job |
|---|---|
| `config` | re-exports codegen'd consts from `hexo.toml` — no magic numbers elsewhere |
| `coords` | axial coord type, hex distance, 3 axis vectors |
| `board` | piece storage, place/undo, occupancy, candidate set |
| `moves` | generate legal candidates, depth-limited radius |
| `win` | detect 6-in-row after placement (O(1)) |
| `threats` | classify shapes per WSC tuples |
| `eval` | compute static score from threat counts |
| `zobrist` | incremental position hash |
| `tt` | TT lookup/store, depth/bound/flag |
| `ordering` | rank moves for alpha-beta pruning |
| `search` | minimax driver, iter deepening, time mgmt |
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
