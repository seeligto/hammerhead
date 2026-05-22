# Hotspots — Phase 25.5 (Tier 1 sweep — new host)

## Phase 25.5 status (2026-05-22)

Phase 25.5 landed five Tier 1 items (R-03, R-04, R-08, R-05, R-02) from the
SealBot comparison review on a new host (Ryzen 7 3700x). All five accepted.

**Headline:**
- bench-quick midgame_12 @ 500ms: 334k → **344.7k NPS (+3.2%)**.
- macro tt_stats midgame_12 @ 1s: 314k → **355k NPS (+12.9%)**.
- macro tt_stats midgame_30 @ 1s: 231k → **253k NPS (+9.5%)**.
- 200-game match vs prior `.bestref`: 171-29-0 (85.5%), Elo +308 [+240, +376].
- `.bestref` promoted to 932c5d8 (commit `promote: 932c5d8d`).

**Breakdown shift (% of engine self-time, post-Tier-1):**

| Band         | Phase 0 (16e4b82) | Post-Tier-1 (932c5d8) | Δ (pp) |
|--------------|------------------:|----------------------:|-------:|
| eval         |          34.13%   |               34.29%  |  +0.2  |
| board        |          23.38%   |               23.82%  |  +0.4  |
| search_other |          26.60%   |               27.23%  |  +0.6  |
| threats      |           8.63%   |                9.09%  |  +0.5  |
| ordering     |           7.26%   |                **5.56%**  | **−1.7**  |
| tt / moves   |              0%   |                   0%  |   0    |

Ordering dropped 1.7pp — R-02's fused AxisProbe + R-05's partial-sort win.
Other bands held within noise. Renormalisation pushes their fractions
slightly up because the total engine slice shrank.

**Reference node counts: drifted (intentional rebaseline event).**
R-05 changed the priority tie-break from generation order to Coord pack,
and R-08-A removed killer cross-call carryover. Both produce ordering
shape drift → different subtrees explored → different node counts.
`baseline.json` refreshed to the post-Tier-1 numbers.

**Item-level perf attribution** (cumulative bench-quick mean):

| After | bench-quick NPS | Δ vs Phase 0 | Note |
|-------|----------------:|-------------:|------|
| Phase 0 | 334k        |   0%         | post-revert baseline |
| R-03    | 331k        | −0.9%        | no measurable NPS — alloc cost was hidden; rule enforced |
| R-04    | 331k        | −0.9%        | no measurable NPS — SmallVec drop-in |
| R-08    | 332k        | −0.6%        | within noise |
| R-05    | 343k        | **+2.7%**    | first clear win — partial sort + total-order key |
| R-02    | 345k        | **+3.2%**    | fused AxisProbe — no Phase 25 regression |

R-05 + R-02 together account for the entire NPS gain; R-03/R-04 are quality
wins (design rule, code clarity) with no measurable perf impact at this
hardware/budget.

**Flamegraph:** `flamegraph-2026-05-22T17-38-37-432ddba.svg`.

---

## Phase 25 (superseded — kept for historical reference)

Phase 25 shipped **measurement-infrastructure cleanup only** — its three
optimization candidates were all attempted and all reverted (below), so
the **engine source was byte-identical to Phase 24** (`44493f6`) until
Phase 25.5. Phase 25.5 then landed code changes; see above.

**Optimization stream — all three reverted** (under-delivered;
A/B-confirmed by independent subagents; carried to Phase 26 candidates):

- **Bit-parallel `LineBitmap` run scan + line cache** — regressed
  −15/−16 % NPS. The original fully-unrolled 5-iteration `get()` loop
  branch-predicts perfectly on the typically-short runs; the word-walk +
  cache indirection is slower.
- **`threats::compute` per-player piece iteration** — flat (within ±3 %
  noise). The threats cost is the linear run-scan in
  `walk_linear_runs` / `run_endpoints`, not the `pieces()` history
  filter — eliminating the filter moves nothing.
