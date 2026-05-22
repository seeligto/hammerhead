# Hotspots — Phase 27 (LineContribution cache)

## Phase 27 status (2026-05-22)

Phase 27 landed the LineContribution cache: per-`(axis, line_id)` memoization
of the Layer-1 window-scan contribution, with invalidation on every
`Board::place` / `undo` / `place_for_test`. The cache attacks the eval
band identified as the top algorithmic lever in the Phase 25 retro
candidate list (36.87% of engine self-time at the Phase 26 91f8114
flamegraph).

**Headline (commit `05cecb3`):**
- bench-quick midgame_12 @ 500ms (3× cold mean): 451k → **537k NPS (+19.1%)**
  vs Phase 26.5 entry baseline (`357153f`, fresh build).
- bench-quick mean cumulative vs `.bestref` (`932c5d8`, Phase 25.5):
  354k → **557k NPS (+57.1%)** at 1000ms macro.
- Reference node-count parity: BYTE-IDENTICAL across all 32 (fixture, depth)
  cells vs Phase 26.5 baseline. Cache is a pure refactor of Layer-1; eval
  output unchanged.
- depth_at_time @ 1000ms: midgame_30 6 → **7** (one extra ID iteration).
- `eval::layer1_window_scan` micro: **−86% to −93% wall-time** across all
  fixtures (cache-hit path is a single load + compare vs the full
  window-scan recompute).

**Flamegraph breakdown (% engine self-time):**

| Module        | Phase 26 (`91f8114`) | Phase 27 (`05cecb3`) | Δ          |
|---------------|---------------------:|---------------------:|-----------:|
| eval          | 36.87%               | **26.55%**           | **−10.32** |
| board         | 25.24%               | 31.51%               | +6.27 (renorm) |
| search_other  | 23.74%               | 24.58%               | +0.84      |
| threats       |  9.98%               | 12.43%               | +2.45 (renorm) |
| ordering      |  4.18%               |  4.93%               | +0.75      |
| tt / moves    |  0.00%               |  0.00%               | —          |

The 10.32-pp drop in eval band is the entire Layer-1 cacheable fraction
materializing (I-HOTPATH projected 26.79% of engine; observed conversion
matches the projection minus L1/TLB pollution). Other bands renormalize
upward as the cache shrinks total time. **board (31.51%)** is now the
single largest band — the Phase 28 hand-off lever is the
search-internal proximity skip on `board::*` hot calls.

**Cache design (committed):**
- Storage: flat `Box<[i32]>` of `3 * LINE_ID_RANGE = 1527` entries
  (~6 KB; fits L1). Index = `axis as usize * LINE_ID_RANGE + line_id`.
  Single signed `i32` per slot (`WINDOW_SCORE_8` already folds X/O into
  one signed scalar — no per-player dimension).
- Dirty marker: sentinel `i32::MIN`. Hot-path read = one bounds-checked
  load + compare; no parallel dirty bitmap.
- Lifetime: owned by `Board` behind `RefCell<LineContrib>`, mirroring
  the `threats_x` / `threats_o` pattern.
- Init/reset: `Board::new` sentinel-fills; `Board::reset` re-fills via
  `slice::fill`, no realloc.
- Invalidation hook: `Board::apply_set` / `Board::apply_clear` helpers
  factor `axes.set/clear + invalidate_coord(c)` together. All three
  mutation sites (`place`, `undo`, `place_for_test`) funnel through
  these helpers. ≤3 cache entries invalidated per mutation (Q, R, S
  lines through the coord).
- Eval consumer: `layer1_window_scan_8cell` takes a single
  `borrow_mut()` for the whole scan, calls `cache.get` per
  `(axis, line_id)` in the X∪O populated union (SmallVec dedup
  preserved), recomputes via `scan_line_8cell` on sentinel + writes
  back. `scan_line_8cell` body untouched.

**Commits (4 atomic, all on master):**

| SHA       | Subject                                                          | Behavior change |
|-----------|------------------------------------------------------------------|-----------------|
| `228a3d3` | `specs: document LineContribution cache (Phase 27)`              | none (spec)     |
| `daa8fe1` | `eval: add LineContribution cache scaffold (Phase 27)`           | none (unconsumed) |
| `1436735` | `board: invalidate LineContribution on place/undo (Phase 27)`    | none (hook only, no reader) |
| `05cecb3` | `eval: consume LineContribution cache in Layer-1 (Phase 27)`     | NPS gain, byte-identical eval |

**Verification protocol (per dispatcher gate):**
- Per-step A/B: bench-quick 3× cold + reference node-count parity at
  32 cells. All four commits passed byte-identical parity.
