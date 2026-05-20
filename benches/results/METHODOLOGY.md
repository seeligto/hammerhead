# Bench Methodology

Phase 14 introduced this document. Treat it as the calling convention
for every subsequent bench-driven optimization phase: what counts as
a win, how to talk about deltas, when to revert.

## What counts as a "win"

A change is a **win** if any of these hold across 3 cold-cache runs:

- Criterion micro-bench median moves outside the noise band
  (criterion's default ±3 % threshold).
- Macro NPS shows ≥ 2 % improvement, reproducing across runs.
- ms-time depth-reached improves by ≥ 1 at any fixture × budget cell.

A change is **trivial** if:

- Macro NPS delta < 1 %, AND no micro-bench reaches the noise band.
- Keep only if it improves clarity, safety, or future-proofs hot code
  paths.

A change is a **regression** if:

- Macro NPS drops AND no related micro-bench improves.
- **Revert immediately.** Do not try to fix in-phase. Log as
  follow-up.

## Cold vs warm

- **Cold-cache run**: `make rebuild` + first invocation. CPU caches
  unloaded, branch predictor cold. Measures startup + first-search
  responsiveness — matters for ms-time scaling.
- **Warm-cache run**: 2nd+ invocation in the same shell. Steady-state
  search behaviour.
- The bench harness defaults to warm (criterion warms up).

## Variance

- NPS run-to-run variance is typically 2-5 % on a fixed host.
- Anything below that is noise; require ≥ 2 % to call a win.
- For 1 % deltas, claim "neutral, kept for code-quality reason".

## Cycles per node

Derived = `cpu_cycles_per_second × time / nodes`. Approximation:
`time_ns / nodes`. At 4 GHz:

- 100k NPS ≈ 40k cycles/node
- 200k NPS ≈ 20k cycles/node
- 350k NPS ≈ 11k cycles/node

Tracking cycles/node trend per phase shows whether we're improving
the inner loop or just the macro.

## ms-time scaling (Phase 14)

`make bench` includes the scaling table by default. The cells at
`(fixture, time_ms)` for short budgets (1 ms / 10 ms) are noisy by
design — they capture iterative-deepening startup cost rather than
steady-state NPS. Track depth-reached separately from raw NPS at
short budgets.

## Per-function cycles breakdown (Phase 14)

`make bench` also emits a breakdown of the share of cycles spent in
each top-level module. The numbers are **estimates** derived from
criterion micro-bench medians × call-counts, not a profile. Their
value is trend tracking. For ground truth, run `make flamegraph`.

## Reference node counts

Any change that touches `search`, `eval`, `threats`, `moves`,
`ordering`, or `tt` MUST preserve reference node counts at every
`(fixture, depth)` row unless the change is *explicitly* about move
ordering or pruning. Node-count drift = behaviour change. Explain or
revert. The Phase 14 "deep optimization sweep" was perf-only by
contract: zero drift was the floor.

## Tiered bench workflow

Use the right tier for the iteration loop:

| Tier | Time | When |
|---|---|---|
| `make bench-quick` | 5-15 s | After every code edit during a sub-step |
| `make bench-micro-quick TARGET=X` | 5-10 s | When iterating on one module |
| `make bench-perf` | 30-60 s | End of sub-step, before commit |
| `make bench` (full) | 3-5 min | End of phase, before baseline commit |

cycles/node is the sensitive metric. NPS can move from depth shifts;
cycles/node is monotonic in per-node work. If cycles/node drops but
NPS doesn't change, the change is real but small (search depth
ratchet hides it). If NPS drops but cycles/node holds, something
non-per-node-work is happening — investigate.

The quick / perf tiers cache the last run under `.hexo/`
(gitignored, per-developer) and print a `Δ vs last` against it.
