# Hotspots — Phase 17 baseline

**Captured:** 2026-05-20 — git `4740d65`, rustc 1.94.0
**Host:** AMD Ryzen 7 8845HS (16 cores), Linux 7.0.3-arch1-2
**Bench data:** `benches/results/baseline.json` (`make bench` with
`--time-ms 1000 --tt-stats`, `.cargo/config.toml`
`target-cpu=native`, default features `simd_eval` + `eval_s1s2`).
Canonical macro/micro run measured at git `a7adbc9`; the four
following commits (`48c1556`..`4740d65`) are non-behavioural
reviewer fixes (dead-config removal, CLI wiring, a progress line,
a comment) — they do not move the numbers.
**Bench wall-clock:** a full `make bench` (criterion micro suite +
macro sweep) takes **~36 min** on this host. The macro-only tiers
(`bench perf` / `bench reference`) are ~1 min — the criterion micro
suite is the long pole.
**Flamegraph:** `benches/results/flamegraph-2026-05-20T19-17-51-4740d65.svg`
(`make flamegraph` — `perf record --call-graph dwarf` over
`bench_search` at depth 2 / 4 / 6).

## Headline numbers

| Metric | Phase 16 | Phase 17 | Δ |
|---|---:|---:|---:|
| NPS, `midgame_12`, t = 1000 ms | 431,686 | **433,813** | +0.5 % |
| NPS, `midgame_30`, t = 1000 ms | 306,434 | **314,297** | +2.6 % |
| NPS, `empty`, t = 1000 ms | 679,270 | 671,364 | -1.2 % |
| NPS, `single_origin`, t = 1000 ms | 690,565 | 679,270 | -1.6 % |
| Depth @ 1 s, `midgame_12` | 6 | **5** | **-1** |
| Depth @ 1 s, `midgame_30` | 8 | **6** | **-2** |
| Depth @ 1 s, `empty` | 7 | 7 | — |
| `cached_eval_cold`, `midgame_30` | 6.72 µs | **5.40 µs** | **-19.6 %** |
| `cached_eval_cold`, `midgame_12` | 3.75 µs | **3.18 µs** | -15.2 % |
| `eval::layer1_window_scan`, `midgame_30` | — | 0.85 µs | (8-cell) |
| `threats::compute_full`, `midgame_30` | 2.57 µs | 2.71 µs | +5.4 % |
| `board::place`, `midgame_30` | 1.58 µs | 1.22 µs | -22.8 % |
| `ordering::bucket_value`, `midgame_30` | — | 2.34 µs | — |

NPS is **flat** vs Phase 16 and **search depth fell 1–2 ply** on the
midgame fixtures. This is the deliberate cost of the S1/S2 eval
removal: the much-improved eval (see Strength, below) produces a
differently-shaped, somewhat larger search tree — more leaf evals
per interior node, so a higher average per-node cost that the
8-cell Layer-1 win (~+10 % raw) only offsets to roughly flat. The
8-cell table is itself behaviourally transparent (reference node
counts identical pre/post). The depth loss traces to the eval
change, **not** the move-ordering bucket — a back-to-back A/B
(bucket-on vs bucket-off builds) showed identical depth either way
and ~9 % more NPS with the creates-S1 ordering bucket *disabled*,
which is the shipped config.

### Phase 17 target table (prompt headline targets)

| Target | Goal | Result |
|---|---|---|
| midgame_12 NPS | ≥ 550 k | 434 k ❌ |
| midgame_30 NPS | ≥ 400 k | 314 k ❌ |
| Depth-at-time midgame_12 @ 1 s | ≥ 7 | 5 ❌ |
| Depth-at-time midgame_30 @ 1 s | ≥ 8 | 6 ❌ |
| `make vs` parallel harness | minutes not hours | 50 g @ 500 ms in 1 m 42 s ✅ |
| S1/S2 decision | KEEP or DROP @ ≥200 g | **DROP** (200 g, 29.0 %) ✅ |

The NPS / depth targets are **missed**. They assumed the prompt's
full-removal STEP 3 (delete the cross-axis matchers → a real
`threats::compute` win) and a larger Layer-1 payoff. The shipped
phase took the user-directed **hybrid** path (zero the S1/S2 eval
weights, keep the detection code as a tunable surface), so
`threats::compute` is unchanged, and the 8-cell Layer-1 net win is
modest because the Phase-16 path already had a 6-cell AVX2 encoder.
The real Phase-17 win is **strength**, not throughput — see below.

### Strength (the actual Phase 17 win)

- S1/S2 ablation A/B (200 games @ 500 ms, parallel harness):
  S1/S2-enabled scored **29.0 %**, Wilson [23.2 %, 35.6 %] → DROP.
- Post-removal strength gate: the S1/S2-zeroed build scored
  **85 / 100 (Wilson [76.7 %, 90.7 %], Elo +301)** vs the Phase-16
  HEAD build. The eval removal is a large strength gain.

## Phase 17 changes that landed

1. **Parallel match harness** (`promote.py` `run_match_parallel`,
   `multiprocessing.Pool` + `imap_unordered`, spawn context). 50
   games @ 500 ms in **1 m 42 s** wall-clock (was ~25 min
   sequential). The ablation harness is parallel too
   (`bench_ablation_parallel`).
2. **S1/S2 hybrid removal**: eval shape weights zeroed in
   `hexo.toml`, `tempo_score` dropped, the creates-S1 *ordering*
   bucket disabled (A/B-confirmed faster). Detection code,
   `ThreatCounts` fields, the `eval_s1s2` feature and the
   `set_eval_s1s2` toggle are retained for a future eval-tuning
   phase.
3. **Layer 1 8-cell window table**: the 6-cell `WINDOW_SCORE` table
   + runtime `extension_factor` (two boundary `is_set` probes + a
   multiply per window) replaced by a single `WINDOW_SCORE_8`
   8-cell ternary lookup (6561 entries, factor folded in at build
   time). Scalar + AVX2 encode paths, both 6561-entry
   byte-identity certified.

## Hotspot ranking (flamegraph + criterion cross-check)

1. **Layer 1 window scan** — `windows8_run` (8-bit window
   extraction) is the single hottest frame; `encode_ternary_8`
   (the per-window scalar encode reached for sub-16-window AVX2
   tails — most HeXO lines are short) is also top-5. The 8-cell
   rework did not dethrone Layer 1.
2. **Proximity updates** — `for_each_in_range` inside
   `add_proximity` / `remove_proximity` (board `place` / `undo`),
   several of the top-10 frames.
3. **Ordering predicates** — `would_make_six`, `creates_s0` (virtual
   placement axis-run probes).
4. **`threats::compute`** (S0 + the still-present cross-axis
   matchers) feeding `cached_eval`.
5. **TT probe / store** + search-loop overhead.

## Phase 18 entry point

- **Layer 1 is still #1.** The 8-cell `encode_ternary_8` scalar tail
  dominates because per-line window counts are typically < 16, so
  the AVX2-16 batch rarely fully engages. A narrower SIMD width
  (encode 8 windows at once) or a per-line direct-index path would
  help.
- **Proximity** `for_each_in_range` is #2 — revisit the
  `creates_s0` per-axis run cache (Phase 18 candidate) or a flatter
  proximity update.
- **Move-ordering bucket refinement** — now that S1/S2 is settled.
- See `SPEC_ROADMAP.md` § Phase 18 candidates, esp. the **Eval
  tuning** entry (re-tune the retained S1/S2 surface).
