# HeXO Bot — Implementation Roadmap

Save as `specs/SPEC_ROADMAP.md`.

## Status

| Phase | Module(s) | State |
|---|---|---|
| 1 | `coords`, `board`, `zobrist` | ✅ done |
| 2 | `axis_bitmap`, `win` | ✅ done |
| 3 | `moves` | ✅ done |
| 4 | `threats` | 🔜 next |
| 5 | `eval` | pending |
| 6 | `tt` (incl. zobrist halfmove parity) | pending |
| 7 | `ordering` | pending |
| 8 | `search` | pending |
| 9 | `pybind` + Python `Bot` + CLI | pending |
| 10 | promotion harness (`vs`/`promote`) | post-baseline |

Order is fixed. Each phase depends on the previous.

## Core decisions (locked)

### Two-stone turn → per-stone minimax

`Board::to_move()` returns the player whose stone goes next, handling the
double-stone case via the `(p-1)/2 % 2` parity rule. Search is per-stone
(not per-turn) and uses **minimax form**, not negamax:

```rust
fn search(board, depth, alpha, beta) -> i32 {
    if depth == 0 || terminal { return eval(board); }
    match board.to_move() {
        Player::X => maximize(board, depth, alpha, beta),
        Player::O => minimize(board, depth, alpha, beta),
    }
}
```

Within-turn continuation is automatic: when X plays stone 1, `to_move()`
still returns X for stone 2. Search recurses into another `maximize` call.
No sign flip needed mid-turn.

**Eval sign convention**: X-positive globally. Never side-relative.

**Why not negamax?** Negamax assumes side flips every ply. Two-stone turns
break that assumption. Mixed-flip negamax is possible but error-prone;
minimax is literal and robust.

### Engine API granularity

`Engine::best_move()` returns **one stone**. Python `Bot` calls it twice per
turn. TT entries from stone 1's search warm stone 2's search.

### Zobrist halfmove parity (CRITICAL — addressed in Phase 6)

Research-surfaced requirement: two positions identical in stones but
differing in "whose second stone of the turn is next" must hash to
different keys, otherwise the TT aliases them and search returns wrong
values.

Implementation: reserve a `Z_HALFMOVE` 128-bit constant. XOR into the hash
whenever the side-to-move is on stone 2 of its turn (i.e. one stone has
already been placed in the current 2-stone turn). The flag flips on every
`place` and `undo` exactly when transitioning into/out of the second half
of a turn.

Phase 6 (TT) implements this **before** introducing the TT array; without
it the TT is unsafe.

### Bitmap representation

Per-axis sparse line bitmaps shared by `win`, `threats`, `eval`. 3 axes
(Q, R, S=q+r) × 2 players. `LineBitmap = SmallVec<[u64; 4]>` with
`base_pos: i16`. Hex 60° rotation = axis permutation Q→S→R→Q. Augmentation-
friendly by construction.

### Single source of truth

All engine tuning (eval weights, search defaults, board constants,
move-gen radii, time-split ratios, LMR thresholds, TT sizing) lives in
`hexo.toml`. Rust ingests via `build.rs` codegen → `crate::config::*`.
Python via `tomllib` → `CONFIG.*` dataclasses. **Magic numbers in code = bug.**

Build metadata (dep versions, Rust edition, Python version, Cargo profile
flags) stays in `Cargo.toml` / `pyproject.toml`.

## Performance targets

| Op | Target | Comment |
|---|---|---|
| `place` | < 500 ns | board + bitmap + zobrist |
| `undo` | < 500 ns | symmetric |
| `is_winning_move` | < 100 ns | bitmap line walk |
| `generate(r=2)` | < 5 μs | < 50 candidates typical |
| `eval` (full) | < 5 μs | incremental cache |
| `search` NPS | > 200 k nps | release, single-thread |

Numbers are estimates. Benchmark on phase 8 completion. Adjust if needed.

## Resolved open questions (informed by research output)

1. **Quiescence depth cap**: hard cap 8 plies + threat-only filter
   (S0+ moves only). Configurable via `search.qsearch_max_plies`.
