# Hotspots — Phase 12 baseline

**Captured:** 2026-05-19 — git `e24b5ed`, rustc 1.94.0
**Host:** AMD Ryzen 7 8845HS (16 cores), Linux 7.0.3-arch1-2
**Source:** `benches/results/20260519-092352-e24b5ed.json` (`make bench` with
`--features tt_stats`, `--time-ms 1000`).
**Capture method note:** `perf` and `cargo-flamegraph` were not available
in the Phase 12 host environment (kernel `perf_event_paranoid = 2`, no
root). The ranking below is derived from objective measurements —
criterion micro-bench medians, macro NPS, and threat-latency micro-
benchmarks. Future captures via `make flamegraph` (script in
`scripts/flamegraph.sh`) should be cross-referenced against this list.

## Headline numbers

| Metric | Value | Target |
|---|---|---|
| NPS, `midgame_12`, t=1000 ms | **234,933** | >200k (met) |
| NPS, `midgame_30`, t=1000 ms | 126,480 | — |
| Depth reached, `midgame_12` @ 1 s | 5 | — |
| Depth reached, `midgame_30` @ 1 s | 6 | — |
| `cached_eval_cold`, `midgame_30` | 8.4 µs | <10 µs (met) |
| `cached_eval_warm`, all fixtures | 0.06 µs | <0.1 µs (met) |
| Threat latency cold, `midgame_30` | 6.64 µs | — |
| Threat latency warm, `midgame_30` | 0.06 µs | (cached read) |
| TT hit rate, `midgame_12` depth=6 | 15.08 % | — |
| TT hit rate, `midgame_30` depth=6 | 23.30 % | — |
| `place`, `endgame_60` | 6.7 µs | <500 ns (MISS — 13× over) |
| `place`/`undo` roundtrip, `endgame_60` | 7.7 µs | <1 µs (MISS) |

The `place` target in `SPEC_ROADMAP.md` (<500 ns) was for an
isolated-bitmap design that pre-dated incremental threat caching and
zobrist parity. The current `place` includes board mutation + axis
bitmap + zobrist + parity overlay + threat-set invalidation. Either
the target is stale (most likely) or there's room to claw back. Note
for Phase 13: re-evaluate the target before chasing it.

## Top 5 hotspots (impact × difficulty)

Ranked by `median_ns × estimated calls-per-search`. Calls/search is
inferred from `(nodes, depth)` for `midgame_30` at 1000 ms (~53k
nodes, depth 6): every node generates moves, evaluates leaves, probes
+ stores TT, runs through ordering once.

### 1. Move generation — `moves::generate(r=8)` (full-legality scan)

- **Cost:** 7 µs (midgame_12) → 16 µs (midgame_30) → 30 µs (endgame_60)
- **Frequency:** O(nodes). For a 53k-node search at midgame_30, full-
  legality scans add up to ~850 ms of pure move-gen if every node
  pays the r=8 cost.
- **Why it's expensive:** at r=8 the legality grid covers ~196 cells.
  The function visits every populated cell and checks hex-distance.
- **Optimization candidates** (Phase 13):
  - Restrict full-legality scan to opening-radius or anti-colony
    extensions only — most internal nodes use the r=2 default.
  - Cache the legality bitmap per-position keyed off TT hit.
  - Lower `MOVE_GEN_INNER_RADIUS` for deeper plies (LMR already
    reduces depth; move radius could mirror).
- **Difficulty:** medium. Move-gen has well-defined inputs but is
  shared across search and qsearch.

### 2. Cold eval / threat recompute — `eval::cached_eval_cold` and `threats::compute`

- **Cost:** `cached_eval_cold` median 8.4 µs at `midgame_30`, 8.7 µs
  at `endgame_60`. Threats alone: 6.64 µs cold at `midgame_30`.
- **Frequency:** every leaf + every node where the threat cache is
  invalidated by `place`/`undo`. With ~53k nodes and 8 µs each, this
  is ~425 ms — the single biggest cost centre.