- **`for_each_in_range` precomputed offset tables** — regressed
  −10/−11 % NPS. The bounded `dq/dr` loop is register-resident and
  compiler-unrolled; a flat 217-entry table walk adds memory loads, L1
  pressure, and a `match radius` branch.

Common thread: the engine is compute-bound at IPC 4.38 (§ G) and its hot
loops are already well-formed for the branch predictor and register
allocator — "obvious" table-driven / bit-parallel rewrites lose to the
existing code. Real wins need *algorithmic* work-reduction (the per-line
`LineContribution` cache, Phase 26 candidate #1), not micro-rewrites of
already-tight loops.

**Cleanup stream — all three landed:**

- `bench breakdown` rederived from flamegraph self-time (was
  structurally broken — summed unweighted criterion medians).
- Flamegraph capture frame-pointer mode locked down + documented.
- `tt_stats` enabled for `make bench` / `make bench-baseline`, so
  `baseline.json` now populates `tt_hit_rate` (was `null`). TT hit rate
  midgame_12 d=4/d=6 = 26.7 % / 13.7 %; midgame_30 d=4/d=6 = 14.1 % /
  11.4 % — unchanged from the Phase 24 dedicated capture.

Reference node counts: **32/32 byte-identical** to Phase 24 / Phase 17.

---

## Phase 24 refresh (ranking still current)

**Captured:** 2026-05-21 — engine git `44493f6`, rustc 1.94.0
**Host:** AMD Ryzen 7 8845HS (Zen 4, 8C/16T, 16 MB L3), Linux 7.0.3
**Bench data:** `benches/results/baseline.json` (`make bench`,
`--time-ms 1000 --tt-stats`, `target-cpu=native`, default features
`simd_eval`). Full sweep wall-clock **33 m 24 s**.
**Flamegraph:** `benches/results/flamegraph-2026-05-21T21-44-40-69e2053.svg`
— frame-pointer capture of `bench_search` (`perf --call-graph fp`).
**Full investigation:** `subagents/reports/phase24-perf-investigation.md`
(953 lines — rankings, sub-rankings, TT/memory/microarch detail, the
Phase 25 candidate ranking). This file is the executive summary; the
Phase 17 ranking it replaces is discarded.

## Headline numbers (Δ vs Phase 17)

| Metric | Phase 17 | Phase 24 | Δ |
|---|---:|---:|---:|
| NPS, `midgame_12`, t = 1 s | 433,813 | **532,480** | **+22.7 %** |
| NPS, `midgame_30`, t = 1 s | 314,297 | **401,569** | **+27.8 %** |
| NPS, `empty`, t = 1 s | 671,364 | **828,710** | **+23.4 %** |
| NPS, `single_origin`, t = 1 s | 679,270 | **836,890** | **+23.2 %** |
| Depth @ 1 s, `midgame_12` | 5 | 5 | — |
| Depth @ 1 s, `midgame_30` | 6 | **7** | **+1** |
| `cached_eval_cold`, `midgame_30` | 5.40 µs | **2.97 µs** | **−45.0 %** |
| `threats::compute_full`, `midgame_30` | 2.71 µs | **1.62 µs** | **−40.1 %** |
| `eval::layer1_window_scan`, `midgame_30` | 0.85 µs | 0.81 µs | −4.6 % |
| `board::place`, `midgame_30` | 1.22 µs | 1.60 µs | noise (MAD ≫ Δ) |
| TT hit rate, `midgame_12` d = 4 / d = 6 | n/a | 26.7 % / 13.7 % | (new) |

NPS is up **+23–28 % on every fixture** and `midgame_30` gained a depth
ply (6 → 7). The win is the Phase 20 S1/S2-detection removal (the
−40–58 % `threats::compute` / `cached_eval` collapse) plus the Phase 22
dead-code subtraction. **Reference node counts are 32/32 byte-identical
to Phase 17** — every gain is pure throughput; Phases 18–23 moved no
search behaviour. Strength smoke: current HEAD beats the (pre-Phase-17)
`.bestref` 16-4-0 at 20 g / 300 ms — healthy, no regression.

## Hotspot ranking (flamegraph self-time + criterion cross-check)

Frame-pointer `perf report` self-time. Percentages are of the whole
`bench_search` capture; the engine search is ~63 % of it (the rest is
the criterion harness + rayon KDE analysis). `≈ engine` renormalises.

1. **`eval` / Layer-1 window scan** — `eval::eval` 14.7 % +
   `LineBitmap::windows8_run` 5.0 % ≈ **31 % of engine**. The per-leaf
   8-cell window scan over every populated line; AVX2 ternary encode is
   inlined into `eval` under `target-cpu=native`. No per-line
   memoisation — re-scanned in full at every leaf.
2. **`threats::compute_with_scratch`** — 12.9 % ≈ **21 % of engine**.
   Full two-player threat recompute on every dirty `Board::threats()`
   read; ~40 % cheaper than Phase 17 (S1/S2 matchers gone) but still #2.
3. **`ordering` predicates** — `would_make_six` 7.1 % + `creates_s0`
   5.8 % ≈ **20 % of engine**. ±5-cell run scans, one `LineBitmap::get()`
   at a time, re-walked per candidate move and again in the qsearch
   `is_threat_move` frontier.
4. **`for_each_in_range` / proximity** — 11.6 % ≈ **18 % of engine**.
   `Board::place`/`undo` walking the r=8 (~217-cell) + r=2 neighbourhood
   to maintain the flat proximity fields.
5. **search recursion** — `pvs_node` + `quiescence_node` self ~3.8 % ≈
   **6 % of engine**. Thin orchestration; healthy. (`quiescence_node` is
   ~47 % *inclusive* — quiescence is where the engine lives.)

`perf stat`: IPC **4.38**, branch mispredict **0.35 %**, LLC miss
**2.9 %** — the engine is **compute-bound**. Phase 25 should cut work,
not chase cache locality.

## Dropped out since Phase 17

- **TT probe / store** — Phase 17 #5; now < 0.5 % self-time, off the
  list. The TT is 98 % empty after a 1 s search with < 1 % collisions;
  its probe latency is hidden by the out-of-order core. The "4-bucket
  TT" candidate is **dead** — it solves a non-problem.
- **Cross-axis S1/S2 matchers** — the bulk of the old `threats::compute`
  cost; deleted in Phase 20. The threats path is now a single linear-run
  scan.
- **`window6`, `single_cell_blocks_all`** — micro-benches removed with
  the code in Phase 22 (218 → 200 criterion entries).
- **`extension_factor` runtime multiply** — folded into the 8-cell
  `WINDOW_SCORE_8` table back in Phase 17.

## Phase 25 entry point

**Top candidate: bit-parallel `LineBitmap` run scan + shared line-lookup
cache.** `run_backward`/`run_forward` loop up to 5 `get()` calls, each
re-deriving the word/bit index — replace with one masked `u64` read
(`trailing_ones`/`leading_zeros`). Speeds `would_make_six`, `creates_s0`,
`run_endpoints` (threats) and win detection at once; byte-identical
results → reference-node-count-safe. Est. **+5–9 % NPS**, low risk, a
single-prompt quick win.

Strong follow-on: a per-line `LineContribution` cache for Layer 1
(turns the full per-leaf line re-scan into a ≤3-line delta, ~+8–15 %,
high difficulty). Full ranking + rationale in
`subagents/reports/phase24-perf-investigation.md § K`.

## How to refresh this report

1. `make bench BENCH_TIME_MS=1000` → `cp benches/results/<isodate>-<sha>.json
   benches/results/baseline.json`.
2. `make flamegraph` (frame-pointer capture) → new
   `flamegraph-<date>-<sha>.svg`; update the `!`-exception in
   `.gitignore` to keep it; `git rm` the superseded SVG.
3. `perf report --no-children -i <perf.data>` for authoritative
   per-function self-time (the inferno folded stacks are FP-shallow).
4. TT diagnostics need a `maturin develop --release --features tt_stats`
   build (the production build records `tt_hit_rate: null`).
5. Rewrite this file; discard the prior ranking. Long-form investigation
   goes under `subagents/reports/`.