2. **LMR parameters**: enable at depth ≥ 3, move index ≥ 6 (i.e. skip the
   first 6 ordered moves), `R = 1` initially. Re-search at full window on
   fail-high. Disable if move is TT-move, killer, S0 threat, or S0-block.
3. **TT replacement scheme**: two-bucket (always-replace + depth-preferred)
   from v1. Cheap, more robust than depth-preferred alone. Aging via
   per-search generation counter.
4. **TT size**: 64 MB default (~2²² entries × 16 bytes). Configurable in
   `hexo.toml` via `tt.size_mb`. Power-of-two bucket count.
5. **Aspiration windows**: initial delta = 50 cp around prior-iteration
   score. Widen 2× on fail (fail-high → β += 50, 100, 200, then full
   window). Enable from depth ≥ 4.
6. **Eval window-scan weights**: keep existing `window_k_scores`
   `[0, 1, 8, 64, 512, 4096, 1_000_000]`. Use 729-entry ternary lookup
   built at build time from these weights + open/closed extension rules.
7. **VCF/VCT proof search**: inline as **threat-only quiescence** in v1.
   Separate root-time-boxed solver (~10% of turn budget) is post-baseline.
8. **Stone-2 ordering**: standard ordering plus a "completes-stone-1-S0"
   bonus bucket. Implemented in Phase 7.
9. **Per-turn time split**: stone 1 gets 60%, stone 2 gets 40% of the
   per-turn budget by default. Configurable in `hexo.toml` via
   `search.time_stone1_pct = 0.6`. Rationale: stone 1 sets up stone 2 and
   benefits more from depth; stone 2 reuses the warmed TT cheaply.

## Contradictions with research output (decisions retained)

- **Null-move pruning**: research advocates Stage 2 (+150-300 Elo). We
  **skip in v1**. Two-stone turn parity is fragile; null-move can corrupt
  the halfmove flag transition. Revisit post-baseline once search is
  proven correct against SealBot.
- **Eval table size 729 vs explicit shape detection**: research treats the
  729-entry ternary table as the dominant eval signal. We use it as
  **Layer 1 only**; Layers 2 (shape bonuses via WSC) and 3 (fork detection
  via defense-set vertex cover) carry the threat-arithmetic awareness
  research-output's flat table cannot express on a hex grid with
  cross-axis shapes (rhombus, triangle, bone, arch).

## Phase sketches

### Phase 4 — `threats`

Inputs: `Board`, `axis_bitmap` data, last-placed cell.
Outputs:
- `ThreatCounts` per player (open_5, closed_5, open_4, closed_4, open_3,
  rhombus, arch, bone, trapezoid, open_2, closed_3, triangle)
- `ThreatSet` per player: list of S0 threat instances with defense cells
  (sufficient cell set such that occupying any defense cell denies the
  threat-completion next stone).

Algorithm:
1. For each piece of player P within radius 5 of last move: walk lines on
   3 axes, classify endpoints (open / closed), match against shape table.
2. Cross-axis shapes (rhombus, triangle, bone): piece-cluster scan within
   radius 2 of last move.
3. Cache `ThreatCounts` + `ThreatSet` on board. Invalidate on each
   `place(c)` / `undo(c)`; recompute only within radius 5 of `c`.

Forks (mate-via-multi-threat) are computed by `eval`, not `threats`. The
threats module produces counts and a list of S0 threat instances with
defense cells.

### Phase 5 — `eval`

Three layers:

1. **Window scan (Layer 1)**: 729-entry ternary lookup over 6-cell windows
   per axis. Table built at build time from `window_k_scores` (own k =
   own-only window) and `[engine.eval.open_extension_bonus]` /
   `[engine.eval.closed_extension_bonus]` for windows with empty
   extensions. Mixed windows (both colors) score 0.
2. **Shape bonus (Layer 2)**: sum `ThreatCounts × weight` from
   `hexo.toml`.
3. **Fork detection (Layer 3)**: union of defense cells across S0 threat
   instances. Minimum vertex cover size:
   - 1 → normal threat (covered by Layer 2)
   - 2 → medium bonus (forces full defense turn)
   - ≥ 3 → MATE (return ±MATE_SCORE − ply)

