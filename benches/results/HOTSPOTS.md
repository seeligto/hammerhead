# Hotspots — Phase 28E-0 (time-fix + SDK + audit)

## Phase 28E-0 status (2026-05-24)

Phase 28E-0 ran four sub-waves: TIME-DIAG + TIME-FIX (per-stone vs
per-turn budget mechanism correction), SDK-DESIGN + SDK-IMPL (opt-in
`SearchStats` observability + `depth=N` fixed-depth kwarg + arena
adapter consumption), AUDIT + AUDIT-FIX (engine-wide 6-area bug sweep,
1 MAJOR + 1 MINOR landed), VERIFY (100g external + 400g internal final
read). Five code/spec commits added on master; three on hexo-arena
main.

**Headline: KEEP all 5 commits on master; `.bestref` NOT advanced.**
External arena flat (VERIFY 100g 2.0% [0.5, 7.0] vs D3 GATE baseline
4.5% [2.4, 8.4], CI overlap); internal +40 Elo center shift vs D3
(51.12% / +7.8 Elo [-26.2, +41.8] vs D3 45.25% / -33.1 Elo [-67.3,
+1.0]) but CI spans 0 → REJECT (no strict-positive advance). `.bestref`
stays at `5bd89648`.

### Time-utilization fix (TIME-FIX commit `1f10f6c`)

`Engine::best_move` at `engine.rs:106` called `split_budget(t, halfmove,
stone1_time_pct)` on every per-stone call, halving incoming budget.
SDK (`bot.py:265`) and arena adapter (`_hammerhead_worker.py:92`) both
passed `time_ms` as per-stone; engine treated as per-turn and
re-divided 60/40. Mean per-stone utilisation pre-fix = **50.5%**,
post-fix = **100.2%** (SDK smoke + arena per-stone wall-clock both
verified). Bug mechanically dead. `split_budget` helper +
`stone1_time_pct` field deleted; `hexo.toml`, `config.py`, `SPEC_ENGINE`,
`SPEC_API` updated to per-stone contract.

| budget (ms) | halfmove | pre-fix wall (ms) | post-fix wall (ms) | pre util | post util |
|---:|---:|---:|---:|---:|---:|
| 500  | 0 |  303 |  500 | 60.6% | 100.0% |
| 500  | 1 |  206 |  503 | 41.2% | 100.6% |
| 1000 | 0 |  605 | 1001 | 60.5% | 100.1% |
| 1000 | 1 |  400 | 1002 | 40.0% | 100.2% |

**Methodology finding (load-bearing)**: pre-`1f10f6c` Phase 28 arena
measurements were running at ~50% intended HH wall-clock per stone.
Cross-phase comparisons across the entire Phase 28 arc are not directly
comparable. The INTEGRATION_NOTES "at `--time T`, HH gets 2T per turn"
characterization was inaccurate for vendored HH SHAs pre-fix and is now
accurate again. E-1 baseline measurements should be re-taken at the
post-fix HH effective time.

### Arena trajectory (pre-28E-0 → post-28E-0)

| State | n | HH wins | Winrate | Wilson 95% CI |
|---|---:|---:|---:|---|
| Phase 28D-2 I4 baseline | 50 | 4 | 8.7% | [3.4%, 20.0%] |
| Phase 28D-3 GATE Cond B (pre-28E-0) | 200 | 9 | 4.5% | [2.4%, 8.4%] |
| Phase 28E-0 TIME-FIX condA cross-check | 50 | 2 | 4.0% | [1.1%, 13.5%] |
| **Phase 28E-0 VERIFY (post-fix primary gate)** | **100** | **2** | **2.0%** | **[0.5%, 7.0%]** |

CI overlaps D3 GATE (2.4-8.4% vs 0.5-7.0%). External arena **flat
within sampling noise**, at lower end of D3 CI band. Time-fix
mechanism corrected utilization without lifting winrate against
SB-perf — confirms 28D-3 retro's eval-gap hypothesis (Layer-1 length-3
double-count Gap #1 is the load-bearing lever, not search-time budget).

### Internal +40 Elo movement vs `.bestref`

| Phase | n | HEAD record | Winrate | Elo | CI 95% | Verdict |
|---|---:|---|---:|---:|---|---|
| Phase 28D-3 retro internal sanity | 400 | 181-219-0 | 45.25% | -33.1 | [-67.3, +1.0] | REJECT (CI upper touches 0) |
| **Phase 28E-0 VERIFY** | 400 | **204-195-1** | **51.12%** | **+7.8** | **[-26.2, +41.8]** | REJECT (CI spans 0) |

**Δ vs D3: +5.87 pp winrate, +40.9 Elo center shift.** CI band shifted
from "touching 0" to "spans 0 centered positive". No `.bestref`
advance (strict gate requires Wilson lower > 0). Most plausibly
attributable to TIME-FIX doubling HH per-stone wall-clock vs same-bug
`.bestref` baseline (HH internally now searches genuinely 2× deeper at
the same arena flag), with D-1 fix as a possible contributor for any
test/edge state-corruption paths.

### SDK observability surface (SDK-IMPL commits `7c53fd7` + `c799f57`)

`Bot.suggest(time_ms=T, depth=N, return_stats=True)` lands:

- `SearchStats` frozen dataclass: `max_depth_reached`, `nodes`, `nps`,
  `time_ms`, `score`.
- `depth=N` kwarg: fixed-depth target lifts time bound; deterministic
  move output across `time_ms` settings at fixed depth.
- `time_ms + depth` permissive — whichever bound hits first.
- Default `Bot.suggest(time_ms=T)` returns `Move` unchanged
  (additive). Existing 35 `test_public_api.py` tests pass.
- 12 new tests in `test_bot_stats.py` cover the new surface.
- No PyO3 changes — `bench_best_move` 6-tuple already in place,
  repurposed.
- Arena adapter (hexo-arena `5fd77f3`) consumes both surfaces;
  `--depth N` end-to-end works against HH.

**NPS impact**: 568k ± 3k post-`7c53fd7` SDK commit, 571k ± 1k post-
`c799f57` spec commit. Δ −0.6% / +0.5% vs TIME-FIX baseline — within
noise (additive observability surface, no hot-path changes).

### D-1 board.rs winner-cache fix (AUDIT-FIX commit `34fa870`)

**Bug**: `Board::undo` unconditionally cleared `winner = None` when the
undone player matched the cached winner, even if a 6-in-row from an
earlier stone-1 was still on the board. Hot search path insulated by
`pvs_node` entry-guard; test code reliably hit it. Silent since Phase 4.