- **Why it's expensive:** full threat recompute scans every line in
  every axis. `ThreatSet` accepts `place_center` / `prior` args but
  ignores them (see `SPEC_ROADMAP.md § Known follow-ups` —
  "Incremental threat recompute, Phase 4 ships full recompute on
  every dirty read").
- **Optimization candidates** (Phase 13):
  - **Incremental threat recompute** — the deferred item from Phase
    4. ThreatSet already records the cause-of-invalidation; use it.
  - Reuse threats across siblings (move N+1 differs from N by a
    single stone — re-run threats only on affected axes).
- **Difficulty:** medium-high. Requires correctness checks against
  the current full recompute as a reference oracle.

### 3. TT — low hit rate at midgame

- **Cost:** probe ~250 ns, store ~6 µs. The probe is cheap per call
  but happens at every node.
- **Frequency / waste:** hit rates of 15 % (midgame_12 d=6) and 23 %
  (midgame_30 d=6) mean ~80 % of probes find a non-matching bucket
  and trigger a full re-search. Collisions at midgame_30 d=6 were
  measurable (see baseline JSON).
- **Why:** two-bucket (depth + always-replace) at 64 MB. Bucket-fill
  is healthy (`occupied ≈ 0.1 %`) so the bottleneck isn't size — it's
  index distribution and migration policy.
- **Optimization candidates** (Phase 13):
  - 4-bucket cluster (probe four entries on miss). Stockfish-style.
  - Better hash distribution — currently `(hash as u64 as usize) &
    mask`; xor-fold the upper 64 bits before masking.
  - Cluster co-residency: align bucket pairs to 64-byte cache lines.
- **Difficulty:** low (re-tuning) to medium (4-bucket).

### 4. Move ordering — `ordering::order_moves`

- **Cost:** 6.3 µs at `endgame_60`, ~3–5 µs typical.
- **Frequency:** once per non-leaf node — ~25k calls at midgame_30
  d=6.
- **Why it's expensive:** scores every candidate via bucket lookup +
  history table + killer comparison + virtual-place axis-run probe.
  The score function dominates the cost (verified by the per-fixture
  drift: position with deeper move queues is slower).
- **Optimization candidates** (Phase 13):
  - Two-pass ordering: cheap-score the first 8, full-score the rest
    only if we explore past index 8.
  - SmallVec-only path — currently inlines move list capacity 24,
    pedantic-allocs above that. Trim allocation paths.
- **Difficulty:** low.

### 5. Place / undo roundtrip — `board::place` + `board::undo`

- **Cost:** 6.7 µs `place` + 1.0 µs `undo` ≈ 7.7 µs roundtrip
  (endgame_60). Lighter fixtures see 3–5 µs.
- **Frequency:** every search recursion. At 53k nodes × ~8 µs that's
  ~420 ms — comparable to threat cost. Many of these cycles are
  inside ordering's virtual-place probes, not the search itself.
- **Why it's expensive:** the roundtrip includes axis-bitmap mutation
  + zobrist XOR + parity-overlay + threat-set invalidation. The
  threat invalidation is the highest unit cost.
- **Optimization candidates** (Phase 13):
  - Make `cached_eval` lazy-on-read so `place`/`undo` cycles that
    never observe eval don't pay the invalidation cost (ordering's
    `creates_s0` probe is the main offender — it does `place ; check
    ; undo` per candidate).
  - Replace virtual-place probes with direct axis-bitmap scans —
    `creates_s0` can be checked without touching the board.
- **Difficulty:** medium.

## Phase 13 entry point

Strongest signal: **incremental threat recompute** (#2 above). It
removes a ~425 ms cost centre on a 1-second search and unblocks #5
(virtual-place no longer triggers full threat invalidation). Expected
NPS lift: 20–35 % on midgame_30.

Second priority: TT hit-rate work (#3). A 4-bucket layout typically
brings a 1.5–2× hit-rate improvement; with current ~20 % rate that
translates to roughly +10 % NPS via avoided re-search.

Move-gen (#1) and ordering (#4) are lower-priority refinements.

## How to refresh this report

```bash
# 1. Build with stats enabled (so reference table has hit-rate)
cd hexo-engine && maturin develop --release --features tt_stats

# 2. Capture a flamegraph SVG (Linux: requires `perf` + paranoid<=1)
make flamegraph

# 3. Run the full bench sweep
make bench BENCH_TIME_MS=1000

# 4. Diff against current baseline
make bench-diff A=baseline B=<latest-isodate-sha>

# 5. Re-rank the top 5 here based on observed deltas.
```