Tempo: small advisory contribution. Real tempo emerges from search depth.

Eval cached per board state. Layer 1 incremental (≤ 18 windows touched per
stone). Layers 2/3 recompute locally within radius 5 of last move.

**Defer radius-theory colony discounting** until baseline beats SealBot.

### Phase 6 — `tt` (and zobrist halfmove parity)

Step 1: extend `zobrist.rs` with `Z_HALFMOVE` constant. `Board` XORs it on
every `place` / `undo` when transitioning the halfmove flag. Tests verify
hash differs between (X plays stone 1, O to play) and (X plays stone 1,
X to play stone 2) when the stone configuration is identical but the
halfmove state differs (which never actually happens — included as a
*structural* test of the parity logic, not a state-reachability test).

Step 2: TT structure.

```rust
pub struct TTEntry {
    pub hash: u128,
    pub best_move: Coord,
    pub score: i32,
    pub depth: i8,
    pub flag: TTFlag,
    pub generation: u8,
}

pub enum TTFlag { Exact, LowerBound, UpperBound }
```

Two-bucket scheme: each index slot holds `[depth_preferred, always_replace]`.
Index by low N bits of `hash`. Full `u128` stored for collision verification.

Replacement on store:
- If new entry depth ≥ depth_preferred.depth OR generation differs (aged):
  overwrite depth_preferred. Keep old in always_replace.
- Else: overwrite always_replace.

Aging: increment `current_generation` per root search. Older-generation
entries replaceable regardless of depth.

Size: `tt.size_mb` from `hexo.toml`, default 64 MB. Power-of-two bucket
count derived: `buckets = floor_pow2(size_mb * 1024 * 1024 / 32)` (32 = 2
entries × 16 bytes; tweak if entry layout changes).

### Phase 7 — `ordering`

Stable priority bucket sort. Buckets in priority order:

1. TT best move
2. Winning move (creates 6-in-row)
3. Defensive win (blocks opponent would-be 6-in-row)
4. Completes S0 from stone 1 (only for stone 2 of a turn)
5. Creates S0 threat (open-4 / closed-5 / open-5)
6. Blocks opponent S0 threat
7. Creates S1 threat (open-3 / rhombus / arch / bone)
8. Killer moves at this ply (2 slots)
9. History heuristic
10. Static delta-eval / proximity tie-break

Encoding: `u32 score = (bucket << 24) | history_score`. Sort descending.
`MOVE_GEN_CAP` (from `hexo.toml`, default 24) applied **after** ordering —
never truncate before, the high-priority moves may be late in the unordered
list.

Killers per ply: `[Option<Coord>; 2]`, ring-buffered. Updated on β-cutoff
for non-S0 moves (S0 moves already dominate via bucket 5).

History: global `FxHashMap<(Coord, Player), u32>`. Increment by `depth²` on
β-cutoff (Schaeffer 1989). Decay by ½ between root searches to avoid
runaway accumulation. Read-only during ordering.

### Phase 8 — `search`

Build order:

1. Plain alpha-beta (minimax form) + iterative deepening + TT
2. PVS (null-window + re-search) at depth ≥ 2
3. Aspiration windows at depth ≥ 4 (delta = 50, widen 2×)
4. Killer moves + history heuristic feed `ordering`
5. Late Move Reductions (LMR) at depth ≥ 3, move index ≥ 6, R = 1.
   Disable for TT-move / killer / S0 / S0-block.
6. Threat-only quiescence at depth 0: only S0+ moves considered (creates
   or blocks S0). Hard cap `qsearch_max_plies = 8`.
7. Check extensions: opponent creates S0 threat → extend depth by 1.
   Capped at `max_extensions = 4` per root path.

**Skip in v1**: null-move pruning, MTD(f). Revisit post-baseline.

Time management:
- Per-turn budget split: stone 1 = 60%, stone 2 = 40% (configurable).
- Deadline check every `deadline_check_nodes` nodes (default 4096).
- Soft-fail iterative deepening on timeout: return best move from last
  completed iteration; if none completed, return TT-best-or-first-candidate.