- C-01 A/B initial verdict was a FALSE-POSITIVE REVERT caused by a
  stale `.so` from a pre-session build — the recorded Phase 0
  "baseline" was tainted. Corrected baseline at `357153f` matches
  C-01 / C-02 / C-03 byte-identical. **Lesson: always `make build`
  immediately before recording any benchmark baseline.**
- Cache-coherence audit (review of C-03): only 4 axes write sites in
  the engine (`Board::new`, `Board::reset`, `apply_set`, `apply_clear`).
  All paired with `line_contrib` invalidation. No bypass paths.
- C-03 100g sanity match vs `.bestref` (`932c5d8`):
  **42-58-0, Elo −56.1 [−124.6, +12.5]**, CI straddles zero.
  Phase 26.5 meta-finding holds: 100g cannot resolve sub-25 Elo. The
  point estimate reflects 19 commits of Phase 26 + 26.5 work, not
  Phase 27 itself (which is byte-identical at fixed depth — see
  parity above). 400g promote-match is the strength gate.

**400g promote-match vs `.bestref` (`932c5d8`):**
- **215-184-1 W-L-D, score 53.87%, Elo +27.0, CI95 [−7.1, +61.1]**.
- Verdict: **REJECT** (raw test, gates on CI lower > 0; −7.1 < 0).
- `.bestref` NOT promoted — stays at 932c5d8.

The point estimate moves from Phase 26 R-01's −17.4 Elo (200g) and
Phase 27's own 100g sanity result of −56.1 to **+27.0 at 400g**, with
the upper CI band reaching +61. The 19-commit gap (Phase 25.5 →
Phase 27) is no longer Elo-negative on aggregate, but the lower CI
band still touches negative territory and the dispatcher's strict
`CI lower > 0` gate is not met. Sample-size analysis: 400g at 500 ms
gives CI width ~±34 Elo per Phase 26.5 meta-finding; the observed
width (+34) confirms harness behavior. A 600-800g rerun could
plausibly clear the bar, but the marginal cost-benefit at this Elo
magnitude is poor — defer to a follow-up phase if the next algorithmic
lever lifts the point estimate further.

**Cumulative Phase 26+26.5+27 vs `.bestref`:** NPS up ~+57% on
midgame_12 macro, Elo +27 CI [−7, +61]. The Phase 26.5 meta-finding
predicted that further NPS gains alone might not crack the promotion
threshold at affordable sample sizes; Phase 27 confirms this — a
+19% NPS gain (Phase 27 isolated) on top of +21.8% (R-01) on top
of Tier-1 (Phase 25.5) lands with marginal-positive Elo signal.

**Phase 28 hand-off:**
- **board (31.51%)** is the new top band. Search-internal proximity
  skip is the natural lever — bench shows `board::place` -4% to -26%
  on isolated micro, but real-search board calls dominate the
  proximity-update work. Investigation should start with
  `proximity.rs` and `Board::is_legal_internal` / `place` hot paths.
- **threats (12.43%)** sits at #3. R-09 was DROPPED in Phase 26 retro
  (both-side consumption dominates per-side cacheability). Phase 28+
  could revisit if a different per-line invalidation pattern emerges.
- **eval (26.55%)** post-cache is dominated by `scan_line_8cell`
  recompute on the ≤3 invalidated lines per move + Layer-2 shape
  fold + Layer-3 fork detection. The cache fundamentally cannot
  attack these; they are per-move work. Further eval gain requires
  a different lever (SIMD widening, per-window precomputation, etc.).

**Match harness reminder (from Phase 26.5):** 500ms × 200g CI width is
~±48-67 Elo. Per-step A/Bs should remain at 100g sanity-only.
Promote-matches require ≥400g to resolve sub-25 Elo deltas. Phase 28
should plan around this constraint.

---

# Hotspots — Phase 26 (Tier 2 sweep)

## Phase 26 status (2026-05-22)

Phase 26 ran R-09 (per-player threat dirty flags) and R-01 (staged TT/killer
movegen) from the SealBot review Tier 2 candidates. R-09 deferred at the
investigation stage on a weakened-premise signal; R-01 landed as a single
commit (prereqs R-01a / R-01b collapsed into existing helpers).

**Headline (R-01, commit `91f8114`):**
- bench-quick midgame_12 @ 500ms (3× cold mean): 373.3k → **454.7k NPS (+21.8%)**.
- bench-quick ID depth at 500ms: 4 → **5** (one extra ID iteration in the same budget).
- macro midgame_12 @ 1000ms: 354,851 → **440,320 NPS (+24.1%)**.
- macro midgame_30 @ 1000ms: 253,465 → **286,720 NPS (+13.1%)**.
- depth_at_time @ 1000ms: midgame_12 = 5 (unchanged), midgame_30 = 6 (unchanged).
- 200-game promote-match vs `.bestref` (932c5d8): **95-105-0 (47.5%)**, Elo
  **−17.4 [95% CI: −65.4, +30.7]**, SPRT llr=−0.329 (bounds ±2.944) →
  INCONCLUSIVE. SPRT did not REJECT, but the lower CI bound is
  below zero, so the Phase 3 promote criterion (`CI ≥ 0`) is NOT met.
