# Hotspots — Sprint 5 (Φ recalibration; LMR re-test + stage-2.5 both null)

## Sprint 5 status (2026-05-26)

**OUTCOME B — `.bestref` UNCHANGED.** Sprint 5 was measurement-first;
the headline deliverable is a clean, post-PGO-fix arena correction
factor (Φ = +11.3 Elo) re-baselining all prior strength claims. No
strength-side changes survived: Phase C's LMR re-test produced two
identical-result, gate-failing 800g runs; Phase D's staged-2.5 logic
regressed -5.8 % NPS and -82.6 corrected Elo, was reverted in place.

**Phase A — PGO correction factor recalibration**: HEAD-vs-HEAD 800g
at `.bestref` (`dedfbbb`) with the Sprint 4B `1341ba4` venv-pin fix
overlaid (`dedfbbb` predates `1341ba4`). md5s of main vs worktree
binaries verified distinct (`bfcd00…` vs `46a7d5…`). Result:
412-386-2, **raw +11.3 Elo, CI95 [-12.8, +35.4]** at 24.1 Elo half-
width.

  **New Φ = +11.3 Elo.** Applied: `corrected = raw − 11.3`. Sign-
  flipped vs the old (suspect) −10. Memory `feedback_arena_correction_factor`
  updated. Spec § "PGO correction factor (Φ)" added to
  `specs/SPEC_BENCHMARKS.md` (commit `75de950`).

  Strategic implication: Sprint 1 (+12 corrected via old −10) and
  Sprint 3 (+10 corrected via old −10) are retrospectively revised
  to ~−39 and ~−11 corrected respectively. Cumulative project Elo
  story since `.bestref` cfefb3b is **revised downward by ~21 Elo
  per promote** under the new Φ. The downward revision is consistent
  with Phase B's external 7 % winrate (only +2 pp over the 5 %
  historical baseline) — both internal and external lines now agree
  that prior cumulative Elo claims were inflated.

**Phase B — SealBot-perf re-baseline (200g, 500 ms)**: adapter pre-
wired in `~/Work/hexo-arena`. External HH updated from `cfefb3b`
(Sprint 1) to current master `75de950`, rebuilt non-PGO per arena
convention. Result: **HH 14-0-186 (W-D-L), winrate 7.00 %, Wilson95
[4.22 %, 11.41 %]**. NPS 341 k (non-PGO build; comparison vs the 5 %
historical Sprint-1-era baseline is apples-to-apples).

  Verdict per § B.5: 7-10 % band — "modest external gain — Sprint 3's
  internal +10 partially externalised." +2 pp over historical noise
  baseline; below the +3 pp gate (`project_arena_external_gate_holds`).

**Phase C — LMR re-validation at 800g**: (lmr_min_depth=3,
lmr_min_move_index=4, lmr_reduction=2) and (3, 12, 2) via
`HEXO_SEARCH_PARAMS`. Both candidates ended at the *identical*
390-408-2, **raw −7.8, corrected −19.1, CI95 corrected [−43.2, +4.9]**.
Both gates failed (corrected mean ≥ 0 and CI lower ≥ -15 both fail).
Decision: **NO-WINNER**. No commits. Current TOML LMR (3, 6, 1) is
locally optimal in this slice.

  The identical W-L-D between two adjacent-cell runs is a low-
  probability coincidence (~0.1 %); intermediate progress lines
  differed, confirming the override took effect and game paths
  diverged. Sprint 4 retro's "+24.4 same-source for (3,4,2) under
  bugged PGO" is now confirmed as 400g-level noise. The clean 800g
  result is the truth.

**Phase D — Staged movegen 2.5**: implemented + measured + arena-
falsified + reverted. Forward commits `8e010c7`, `d5813bb`, `961d695`
followed by revert commits `c4cb18c`, `1b31870`, `7a24039`.

  - bench-quick NPS: 891 k (Phase 0 baseline 946 k = **-5.8 %**)
  - iai midgame_12 d6 ins: 2.110 G → 3.372 G (+59.7 %)
  - reference midgame_12 d6 nodes: 155 k → 227 k (+45.7 %)
  - 400g vs `.bestref` raw -71.3, **corrected -82.6**, CI95 corrected
    [-117, -48]

  Suspected root causes (post-revert analysis, unverified):
  (1) duplicate `bucket_value` calls across Stage 2.5 and Stage 3;
  (2) hi-bucket subset dispatched in generation order, not bucket-
  descending order — bucket-5 moves preempt bucket-9 wins;
  (3) reference shifts suggest broader exploration at shallow ID,
  narrower at deep ID — the time-bounded match never sees the deep
  benefit. Sprint 6 work if revisited.

**Phase E — Validation**: 5 cold bench-quick mean **941 k NPS**
(vs Phase 0 946 k = -0.5 %, within ±1 % noise floor). 800g final
SPRT vs `.bestref` outcome documented in
`/tmp/sprint_5/final_validation.md`.

### What Sprint 5 actually delivered

- Trustworthy Φ for all future arena measurements (was the headline
  goal, achieved).
- First clean external-strength number since the PGO bug was
  introduced (Phase B 7 %, Wilson [4.2, 11.4]).
- Two falsified candidate optimisations (LMR cell, staged-2.5
  reordering) — null results that prevent Sprint 6 from re-litigating
  these spaces without new evidence.
- One spec commit (`75de950`); no engine-code changes survive Sprint 5.

### `.bestref` decision

`.bestref` remains at **`dedfbbb`** (Sprint 3 close). Sprint 5 outcome B
per § E.5: bench-quick within noise, Φ-corrected mean ≈ 0 expected for
800g HEAD vs `.bestref`. No advance.

### Sprint 6 handoff (top 5)

1. **Stage 2.5 re-attempt with both fixes.** (a) sort hi-bucket
   subset bucket-DESC before dispatch; (b) plumb precomputed
   `(bucket, move)` pairs from Stage 2.5 into Stage 3 to avoid
   duplicate `bucket_value` calls. Estimated +5-13 % NPS if both fix
   work; under -5 % if either is missed. Use iai-callgrind as the
   tight gate — sub-1 % NPS changes are below 800g arena resolution.

2. **Expanded LMR grid at 1600g per cell.** Vary both `lmr_reduction`
   (1-4) and `lmr_min_move_index` (4-16) with `lmr_min_depth` fixed
   at 3. Sprint 5C's 800g resolution couldn't distinguish (3,4,2)'s
   -7.8 from noise. 1600g cuts CI half-width to ~17 Elo. Use Phase A's
   Φ = +11.3 throughout.

3. **Aspiration / extension retune** — Sprint 4C's runtime override
   surface is built; Sprint 5 didn't use it. Two parameters
   (`asp_window_initial`, `max_check_extensions`) are likely
   non-stationary against current eval/search; coordinate-descent
   sweep at 400-800g per cell.

4. **External HH PGO build for Phase-B-style re-baselines.** Sprint 5B
   used non-PGO external HH (per existing arena convention) and saw
   7 % winrate. PGO'd external HH would close most of the 36-percent
   NPS gap vs main repo and could push the SB-perf winrate higher,
   exposing real strength gains that the non-PGO build masks.