- Aspiration re-search counts against the same deadline; on fail, widen
  before re-searching, do not abort the iteration.

Mate-distance scoring: `MATE_SCORE − ply` for own mate so search prefers
shorter mates; `−(MATE_SCORE − ply)` for opponent mate.

### Phase 9 — `pybind` + Python `Bot` + CLI

PyO3 surface per `SPEC_API.md`. GIL released around `best_move` via
`py.allow_threads(|| ...)`. Python `Bot` wraps `Engine` with `BotConfig`
defaults. Add `Bot.play_turn()` for atomic 2-stone convenience. Add
`Engine.find_pv()` returning principal variation.

CLI commands:
- `hexo play` — interactive REPL vs bot
- `hexo selfplay -n N` — bot vs bot, log games
- `hexo bench` — NPS benchmark
- `hexo analyze <bsn>` — show eval + best line
- `hexo bot` — **subprocess protocol** (needed by Phase 10 harness). Reads
  line-oriented commands from stdin, writes responses to stdout.

Protocol (one command per line):

```
> reset                       < ok
> place Q R                   < ok
> best_move TIME_MS           < Q R
> winner                      < X | O | none
> ply                         < N
> eval                        < SCORE
> quit                        < bye
```

Errors prefix `error: ` and do not terminate the session except for
`quit`.

### Phase 10 — Promotion harness (post-baseline)

Validates that a candidate version is genuinely stronger than the last
validated version before promoting `.bestref`.

Components:
- `.bestref` at repo root: SHA of current `best`
- Git worktree at `.worktree-best/` checked out at `.bestref`
- Per-worktree venv, each engine built as `hexo_engine`
- Subprocess protocol via `hexo bot` (Phase 9)
- `hexo/hexo/benchmark.py` match harness with SPRT / Wilson / raw tests
- `make vs N_GAMES=200` — runs match, reports stats
- `make promote` — advances `.bestref` if threshold met

Default test: SPRT `[0, 5]` Elo, α = β = 0.05. Typically resolves in
60–300 games.

**Do not implement in baseline phases.** Spec at HEXO_CONTEXT § 9 is the
reference; this is Phase 10, after Phase 9 is green and the bot plays
full games via CLI.

## Out of scope for v1

- Null-move pruning (revisit post-baseline)
- MCTS / hybrid
- Neural net eval
- Multi-threaded search (lazy-SMP later)
- Opening book (collect self-play games first)
- Endgame tables (game theoretically a draw with perfect play)
- WebSocket / SealBot live integration (separate workstream)
- Radius-theory colony discounting (post-baseline eval extension)

## References

- Connect-6 + Alpha-Beta-TSS: Wu et al., NCKU/NYCU group
- Yixin engine architecture: TT + PVS + LMR + threat-aware quiescence
- Stockfish: general alpha-beta best practices
- Schaeffer, "The History Heuristic," ICCA Journal 6(3), 1983
- SealBot: github.com/Ramora0/SealBot (closest comparable)
- Research output: see project knowledge for full citation list

## Phase prompt template

Each Phase 4–9 prompt lives in `prompts/PHASE_N_PROMPT.md` and follows
the structure used for Phases 2–3:

```
# Claude Code Prompt — Phase N: <module(s)>

## Response Style
Caveman mode. No articles, no filler. Short. Direct.

## Scope
1. Implement <module>.
2. <Other deliverables>
Out of scope: <list>.

## STEP 0 — Spec & config updates
### 0.1 specs/SPEC_*.md — <section>
### 0.2 hexo.toml — <keys>
### 0.3 build.rs, config.py

## STEP 1..N — <Module(s)>
Types. Operations. Tests.

## STEP N+1 — Verify
make check must pass.

## Hard Rules

## Out of Scope

## When Done
Report:
1. Spec diff summary
2. cargo test --release pass count
3. pytest output
4. Spec ambiguities (do NOT improvise)
5. Phase-specific notes
```