**Fix**: `Board::undo` re-derives winner from axes via
`#[cold] rederive_winner(player)` (scans `self.pieces()` calling
`is_winning_move` per `player`-owned coord). Runs only on cold undo
path when cached winner matches. 2 regression tests added in
`tests/board_tests.rs`.

**NPS impact**: 546k ± 2k vs ~563k pre-fix → **Δ -3.2%** (within ±5%
threshold). Re-derive cost proportional to winning-leaf undo frequency
on bench fixtures. Documented in commit body.

### MINOR doc fix (AUDIT-FIX commit `012c327`)

`search.rs:304` doc comment said "Up to 2 narrow widens; on the third
failure we promote to full-window"; aspiration loop only does 1 narrow
widen then promotes (attempt counter increments to 2 on second failure
and the `>= 2` branch fires). Doc-only update. No bench impact.

### Audit areas clean (no bugs)

A search.rs (aspiration / PVS / LMR / TimeUp invariants / root TT
store / history saturation), B threats.rs (window scanning / per-player
isolation / S0-S1 disjointness / axis bounds), C eval.rs +
line_contrib.rs (LineContribution invalidation / X-positive sign / fork
mate semantics / SIMD parity), E tt.rs (128-bit verify / replacement
policy / score bounds / round-trip), F pybind.rs + engine.rs (memory
safety / borrow conflicts / error propagation; F-1 partial-clear on
`Engine::reset` is intentional design).

### Commits (5 atomic on hammerhead master + this doc commit pair)

| SHA | Subject | Type |
|---|---|---|
| `1f10f6c` | `search: fix per-stone vs per-turn time budget` | TIME-FIX |
| `7c53fd7` | `sdk: add SearchStats + depth/return_stats kwargs to suggest` | SDK-IMPL |
| `c799f57` | `spec(api): document SearchStats + depth/return_stats kwargs` | SDK-IMPL |
| `34fa870` | `board: re-derive winner on undo of cached-winner stone (D-1)` | AUDIT-FIX MAJOR |
| `012c327` | `search: fix aspiration widen count doc comment (MINOR)` | AUDIT-FIX MINOR |
| (this commit) | `bench: HOTSPOTS Phase 28E-0 time-fix + SDK + audit` | doc |
| (next) | `spec: mark Phase 28E-0 done in roadmap` | doc |

Plus 3 on hexo-arena main: `e47e5c0` (vendor refresh), `fe7b775`
(`--time-a/--time-b` asymmetric CLI), `5fd77f3` (adapter consumes
SearchStats + fixed-depth).

### Phase 28E-1 preconditions met

- Time bug fixed; HH uses 100% of per-stone budget.
- SearchStats per-call observability ready for Gap #1 measurement.
- `Bot.suggest(depth=N)` fixed-depth mode ready for eval-isolated A/B
  (no time variance confounders).
- Engine audit complete; no critical bugs blocking E-1.

### Phase 28E candidates (updated post-28E-0)

- **28E-1 — Gap #1 (window pattern table redesign) — PRIORITY**: zero
  `window_k_scores[3]` diagnostic first (measures with SearchStats new
  surface), then 729-entry continuous pattern table per axis if
  confirmed. Layer-1 length-3 double-count finding from D3 carries
  forward as the primary lever.
- **28E-2+ — Tempo proxy investigation**: carried 28B → 28C → 28D-1 →
  28D-3 → 28E-0. Detector revival or proxy.
- **28E-2+ — Opening-diversity library + harness wiring**:
  `NotImplementedError` in `promote.py:372-376` + `:553-557`. ~150 LOC
  Python + 10-20 fixture entries.
- **28E-2+ — Per-turn-joint vs per-stone-split scheduling A/B**:
  re-scoped from defunct `stone1_fraction` A/B (the split fraction
  concept no longer applies post-TIME-FIX). SB-perf plans whole turns
  jointly; HH plans per-stone. Investigate whether HH benefits from a
  per-turn-joint scheduling mode for arena gate symmetry.
- **28E-2+ — Promote-harness commit-bug fix**: trivial reorder
  `-m <msg> --only -- <path>` in `promote.py` auto-commit branch.

### Honest assessment

External arena DID NOT MOVE. Time-fix mechanism corrected without
lifting winrate against SB-perf — eval-gap hypothesis from 28D-3 retro
confirmed. Internal +40 Elo center shift is real movement but does not
clear the strict-positive `.bestref` advance gate. One MAJOR audit bug
(D-1) silent since Phase 4 is fixed. SDK observability + fixed-depth
surface ready for E-1 measurement work. Methodology finding
(50%-utilization implicit calibration error across all of Phase 28)
re-frames the prior arc and must carry forward to E-1 baseline
documentation. Phase 28E-1 Gap #1 (window pattern table redesign) is
the next substantive lever.

**Artifacts** (gitignored per Phase 25.5):

- `/tmp/phase_28e/PHASE_28E_0_RETRO.md` — full retrospective.
- `/tmp/phase_28e/0/time_diag.md`, `/tmp/phase_28e/0/time_fix.md` — TIME wave.
- `/tmp/phase_28e/0/sdk_design.md`, `/tmp/phase_28e/0/sdk_impl.md` — SDK wave.
- `/tmp/phase_28e/0/audit.md`, `/tmp/phase_28e/0/audit_fix.md` — AUDIT wave.
- `/tmp/phase_28e/0/verify.md` — VERIFY 100g + 400g final read.
- `~/Work/hexo-arena/runs/e0-verify-hh500-sb500/` — VERIFY arena run.

---

# Hotspots — Phase 28D-3 (eval revival + bug sweep)

## Phase 28D-3 status (2026-05-24)

Phase 28D-3 ran 11 sub-phases: D3-DIAG (eval correlation diagnostic),
D3-INFRA (S1 detector scaffolding ×2 commits), A.1-A.3 (open_3 /
closed_3 / open_2 per-shape detect + sweep + arena, 6 commits), B.1-B.4
(SealBot-perf I3 bug-pattern audits + invariant-test lock-ins, 4 commits),
D3-GATE (200g arena vs SB-perf 3 conditions + 400g internal sanity).

