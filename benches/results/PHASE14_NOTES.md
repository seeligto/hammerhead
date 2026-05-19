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