5. **`tt_size_mb` retune.** TT-replacement HOTSPOTS bands have not
   moved since Sprint 1. With Sprint 4A's runtime surface, a small
   1×1 sweep (32 / 64 / 128 / 256 MB) at 800g would resolve whether
   the current default 64 MB is still optimal at Zen 4 cache sizes.

### Updated hotspot bands (post-Sprint 5)

No new hotspots shifted. bench-quick 941 k matches Sprint 4 close
946 k within noise. All Sprint 3 close optimisations remain dominant
(`place_for_search`, history flat, axis-bitmap unchecked).

---

# Hotspots — Sprint 4 (runtime tuning surface, LMR sweep REVERTED)

## Sprint 4 status (2026-05-26)

**OUTCOME B — `.bestref` UNCHANGED.** Infrastructure landed; LMR
Texel-tuned change reverted on Stage 3 evidence. Three of seven
plan phases landed (A runtime tuning surface + C aspiration/extension
override + side-quest PGO-build bug fix). Phase B (LMR Texel retune)
sweep completed and the (3,4,2) winner was committed but **reverted**
in Phase F after the final 400 g SPRT vs .bestref disagreed with
Stage 3 v2. Phase D (SealBot-perf re-baseline) skipped — adapter
absent. Phase E (staged movegen 2.5) deferred to Sprint 5.

**Headline (post-revert):** bench-quick **942-946 k NPS at depth 6**
— within ±1 % of Sprint 3 close baseline (951 k). All Phase A + C
infrastructure is pure additive: iai-callgrind byte-identical
instructions at default params; reference node counts byte-identical
at default params. The runtime tuning surface (`Engine.search_params`
/ `set_search_params(dict)` / `reset_search_params`) is now live but
unused on production paths — defaults read from `hexo.toml` exactly
as before.

**Phase B mid-sprint headline (pre-revert):** depth 6 → 7 at the
same 500 ms time budget with LMR (3,4,2), iai-callgrind midgame_12
d6 **−55.3 % ins** (2.110 G → 943 M), reference node counts dropped
dramatically (e.g. single_origin d7: 1.73 M → 202 k = −88 %). The
search-shape change was real and substantial — but the arena
disagreed (see decision below).

### Phase A — Runtime tuning surface (5 commits)

`Engine.search_params` / `set_search_params(dict)` / `reset_search_params`
mirror the existing `set_eval_overrides(dict)` pattern. Sprint 4A
exposed the LMR triplet; Sprint 4C extended to aspiration + extension
knobs. The plan assumed `pvs_node` referenced `LMR_*` constants
directly and a `*_DEFAULT` rename + `TunableParams` struct would be
required — in reality `SearchConfig` already had the fields populated
from `Default::default()` and the hot path already read `cfg.lmr_*`.
Phase A simplified to a pure Python-surface addition. Verified
zero-codegen impact via iai-callgrind (byte-identical instruction
counts) and byte-identical reference node counts at default params.
400 g arena vs .bestref: 200-200-0 exact tie (raw +0.0 Elo, CI ±34),
confirming behaviour-preserving.

### Phase B — LMR Texel retune (4 commits + 1 fix)

`scripts/tune_lmr.py` harness drives a three-stage Texel sweep over
the LMR triplet via the new `HEXO_SEARCH_PARAMS` env var (Sprint 4A
counterpart of `HEXO_EVAL_OVERRIDES`).

- **Stage 1** (24 cells × 80 g × 250 ms): reduction=2 dominated.
  Apparent leader (3,6,2) at corrected +92.8, CI [+24, +182].
- **Stage 2** (top-5 × 400 g × 500 ms): apparent leader (3,6,2)
  **reversed** to −6.1 raw — Stage 1's CI ±75 had been masking
  noise. (3,4,2) survived at +24.4 raw same-source-tree A/B (no
  worktree, so no correction factor).
- **Stage 3 v1** (400 g vs .bestref): showed +39.3 Elo BUT post-match
  inspection found both main and worktree `.so` files were
  byte-identical (md5 match). Root cause: pre-existing PGO build bug.
- **PGO build bug** (commit `1341ba4`): `scripts/pgo_build.sh` ran
  `maturin develop` without `VIRTUAL_ENV` set; maturin fell back to
  `python` on PATH; when called from `setup_worktree.sh` after its
  `deactivate`, PATH had main `.venv/bin` first — so **worktree's
  maturin installed the worktree's PGO'd .so into MAIN .venv**.
  Empirically reproduced: `HEXO_PGO=1 setup_worktree.sh` changed
  main .so's md5 to match worktree's. Fix: `export VIRTUAL_ENV="${VENV_DIR}"`
  before maturin calls in `pgo_build.sh`. **Pre-existing latent issue
  since Sprint 2A** — all prior Sprint 1/2/3 arena measurements may
  be subtly contaminated; the empirically-fitted −10 correction factor
  was derived with the bug present.
- **Stage 3 v2** (clean, post-fix): 207-193-0, raw +12.2 Elo / corrected
  +2.2, CI95 raw [−21.8, +46.2]. .so files distinct throughout (verified
  md5 mid-match). Marginally above the corrected mean ≥ 0 promote bar.
- **Final SPRT** (clean, after committing winner): 194-204-2, raw −8.7
  Elo / corrected −18.7, CI95 raw [−42.7, +25.3], llr inconclusive.
  Disagrees with v2 by ~21 Elo — within one 400 g CI (±34), but means
  the underlying Elo is statistically indistinguishable from zero
  (pooled v2 + final: raw ~+1.7 / corrected ~−8.3 Elo at ~800 g,
  CI ±24).

Decision: **REVERT** the `hexo.toml` LMR change. Same-source Stage 2
strongly favoured (3,4,2) at +24.4 raw, but two clean 400 g arenas
against `.bestref` give pooled +2 raw / −8 corrected Elo — not
distinguishable from noise. Possible explanation: the more aggressive
LMR pruning loses more tactical sight in some lines than it gains in
nominal depth, even though bench-quick reports +1 nominal ply. Keep
all infrastructure (Phase A pybind, Phase C extension, Phase B's tune
harness + spec + bug fix). Sprint 5 should re-attempt LMR tuning at
higher sample size (≥ 800 g per Stage 3 cell) before promoting.

### Phase C — Generalised override (3 commits)

Extended the search-params dict to include `asp_window_initial`
[1, 10 000], `asp_window_widen_factor` [2, 16], `max_check_extensions`
[0, 32], `qsearch_max_plies` [0, 32]. Pure additive; same hot-path
neutrality as Phase A. Unlocks Sprint 5+ tuning sweeps across these
parameters without further infrastructure cost.

### Phase D — SB-perf re-baseline (SKIPPED)

`bots/external/sealbot_perf` adapter not present in repo. Documented
in `/tmp/sprint_4/D_sb_perf_skip.md`. External validation deferred
to Sprint 5.

### Phase E — Staged movegen 2.5 (DEFERRED)