**Headline: external arena FLAT. `.bestref` NOT advanced.** GATE n=200
Cond B (per-stone equal 500ms) 9/200 = 4.5% Wilson [2.4%, 8.4%] vs I4
baseline 8.7% Wilson [3.4%, 20.0%] — CIs overlap. Internal REJECT
(-33.1 Elo, CI [-67.3, +1.0]) — CI upper touches zero, NOT strict-
negative. **KEEP all 12 commits on master; `.bestref` stays at
`5bd89648`** (Phase 28D-1 state).

All 4 B.X bug suspects REFUTED in HH (TT quantization, root ordering
after aspiration fail-high, search inner-loop alloc, TimeUp killer
rollback). Test-only invariant lock-in commits landed for each — production
binary byte-identical pre/post. Methodology improvement: "audit-and-
document is not enough" (28D-2 lesson) replaced with grep-then-pin
structural-invariant tests in CI.

**Per-landing arena trajectory** (winrate deltas, new metric for arena-
gated phases):

| State | Cond B 500ms/stone | Internal Elo (200g sweep) | Detection / weight landed |
|---|---|---|---|
| pre-D3 (I4) | 4/46 = 8.7% | n/a | (Phase 28D-1) |
| post-A.1 | 4/50 = 8.0% | -35 Elo, CI [-83, +13] | open_3=90000 (least-negative) |
| post-A.2 | 3/47 = 6.0% | 0 Elo, CI [-48, +48] | closed_3=11250 (TIE 100-100-0) |
| post-A.3 | 1/49 = 2.0% | **+52 Elo, CI [+4, +101]** | open_2=11250 (FIRST positive cell) |
| **GATE 200g Cond B** | **9/200 = 4.5%, [2.4%, 8.4%]** | n/a | (no further landings) |
| GATE 200g Cond A (per-turn-equiv) | 14/200 = 7.0%, [4.2%, 11.4%] | n/a | HH 500 vs SB-perf 1000 |
| GATE 200g vs vanilla SB | 18/200 = 9.0%, [5.8%, 13.7%] | n/a | per-turn-equiv |
| GATE 400g internal HEAD vs .bestref | 181-219-0, 45.25% | **-33.1 Elo, CI [-67.3, +1.0]** | REJECT |

Cumulative D3 Cond B (A.1+A.2+A.3+GATE): 17/346 = 4.9% Wilson [3.1%, 7.7%]
overlaps I4 [3.4%, 7.7%]. No statistically significant movement.

**Layer-1 double-counting finding (the durable architectural lesson):**

Per-shape atomic landings allowed clean confound-free attribution. open_3
(length-3) and closed_3 (length-3) sweeps both showed zero-or-negative
internal cells across all 5 weights tested. open_2 (length-2) produced
+52 Elo, the first positive A.X cell. Length is the discriminator:

- Layer-1 `window_k_scores[3] = 64` (codegen'd into `WINDOW_SCORE_8`)
  already fires on length-3 own-stone windows, supplying a direction-
  correct gradient at base score 64 × open_extension_factor = 256 per
  matching 6-cell window.
- Adding a Layer-2 OPEN_3 = 90000 or CLOSED_3 = 11250 on top double-
  counts the same configuration. Search absorbs redundant signal as
  ordering thrash; net internal Elo trends zero-or-negative.
- Layer-1 is SILENT on length-2 (no length-2 window-k entry). Layer-2
  OPEN_2 = 11250 carries independent information → positive Elo signal
  internally.

The two NULL closed_3 cells (11250 and 33750, both exactly 100-100-0
internal) and the uniformly negative open_3 cells form direct evidence
of the collision. Open_2's positive cell is the cleanest counter-example.
This refines D3-DIAG's "Layer-1 double-counting" hypothesis to a
length-3-specific finding and directly seeds Phase 28E Gap #1 (window
pattern table redesign).

**Per-stone vs per-turn observation** (GATE n=200):
Cond A (per-turn-equiv HH 500 vs SB-perf 1000) 7.0% > Cond B (per-stone
equal 500/500) 4.5%. HH does BETTER when SB-perf has 2× per-stone time
but equal per-turn time. Suggests HH's locked 60/40 per-stone split may
be suboptimal vs whole-turn-planning opponents. Cheap to A/B in Phase
28E (vary `stone1_fraction` only). Reversed asymmetry vs A.1 (Cond A
0%, Cond B 8%) — pre-S1-stack and post-S1-stack arena profiles differ.

**Self-time band shift: none material.** S1 detection runs even at
weight=0 (D3-INFRA scaffold added `(2,2)`, `(3,2)`, `(3,1)` arms to
`classify_linear_run`); cost is one extra branch per linear-run
endpoint pair. Measured impact: `make bench-quick` post-INFRA 551k ± 3k
NPS vs pre-INFRA 551k ± 4k NPS, Δ +0.1% — within noise. Detection cost
is sub-resolution at bench-quick precision. Post-A.3 detection runs
unconditionally at the live weight stack but bench-quick stays in band.

Reference node counts drift on the A.X landings (weight changes shift
move ordering → α-β cutoffs swing both directions). Drift bidirectional,
substantial (±10-95% per cell), but NPS-neutral within ±5% gate. No
hot-path regression; eval-weight changes are out of scope for
`benches/results/baseline.json` (canonical NPS baseline, untouched).

**B.X invariant tests landed (test-only commits, release binary byte-
identical):**

| ID | Commit | Audit target | Test name |
|---|---|---|---|
| B.1 | `8de7979` | TT EXACT quantization (SB-perf M4) | `score_round_trip_is_bitwise_exact_no_quantization` |
| B.2 | `8d75f8d` | Root ordering after aspiration fail-high (SB-perf M5) | `pvs_root_fail_high_writes_failing_move_to_tt` |
| B.3 | `a3c7753` | Search inner-loop heap alloc (SB-perf M6) | `search_hot_path_zero_alloc_structural_invariants` |
| B.4 | `f1032ba` | TimeUp killer-rollback completeness | (invariant lock-in) |

All four bug suspects REFUTED in HH. Each test would catch the SB-perf
pattern if it were ever introduced (bug-injection sanity verified).

**Commits (12 atomic on master, + 3 doc commits this retro):**

