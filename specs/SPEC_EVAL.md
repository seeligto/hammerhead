# HeXO Eval Spec

## Approach

Threat detection per WSC theory (Weight / Strength / Cost, tenderloin345).

Score = (own threat sum) − (opponent threat sum).

Three layers:
1. Window scan (cheap, baseline)
2. Shape detection (named WSC patterns)
3. Fork detection (multi-threat overlap)

## Layer 1: Window Scan

For each axis, slide 6-cell window across active board region.

For each window: count own `k`, opponent `o`, empty `e`. (`k + o + e = 6`)

- If `o > 0` and `k > 0`: dead. Score 0.
- If `o > 0`: opponent-only (count for them).
- If `k > 0`: live for own player.

Window score table:
| `k` | base score |
|---|---|
| 0 | 0 |
| 1 | 1 |
| 2 | 8 |
| 3 | 64 |
| 4 | 512 |
| 5 | 4096 |
| 6 | WIN (+INF) |

Symmetric for opponent (negated).

Implementation:
- Maintain `axis_lines: FxHashMap<(Axis, LineId), LineState>` where `LineId` indexes parallel lines per axis.
- On `place(c)`: invalidate 3 lines through `c`, rescan only windows touching `c` (max 6 windows × 3 axes = 18 windows).
- Cache totals.

## Layer 2: Shape Detection (WSC tuples)

Detect named threat shapes. Score per WSC tuple `(W, S, C)`.

### S0 threats (mate-in-one-move)

| Shape | W | S | C | Score | Notes |
|---|---|---|---|---|---|
| Open 5 `_XXXXX_` | 2 | 0 | 0 | 8000 | Defender plays 2 ends, attacker wins after |
| Closed 5 `OXXXXX_` | 1 | 0 | 0 | 5000 | Defender plays 1 end |
| Open 4 `_XXXX_` (≥1 empty each side, room for 6) | 2 | 0 | 2 | 6000 | Two-end threat |
| Closed 4 | 1 | 0 | 2 | 2000 | |

### S1 pre-emptives (mate-in-two-moves if undefended)

| Shape | W | S | C | Score |
|---|---|---|---|---|
| Open 3 `_XXX_` | 3 | 1 | 2 | 1500 |
| Rhombus (4-piece diamond) | 3 | 1 | 2 | 1500 |
| Arch / Banana | 3 | 1 | 2 | 1500 |
| Bone / Bowtie (5-piece) | 4+ | 1 | 2 | 3000 |
| Trapezoid / Pentagon | 3+ | 1 | 2 | 2500 |

### S2 pre-emptives

| Shape | W | S | C | Score |
|---|---|---|---|---|
| Open 2 isolated | 2 | 2 | 2 | 200 |
| Closed 3 | 2 | 2 | 2 | 150 |
| Triangle (3 mutually adjacent hexes) | 2 | 2 | 2 | 250 |

### Detection method

Shape detection runs after window scan. Inputs: piece set local to last move, axis lines.

Algorithm:
1. For each piece of `p` at position `c`, enumerate 3 axes.
2. Walk line `±5` cells. Find max-length contiguous own run.
3. Classify endpoints: open / closed / dead.
4. Match against shape table.
5. Cross-axis shapes (rhombus, triangle, bone): detect by piece-cluster scan.

Cache shape counts incrementally per player:
```rust
struct ThreatCounts {
    open_5: u8, closed_5: u8,
    open_4: u8, closed_4: u8,
    open_3: u8, rhombus: u8, arch: u8, bone: u8, trapezoid: u8,
    open_2: u8, closed_3: u8, triangle: u8,
}
```

## Layer 3: Fork Detection (mate-via-multiple-threats)

Defender has 2 stones per turn. If attacker has more S0 threats than 2, mate.

Concretely:
- Count own active S0 threats. If `≥ 2` AND they're not all blockable by a single 2-stone response: mate.
- Two open-4s sharing no defense cell → mate.
- Open-4 + closed-5 → mate.

Per veganwater45 finisher shapes (V, T, Y, L, scissors): two overlapping 4-lines that share at least one cell.

### Detection

For each own S0 threat: compute "defense set" (cells that block it).
- Open-4: defense = both endpoints (2 cells)
- Closed-4: defense = 1 cell
- Open-5: defense = both endpoints
- Closed-5: defense = 1 cell

Mate condition: union of defense sets cannot be covered by any 2-cell subset.

Mathematically: minimum vertex cover of the threat-graph ≥ 3.

Eval bonus:
- Cover-size 1: normal threat score
- Cover-size 2: medium bonus (forces full defense turn)
- Cover-size ≥ 3: MATE (return ±MATE_SCORE)

## Disjoint vs. Overlapping (veganwater45)

Disjoint quads (separated, no shared piece): defender handles sequentially. Sum scores normally.

Overlapping quads (V/T/Y/L/scissors): score × 1.5 bonus.

## Tempo (advisory layer)

Per uncreative172 tempo notation:
- `+1` pre-emp (open-3): defender needs 1 more move than attacker
- `+0`: equal
- `-1`: defender wastes attacker time

Tempo sum per player. Small eval contribution. Real tempo emerges from search depth.

## Eval Pseudocode

```rust
pub fn eval(board: &Board) -> i32 {
    if let Some(winner) = board.winner() {
        return match winner {
            Player::X => MATE_SCORE,
            Player::O => -MATE_SCORE,
        };
    }
    
    let mut score = 0;
    
    // Layer 1: window sum
    score += window_score(board, Player::X);
    score -= window_score(board, Player::O);
    
    // Layer 2: shape bonus
    score += shape_score(board, Player::X);
    score -= shape_score(board, Player::O);
    
    // Layer 3: fork / mate detection
    if is_fork_mate(board, Player::X) { return MATE_SCORE - board.ply as i32; }
    if is_fork_mate(board, Player::O) { return -MATE_SCORE + board.ply as i32; }
    
    // Tempo
    score += tempo_score(board);
    
    score
}
```

Mate-distance tracking: subtract `ply` from `MATE_SCORE` so search prefers faster mates.

## Incremental Maintenance

Every layer cached. On `place(c)`:
- Window scan: invalidate 18 windows max (3 axes × 6 windows touching `c`)
- Shape: re-evaluate piece clusters within radius 5 of `c`
- Fork: recompute defense sets only for invalidated shapes

On `undo(c)`: same, reverse.

Avoid full-board scan in search inner loop.

## Tuning

Eval weights start as listed above. Tune via:
- Self-play tournaments
- SealBot benchmark suite
- Position library (known mate-in-N, known draws)

**Storage:** weights live in `hexo.toml` (see [SPEC_CONFIG](SPEC_CONFIG.md)) and are
codegen'd into `crate::config` at build time. Edit `hexo.toml`, rebuild,
both Rust and Python see new values.

Future: PyO3 runtime override hook for tuning experiments without rebuild.

## Radius Theory Integration (Hexo Radius Theory.pdf)

- Single stone in C-ring (dist 3) defends colony + open-3 potential
- Pair in D-ring (dist 4) defends colony + open-3 potential
- Apply: when opponent makes far colony, eval considers whether own pieces lie within defense radius. Reduce threat weight of colony if defended.

Implementation: for each enemy colony cluster, check if own pieces within C/D ring. Discount threat score accordingly.