Optional per plan §7.1. Sprint 4 wall-clock budget exhausted by
PGO-bug investigation + double Stage 3 run. Sprint 5 handoff:
predicted +5-13 % NPS via hi-bucket pre-emission in pvs_node Stage 2.5.

### `.bestref` decision: UNCHANGED

After Phase B's LMR change was reverted, the remaining Sprint 4
content is pure infrastructure: pybind `set_search_params(dict)` surface
covering LMR / aspiration / extension knobs (Phase A + C), the
`tune_lmr.py` harness driving HEXO_SEARCH_PARAMS env-var sweeps
(Phase B), the `pgo_build.sh` cross-venv contamination fix (side
quest), and the spec touchpoints. None of this changes default
search behaviour — iai-callgrind byte-identical at default params,
bench-quick 942-946 k NPS recovered to baseline ±1 %, reference
node counts byte-identical at default params.

`.bestref` **does not advance**. Sprint 4 outcome B per plan F.5:
"Keep commits. `.bestref` unchanged. Investigate B/C/E for failure."
The infrastructure value remains; the LMR-shape claim awaits Sprint 5
re-attempt at higher sample size.

### Updated hotspot bands (post-Sprint 4)

Defaults restored to Sprint 3 close LMR (3,6,1). bench-quick recovered
to 944 k mean NPS / depth 6 / 4466 cyc/node at 500 ms midgame_12 —
within ±1 % of Sprint 3 close, all the place_for_search / history-flat
/ axis-bitmap unchecked optimisations of Sprint 3 remain dominant.
No new hotspot bands shifted from Sprint 3 close.

### Sprint 5 handoff (top 5)

1. **LMR retune at higher sample size** — Sprint 4's 400 g Stage 3
   was too noisy (CI ±34) to distinguish small effects. Re-run with
   ≥ 800 g per Stage 3 candidate. (3,4,2) and (3,12,2) are the most
   plausible candidates from Sprint 4 Stage 2. Same-source Stage 2
   for (3,4,2) was +24.4 — the gap to vs-`.bestref` pooled +1.7 raw
   suggests either a small true Elo gain or the worktree-PGO penalty
   is larger than the −10 correction factor.
2. **PGO correction factor re-calibration** — with the Sprint 4B
   cross-venv contamination bug fixed, the empirically-fitted −10
   correction may be off. Re-baseline by running `make vs`
   HEAD-vs-HEAD (current `.bestref` against itself) at 800 g to
   measure pure worktree-PGO variance.
3. **Phase E staged movegen 2.5** — hi-bucket pre-emission in pvs_node.
   Predicted +5-13 % NPS. Plan §7 in the Sprint 4 prompt has the full
   design. Sprint-4-deferred.
4. **Aspiration / extension retune** — unblocked by Sprint 4C's
   override surface. Same 3-stage Texel protocol; no infra cost.
5. **SealBot-perf adapter setup + 200 g re-baseline** — external
   validation of Sprint 3 + Sprint 4 cumulative Elo gain. Sprint 4D
   deferred (adapter absent).

# Hotspots — Sprint 3 (place_for_search + history flat + axis_bitmap unchecked)

## Sprint 3 status (2026-05-26)

Three of seven plan phases landed (A design + B place_for_search + C
history flat + D axis_bitmap unchecked). Phases E (LMR retune) and F
(staged 2.5) deferred to Sprint 4 with documented rationale. Outcome A
secured on throughput; .bestref advance per final 400 g gate
(decision section below).

**Headline:** **+39.2 % bench-quick NPS cumulative** (683 k → 951 k
mean @ 500 ms midgame_12). iai-callgrind midgame_12 d6 -25.4 %
instructions (2.83 G → 2.11 G), midgame_30 d6 -20.4 % (512 M → 408 M).
LL-hit reduction -57.9 % on midgame_12 — flat-array localities
removed the hashbrown / boundscheck-induced speculative-load misses
the Sprint 2 flamegraph flagged. Reference node counts byte-identical
across all 32 rows (4 fixtures × 8 depths) — every gain pure
throughput, no search-behavior change. All 255 unit/integration tests
+ 4 new chimera-position oracle tests pass.

Per-phase arena measurements vs `.bestref` (`cac186e`):
- Phase B 400 g: 211-189-0, raw +19.1 / corrected +9.1 Elo (gate PASS).
- Phase C 200 g: 116-83-1, raw +57.9 / corrected +47.9 Elo (gate PASS).
- Phase D 400 g: 203-196-1, raw +6.1 / corrected -3.9 Elo. Strictly
  fails -30 corrected gate but evidence overwhelming for "at least
  same strength": byte-identical refs, +3 % NPS, -1.65 % iai ins.
  Accepted under user escalation policy.

### Phase B contract-fix lesson

Sprint 2C's `place_for_search` failed because it inherited the normal
`place()`'s `candidates.remove(c)` without the symmetric undo re-add,
progressively corrupting `candidates` over a search descent.

Sprint 3 used a design pass first: contract audit of every outer-state
reader (3 search-internal `is_legal` sites in `search.rs`, 0 search/
eval/threats/movegen callers of `Board::candidates()`), then chose
architecture A2 (two-variant `is_legal` with `is_legal_during_search`
that walks `history` O(n) instead of probing `proximity.outer`).

The critical simplification: **no `resync_outer_proximity` needed**.
The audit revealed search is balanced — every `place_for_search` is
matched by exactly one `undo_for_search` before returning to caller —
so outer state at search exit ≡ outer state at search entry, still
valid for the root position. Sprint 2C's bug was the unbalanced
candidate removal, not the outer-count drift. Skipping candidates
maintenance uniformly (including the remove at place_for_search start)
removes the asymmetry by construction.

### Phase E / F deferral

Both deferred to Sprint 4. Phase E (LMR retune) needs a runtime
`Engine.set_lmr_params(...)` Python API; `hexo.toml` is compile-time
constant via `build.rs`, so per-cell rebuilds would take ~3-4 hr for
the full 24-cell × 80 g Stage 1 + top-5 × 400 g Stage 2 + winner ×
400 g Stage 3. Building the runtime setter is the right Sprint-4
seed task — it cuts the tune wall-clock to ~10 min.

Phase F (staged 2.5 hi-bucket pre-emission) deferred because
Outcome A was already secured by B+C+D and Phase F's tree-shape
change carries non-trivial regression risk for a +5-13 % NPS upside
that is marginal against Sprint 3's already-substantial +37 % NPS.

### `.bestref` decision: ADVANCE

Sprint 3 final 400g vs `.bestref` (cac186e): **211-188-1, raw +20.0
Elo, corrected +10.0, CI95 corrected [-24.1, +44.1]**. Strict plan
G.5 outcome A asks corrected CI lower ≥ 0; observed -24.1 fails
strictly. However the corrected mean +10 matches Sprint 1's promote
threshold (memory recalibration), bench-quick NPS gain is +39.2 %
(massively exceeds the +10 % outcome A floor), and reference node
counts cross-sprint are byte-identical (full search-behavior
preservation). External SealBot-perf 100g: 6 % winrate, equal to
historical ~5 % baseline with positive mid-sprint → close trend
(4 % → 6 %).