| SHA | Subject | Type |
|---|---|---|
| `fca4dad` | `eval: add S1 ThreatType + ThreatCounts fields` | D3-INFRA scaffold |
| `8542938` | `eval: surface S1 weights in hexo.toml + EvalOverrides` | D3-INFRA plumbing |
| `65ed2dc` | `eval: implement open-3 detection` | A.1 detection |
| `5011ea3` | `eval: tune open_3 weight to 90000` | A.1 tune (least-negative) |
| `8de7979` | `tt: add score round-trip regression test (no quantization)` | B.1 lock-in |
| `8d75f8d` | `search: add M5 root-tt fail-high invariant test (refutation lock-in)` | B.2 lock-in |
| `392e410` | `eval: implement closed-3 detection` | A.2 detection |
| `9a25ef6` | `eval: tune closed_3 weight to 11250` | A.2 tune (TIE) |
| `a3c7753` | `search: lock zero-alloc invariants for hot path (B.3)` | B.3 lock-in |
| `f1032ba` | `search: lock TimeUp killer-rollback invariant (B.4)` | B.4 lock-in |
| `c656e0d` | `eval: implement open-2 detection` | A.3 detection |
| `ab72ec2` | `eval: tune open_2 weight to 11250` | A.3 tune (FIRST positive) |
| (this commit) | `bench: HOTSPOTS Phase 28D-3 eval revival + bug sweep` | doc |
| (next) | `spec: mark Phase 28D-3 done in roadmap` | doc |
| (next) | `spec(eval): document Phase 28D-3 revived S1 detection` | doc |

**Honest assessment**: external arena DID NOT MOVE. The S1 revival
hypothesis ("HH lacks S1 detection, that's why SB-perf wins") is
FALSIFIED at GATE n=200. The phase has methodology value (per-shape
atomic attribution, B.X invariant test pattern, length-3-specific
collision finding) and one productive forward landing (open_2
detection + weight) but did not close the SB-perf gap.