- **`.bestref` NOT promoted** — stays at 932c5d8. R-01 commits remain on
  master as the Phase 26 outcome; future Phase 27 work runs on the faster
  baseline and will get a fresh promote opportunity if strength improves.

R-01 is therefore a **NPS-positive, Elo-flat refactor.** The bench-quick
+1 ID depth gain at 500ms does not translate to strength at 200 games —
the staging reordering (killers before bucket-5/6/7 tactical moves)
appears to cost about as much accuracy as the deeper search gains back.

**R-09 deferred — investigation outcome:**

Empirical signal on midgame_12 @ 500ms (single search_root, 73,728 nodes,
counter instrumentation reverted before commit): **88.8% of reconcile
episodes touch BOTH sides** before the next dirty-flag mark; only 11.2%
single-sided. Projected NPS ceiling for per-player dirty flags: ~0.5-1%.
A single stone-place invalidates BOTH sides because opponent stones flip
own runs' open/closed classification (`threats.rs:167-169`). Eval reads
both sides unconditionally (`eval.rs:58-59`).

R-09 not implemented; the investigation result alone is the Phase 26
deliverable for the item.

**Phase 27 priority decision** (per dispatcher's tree, R-09 noise band +
R-01c NPS > +3%):

- **LineContribution cache (eval band 34.3%)** — primary Phase 27 lever.
- **Threats classification cache** — DROPPED from candidate list. R-09's
  signal showed both-side consumption dominates, so per-side caching has
  no headroom on a 9.1% band.
- **Search-internal proximity skip (board 23.8%)** — hold for Phase 28.

**Reference node counts** (depth-fixed; from `bench reference` at HEAD
`91f8114` vs `baseline.json` at 132d7ac, captures R-01 alone):

| Fixture       | Depth | Baseline | R-01    | Delta   |
|---------------|------:|---------:|--------:|--------:|
| midgame_12    |     1 |      341 |     341 |      0  |
| midgame_12    |     2 |    1,186 |     559 | **−52.9%** |
| midgame_12    |     3 |    7,424 |   6,634 |  −10.6% |
| midgame_12    |     4 |   29,403 |  24,986 |  −15.0% |
| midgame_30    |     1 |      140 |     140 |      0  |
| midgame_30    |     2 |      622 |     622 |      0  |
| midgame_30    |     3 |    2,375 |   2,395 |   +0.8% |
| midgame_30    |     4 |    4,091 |   3,962 |   −3.2% |

7/8 rows identical or improved. The single +0.8% blip at midgame_30 d3
is within killer-reorder subtree variance — acceptable per prompt.

**Bench artefact:** `benches/results/20260522-173433-91f8114.json`
(NOT promoted to `baseline.json` because `.bestref` did not advance).

**Breakdown shift (% of engine self-time):**

| Band         | Post-Tier-1 (932c5d8) | Post-Phase-26 (91f8114) | Δ (pp) |
|--------------|----------------------:|------------------------:|-------:|
| eval         |               34.29%  |                 36.87%  |  +2.6  |
| board        |               23.82%  |                 25.24%  |  +1.4  |
| search_other |               27.23%  |                **23.74%**  | **−3.5**  |
| threats      |                9.09%  |                  9.98%  |  +0.9  |
| ordering     |                5.56%  |                **4.18%**  | **−1.4**  |
| tt / moves   |                   0%  |                     0%  |    0   |

`ordering` and `search_other` together dropped ~5pp — the staged TT-cutoff
fast path skips `order_moves_with_buckets` on ~89-95% of nodes and avoids
the candidate-buffer fill + sort on the cutoff path. Renormalisation
pushes `eval`, `board`, and `threats` fractions up because the total
engine self-time slice shrank.

**Flamegraph:** `flamegraph-2026-05-22T20-11-06-91f8114.svg`.

---

# Hotspots — Phase 26.5 (ordering quality investigation + tuning sweep)

## Phase 26.5 status (2026-05-22)

Phase 26.5 was a measurement-heavy diagnostic phase. Goal: explain why
Phase 26 R-01 was NPS-positive (+21.8%) but Elo-flat at 200g, then ship
any ordering / LMR / aspiration / history parameter the diagnosis
surfaced. The phase added a feature-gated `ordering_stats` counter
surface, ran 7 parallel investigators against a fixed fixture set,
attempted 3 tuning A/Bs, and ended with **no `.bestref` change**.