PROMOTE. `.bestref` advances cac186e → Sprint 3 HEAD per user
escalation policy (mean positive + strong evidence elsewhere =
"at least same strength" satisfied).

### Updated hotspot bands (post-Sprint 3)

Flamegraph-relative self-time (no fresh capture this sprint; numbers
extrapolated from Sprint 2 close + the iai-callgrind delta):

- Proximity walk: ~5-7 % → **~1-2 %** (place_for_search skipped r=8
  walk entirely during search; only inner r=2 retained)
- `is_occupied` / `is_set` bounds-check stubs: gone (Sprint 3D)
- `record_cutoff` → FxHashMap probes: gone (Sprint 3C flat history)
- Layer-1 eval: ~10-15 % (unchanged — eval is now the dominant
  fraction post-place_for_search)
- TT probe: ~5-8 % (unchanged)
- Threats reconcile: ~10 % (unchanged — cold-path)

The post-Sprint-3 bottleneck shifts toward eval (which Sprint 4
LMR retune may exploit by reducing the eval call count).

## Sprint 4 handoff (ranked by Elo lever)

1. **Runtime `Engine.set_lmr_params` Python API** + full Phase E
   LMR retune (Sprint 3 carryover, ~10 min after setter lands).
2. **Phase F staged 2.5 hi-bucket pre-emission** in `pvs_node`
   (Sprint 3 carryover; predicted +5-13 % NPS).
3. **SB-perf re-baseline** at 200 g to confirm Sprint 3's external
   Elo (internal arena shows clean wins on Phase B+C, ambiguous on
   D; 100 g mid-sprint snapshot was 4 % vs ~5 % historical — within
   noise but unconfirmed at higher resolution).
4. **Runtime SearchConfig setter generalised** beyond LMR — same
   pattern unlocks aspiration widen-factor + extension-cap as
   runtime knobs (broadens Sprint 4 tune surface beyond LMR).
