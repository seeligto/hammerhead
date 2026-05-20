# Phase 17 STEP 2.4 — Why does S1/S2 hurt?

A sanity-check investigation before removing S1/S2 (STEP 3). Not a
redesign — the goal is a documented hypothesis to carry into the
removal commit, and a check for an "obvious bug" (e.g. a weight off
by 10×) that would warrant a pause instead of a removal.

## The data

Parallel ablation harness, Phase 16 build, S1/S2-enabled eval vs
S1/S2-disabled eval, colours alternating:

| Match | n | time/stone | S1/S2 winrate | Wilson 95% | Verdict |
|---|---|---|---|---|---|
| Phase 16 probe | 50 | 200 ms | ~32 % | [19.9 %, 44.8 %] | (deferred) |
| Phase 17 re-run | 200 | 500 ms | 29.0 % (57/200) | [23.2 %, 35.6 %] | DROP |

The n=200 re-run reproduces and tightens the Phase 16 signal. The
Wilson upper bound (35.6 %) sits well below 50 %, so the decision
matrix gives DROP from Match 1 alone — Match 2 was not needed.

## Relevant weights

S1/S2 Layer-2 shape weights (`hexo.toml [engine.eval]`):

| shape | weight | tier |
|---|---|---|
| open_3 | 1500 | S1 |
| rhombus | 1500 | S1 |
| arch | 1500 | S1 |
| bone | 3000 | S1 |
| trapezoid | 2500 | S1 |
| open_2 | 200 | S2 |
| closed_3 | 150 | S2 |
| triangle | 250 | S2 |

For comparison:

- S0 weights: open_4 = 60 000, closed_4 = 20 000, closed_5 =
  500 000, open_5 = 800 000.
- Layer 1 `window_k_scores = [0, 1, 8, 64, 512, 4096, 1e6]`. A
  6-cell window with three own stones scores `64`; with the open
  extension factor (×4) the realised contribution is `256`, the
  closed factor (×1) gives `64`. A four-stone window: `512 → 2048`
  open / `512` closed.
- `tempo_weight = 50`; `tempo = 50 · (X.open_3 − O.open_3)`.

## Hypotheses checked

### H1 — Double counting (open_3): **confirmed contributor**

A loose open three-in-a-row is scored twice:

- **Layer 1**: the 6-cell window holding the three stones with both
  ends open contributes ≈ `256` (64 × open-extension 4).
- **Layer 2**: `layer2_shapes` adds `OPEN_3_SCORE = 1500` for the
  same shape.

Combined ≈ `1756`, of which Layer 2 is ~85 %. The same physical
shape is rewarded by two layers, and the Layer-2 term dominates the
Layer-1 term by ~6×. Every speculative three thus carries roughly
the weight the designers intended for a *much* rarer structure.

### H2 — Weight magnitude vs genuine tactics: **confirmed contributor**

S1/S2 weights are small next to S0 (1500 vs 20 000+), so they never
override a real mate threat. But they are *not* small next to
Layer 1, the only other quiet-position signal:

- `open_3` (1500) ≈ 73 % of a Layer-1 *open-four* window (2048).
- `bone` (3000) **exceeds** a Layer-1 open-four window.

So in a quiet position the engine values a speculative cluster
nearly as highly as — or higher than — a concrete open-four's
positional contribution. That biases move selection toward
shape-building over tactically forcing play whenever no S0 threat
is on the board, which is most of the midgame.

### H3 — Cross-axis false positives: **plausible contributor**

`rhombus / arch / bone / trapezoid / triangle` are pure geometric
pattern matches (`anchor_cross_axis` over fixed pattern tables).
They carry no mate-relevance or follow-up check — a `bone` worth
3000 fires on any five stones in that arrangement regardless of
whether the shape can actually be converted. With weights of
1500–3000 these heuristic matches inject substantial noise into the
quiet-position eval.

### H4 — Tempo confusion: **ruled out as primary cause**

`tempo_score` reads `open_3` counts, but `tempo_weight` is only
`50` — two orders of magnitude below `OPEN_3_SCORE`. Tempo cannot
be the driver. It does, however, share the `open_3` metric, so once
S1/S2 (and the `open_3` count) is removed, `tempo_score` collapses
to zero and is removed alongside it (STEP 3.1).

## Obvious bug? No.

No weight is off by an order of magnitude; the values are the
deliberate Phase-16 tuning set. There is nothing to pause for. The
failure is a *design / tuning* effect, not a coding bug:

> **Hypothesis carried into STEP 3:** S1/S2 systematically
> over-rewards speculative shape-building — double-counted against
> Layer 1 (H1), weighted on par with genuine open-fours (H2), and
> fed by mate-blind cross-axis matchers (H3). In quiet midgame
> positions this pulls the engine off the forcing line. Removing
> S1/S2 lets Layer 1 + Layer 3 (forks) drive quiet eval, which the
> n=200 A/B shows is the stronger configuration by a wide margin.

Proceed to STEP 3 (removal).
