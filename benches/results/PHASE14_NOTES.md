# Phase 14 — sub-step results log

Captured incrementally as each STEP lands. STEP 11 folds these into
the final HOTSPOTS refresh + baseline commit. Cold-cache, single-run
measurements unless noted. Host: AMD Ryzen 7 8845HS, rustc 1.94.0.

| Step | Change | NPS midgame_12 | NPS midgame_30 | Notes |
|---|---|---|---|---|
| baseline | Phase 13 baseline | 237,449 | 128,308 | from `baseline.json` |
| step 0 | release profile + target-cpu=native | — | — | config only; runtime measured in step 2 |
| step 1 | bench scaling + breakdown wiring | — | — | infra; no runtime change |
| step 1.5 | search: depth-only no time cap | — | — | reference-table fix; deep depths now truly fixed-depth |
| step 2 | release profile delta (cold, 1 run × 3 runs avg) | 244,132 (+2.8%) | 133,040 (+3.7%) | `lto=fat` + `codegen-units=1` were already on; pure `target-cpu=native` lift |
| step 3.2 | mimalloc behind feature (cold, 3 runs avg) | 243,729 (−0.2%) | 133,694 (+0.5%) | within noise; feature gate kept, not default-enabled |
| step 3.3 | threats::compute scratch buffer (3 runs avg) | 269,517 (+10.4%) | 144,678 (+8.7%) | hoisted `FxHashSet seen` + `Vec pieces` into `Board.threat_scratch` |
| step 4 | piece_at → is_player single-probe (3 runs avg) | 291,375 (+8.1%) | 159,326 (+10.1%) | `AxisBitmaps::is_player`; threats `matches_pattern` + flank-cell checks |
| step 5 | inline + cold sweep (3 runs avg) | 289,953 (−0.5%) | 160,890 (+1.0%) | trivial; kept for intent / future-proofing terminal paths |
| step 6 | LineBitmap align + windows6_run (3 runs avg) | 325,133 (+12.1%) | 196,156 (+21.9%) | batched 6-bit window extract from u64 words; #[repr(align(64))] keeps lines off cache-line straddles |
| step 7 | incremental threats — **deferred to Phase 15** | — | — | full delta requires per-anchor tracking + a prior-snapshot lifecycle on Board (paired place / undo deltas) that didn't fit the phase budget; oracle test would have caught any partial impl, so reverted before any code shipped. `threats::compute_with_scratch` still accepts the `center` / `prior` hints — they just stay unused, matching today's behaviour. |
| step 8 | SIMD encode_ternary (default `simd_eval`, 3 runs avg) | 343,297 (+5.6%) | 208,103 (+6.1%) | AVX2 16-window batch + scalar fallback; 729-table identity test certifies byte-equality with scalar; default-on after correctness gate |
| step 9 | PGO build (one training run, 3-fixture × depth-6, 3 runs avg) | 334,641 (−2.5%) | 202,105 (−2.9%) | within noise; reverted to non-PGO build for the final baseline. `scripts/pgo_build.sh` + `scripts/pgo_training.py` ship for future runs (richer training workload should help); `rustup component add llvm-tools-preview` is the prerequisite |
| step 10 | bench harness — hoist fixture history out of inner loop | — | — | criterion `search::search_root(depth=6)/midgame_12` micro improves 3-3.8%; macro NPS unaffected (production path doesn't use the bench harness). `strip = "none"` on `[profile.bench]` restored symbol names on the flamegraph. |
| step 11 | Phase 14 baseline (1 cold-cache run, tt_stats build) | **337,077 (+42.0%)** | **209,285 (+63.1%)** | midgame_30 depth-at-1s = 7 (was 6); `cached_eval_cold` midgame_12 4.08 µs (−53%); `threats::compute_full` midgame_30 2.68 µs (−23%); TT hit rate midgame_12 d=6 16.7% (was 15.6%) |

## Cumulative cumulative deltas

Phase 13 → Phase 14, headline:

- midgame_12 NPS: 237,449 → 337,077 — **+42.0 %**
- midgame_30 NPS: 128,308 → 209,285 — **+63.1 %**
- midgame_12 depth @ 1 s: 5 → 5 (no change; STEP 7's incremental
  threats was the depth-cliff lever, deferred to Phase 15)
- midgame_30 depth @ 1 s: 6 → **7**
- ms-time scaling midgame_12 @ 50 ms: **depth 3** (new metric)
- ms-time scaling midgame_30 @ 500 ms: **depth 6** (new metric)

vs Phase 14 prompt targets:

| Target | Phase 14 result | Met? |
|---|---|---|
| midgame_12 NPS ≥ 350k | 337k | ✗ marginal (−3.7%) |
| midgame_30 NPS ≥ 200k | 209k | ✓ +4.6% over target |
| midgame_12 depth @ 1 s ≥ 7 | 5 | ✗ |
| midgame_30 depth @ 1 s ≥ 7 | 7 | ✓ |
| midgame_12 @ 50 ms ≥ depth 3 | depth 3 | ✓ |
| midgame_30 @ 500 ms ≥ depth 5 | depth 6 | ✓ exceeded |

Four of six targets met; the two misses both tie back to the
deferred STEP 7 incremental threats. The depth-cliff at midgame_12
isn't a per-node NPS problem — it's that midgame_12 starts at d=5
and the depth-6 search ~doubles the tree. STEP 7's per-node speedup
(avoiding `walk_cross_axis` repeats) would have unlocked it; that's
the Phase 15 entry point.

## Reference table — Phase 14 truly fixed-depth counts (post step 1.5)

Phase 13 reference values at d≥6 were time-truncated. Phase 14
fixes `best_move` so depth-only requests have no time cap; the
following are the new ground truth:

| fixture | d=1 | d=2 | d=3 | d=4 | d=5 | d=6 | d=7 | d=8 |
|---|---|---|---|---|---|---|---|---|
| empty | 3 | 41 | 901 | 5,881 | 34,569 | 40,762 | 79,650 | 1,618,157 |
| single_origin | 37 | 896 | 3,110 | 22,362 | 28,787 | 80,505 | 1,504,626 | 2,177,529 |
| midgame_12 | 607 | 3,127 | 7,344 | 15,660 | 28,419 | 189,103 | 245,785 | 711,810 |
| midgame_30 | 99 | 549 | 2,025 | 3,512 | 10,501 | 28,392 | 56,744 | 94,838 |

These are the regression net for all subsequent Phase 14 sub-steps.
Drift against this table — at ANY depth — must be explained.