5. **Threats classification cache** (D #4 from Sprint 2 verdict) —
   defer until LMR + Phase F land.

## Sprint 2 status (2026-05-25)

Six-item Elo-aimed sweep from the Sprint 1 handoff verdict. Three
landed (D bounds-elim bundle, E SparseCellSet u16 slot, G EvalCache
align). Three aborted (C outer-halo skip, F row-major reorder, H
RefCell unsafe shortcut) — each by its own falsification branch, not
by surprise. `.bestref` UNCHANGED (`cac186e`); Outcome C of plan
§ I.5 (partial — keep commits, no `.bestref` advance).

**Headline:** +3.3% bench-quick NPS cumulative (672 k → 694 k mean @
500 ms midgame_12). 200 g vs `.bestref`: 96-103-1, **Elo -12.2,
Wilson CI [-60.2, +35.9], SPRT continuing/INCONCLUSIVE**. Phase B
correction (-10 Elo for residual worktree-PGO NPS handicap) → -22
Elo corrected mean; CI [-70, +26] crosses zero. iai-callgrind
midgame_12 -7.0 % instructions (3.04 G → 2.83 G), midgame_30 -5.6 %
(542 M → 512 M), -1366 ins/node — the iai win is real and
deterministic; the arena landing in the noise mid-zone is consistent
with a behaviour-preserving change at +3.3 % NPS giving no
statistically demonstrable Elo movement.

The big asterisk this sprint: **Phase B hypothesis check**
disambiguated Sprint 1's +52 Elo result. With both sides PGO-built
at identical source code, the residual gap is +10.4 Elo
(INCONCLUSIVE at 200 g), placing Sprint 1's +52 firmly in the H2
(artefact) bucket. Sprint 1 produced a real ~+12 Elo of code-side
gain (PGO + TT prefetch on this codebase), not the headline +52.
The Sprint 1 retro NPS / PGO methodology stands; the Elo
interpretation should be recalibrated. All Sprint 2+ arena
measurements apply a **-10 Elo correction** to extract code-side
delta from the persistent worktree-PGO NPS-only asymmetry.

### 2A — Worktree-PGO opt-in (Sprint 1B deferral closed)

Wires `HEXO_PGO=1` through `setup_worktree.sh` → `pgo_build.sh`
(parameterised via `HEXO_PGO_ROOT` / `HEXO_PGO_VENV` /
`HEXO_PGO_ENGINE_DIR` / `HEXO_PGO_TARGET_DIR`). Default ON for
`make vs` / `make promote`. Plus a `cp -f` workaround for pip 26's
wheel-skip behaviour (maturin's `install` step silently no-ops on
unchanged version → installed .so doesn't get the PGO bytes). All
Sprint 2+ arena measurements are now PGO-symmetric, give or take
PGO training non-determinism (~7 % NPS swing run-to-run on
identical source — Phase A measured 668 k main vs 620 k worktree).

Commits: `6ba9432` (pgo_build.sh parameterise), `0a56db2`
(setup_worktree opt-in), `beef469` (Makefile HEXO_PGO=1 default).

### 2B — Hypothesis check (no commits, measurement only)

200 g PGO-vs-PGO arena at identical `.bestref` source: **+10.4
Elo, CI [-37.6, +58.4], INCONCLUSIVE**. Verdict: H2 (artefact)
dominates. Sprint 1's headline +52 reduces to ~+12 Elo code-side
gain after applying the worktree-handicap correction.

Apply -10 Elo correction to all future arena measurements vs
`.bestref` (until a future re-bench measures the residual at a
different value).

### 2C — `place_for_search` outer-halo skip — **REJECTED**

Implemented the `place_for_search` / `undo_for_search` variants
that skip the r=8 outer-halo walk per the D #1 verdict from
`analysis/baseline_ae539b7/D_algorithmic.md`. Initial iai showed
**-27.7 % midgame_12 instructions**, bench-quick **+37 % NPS** —
far above the +6.7 % predicted. Then the gate hit:

- Reference node counts: midgame_12 / midgame_30 / single_origin
  byte-identical; **empty fixture deviated** 1-8 % at depths 4-8.
- 100 g light arena vs `.bestref`: **40-60-0, -70.4 Elo, CI
  [-139.5, -1.4]**. Hard fail.

Root cause: `is_legal` reads `proximity.outer_at` inside search
(TT-move probe at search.rs:461, killer-move probe at search.rs:493,
qsearch TT-move at search.rs:954). With the outer walk skipped, the
count is stale-relative-to-search-state. Even with an `is_legal`
fallback to `inner_at` (which catches r ≤ 2 of search-added pieces),
TT / killer moves at r = 3..r = 8 of search-added pieces fall into
the falsely-rejected bucket. Fixed-depth reference fixtures don't
trigger this on midgame positions (the top-level halo already
covers the search frontier), but real game play does, because TT
entries persist across game turns and reference moves that may now
sit outside any current piece's top-level halo.

Reverted all changes; no commits landed. Sprint 3 reattempt
candidate — see § handoff below for design.

### 2D — Bounds-elim + inline bundle (A P-1/P-2/B-4, LC-1)

Three commits unchecking the proximity / line_contrib hot-path
indexing + forcing `for_each_in_range` to `#[inline(always)]`:

- `1e8d439` `proximity: unchecked indexing on SparseCellSet + counts`
- `4ea2435` `line_contrib: unchecked invalidate`
- `e54c4f5` `coords: for_each_in_range inline(always)`

`Board::place` body lost 6 of ~10 panic_bounds_check stubs (4
remaining trace to `AxisBitmaps::is_occupied` — out-of-scope, filed
for Sprint 3). iai-callgrind delta vs Sprint 1 close:

| Bench | Sprint 1 ins | Phase D ins | Δ ins/node |
|-------|-------------:|------------:|-----------:|
| midgame_12 d6 (155 k nodes) | 3,042 M | 2,830 M | **-1,366** |
| midgame_30 d6 (20 k nodes) | 542 M | 512 M | -1,488 |

100 g light arena: **+41.9 Elo, CI [-26.3, +110.0]** — within the
±50 acceptance band (gate cleared comfortably). Reference node
counts byte-identical for all 4 fixtures.

### 2E — `SparseCellSet.slot` u32 → u16

Single commit (`ac9f245`). Halves the per-set slot footprint
(290 KB → 145 KB at `ZOBRIST_WINDOW=127`); both sides' candidate
sets now total 580 KB vs 1.16 MB — comfortably L2-resident with
proximity counts + threat scratch on neighbouring lines. Range
audit: `members.len()` is bounded by live cells, peaks well under
u16 capacity (65,535) on realistic boards. `insert` debug-asserts
the pre-bump length to catch any future config bump (e.g.
`ZOBRIST_WINDOW=255`) that would invalidate the assumption.

bench-quick +2.2 % over Phase D, reference byte-identical.

### 2F — `for_each_in_range` row-major reorder — **ABORTED at F.1**

Falsification branch fired at the asm-pre-check step. The D
verdict's "271-byte stride per inner step" claim assumed a
transposed `prox_idx` layout. The actual layout is
`q * 271 + r` — outer dq / inner dr already walks **row-major
with stride-1 within rows**. Swapping dq/dr would have produced
the **column-major** (stride-271) pattern the verdict was trying
to avoid. No code change; documented in Sprint 3 handoff as
resolved-as-no-op for a simple loop swap.

### 2G — `EvalCache repr(align(64))` (verdict #12c)

Wraps `Board::eval_cache` in a 64-byte-aligned `EvalCache` struct
so it doesn't share a cache line with frequently-mutated neighbours
(`threats_dirty`, `line_contrib`). Cleanup-pass cachegrind verdict
attributed 18.3 % of all D1 read misses to `Board::cached_eval`
reads — the line was flushing on every `place`/`undo`.

bench-quick flat at noise (-0.1 % vs Phase E), iai flat at +0.02 %
ins (the canonical fixtures don't reproduce the cachegrind miss
pattern). Reference byte-identical. The win is location-specific
cache locality, visible only in cachegrind. Commit `e9b9eff`
documents the layout intent; future cachegrind regressions on
`cached_eval` are now structurally prevented.

### 2H — RefCell unsafe shortcut — **REJECTED**

Replaced `Ref<'_, ThreatSet>` with `&ThreatSet` via
`&*self.threats_x.as_ptr()` to bypass `BORROW_FLAG` inc/dec.
`cargo asm` confirmed 0 borrow_inc/dec/panic_borrow stubs in the
eval hot path (down from 4-6 baseline). iai measured a real but
tiny win: -0.15 % instructions, -2.8 % LL hits. Both fixtures
showed -0.11 % cycles.

100 g light arena: **-20.9 Elo, CI [-88.7, +46.9]** — CI lower
crossed the -50 acceptance floor. Per the strict-revert clause
("any Elo drift on behaviour-preserving change = UB leak"),
reverted entirely. No commits landed.

The -20.9 point estimate is consistent with single-100 g noise
(±25 Elo half-width), and the iai delta says the change was
genuinely effect-free on asm-level hot work. But the gate is what
it is. Sprint 3 reattempt: use 200 g (not 100 g) light arena;
gate behind `#[cfg(feature = "fast_threats")]` for a longer
validation window; run miri to rule out UB.

## Flamegraph diff vs Sprint 1 close

Captured `flamegraph-2026-05-25T21-20-16-e9b9eff.svg` post-Sprint-2.

**Proximity bundle still visible**: `for_each_in_range
<remove_proximity::closure>` and `<add_proximity::closure>` remain
among the hot frames (Sprint 1 close had them in the top 5).
Phase C would have moved them out; Phase D reduced per-call cost
(-7 % ins) but didn't remove the calls.

**New top-of-flame**:
- Layer-1 window-scan + `encode_ternary_8` is now a visible
  fraction (eval.rs:layer1_window_scan_8cell). Sprint 1's PGO
  shrunk it but it's still in the top user-space hot path.
- Ordering history map (`record_cutoff` → `rustc_entry::find` on
  `FxHashMap`) is hot. The history table touches at every cutoff,
  one FxHashMap probe + insert per visit. Sprint 3 candidate
  (move to a flat `[u32; INNER_HALO_SIZE × players]` table or
  similar bounded structure — see ordering.rs `record_cutoff`).
- `tried.contains` (SmallVec scan in `pvs_node`) is hot at depth
  ≥ 5. `tried` holds ≤ 3 elements; linear scan IS the right
  choice, but the leaf shows up because it's called 30+ times per
  node. Probably not worth optimising further on its own.

Self-time bands (qualitative, perf-noisy):
- Proximity walk: still ~5-7 % of cycles (was ~8 % at Sprint 1
  close; Phase D shaved ~1 %)
- Layer-1 eval: ~10-15 % of cycles (post-PGO band, unchanged)
- TT probe / FxHashMap-on-ordering: ~5-8 % of cycles
- Threats reconcile: ~10 % (cold-path)

## Sprint 3 handoff (ranked by Elo lever)

1. **Place-for-search reattempt** — Phase 2C showed +37 % NPS upside;
   the failure mode was `is_legal` reading stale `proximity.outer`.
   A clean reattempt needs an architectural change: either drop
   `outer_candidates` from `Board` entirely (and rework `is_legal`
   to a direct piece-distance check), or maintain a search-internal
   secondary count.
2. **Ordering history flat table** — flamegraph confirms the
   `record_cutoff` → FxHashMap pattern is now in the top user-space
   slots. The history is per-(coord, player) → u32. Coord is bounded
   to the proximity field; could move to a flat array indexed by
   `prox_idx + player_offset`. Cost: ~140 KB per board × 2 players.
3. **AxisBitmap unchecked indexing** — `is_occupied`'s 2 bounds
   checks per call survive Phase 2D's elim pass. Estimated
   +1-2 % NPS.
4. **LMR retune** — Sprint 1 unblocks; iai gate plus Sprint 2's
   PGO-symmetric arena lets sub-1 % Elo measurement on individual
   reduction-bucket tweaks (was the top non-bundle candidate from
   the original verdict).
5. **Staged movegen stage 2.5** (D #2) — Phase 26 R-01-style
   pattern at one level deeper. +13 % NPS predicted, +0 Elo direct
   but depth-knock-on positive.
6. **Threats classification cache** (D #4) — defer until 1/2 land.

Out of Sprint 2 scope (re-confirm in Sprint 3 prompt):
symmetry/canonicalisation, opening book, mate DB, BOLT.

---

# Hotspots — Sprint 1 (Free-wins bundle: iai-callgrind + PGO + TT prefetch)

## Sprint 1 status (2026-05-25)

3-item bundle from `analysis/baseline_ae539b7/verdict.md` (Free Wins
group). All three landed; `.bestref` advanced.

**Headline:** +7.4% bench-quick NPS (622k → 668k @ 500 ms midgame_12)
cumulative across the sprint. 200 g vs `.bestref` (cfefb3b @ 500 ms):
115-85-0, **Elo +52.5, Wilson CI [+4.0, +101.1]**. SPRT inconclusive
(llr +0.822 inside ±2.944), but the Wilson lower-bound > 0 criterion
(plan § 7.3) cleared, so manual promote per plan § 8.3. `.bestref`
advanced cfefb3b → `cac186e`.

### 1A — iai-callgrind deterministic gate

Adds `hammerhead-engine/benches/iai_search.rs`: two fixtures (midgame_12,
midgame_30) at depth 6, instrumented under valgrind callgrind. Two
consecutive runs **byte-identical** (3,041,711,776 ins midgame_12 / 542,394,020
ins midgame_30). Resolves sub-1% regressions in 40 s vs the multi-hour
arena previously required (Phase 26.5 / 28F-2 noise-bound at 200 g × 500 ms).

Host caveat: `target-cpu=native` on Zen2+ emits sha-ni / rdpru / etc. which
valgrind 3.25.1 cannot translate (SIGILL). `make bench-iai` overrides
`RUSTFLAGS=-C target-feature=+avx2,+bmi2,+fma,+popcnt,+sse4.2`. AVX2-baseline
codegen differs slightly from the deployed binary; iai is for *relative*
regression detection, not absolute modelling.

Commits: `75899b3` (spec), `caf6cf3` (bench + Cargo.toml + Makefile).

### 1B — PGO shipped

`make pgo` now the canonical release build path. Re-runs the 4-pass
instrumented → training → merge → optimized pipeline (~3 min wall-clock).
`scripts/pgo_build.sh` patched to export `CARGO_TARGET_DIR=hammerhead-engine/target-pgo`,
isolating PGO builds from the main `target/` (used by `make build`,
`make bench-iai`). Merged profile is 5.6 MB.

**+5.0% bench-quick NPS** (622k → 653k mean of 3 runs). Verdict predicted
+5.4%; observed +5.0% — in band. Reference node counts byte-identical
(PGO doesn't change algorithm, only codegen). 100 g vs .bestref: +52.5 Elo,
CI lower -15.9 (gate ≥ -50, clears comfortably).

Worktree-PGO opt-in skipped per plan § 11 fallback. `pgo_build.sh`
hardcodes `.venv` (worktree uses `.venv-best`), and the worktree at
cfefb3b has no `rust-toolchain.toml`. Cross-version invocation too
fragile for sprint scope. Worktree stays non-PGO; documented ~+3 Elo
asymmetric bias toward HEAD in time-budgeted arena.

Commits: `8320aa1` (spec), `a69ed56` (script + gitignore).
`make pgo` target already existed (Phase 14, Makefile:117), no new commit.

### 1C — TT prefetch on child probe

Added `TranspositionTable::prefetch(hash)` → `_mm_prefetch::<_MM_HINT_T0>`
on the bucket pointer (x86_64 only, no-op stub elsewhere). Hint-only;
cannot fault, cannot race, cannot alter correctness. Wired at all
three post-`board.place(m)` sites in `search.rs`:

1. `try_one_move` (PVS main move loop)
2. `quiescence_node` (qsearch TT-move attempt)
3. `quiescence_node` (qsearch threat-move loop)

Phase-25 guardrail satisfied: `cargo asm` confirms **3 `prefetcht0`
instructions** across `try_one_move` (1) + `quiescence_node` (2). LLVM
did not elide the intrinsic.

**+2.3% bench-quick NPS** vs 1B PGO baseline (653k → 668k). Verdict
predicted +1.5-3%; observed +2.3% — in band. iai-callgrind midgame_12:
**LL hits -1.4%** (the DRAM-hiding signal), instructions +488 k (the
prefetch ops themselves). Reference node counts byte-identical.
100 g arena: +63.2 Elo, CI lower -5.6 (clears -50 gate).

Commits: `33ebb3c` (spec), `cac186e` (impl).

### Aggregate

| Phase     | bench-quick NPS | Δ vs Phase 0 | Δ step |
|-----------|----------------:|-------------:|-------:|
| baseline  | 622 k          | —            | —      |
| post-1A   | 623 k          | +0.2%        | —      |
| post-1B   | 653 k          | +5.0%        | +5.0%  |
| post-1C   | **668 k**      | **+7.4%**    | +2.3%  |

| Bench (post-1C iai)    | ins         | LL hits   | RAM hits | Est cyc      |
|------------------------|------------:|----------:|---------:|-------------:|
| midgame_12             | 3,042,199,968 | 3,383,540 | 44,434  | 3,897,481,285 |
| midgame_30             | 542,468,100   | 594,353   | 10,642  | 696,573,982   |

### `.bestref` decision

**ADVANCE** cfefb3b → `cac186e`. Manual promote per plan § 8.3
(SPRT was INCONCLUSIVE; Wilson lower-bound was the binding criterion).

### Sprint 2 handoff (ranked)

Ordered by Elo lever, not NPS lever. Sprint 1 was NPS-only; Sprint 2 is
the Elo-aimed sweep.

1. **Proximity bundle** (D #1, A P-1/P-2, B #1, B #3 from
   `analysis/baseline_ae539b7/`). The hot ordering / candidate-buffer
   path is the largest single self-time sink per HOTSPOTS Phase 28E-2.
2. **eval_cache `repr(align(64))` split** — false-sharing audit on the
   inner-loop eval cache lines. Verdict #12c.
3. **LMR retune** — Sprint 1 unblocks this by providing the iai gate
   needed for sub-1% Elo measurement on individual reduction-bucket
   tweaks.
4. **RefCell unsafe shortcut** (E #1) — small NPS win, surveying-only.

Out of Sprint 1 scope per plan § 10: symmetry/canonicalisation,
opening book, mate DB, BOLT (still deferred per verdict).

---

# Hotspots — Phase 28E-2 (cluster shape falsification + opening diversity)

## Phase 28E-2 status (2026-05-25)

Phase 28E-2 ran two stages: Stage 0 (opening diversity, 20 HeXOpedia §6
openings, harness infra) + Stage 1 (rhombus cluster detector, 3-sweep
arc Step 0 → 1 → 3). Stages 2 (bone) + 3 (arch/trapezoid) SKIPPED per
user direction after Stage 1 reached NO-LAND × 3 sweeps with empirical
falsification of the cluster-detector lever. 4 code/spec commits on
master + 1 on hexo-arena main + 2 doc this retro.

**Headline: KEEP all 4 commits on master; `.bestref` NOT advanced.**
NO weight applied (detector dormant at default `rhombus = 0`); no
engine behavior change → no arena gate run. E-0 VERIFY baseline (HH
2.0% per-stone 500ms vs SB-perf) stands unchanged. `.bestref` stays at
`5bd89648` (Phase 28D-1 advance).

### Stage 0 — opening diversity (HH `a1245a1` + arena `d6b91ba`)

Pair-seeded curated openings (`pick_opening(i // 2)` shares opening
across both games of pair `k`, swap colors). 20 HeXOpedia §6 entries:
18 named (Pair, ClosedGame, ClosedMainLine, Longsword, Shortsword,
Sword, Wrongsword, Pistol, Shotgun, Revolver, PistolSnail, IslandGambit,
NearIsland, Marge, Eclipse, C_and_B, PairSideStep, PairCShift) + 2
explicit control variants (`Control_A0A4`, `Control_A2A5`). Two
`NotImplementedError` raises at `promote.py:372-376` + `:553-557`
DELETED. Arena `python/hexo_arena/arena/opening.py` mirrors catalog as
flat tuples + `--opening hh:curated` CLI hook. 29 new HH tests + 9 new
arena tests. SPEC_BENCHMARKS updated.

Smoke validation: 15 distinct game lengths across 20-game self-play
(75% trajectory uniqueness); 10 distinct openings hit per full catalog
cycle; per-pair color symmetry holds; pair-seed determinism holds.

**S0-REV PASS-WITH-MINOR** (3 minor items carried to retro: docstring
nit, INTEGRATION_NOTES operator-doc gap, optional Shotgun/Revolver
BKE→axial fidelity).

**Attribution**: Stage 0 is measurement infrastructure, NOT strength.
No expected arena delta. Real value: unblocks DIAG-1 fixed-depth
determinism collapse for future eval-isolated A/Bs.

### Stage 1 — rhombus detector + 3-sweep arc (HH `042f020` + `6d57f8e` + `5295561`)

#### Detector design

`threats.rs::detect_rhombi`: pattern = 4 cells whose 6 pairwise
distances are `{1,1,1,1,1,2}` (canonical rhombus = 3 mutually adjacent +
4th forming parallelogram). Rotation- and reflection-invariant by
construction; all 12 orientations covered. Algorithm: for each own piece
anchor P, enumerate 6 adjacent-pair rotations (u, v); test `P + u + v ∈
own_coords`; emit sorted 4-tuple. Dedup via `FxHashSet<[Coord; 4]>` (each
rhombus emitted up to 4 times across anchors). Isolation: centroid =
mean of 4 cells; reject if any opp piece within `rhombus_isolation_radius
= 3` (Ring C per HeXOpedia Radius Theory). Detector gated on
`rhombus_weight != 0` (short-circuits all work at default).
`ThreatCounts.rhombus` field; `layer2_shapes` adds `ov.rhombus *
c.rhombus` to existing per-axis Layer-2 sum. 13 → 15 tests across
detector + Step 3 isolation-correctness updates.

#### Sweep arc — 3 runs × 5 cells = 15 cells, NO positive cell

All sweeps: Stage B, `param=rhombus`, 200 games/cell, 500ms/stone,
10 workers, `opening_diversity=ON`.

**Step 0 (vertex centroid, akra grid)** — `042f020` HEAD:

| Cell  | W-L-D    | Elo   | Wilson 95% CI       |
|------:|----------|------:|---------------------|
| 22500 | 84-116-0 | -56.1 | [-104.7,  -7.4]     |
| 33750 | 89-111-0 | -38.4 | [-86.7,   +9.9]     |
| 45000 | 100-100-0|   0.0 | [-48.0,  +48.0]     |
| 67500 | 83-117-0 | -59.6 | [-108.3, -10.9]     |
| 90000 | 91-109-0 | -31.4 | [-79.5,  +16.8]     |

V-pattern. Anchor 45000 flat at 0; both flanks negative; two cells
statistically-significantly negative (CI fully below 0).

**Step 1 (vertex centroid, neg-pos grid)** — Q2 falsification of
double-count hypothesis:

| Cell    | W-L-D    | Elo   | Wilson 95% CI       |
|--------:|----------|------:|---------------------|
| -22500  | 80-120-0 | -70.4 | [-119.4, -21.5]     |
| -11250  | 76-124-0 | -85.0 | [-134.5, -35.6]     |
|      0  | 96-104-0 | -13.9 | [-61.9,  +34.1]     |
| +11250  | 77-123-0 | -81.4 | [-130.7, -32.1]     |
| +22500  | 83-117-0 | -59.6 | [-108.3, -10.9]     |

Symmetric-negative around 0. **Double-count REFUTED as primary
mechanism**: if double-count were the cause, negative weights would
cancel per-axis over-count and improve play; they degrade WORSE than
positive (-70 to -85 vs -60 to -85). Rhombus positions are genuinely
good; negative weight makes search AVOID strong positions.

**Step 3 (cube-round centroid, akra grid)** — `5295561` HEAD,
isolation-correctness fix per S1-REV Check 2 MAJOR. Replaced
per-component `round_div4` (axial centroid lands on rhombus vertex,
asymmetric Ring-C reject zone) with cube-round centroid (Red Blob Games
§"Rounding", restores symmetric reject zone). 5 existing negative tests
updated; 2 new tests cover asymmetric-far-side case.

| Cell  | W-L-D    | Elo   | Wilson 95% CI       |
|------:|----------|------:|---------------------|
| 22500 | 92-108-0 | -27.9 | [-76.0,  +20.3]     |
| 33750 | 83-117-0 | -59.6 | [-108.3, -10.9]     |
| 45000 | 81-119-0 | -66.8 | [-115.7, -17.9]     |
| 67500 | 81-119-0 | -66.8 | [-115.7, -17.9]     |
| 90000 | 75-125-0 | -88.7 | [-138.3, -39.2]     |

Monotonic-negative. Cross-sweep cube-round delta: cell 22500 +28, cells
45000 -67, 90000 -57; WORSE on average than Step 0. **Isolation
geometry REFUTED as root cause** — fixing centroid to a more
theoretically-faithful algorithm did NOT flip any cell positive.

#### Final mechanism reading

Three candidate mechanisms diagnosed in sequence, each falsified:

1. **Step 0 V-pattern**: consistent with "any weight disrupts; akra
   anchor is the noise floor".
2. **Step 1 symmetric-negative**: double-count refuted (negative
   weights worse than positive — opposite of pure double-count
   prediction). Note: `eval.rs:374-383` `layer2_shapes` DOES
   structurally add `ov.rhombus * c.rhombus` to per-axis sum without
   exclusion (S1-REV Check 3 CRITICAL: canonical rhombus generates 5
   open_2 firings = 56250 Elo before any rhombus weight, refining
   DIAG-2's 3-firing estimate to 5). But Step 1 refutes double-count
   as the load-bearing mechanism.
3. **Step 3 monotonic-negative (cube-round)**: isolation geometry
   refuted.

**Final diagnosis**: cluster-shape revival as a standalone Layer-2
detector is empirically falsified for rhombus in current HH eval
architecture. Per-axis S1 sum + Layer-1 buckets already implicitly
evaluate the rhombus structure (DIAG-2 §"Layer decomposition" refined:
rhombus reads as `5 × open_2`). Adding ANY explicit weight, at ANY
sign, with ANY isolation algorithm, disrupts the eval balance the
existing system encodes.

### Stages 2 (bone) + 3 (arch + trapezoid) — SKIPPED

A-priori: bone / arch / trapezoid share the per-axis decomposability
mechanism with rhombus (bone = 4 open preempts + 2 triples per
HeXOpedia §4.4; arch "functions very similarly to Rhombus" per §4.5;
trapezoid per-axis decomposition similar to bone). Same negative result
expected from 4-6 days of arena time across 4 sweeps. Per user direction
post-Stage-1 falsification: SKIP. Saves 4-6 days for E-3 Path 2B work.

### Self-time band shift: NONE (detector dormant at default)

Detector adds eval% only when `rhombus_weight != 0` (gated). At default
weight = 0:

- `WINDOW_SCORE_8` cache + Layer-2 sum byte-identical pre/post.
- `bench reference` node counts byte-identical (verified `make bench-diff
  A=20260525-010119-6d57f8e.json B=20260525-002223-6d57f8e.json` — all
  `reference / *.d*` rows = 0.0% post-`5295561`).
- `bench-quick` post-`5295561` 558k NPS, Δ -1.0% vs prior (within ±5%
  noise band; gated detector cannot affect hot path at weight 0).

When weight ≠ 0 (sweep cells only): detector adds one extra pass over
own-coord set + 6-rotation enum per anchor + sorted-4-tuple HashSet
dedup, plus isolation check (centroid + Ring-C scan). Cost
proportional to number of own pieces × 6; expected ~1-3% NPS hit at
non-zero weight per analogy to Phase 28D-3 Layer-2 S1 revival. Not
measured for production callers because weight stays 0.

### Arena trajectory (unchanged)

| State | n (HH vs SB-perf 500ms) | Winrate | Wilson 95% CI |
|---|---:|---:|---|
| Phase 28E-0 VERIFY (pre-E-2) | 100 | 2.0% | [0.5, 7.0] |
| Post-Stage-0 (a1245a1) | NOT RUN | — | — |
| Post-Stage-1 (042f020 + 6d57f8e + 5295561) | NOT RUN | — | — |

No arena run because no weight applied. E-0 VERIFY 2.0% stands as
current external-arena state.

### Commits (4 atomic on hammerhead master + 1 on hexo-arena + this doc commit pair)

| SHA | Subject | Type |
|---|---|---|
| `a1245a1` | `harness: implement opening diversity library` | Stage 0 (HH) |
| `042f020` | `eval: implement rhombus detection with isolation` | Stage 1 detector + 13 tests |
| `6d57f8e` | `tune: add --diversity flag to tune-sweep` | Stage 1 instrumentation |
| `5295561` | `eval: cube-round centroid for rhombus isolation` | Stage 1 Step 3 isolation-correctness fix + 2 new tests |
| (this commit) | `bench: HOTSPOTS Phase 28E-2 cluster falsification + diversity` | doc |
| (next) | `spec: mark Phase 28E-2 done in roadmap` | doc |

Plus 1 on hexo-arena main: `d6b91ba` (`adapter: consume opening
diversity from HH harness`).

### Phase 28E-3 handoff

- **Path 2B (SB-perf 729-table port) — PRIMARY**. E-1 SYN Section C
  ranked as Arm B; Stage 1 falsifies Arm A (Path 3). DIAG-4 + DIAG-5
  established M effort (codegen-only, 3-5 commits, 1-3 days). Honest
  caveat: DIAG-2 §"Implication" + DIAG-4 §"Per-axis tables" flag that
  per-axis 729-table cannot fix the load-bearing cluster gap; Path
  2B's expected gain is from denser per-pattern evaluation of LINEAR
  shapes (where HH is already at 100%), not cluster recovery. May
  move external winrate via linear-eval density refinement; will NOT
  close the cluster gap DIAG-2 highlighted.
- **Triangle detection** — NOT revisited. Cluster-detector mechanism
  falsification argues against per-shape Layer-2 revival generally;
  triangle would face same mechanism.
- **Tempo proxy** — still pending (carried 28B → 28C → 28D-1 → 28D-3 →
  28E-0 → 28E-2). TT p.11 "tempo is the most important currency"
  remains strongest PDF evidence of any deferred item.
- **Promote-harness commit bug fix** — still pending from E-0
  (`promote.py` `-m`-after-`--` reorder, trivial).
- **Eval architecture restructure (long-form)**: load-bearing finding
  is per-axis S1 + Layer-1 already implicitly evaluate cluster shapes.
  Restructuring options: (a) DISABLE per-axis S1 from cluster-positions
  (runtime per-position S1-suppression — complex); (b) REPLACE per-axis
  S1 with per-pattern table (≈ Path 2B). Path 2B is cheaper. Open
  question for E-3: if Path 2B fails to move arena, the cluster gap
  may be untouchable without ML-trained eval (Texel pipeline, L
  effort, deferred per E-1 SYN).

### Honest assessment

External arena DID NOT MOVE (no weight applied → no behavior change).
Phase 28E-2 DID empirically falsify Path 3 as a standalone lever via
3 cleanly-diagnosed sweeps spanning vertex/cube-round centroid ×
positive/negative weight space. That is real progress — Path 3 was the
largest unmeasured arm in E-1 SYN's decision matrix; falsifying it
sharpens E-3's path-2B-or-Texel choice. Opening diversity library is
real infrastructure that future eval-isolated A/Bs benefit from
(DIAG-1 fixed-depth determinism collapse can no longer re-trip with
diversity ON). Rhombus detector code (~430 LOC + 15 tests) is real
infrastructure preserved in repo for any future revisit (e.g. if eval
is restructured to subtract per-axis S1 from cluster-positions, the
detector can be reused as-is). Net: ~3-5 days of arena time spent for
high-quality falsification + 2 pieces of reusable infrastructure. Not
a winrate gain, but methodologically clean.

**Artifacts** (gitignored per Phase 25.5):

- `/tmp/phase_28e/PHASE_28E_2_RETRO.md` — full retrospective.
- `/tmp/phase_28e/2/stage-0/{implementer,review}.md` — Stage 0 reports.
- `/tmp/phase_28e/2/stage-1/{implementer,review}.md` — Stage 1 reports
  (Step 0 + 1 + 3 arc).
- `benches/results/tune/rhombus/B/20260524T210750/*.json` — Step 0 sweep.
- `benches/results/tune/rhombus/B/20260524T225139/*.json` — Step 1 sweep.
- `benches/results/tune/rhombus/B/20260525T013817/*.json` — Step 3 sweep.

---

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
