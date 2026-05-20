# CLAUDE.md — instructions for Claude Code in this repo

## Style

Caveman mode. No articles. No filler. No pleasantries.
Short sentences. Code speaks for itself. Explain only when asked.

## Workflow (every phase)

- **Use `make` commands.** They wire `.venv` paths and toolchain
  correctly. Avoid bare `pytest` / `cargo` calls except for narrow,
  targeted runs.
- **Previous-phase agent may still be committing.** Proceed with tests
  and implementation. On merge conflict, stop and flag.
- **End every phase with a review pass.** Independent reviewer checks
  for bugs, missed cases, bad practice, inefficiencies.
  - For **spec-vs-code discrepancies**: pick the more efficient /
    optimized side. Update the loser. Test the change is sound.
  - Reviewer uses `make` commands to verify.
  - Fix what the reviewer finds. Report all changes in "When Done".

## Commits

Atomic. Descriptive. Simple. Short (< 72 char subject).
**Never include `Co-Authored-By: Claude` or any Claude Code attribution.**
Each commit does one logical thing.

Examples:
- `threats: add ThreatSet defense_cells extraction`
- `tt: implement two-bucket replacement policy`
- `zobrist: add Z_HALFMOVE constant + parity transitions`

## Specs are source of truth

Before writing any code, read the relevant `specs/SPEC_*.md`. If a
spec is ambiguous, flag it — do not improvise. If a spec needs
updating, update the spec first, in its own commit, with a clear
rationale.

Phase prompts start with STEP 0 (spec & config updates). Do not skip.

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
- Single `pvs_node` dispatches on `board.to_move()` (not separate
  `pvs_max` / `pvs_min`).
- `Board::to_move()` parity rule handles the same-player-twice case.
- `Engine::best_move()` returns one stone. Python `Bot` calls twice per
  turn.
- Zobrist 128-bit. **`Z_HALFMOVE` parity flag** disambiguates
  "whose second stone is next" (Phase 6).
- `Z_TURN_X` XOR'd iff side == X (regardless of halfmove).
- Per-axis sparse line bitmaps (`axis_bitmap.rs`) shared by
  win/threats/eval.
- TT two-bucket (depth-preferred + always-replace), generation-aged,
  u128-verified.
- TT mate-score adjustment via `score_to_tt` / `score_from_tt`.
- **No null-move pruning** in v1 (two-stone parity fragile).
- Threat-only quiescence with hard cap 8 plies.
- LMR at depth ≥ 3, move index ≥ 6, R = 1. Disabled for
  TT / killer / S0 / S0-block. 3-step dance (reduced-null →
  full-null → full-window).
- Time split: stone 1 = 60% of turn budget, stone 2 = 40%.
- `MAX_PLY` = total recursion ply ceiling (incl. extensions + qsearch),
  not search-target depth. Default 128.

## PyO3 0.28 specifics

- `Python::detach` for GIL release (renamed from `allow_threads`).
- `#[pyclass(unsendable)]` on `Engine` — `Board`'s `RefCell` / `Cell`
  caches make it `!Sync`.
- `pybind.rs` is **type conversion + GIL only**. No game logic.
- Every method delegates to `RustEngine`.

## Verification gate

Every phase ends green:

```
make check    # cargo clippy + cargo test --release + pytest
```

After Phase 10:

```
make bench all --time-ms 50    # smoke must pass
```

After Phase 11:

```
make vs N_GAMES=4 TIME_MS=50 TEST=raw    # current vs .bestref worktree
                                          # bootstraps .bestref to HEAD on
                                          # first run; non-zero exit on
                                          # REJECT is by design
```

## Phase plan

See `specs/SPEC_ROADMAP.md`. Do not skip phases. Do not implement
out of order without flagging the deviation. Each phase has a prompt
in `prompts/PHASE_N_PROMPT.md`.

When a prompt has a STEP 0, do it first, in its own commit.

## Reporting

End of each phase: produce the "When Done" report from the prompt
verbatim. Do not paraphrase. Do not omit ambiguities encountered.

## File layout reference

```
hexo.toml                       single source of truth for engine tuning

specs/
  SPEC_ARCHITECTURE.md          crate layout
  SPEC_CONFIG.md                hexo.toml schema
  SPEC_ENGINE.md                Rust internals
  SPEC_EVAL.md                  3-layer eval + WSC theory
  SPEC_API.md                   Python surface + subprocess protocol
  SPEC_BENCHMARKS.md            bench infrastructure (Phase 10)
  SPEC_ROADMAP.md               phase plan + locked decisions

prompts/
  PHASE_{4..11}_PROMPT.md       Claude Code prompts, one per phase

hammerhead-engine/               Rust crate
  src/                          {coords, board, zobrist, axis_bitmap,
                                 moves, win, threats, eval, tt,
                                 ordering, search, pybind, config}.rs
  benches/                      criterion micro-benches (Phase 10)
  src/bin/bench_drain.rs        criterion → JSON consolidator
  tests/                        per-module unit tests

hammerhead/                      Python package
  hammerhead/                   {bot, game, cli, config, benchmark,
                                 promote, notation}.py
  tests/                        Python integration tests

benches/
  fixtures/positions.json       shared fixture library (Phase 10)
  results/baseline.json         baseline bench result (committed)
  results/*.json                run outputs (gitignored)

scripts/
  setup_worktree.sh             Phase 11 worktree bootstrap

.bestref                        Phase 11: SHA of validated best
.worktree-best/                 Phase 11: gitignored worktree
```