**`.bestref` NOT promoted** — stays at 932c5d8. Net behavior delta vs
Phase 26 entry HEAD (`2cd2ba6`) is **zero in production builds**: only
behavior-affecting addition is the `ordering_stats` feature flag, which
is off by default and zero-cost when off.

### Diagnosis (from Phase 1 investigation cohort)

Seven parallel investigators against midgame_12, midgame_30,
single_origin (and endgame_60 for I-BUCKETS):

| Hypothesis | Investigator | Verdict |
|------------|--------------|---------|
| H1 — Killer stage cuts before win/block | I-CUTOFF, I-KILLER | **Supported.** b3 cuts dominate b9 cuts 1.4×–29.3× across fixtures; midgame_30 extreme b3=14684 vs b9=20. |
| H2 — LMR mis-tuned post-Phase-17 | I-LMR | **Partial.** Re-search rate fixture-split: 3 fixtures <5% (over-conservative), midgame_30 20.75%. Weighted mean 10.92% — below 15-30% sweet spot. |
| H3 — History decays too fast | I-HISTORY | **Weak.** b1 cut precision 0.37-1.05%, killer 45-56%. History does little, but bucket-1 is the leftover pool — signal ceiling capped by population. |
| H4 — Aspiration untuned since Phase 8 | I-ASP | **Supported.** 65% first-attempt fail rate at depth ≥4; fail-high 40% > fail-low 25%; 22.7% promote to full window. (Correction: untouched since Phase 8, not Phase 22.) |
| H5 — Bucket consolidation needed | I-BUCKETS | **Cosmetic only.** Empty bucket slots {0, 2, 4} confirmed inert across all fixtures. No behavior delta from renumbering. |

I-VERIFY audited citations across the cohort and ranked candidates by
expected Elo × confidence × scope-fit. Top 5: T-04 (asp window),
T-03 (lmr reduction), T-07 (killer slots), T-01 (Stage 1.5 forced
tactical), T-05 (asp widen). Dropped at audit: T-02, T-06, T-08.

### Phase 2 A/B results

All three attempted candidates landed CI-straddles-zero. Per dispatcher
rule "inconclusive → revert (don't ship inconclusive tuning)":

| ID | Change | bench-quick Δ | Match (n, Elo, CI 95%) | Verdict |
|----|--------|---------------|------------------------|---------|
| T-04 | `asp_window_initial` 50 → 100 | −2.0% (448k vs 457k) | 100g: −20.9, [−88.7, +46.9] | reverted |
| T-07 | `killer_slots` 2 → 1 | **+5.5%** (482k vs 457k) | 200g: +6.9, [−41.1, +55.0] | reverted |
| T-03 | `lmr_reduction` 1 → 2 | −6.1% (429k vs 457k) | 100g: +13.9, [−53.8, +81.6] | reverted |

T-07 specifically reproduces the Phase 26 R-01 pattern: NPS-positive,
Elo-flat. T-04 and T-03 both have measurably worse NPS without strength
signal.

### Meta-finding: match harness resolution floor

At 500 ms × 100g, 95% CI width is ~±67 Elo. At 200g, ~±48 Elo. None of
the three candidates' true Elo delta clearly exceeds ±25 Elo, so all
three sit below the harness's resolution. The remaining ranked
candidates (T-05 conditional on T-04 that didn't ship; T-01 magnitude
bounded ≤1% Elo by I-KILLER's displacement arithmetic) have expected
gains under the same floor; further A/Bs would consume match time
without recovering signal at this control. **Phase 26.5 stopped at 3
attempts of the 8-attempt cap.**

The bench-quick NPS measurements at the three accepted candidates
provide an internal check: T-07 (+5.5% NPS) and T-04 (−2.0% NPS) and
T-03 (−6.1% NPS) are all real engine-side changes. The match harness
cannot distinguish those magnitudes from noise.

### Phase 27 hand-off

- **LineContribution cache (eval band 36.87%)** — still the primary
  Phase 27 lever, unchanged by 26.5.
- **Ordering surface tune was attempted and parked.** The diagnostic
  findings remain valid for any future Tier-3 work; if a strength
  signal materializes from Phase 27 LineContribution (which alters NPS
  and depth reach), the ordering candidates may resurface from a
  different operating point. The harness needs ≥400g per A/B to
  resolve sub-25-Elo deltas; bookmark this as a constraint when
  planning future tuning phases.
- **`ordering_stats` feature instrumentation kept** — it remains
  available for future diagnostic passes at zero cost when off.

**Commits:**
- `418afcb` — instrumentation (kept).
- `678f131` (T-04) reverted by `9dc0527`.
- `086ac5c` (T-07) reverted by `5035a80`.
- `f1b916e` (T-03) reverted by `79f96b0`.
- Net production code delta vs entry: instrumentation only.

---

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
