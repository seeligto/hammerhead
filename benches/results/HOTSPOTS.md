# Hotspots — Phase 24 refresh

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
