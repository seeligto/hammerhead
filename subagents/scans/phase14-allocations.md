# Phase 14 — Hot-path allocation survey

Source: `rg -n 'Vec::|FxHashSet::|FxHashMap::|HashMap::|Box::new|::with_capacity' hexo-engine/src/`
plus targeted reads of the matching code.

## Classification

| File:line | Allocation | Hotness | Verdict |
|---|---|---|---|
| `threats.rs:125` | `Vec<Coord> pieces = board.pieces()...collect()` in `full_recompute` | hot — per node (when cache invalidated) | **scratch** |
| `threats.rs:141` | `FxHashSet<(Axis, i16, i16)> seen` in `walk_linear_runs` | hot — per node (paired with above) | **scratch** |
| `threats.rs:282,289,296` | three `SmallVec::new()` helpers (`smallvec_one`, `smallvec_two`, `run_pieces`) | hot but stack-only (inline `[Coord; 4..=5]`) | leave |
| `moves.rs:44` | `MoveList = SmallVec::new()` in `generate` | hot — per node | stack-only via `[Coord; MOVE_GEN_CAP_INLINE]` |
| `moves.rs:76` | `FxHashSet<Coord> seen` in `sweep_neighbourhood` | warm — radius > inner only (extension nodes) | acceptable; out-of-scope |
| `eval.rs:139` | `SmallVec<[i16; 32]> line_ids` in `layer1_window_scan` | hot — per node | stack unless spillover; ≤ 32 lines covers midgame; acceptable |
| `eval.rs:323` | `SmallVec<[Coord; 16]> union` in `min_vertex_cover_size` | warm — only when ≥ 3 S0 instances | stack-only; acceptable |
| `search.rs:593` | `SmallVec<[Coord; MOVE_GEN_CAP_INLINE]> threats` in PVS root scan | warm — per root | stack-only |
| `search.rs:719,894,896` | qsearch / PV reconstruction | cold | leave |
| `axis_bitmap.rs:284,454` | one-time init of `LineBitmap` flat arrays + populated-ids | once per Engine | cold; init path |
| `ordering.rs:79,80` | killer slots + history map | once per Engine; history grows over time | cold init / amortized |
| `board.rs:107-124` | constructor HashSet / HashMap pre-sized | once per Engine | cold init |
| `zobrist.rs:66` | `FxHashMap` of u128 lazy keys | once per Engine | cold init |

## Top scratch candidates (acted on in STEP 3.3)

1. **`threats::full_recompute`**:
   - `Vec<Coord> pieces` and `FxHashSet seen` both allocated per call.
   - Hoist into `ThreatScratch { pieces, seen }` owned by `Board` behind `RefCell`. `reset()` between calls preserves capacity, eliminating churn after the first warm-up.

## Lower-priority / out of scope

- `moves::sweep_neighbourhood::seen` — only when search needs the
  extended radius. Less than 1% of nodes. Out of Phase 14 scope.
- `ordering::history` — `FxHashMap<Coord, u32>` — bounded by ordering
  cap; grows then plateaus. Already amortized.
- `search::find_pv::pv` — once per root.

## mimalloc

Even with the scratch buffer above there's residual `Vec` /
`SmallVec` spillover. mimalloc as a global allocator can speed up the
remaining churn. Phase 14 STEP 3.2 evaluates the delta behind the
`mimalloc` Cargo feature; decision is recorded in the commit body and
this scan.

Outcome (STEP 3.2): see `bench-record` commit body.