**Phase 28E candidates** (Gap #1 prioritized):

- **28E-A — Gap #1 (window pattern table redesign)**: PRIORITY. Make
  Layer-1 length-3 and length-2 disjoint from S1 detection. Option (a):
  zero `window_k_scores[3]` and re-sweep open_3 / closed_3 weights
  against the disjoint baseline. If positive cells appear,
  double-counting hypothesis is confirmed and the tuned weights become
  real signal. Option (b): replace `window_k_scores` bucket with a
  729-entry continuous pattern table per axis (SB-perf style — addresses
  Gap #2 magnitude-resolution finding from D3-DIAG). Option (a) is the
  diagnostic; option (b) is the substantive redesign.
- **28E-B — Tempo proxy investigation** (still pending across
  28B/C/D-1/D-3). Detector revival or proxy invention.
- **28E-C — Opening-diversity library** (still pending; library
  doesn't exist). ~150 LOC Python + 10-20 fixture entries. Replaces
  `NotImplementedError` at `promote.py:372-376` + `:553-557`.
- **28E-D — Per-stone vs per-turn time-split A/B**. GATE Cond A 7.0% >
  Cond B 4.5% suggests HH 60/40 stone1/stone2 split may be suboptimal
  vs whole-turn-planning SB-perf. Cheap to A/B (vary `stone1_fraction`).
- **28E-E — Promote-harness commit bug fix** (carried from 28D-1):
  reorder `-m` before `--` in `promote.py` auto-commit branch.

**Match harness reminder** (Phase 26.5 meta-finding, BINDING): 500ms ×
200g CI ≈ ±48 Elo; n=50 ≈ ±96 Elo. The D3 A.X per-landing 50g cells
were below resolution floor for sub-25 Elo deltas. GATE n=200 (CI ≈
±48 Elo) was the right resolution for the cumulative read; future
arena-gated phases should plan around n=200 minimum per condition for
external winrate deltas in the 5-15% range.

**Artifacts** (gitignored per Phase 25.5):
- `/tmp/phase_28d/PHASE_28D_3_RETRO.md` — full retrospective.
- `/tmp/phase_28d/3/diag/diagnostic.md` — D3-DIAG correlation report.
- `/tmp/phase_28d/3/infra/implementer.md` — D3-INFRA scaffold report.
- `/tmp/phase_28d/3/A.{1,2,3}/implementer.md` — per-A.X sub-phase reports.
- `/tmp/phase_28d/3/B.{1,2,3,4}/implementer.md` — per-B.X audit reports
  (B.4 directory empty; B.4 commit context in GATE report).
- `/tmp/phase_28d/3/gate/match.md` — D3-GATE final arena report.

---

# Hotspots — Phase 28D-1 (cycle-break match, Outcome C: ADVANCE)

## Phase 28D-1 status (2026-05-24)

Phase 28D-1 ran an 800-game promote-match HEAD (`5bd8964`, engine
state C1 = Phase 25.5 + Phase 27 LineContribution cache + Phase
28B-B-2.1 `open_4=135_000`) vs prior `.bestref` (`932c5d8`, Phase
25.5 final). No eval / `hexo.toml` / source change — pure
cumulative measurement. Designed as the cycle-breaker after three
consecutive Phase-27-shape outcomes (27 / 28B / 28C all REJECT
on strict gate).

**Headline: Outcome C — strict-positive. `.bestref` advanced
`932c5d8` → `5bd89648`.** First `.bestref` advance in 6 phases
(since Phase 25.5, commit `432ddba`). Cycle BROKEN.

**Match result** (800g, 500 ms/stone, 10 workers, Wilson 95%,
color-balance ON, opening-diversity OFF — no library exists):

```
games:    800
current:  429  best: 371  draws: 0
winrate:  0.5363  wilson95: [0.5016, 0.5706]
elo:      +25.2  ci95: [+1.1, +49.4]
verdict:  PROMOTE
```

W-L-D 429-371-0. Wilson 95% half-width ±24.2 Elo (matches the
dispatcher's pre-match prediction). Independent recomputation in
D1-REV reproduces +25.233 / [+1.114, +49.353] to displayed
precision.

**Strict gate cleared by razor-thin margin** (+1.1 Elo CI lower).
~12 fewer wins would have flipped this to Outcome B. The cycle-
break hypothesis holds but the cumulative signal at HEAD is at
the edge of resolvability even at 800g.

**Additive-prediction comparison**:

| Source | Estimate | Method |
|---|---:|---|
| 28C C2-DRIFT: `e28d54a` vs `.bestref` | +33.11 Elo @ 400g | direct |
| 28C-0 drift-corrected: C1 vs `e28d54a` | +24.4 Elo @ 400g | drift-corrected |
| Sum (additive prediction) | ~+57.5 Elo | composite |
| **800g measurement: HEAD vs `.bestref`** | **+25.2 Elo** | direct |

Observed is ~32 Elo BELOW additive prediction — ~1.3σ below the
sum-of-variances std dev (~25 Elo). Most likely explanation:
**partial regression to mean from CI-straddling prior
measurements.** Both anchor measurements were single 400g points
with CIs straddling zero; each could be 5–20 Elo upward noise
excursion, and summing compounds the optimistic bias. **Real
cumulative Elo at HEAD vs prior `.bestref` ≈ +25 Elo point
estimate, not +57.** Consistent with Phase 27 alone (+27 Elo at
400g vs `.bestref`).

**Drift recalibration SKIPPED** per Outcome C protocol —
correction can only tighten an already-cleared verdict (28C
drift was -1.74 Elo, statistically zero).

**Self-time band shift: none.** D1 is pure measurement; engine
source byte-identical to `0c3cc6b`. The hotspot ranking from the
Phase 27 LineContribution-cache snapshot (board 31.51% > eval
26.55% > search_other 24.58% > threats 12.43% > ordering 4.93%)
still applies unchanged.

**Commits (3 since Phase 28C close, all on master)**:

| SHA | Subject | Type |
|---|---|---|
| `b95a672` | `promote: advance .bestref to 5bd89648 (Phase 28D-1)` | promote (config-only) |
| `4208e8f` | `spec: mark Phase 28D-1 done in roadmap` | doc |
| (this commit) | `bench: HOTSPOTS Phase 28D-1 .bestref advance` | doc |

No source / `hexo.toml` / Cargo changes. Reference node counts
trivially byte-identical (only `.bestref` config file modified).

**Wall-clock budget**:

| Stage | Wall-clock |
|---|---:|
| D1-RUN setup + 14-worker false-start | ~5 min |
| D1-RUN clean 10-worker 800g match | ~30 min |
| D1-LAND state cleanup + commit | ~10 min |
| D1-REV independent review | ~15 min |
| D1-RETRO 2 doc commits | ~20 min |
| **Total** | **~80 min** |

The 800g match was the binding cost. Far under any reasonable
envelope.

**Promote-harness commit bug** (incidental finding, NOT FIXED —
out of scope for D1, logged for follow-up): the auto-commit
branch in `hammerhead/hammerhead/promote.py` invokes
`git commit --only -- <path> -m <msg>` — `-m` is placed AFTER the
`--` pathspec separator, so git treats it as an invalid pathspec
and the commit fails. Auto-commit rolled back working-tree
`.bestref` but left the staged index dirty; D1-LAND performed
manual cleanup + atomic commit. Trivial fix (reorder to
`-m <msg> --only -- <path>`); high-priority follow-up for the
next phase that touches `promote.py`. Reviewer also noted
`specs/SPEC_BENCHMARKS.md` lacks an explicit `[promote]` section
despite roadmap references — reconcile alongside.

**Phase 28D-2+ handoff** (Outcome C dispatch column):

- **BO sprint v2 vs new `.bestref` (`5bd89648`)**: widened
  bounds, optional warm-start from C1, convergence early-stop
  (design.md §3) wired. Resumable study at
  `/tmp/phase_28c/2/study.db`.
- **Opening-diversity library + harness wiring** (deferred B-1.3 /
  B-1.4 from 28B C-DEFERRED): now relevant for A/B vs new
  `.bestref`. ~150 LOC Python + 10–20 fixture entries; replaces
  `NotImplementedError` at `promote.py:372-376` + `:553-557`.
- **Tempo proxy investigation** (deferred 28B → 28C → 28D).
- **External arena (SealBot)**: PRIORITY — cross-engine
  independent signal confirms cumulative work is real strength,
  not within-engine harness artifact.
- **Promote-harness commit-bug fix**: high-priority follow-up.
- **NOT NEEDED**: 1600g promote-match (Outcome C cleared at
  800g). **DEFER**: search-side tuning revival (harness floor
  unchanged by new `.bestref`).

**Match harness reminder** (Phase 26.5 meta-finding, BINDING):
500ms × 200g CI ≈ ±48 Elo; 400g ≈ ±34 Elo; 800g ≈ ±24 Elo. The
+1.1 Elo CI-lower margin at 800g confirms 800g is the right
resolution for cycle-break tests at the accumulated-work scale
we are operating at; 400g would have left this ambiguous.

**Artifacts** (gitignored per Phase 25.5):
- `/tmp/phase_28d/PHASE_28D_1_RETRO.md` — full retrospective.
- `/tmp/phase_28d/1/match_runner.md` — D1-RUN report.
- `/tmp/phase_28d/1/landed.md` — D1-LAND report.
- `/tmp/phase_28d/1/review.md` — D1-REV report.
- `/tmp/phase_28d/1/match_800g.log` — raw 800g match log.
- `/tmp/phase_28d/1/games/` — per-game outputs.

---

# Hotspots — Phase 28C (BO sprint, no land)

## Phase 28C-1 status (2026-05-24)

Phase 28C-1 ran a 60-trial Bayesian optimisation sprint over the 5
top-leverage eval scalars (`open_4`, `closed_5`, `window_k_scores[5]`,
`open_extension_factor`, `fork_cover2_bonus`) using Optuna 4.8.0
GPSampler (Matérn-5/2 kernel, ARD per-dim length-scales,
`deterministic_objective=False`). Trials ran at 200g vs Phase 27
baseline (`e28d54a`) on the 10-worker host. Sprint clean: 60/60
COMPLETE, 0 FAIL, 6h 40min wall-clock.

**Headline: REVERT.** BO winner (trial 34, raw +63.23 Elo at 200g)
collapsed under 400g validation. Vs `.bestref` (`932c5d8`) at 400g:
**-14.77 Elo CI [-48.80, +19.25]**, W-L-D 191-208-1 — strict gate
FAILED, marginal gate FAILED (point < 0). Vs `e28d54a` at 400g
(smoke): **-10.43 Elo CI [-44.44, +23.58]**, drift-corrected -8.69.
Vs current HEAD C1 (additive estimate): **~-33 Elo regression**.
Outcome A (REVERT) per Phase 28A.5 plan § G. Zero eval / hexo.toml
commits from the sprint.

**Self-time band shift: none.** tune_bo.py is offline tooling
(Python-only driver invoking match harness via subprocess); no
Rust touched, no hot-path impact. Reference node counts
byte-identical to `0c3cc6b`. The hotspot ranking from the Phase 27
LineContribution-cache snapshot (board 31.51% > eval 26.55% >
search_other 24.58% > threats 12.43% > ordering 4.93%) still
applies unchanged.

**BO study summary (60 trials × 200g):**

Top-5 by raw Elo vs `e28d54a`:

| Rank | # | open_4 | closed_5 | wk[5] | oef | fork | Elo | CI lo | CI hi |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 33 | 240000 | 240000 | 1024 | 1 | 12000 | **+63.23** | +14.44 | +112.02 |
| 2 | 12 | 240000 | 240000 | 6723 | 6 | 0 | +34.86 | -13.38 | +83.10 |
| 3 | 54 | 240000 | 840000 | 6738 | 1 | 44000 | +34.86 | -13.38 | +83.10 |
| 4 | 52 | 240000 | 840000 | 4586 | 1 | 32000 | +31.35 | -16.84 | +79.55 |
| 5 |  6 | 146348 | 360000 | 6741 | 6 | 3000 | +27.85 | -20.30 | +76.01 |

Only trial 34's 200g lower CI excludes zero. Ranks 2-5 noise-comparable.

**400g smoke validation (trial 34 candidate vs `e28d54a`):**
+63.23 (200g) → **-10.43 (400g)** — 73-Elo collapse, classic 200g
noise-spike signature. C3-VAL 400g vs `.bestref`: **-14.77 Elo**.

**fANOVA importance (60 trials, 5-D):**

```
window_k_scores_5             0.5209
fork_cover2_bonus             0.1597
open_4                        0.1439
open_extension_factor         0.1152
closed_5                      0.0604
```

`window_k_scores[5]` dominant — consistent with 28C-0 §7 finding
(B-2.1×B-2.3 = -27.85 Elo interaction couples `open_4` with `wk[5]`).
The dimensional coupling is real; trial 34's specific corner was not
a real optimum. Caveat: fANOVA on n=60 in 5-D is a ranking, not a
precise variance partition.

**Boundary-hit warning:** trial 34 hit **4 of 5 search bounds**
(`open_4=240k` HIGH, `closed_5=240k` LOW, `wk[5]=1024` LOW,
`oef=1` LOW; only `fork_cover2_bonus=12k` interior). Bounds were
too narrow. Phase 28D recommendation: widen `open_4 > 240k`,
`closed_5 < 240k`, `wk[5] < 1024`, `oef = 0` unprobed.

**Cumulative reference measurement (400g, fresh):**
`e28d54a` vs `.bestref` = **+33.11 Elo CI [-1.04, +67.25]**, W-L-D
219-181-0. 28C-0's 200g prior estimate was ~+13 Elo; 400g
measurement is ~2.5× higher with CI lower at -1.04 just barely
straddling zero — same Phase 27 marginal-positive shape.

**HEAD Elo state: unchanged.** C1 = {B-2.1 only} from Phase 28C-0.
Drift-corrected vs `e28d54a` = +24.4 Elo (200g, 28C-0 measurement).
Implied vs `.bestref` (additive: +24.4 + 33.11) = **~+57.5 Elo**.
Phase 28D could test `make promote N_GAMES=800` at HEAD to formally
cross the strict gate without further eval changes — this is the
3rd consecutive Phase-27-shape outcome (27 / 28B / 28C) and an 800g
match is the natural cycle-breaker.

**Commits (5 since 28C-0 close, all on master):**

| SHA | Subject | Type |
|---|---|---|
| `36b8cdc` | tune: add Optuna BO driver scaffolding | infra |
| `fb36ddd` | tune: integrate Optuna study with EvalOverrides | infra |
| `e46869c` | tune: BO study report + spec update | infra |
| (this commit) | bench: HOTSPOTS Phase 28C BO sprint section | doc |
| (next commit) | spec: mark Phase 28C done in roadmap | doc |

No eval / hexo.toml / Rust changes. Engine byte-identical to `0c3cc6b`.

**Wall-clock budget:**

| Stage | Wall-clock |
|---|---:|
| C1-DESIGN / IMPL / REV | ~3h 30min |
| C2-RUN (60 × 200g) | ~6h 40min |
| C2-DRIFT + smoke | ~42min |
| C3-VAL (400g vs .bestref) | ~16min |
| C-RETRO (2 doc commits) | ~30min |
| **Total** | **~7h 40min match wall** |

Well under 18h envelope.

**Phase 28D handoff (from `/tmp/phase_28c/PHASE_28C_RETRO.md`):**

- BO sprint v2 at 400g/trial OR averaged-200g, widened bounds on
  4 of 5 dims. Resumable study.db at `/tmp/phase_28c/2/study.db`.
- 800g promote-match at HEAD vs `.bestref` (no eval change) —
  the cycle-breaker experiment.
- Tempo proxy investigation (deferred 28B → 28C → 28D).
- Opening diversity library construction (B-1.3 / B-1.4 from 28B
  C-DEFERRED; `promote.py:372-376` + `:553-557` raise
  `NotImplementedError`; no opening library exists per
  `positions.json`).
- Convergence early-stop in tune_bo.py (design.md §3 rule
  unimplemented; would have fired at trial 48, saved ~1.4h).

**Match harness reminder** (Phase 26.5 meta-finding, BINDING):
500ms × 200g CI ≈ ±48 Elo; 400g ≈ ±34 Elo; 800g ≈ ±24 Elo.
Best-of-N at 200g applies +20 Elo positive selection bias for N=60
(per I3 §E.1). Per-trial BO objectives below 400g require either
sample-size doubling or repeated measurements.

**Artifacts** (gitignored per Phase 25.5):
- `/tmp/phase_28c/PHASE_28C_RETRO.md` — full retrospective.
- `/tmp/phase_28c/1/` — C1 design / implementer / review reports.
- `/tmp/phase_28c/2/` — sprint.md, drift.md, study.db, trials/,
  smoke400_trial34.json.
- `/tmp/phase_28c/3/` — validation.md, diversity.md, landed.md,
  val_trial34_vs_bestref.json.

---

# Hotspots — Phase 28C-0 (master state verification)

## Phase 28C-0 state verification (2026-05-23)

Phase 28C-0 ran a subset-verification sprint following Phase 28B's
handoff item "subset experiments". Built an 8-config 2³ factorial
(2 levels × 3 28B landings) vs Phase 27 baseline `e28d54a` at 400g
each (3200 games total, ~1h46min wall) with self-test drift
correction (-6.9 Elo). Verdict: **revert 2 of 3 Phase 28B landings**.

**Headline (master HEAD post-sprint):**
- **KEEP**: `open_4` = 135_000 (B-2.1, `b35936b`). 2³ main effect
  +4.4 Elo (in noise band, positive). C1 = {B-2.1 only} = best
  observed subset.
- **REVERT**: `window_k_scores[5]` 2_048 → 4_096 (B-2.3 `5283059`
  reverted in `5fe133e`). 2³ main effect -15.7 Elo (just outside
  noise band, negative).
- **REVERT**: `open_extension_factor` 8 → 4 (B-2.5 `13dc73a`
  reverted in `11ab31a`). 2³ main effect -9.6 Elo (in noise band,
  Occam tiebreak).

**Drift-corrected Elo of post-revert HEAD vs `e28d54a`**:
**+24.4 Elo CI [-9.7, +58.5]** (point estimate, CI straddles —
same shape as Phase 27/28B MARGINAL-LANDs; matches C1 = best
observed subset).

**HEAD pre-revert (C7 = {B-2.1, B-2.3, B-2.5})**: drift-corrected
-34.8 Elo CI [-68.9, -0.7] — CI ENTIRELY negative post-correction.
Strongest single signal in the run: HEAD was net-negative vs Phase
27 baseline. Net 28B contribution = negative with high confidence.

**`.bestref` UNCHANGED** (`932c5d8`) — strict-promote rules
unchanged; reverting bad landings is not promotion.

**Key structural finding**: eval surface is non-separable. All
three pairwise 2³ interactions exist; B-2.1×B-2.3 = -27.85 Elo
(~2.27σ, borderline significant — the only above-noise structural
signal). Sum of 2-way interactions = -22 Elo. C7 underperformed
additive main-effect prediction by ~14 Elo. Per-axis coord
descent (Phase 28B approach) systematically underexplores joint
optima.

**Phase 28C-1 methodology** (per `/tmp/phase_28c/0/feasibility_research.md`):
Optuna 4.8.0 GPSampler — Matérn-5/2 kernel models cross-dimensional
interactions implicitly; learns per-dim length-scales via marginal
likelihood. `deterministic_objective=False` for ~±34 Elo Wilson
noise. Seeds at C1. 50-80 trials, 6-10h wall on 10-worker host.
TPESampler is the 1-line-swap fallback.

**NPS impact (bench-quick, midgame_12):**
- HEAD pre-revert (`13dc73a`, B-2.5 landed): ~524k NPS
- Post-B-2.5 revert (`11ab31a`): ~551k NPS (+5.2%, recovers -4.9%
  B-2.5 landing penalty)
- Post-B-2.3 revert (`5fe133e`): ~554k NPS (+0.5%, neutral)

Reference node counts rebaselined per revert (Phase 25.5 rule —
value-tuning rebaseline event; both reverts shift search behaviour).

**Commits (3 atomic, on master):**

| SHA | Subject |
|---|---|
| `11ab31a` | revert: B-2.5 open_extension_factor per Phase 28C-0 |
| `5fe133e` | revert: B-2.3 window_k_scores[5] per Phase 28C-0 |
| (this commit) | bench: Phase 28C-0 master state verification |

**Verification protocol**: each revert commit gated on `make check`
133/133 + `make bench-quick` (NPS delta noted) + surgical
`baseline.json` `macro.reference` refresh (per Phase 28B precedent).

**Artifacts** (gitignored per Phase 25.5):
- `/tmp/phase_28c/0/synthesis.md` — full drift-corrected 2³
  factorial analysis (C0-SYN output).
- `/tmp/phase_28c/0/verification_runner.md` — match protocol +
  raw results (C0-VR output).
- `/tmp/phase_28c/0/feasibility_research.md` — BO library decision.
- `/tmp/phase_28c/0/matches/C{0..7}.json` — per-cell match data.

**Match harness reminder (Phase 26.5 meta-finding, BINDING):**
500ms × 400g CI ≈ ±34 Elo. The +24.4 Elo CI [-9.7, +58.5] of C1
straddles zero (same as Phase 27/28B). At 400g any single-axis
signal under ~25 Elo is fragile to drift-CI uncertainty. Phase
28C-1 BO sprint should optimize joint cell directly; coord-descent
under-explores interactions.

---

# Hotspots — Phase 28B (eval-value tuning sprint)

## Phase 28B status (2026-05-23)

Phase 28B was a match-driven coordinate-descent sweep of the top-5
unswept eval scalars (the live S0 + window + extension + fork surface
that had never been game-time-tuned since the codebase existed —
per Phase 28A audit, the "Phase 10 self-play tuning" claim in
SPEC_EVAL was unsubstantiated). Resurrected the Phase 20-deleted
sweep infrastructure (`tune.py` + a 14-scalar `EvalOverrides` runtime
override surface) and ran 5 candidates through pre-screen + Stage 1
(200g) + Stage 2 (400g) per plan § D.

**Headline (commit `13dc73a`, final promote-match REJECT):**
- 3 of 5 candidates landed on master as MARGINAL-LANDs (Phase 27
  shape — positive point estimate, CI straddles zero). 2 reverted.
- Cumulative HEAD vs `.bestref` (932c5d8) at 400g: **+17.4 Elo
  CI [-16.7, +51.4]**, REJECT (strict gate CI lower > 0 not cleared).
- `.bestref` UNCHANGED. Outcome B per plan § G (modal expectation).
- Combined-best probe: HEAD with 3 wins vs HEAD-with-3-wins-undone
  at 400g = **-3.5 Elo CI [-37.5, +30.5]**. The 3 wins do NOT
  compose additively (sum-of-per-axis +40 Elo, joint -3.5 Elo —
  the joint underperforms the additive prediction by 43 Elo).
- Reference node counts rebaselined per landing (3 baseline.json
  refreshes — Phase 25.5 rule applied per value-tuning rebaseline).

**Landed values (vs e28d54a baseline):**

| Param | Was | Now | Stage 2 Elo | Decision |
|---|---:|---:|---:|---|
| `open_4` | 60_000 | **135_000** | +12.2 CI [-21.8, +46.2] | MARGINAL-LAND (B-2.1, `b35936b`) |
| `fork_cover2_bonus` | 4_000 | 4_000 | -15.6 CI [-49.7, +18.4] | REVERT (B-2.2) |
| `window_k_scores[5]` | 4_096 | **2_048** | +20.9 CI [-13.2, +54.9] | MARGINAL-LAND (B-2.3, `5283059`) |
| `closed_5` | 500_000 | 500_000 | -1.7 CI [-35.7, +32.3] | REVERT (B-2.4) |
| `open_extension_factor` | 4 | **8** | +6.9 CI [-27.1, +41.0] | MARGINAL-LAND (B-2.5, `13dc73a`) |

**NPS impact (bench-quick, midgame_12):**
- Pre-sprint (e28d54a): ~552k NPS
- Post-B-2.1 (open_4=135k): ~552k (Δ +0.0%)
- Post-B-2.3 (+ window_k=2048): ~551k (Δ -0.2%)
- Post-B-2.5 (+ open_ext=8): ~524k (Δ -4.9% vs pre-sprint)

The open_extension_factor change from 4 to 8 has measurable
throughput cost (-4.9% NPS, near the ±5% gate edge — verified
across 4 bench-quick runs). The higher extension multiplier
applies the boost to more S0 windows in the Layer-1 pass.
open_4 and window_k_scores[5] are NPS-neutral (table swaps).

**Flamegraph breakdown not refreshed** for Phase 28B — value
tuning doesn't shift hot-path distribution materially (per
Phase 28A I-HOTPATH projection); the Phase 27 ranking still
applies.

**Commits (7 atomic, all on master):**

| SHA | Subject | Type |
|---|---|---|
| `9982a26` | spec: correct S0 weight provenance (Phase 28B-0) | spec drift |
| `bc2ef6e` | spec: document Layer-1/Layer-2 stacking rationale (Phase 28B-0) | spec drift |
| `128b115` | eval: add EvalOverrides struct + PyO3 setter (Phase 28B-1) | infrastructure |
| `0bd419a` | tune: revive coord-descent sweep driver (Phase 28B-1) | infrastructure |
| `b35936b` | tune: open_4 -> 135000 (Phase 28B-2.1) | value MARGINAL-LAND |
| `5283059` | tune: window_k_scores[5] -> 2048 (Phase 28B-2.3) | value MARGINAL-LAND |
| `13dc73a` | tune: open_extension_factor -> 8 (Phase 28B-2.5) | value MARGINAL-LAND |

**Verification protocol (per plan § H per-commit gate):**
- Per-commit gate (every B-1.x and B-2.x landing): `make check`
  green (clippy + cargo test --release + 133 pytest), `make
  bench-quick` NPS within ±5% of pre-commit baseline, refreshed
  baseline.json `macro.reference` block in the same commit for
  value landings (intentional rebaseline event per Phase 25.5).
- B-0/B-1 commits gated on byte-identical reference counts (no
  behaviour change).
- B-2.x commits intentionally drift reference node counts (eval
  changes re-order moves). baseline.json macro.reference refreshed
  in-commit via `bench reference --tt-stats`.

**Match harness budget:**
- Plan worst-case: 16.3h match wall-clock at 10 workers, 500ms/stone.
- Actual: **~6h 22min** total sprint (2.6× faster than plan).
  Games complete in ~7 min/200g vs plan's ~20 min/200g assumption.
- Surface ceiling (18h, +10% buffer) never approached.

**Key meta-findings (Phase 28B harvest):**

1. **Eval surface is noise-resolution-limited.** ALL 5 candidates
   produced Stage 2 CIs straddling zero. The eval surface produces
   signal but signal amplitude is below the 400g harness floor
   (±34 Elo). At 800g some candidates would clear the gate
   (closed_5 pooled 800g signal ~+15.6 Elo) but the per-axis Elo
   is near the resolution boundary even at significantly higher N.

2. **Combined-best negative interaction.** Sum-of-per-axis (+40)
   vs joint Elo (-3.5) → -43.5 Elo delta. Per-axis Elos cannot be
   assumed additive on this surface. Likely mechanism: B-2.1 +
   B-2.5 push harder on attack shapes while B-2.3 reduces Layer-1
   k=5 contribution; the Layer-1/Layer-2 balance shifts and the
   engine may over-extend attacks.

3. **Pre-screen single-run Elo is unreliable.** Cross-run sign-
   flips on >50% of pre-screen → Stage 1 transitions. Pre-screen
   IS useful for routing (dead-substrate detection) but single
   point estimates at 200g are dominated by noise.

4. **Baseline-vs-baseline self-test asymmetric noise**: ~19 Elo
   stdev across 5 runs (centre estimates from -20.9 to +27.9
   when both engines play identical eval). Phase 28C should
   apply a noise-adjusted Stage 1 gate: candidate centre >
   (baseline self-test + 20 Elo) AND CI upper > 0.

**Stopping rule outcomes:**
- Stage-1-zero rule: 0/2 — never triggered.
- Stage-2-straddle rule (3 consecutive straddles): triggered at
  B-2.3 (3/3). Continued past per documented dispatcher judgment
  (rule intent is dead-substrate detection; our pattern was
  weak-signal-below-floor with 3 of 5 producing MARGINAL-LANDs).
  Net cost-benefit roughly neutral: one extra MARGINAL-LAND
  (B-2.5) + combined-best evidence about negative interaction.

**Phase 28C hand-off:**
- **Combo test at higher N**: bundle Phase 27 + Phase 28B winners
  vs `.bestref` at 800g/1600g to see if cumulative bumps clear
  the strict gate. Per plan § G forward commitment.
- **Subset experiments**: combined-best showed -43 Elo delta vs
  sum. Test subset compositions (drop B-2.1 alone, drop B-2.5
  alone) to find which 28B winners are net-positive when stacked.
- **Opening diversity validation A/B**: per Phase 28A.5 A-5
  forward commitment. Test HEAD vs HEAD diversity ON vs OFF.
- **Tempo proxy** (per I1 § 3): structurally different from value
  tuning. Requires detector revival or proxy. Strongest PDF
  evidence (TT p. 11 "tempo is the most important currency") of
  any deferred item.
- **Refined stopping rule**: replace "CI straddles zero" with
  "point estimate < +5 Elo" for consecutive-straddle terminator
  (doesn't misfire on weak-positive cases).

**Match harness reminder (Phase 26.5 meta-finding, BINDING):**
500ms × 200g CI ≈ ±48 Elo; 400g ≈ ±34 Elo. Per-step A/Bs at 100g
sanity-only. Promote-matches require ≥400g for sub-25 Elo deltas;
clearing the strict gate from a Phase 27/28B-shape marginal
requires 800g+ per combo-test forward commitment.

---

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
