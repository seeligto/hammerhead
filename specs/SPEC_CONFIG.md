# HeXO Config Spec

## Principle

One file. One source of truth. Both sides read it.

All engine *tuning* parameters live in `hexo.toml` at workspace root. Edit it,
rebuild, Rust and Python pick up the new values.

This file does **not** hold build-system metadata (dep versions, edition, Python
version). Cargo and PEP 621 cannot reference external TOML for those, so they
stay in their respective `Cargo.toml` / `pyproject.toml`.

## What lives in `hexo.toml`

- Eval weights (WSC shape scores, mate score, window-scan k→score table)
- Search defaults (depth, time budget, TT size, radii, deadline-check cadence)
- Board constants (max piece distance for legality)

## What does NOT live in `hexo.toml`

- Crate dep versions → `hexo-engine/Cargo.toml`
- Python deps / build backend → `hexo/pyproject.toml`, `hexo-engine/pyproject.toml`
- Cargo profile flags → `hexo-engine/Cargo.toml` `[profile.release]`
- Rust edition / rust-version → `hexo-engine/Cargo.toml`

Rationale: keeping build metadata where build tools expect it avoids brittle
codegen of `Cargo.toml` and respects each ecosystem's conventions. The user-
facing knob — engine tuning — lives in one obvious place.

## File layout

```toml
[engine.eval]
mate_score = 1_000_000
open_5     = 8000
# ...
window_k_scores = [0, 1, 8, 64, 512, 4096, 1_000_000]

[engine.tt]
default_size_mb = 64

[engine.search]
default_max_depth      = 64
default_time_ms        = 1000
# ...

[engine.board]
max_piece_distance = 8
```

See `hexo.toml` for the full schema.

## Rust side: build-time codegen

`hexo-engine/build.rs` reads `../hexo.toml` at compile time and emits
`$OUT_DIR/config_generated.rs` containing `pub const` definitions.

`hexo-engine/src/config.rs` does nothing but `include!` that file.

Other modules reference values as `crate::config::OPEN_5_SCORE`,
`crate::config::DEFAULT_TT_SIZE_MB`, etc. No magic numbers anywhere else.

Cargo `cargo:rerun-if-changed=../hexo.toml` ensures rebuilds on edits.

## Python side: runtime load via `tomllib`

`hexo/hexo/config.py`:

- Resolves `hexo.toml` by walking parents from `__file__` (or `$HEXO_CONFIG` env override).
- Parses once at import; cached via `functools.lru_cache`.
- Exposes typed, frozen `@dataclass` views: `EvalConfig`, `SearchConfigDefaults`, `BoardConfig`.
- Module-level `CONFIG: HexoConfig` for convenient access.

```python
from hexo.config import CONFIG
CONFIG.eval.open_5             # 8000
CONFIG.search.default_time_ms  # 1000
```

## Adding a new constant

1. Add the key to `hexo.toml` under the appropriate `[engine.*]` table.
2. Add an `emit_*` call in `hexo-engine/build.rs`.
3. Add the field to the matching dataclass in `hexo/hexo/config.py`.
4. Use `crate::config::NAME` from Rust, `CONFIG.section.name` from Python.

## Invariants

- Both sides must read the **same** file. CI should verify by exercising both.
- `hexo.toml` is the spec. Eval/search constants in code = drift = bug.
- If a value is needed at compile-time-only (e.g. array length), still put it
  in `hexo.toml` and `include!` it — never hard-code in two places.

## Open issues

- Tuning runs may want to override eval weights at runtime. Future work: expose
  a PyO3 `set_eval_override(name, value)` that writes through to mutable
  statics or a `RwLock<EvalParams>` struct alongside the codegen'd defaults.
  Not in scope for initial scaffold.
