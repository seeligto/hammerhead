//! Threat / shape detection per WSC theory (tenderloin345).
//!
//! Produces per-player [`ThreatCounts`] and a list of S0 [`ThreatInstance`]s
//! with defense cells. Cached on [`Board`]; the cache is invalidated on
//! every `place` / `undo` and refilled by [`compute_with_scratch`] on the
//! next read. Detection is a single linear-run scan ([`walk_linear_runs`])
//! over every populated axis line.

// `span` is guaranteed to be in `[2, 5]` by the surrounding range check
// before each `as u8` cast in `walk_linear_runs`.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]

use crate::axis_bitmap::Axis;
use crate::board::{Board, Player};
use crate::config::MAX_S0_INSTANCES;
use crate::coords::{Coord, hex_distance};
use fxhash::FxHashSet;
use smallvec::SmallVec;

/// Per-player count of every detected shape. All u8 — saturated at
/// 255 by the detection loop (deep enough never to be reached in legal
/// play).
///
/// S0 fields (`open_5`, `closed_5`, `open_4`, `closed_4`) and S1
/// fields (`open_3`, `closed_3`, `open_2`) are all populated by
/// [`walk_linear_runs`]. The S1 trio was revived as types in
/// Phase 28D-3 D3-INFRA; detectors landed in D3-A.1 (`open_3`),
/// D3-A.2 (`closed_3`), and D3-A.3 (`open_2`).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreatCounts {
    /// `_XXXXX_` (both ends empty).
    pub open_5: u8,
    /// `OXXXXX_` or `_XXXXXO` (one end empty).
    pub closed_5: u8,
    /// `_XXXX_` (both ends empty).
    pub open_4: u8,
    /// `OXXXX_` (one end empty + extension space).
    pub closed_4: u8,
    /// S1 — `_XXX_` (both ends empty). Phase 28D-3 D3-A.1 detector.
    pub open_3: u8,
    /// S1 — `OXXX_` (one end empty + extension space).
    /// Phase 28D-3 D3-A.2 detector.
    pub closed_3: u8,
    /// S1 — `_XX_` (both ends empty). Phase 28D-3 D3-A.3 detector.
    pub open_2: u8,
    /// Cross-axis cluster: 4 cells in a diamond (pairwise distances
    /// `{1,1,1,1,1,2}`) — see `HeXOpedia` §4.3. Counted only when
    /// isolated (no opp inside Ring C of centroid per Radius Theory).
    /// Phase 28E-2 Stage 1 detector.
    pub rhombus: u8,
}

/// Tag of a threat shape. S0 variants (mate-in-one-turn) are populated
/// by detection; S1 variants are reserved for the Phase 28D-3 A.X
/// detectors and never appear in a [`ThreatInstance`] until those
/// detectors land.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ThreatKind {
    /// `_XXXXX_` — opponent must play one of two endpoints to deny win.
    OpenFive,
    /// `OXXXXX_` — opponent must play the single empty endpoint.
    ClosedFive,
    /// `_XXXX_` — opponent must play both endpoints to deny.
    OpenFour,
    /// `OXXXX_` — opponent must play the single open extension.
    ClosedFour,
    /// S1 — `_XXX_` (both ends empty). Phase 28D-3 D3-A.1 detector.
    OpenThree,
    /// S1 — `OXXX_` (one end empty + extension space).
    /// Phase 28D-3 D3-A.2 detector.
    ClosedThree,
    /// S1 — `_XX_` (both ends empty). Phase 28D-3 D3-A.3 detector.
    OpenTwo,
}

/// One detected S0 threat with its participating pieces and the minimal
/// blocker set that denies completion next stone.
#[derive(Clone, Debug)]
pub struct ThreatInstance {
    /// Shape category.
    pub kind: ThreatKind,
    /// Stones forming the run, in axis-order (low pos → high pos).
    pub pieces: SmallVec<[Coord; 5]>,
    /// Cells whose occupation by the opponent denies completion. Size 1 for
    /// closed shapes, size 2 for open shapes.
    pub defense_cells: SmallVec<[Coord; 4]>,
}

/// Per-player threat snapshot. Cheap to clone (counts + inline `SmallVec`).
/// Search consumers see only `counts` + `s0_instances`.
#[derive(Clone, Debug, Default)]
pub struct ThreatSet {
    /// Shape counts across all detected threats.
    pub counts: ThreatCounts,
    /// S0 threats (mate-in-one-turn). One entry per distinct run.
    /// Inline cap-8 absorbs typical midgame plus fork density without
    /// heap alloc; spills (≤ `MAX_S0_INSTANCES`) hit pathological depth only.
    pub s0_instances: SmallVec<[ThreatInstance; 8]>,
}

/// Reusable scratch buffers for `compute`. Owned by `Board` and reset
/// between calls so the `FxHashSet` `seen` dedup and the per-player
/// pieces `Vec` keep their backing capacity instead of reallocating on
/// every dirty read.
///
/// Phase 28E-2 Stage 1 adds the rhombus-detection sub-scratch:
/// per-player coord sets (own + opp) and the canonicalized 4-tuple
/// dedup set.
#[derive(Debug, Default)]
pub struct ThreatScratch {
    seen: FxHashSet<(Axis, i16, i16)>,
    pieces: Vec<Coord>,
    /// Rhombus pass: opp coord set for isolation check.
    rhombus_opp_set: FxHashSet<Coord>,
    /// Rhombus pass: own coord set for vertex membership check.
    rhombus_own_set: FxHashSet<Coord>,
    /// Rhombus pass: canonical sorted 4-tuple dedup set.
    rhombus_seen: FxHashSet<[Coord; 4]>,
}

impl ThreatScratch {
    /// Reset the scratch buffers (`seen` dedup and `pieces` work list)
    /// while keeping their backing capacity.
    #[inline]
    fn reset(&mut self) {
        self.seen.clear();
        self.pieces.clear();
        self.rhombus_opp_set.clear();
        self.rhombus_own_set.clear();
        self.rhombus_seen.clear();
    }

    /// Clear every internal buffer. Called from `Board::reset` so a
    /// fresh game starts without stale scratch state.
    #[inline]
    pub fn clear_all(&mut self) {
        self.seen.clear();
        self.pieces.clear();
        self.rhombus_opp_set.clear();
        self.rhombus_own_set.clear();
        self.rhombus_seen.clear();
    }
}

/// Compute the threat set for `player` on `board`.
///
/// Detection is a single linear-run scan over every populated axis line.
///
/// This convenience wrapper allocates a fresh `ThreatScratch` per call.
/// `Board::threats` uses [`compute_with_scratch`] directly so the
/// search hot path reuses backing storage across nodes.
#[must_use]
pub fn compute(board: &Board, player: Player) -> ThreatSet {
    let mut scratch = ThreatScratch::default();
    compute_with_scratch(board, player, &mut scratch)
}

/// Variant of [`compute`] that reuses caller-provided scratch buffers.
/// `scratch` is reset on entry, so the caller can freely reuse the same
/// buffers across many calls — only the buffers' capacities are
/// retained, eliminating per-call allocation.
#[must_use]
pub fn compute_with_scratch(
    board: &Board,
    player: Player,
    scratch: &mut ThreatScratch,
) -> ThreatSet {
    let mut out = ThreatSet::default();
    scratch.reset();
    let ov = board.eval_overrides();
    // Gate the rhombus pass on a non-zero weight: building per-player
    // own/opp coord sets and running the cross-axis enumerator costs
    // ~5-30% NPS depending on board density. Default `rhombus = 0`
    // (codegen'd from hexo.toml) keeps the hot path byte-equivalent
    // to the pre-Stage-1 build. Sweep / EvalOverrides callers pay
    // only when they opt in via a non-zero weight.
    let rhombus_active = ov.rhombus != 0;
    let opp = player.opponent();
    for (c, p) in board.pieces() {
        if p == player {
            scratch.pieces.push(c);
            if rhombus_active {
                scratch.rhombus_own_set.insert(c);
            }
        } else if p == opp && rhombus_active {
            scratch.rhombus_opp_set.insert(c);
        }
    }
    walk_linear_runs(board, player, &scratch.pieces, &mut scratch.seen, &mut out);
    if rhombus_active {
        detect_rhombi(
            &scratch.pieces,
            &scratch.rhombus_own_set,
            &scratch.rhombus_opp_set,
            &mut scratch.rhombus_seen,
            ov.rhombus_isolation_radius,
            &mut out.counts,
        );
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Linear shape detection
// ─────────────────────────────────────────────────────────────────────────────

fn walk_linear_runs(
    board: &Board,
    player: Player,
    pieces: &[Coord],
    seen: &mut FxHashSet<(Axis, i16, i16)>,
    out: &mut ThreatSet,
) {
    let axes = board.axes();

    for &c in pieces {
        for axis in Axis::all() {
            let Some((start_pos, end_pos)) = axes.run_endpoints(c, axis, player) else {
                continue;
            };
            let line_id = axis.line_id(c);
            if !seen.insert((axis, line_id, start_pos)) {
                continue;
            }

            let span = end_pos - start_pos + 1;
            if !(2..6).contains(&span) {
                continue;
            }
            let length = span as u8; // span ∈ [2, 5], fits.

            let left_cell = coord_at(axis, line_id, start_pos - 1);
            let right_cell = coord_at(axis, line_id, end_pos + 1);
            let opp = player.opponent();
            let left_open = !axes.is_player(left_cell, opp);
            let right_open = !axes.is_player(right_cell, opp);
            debug_assert!(
                !axes.is_player(left_cell, player),
                "non-maximal run on left"
            );
            debug_assert!(
                !axes.is_player(right_cell, player),
                "non-maximal run on right"
            );

            classify_linear_run(
                board, player, axis, line_id, start_pos, end_pos, length, left_open, right_open,
                out,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_linear_run(
    board: &Board,
    player: Player,
    axis: Axis,
    line_id: i16,
    start_pos: i16,
    end_pos: i16,
    length: u8,
    left_open: bool,
    right_open: bool,
    out: &mut ThreatSet,
) {
    let pieces = run_pieces(axis, line_id, start_pos, end_pos);
    let left_cell = coord_at(axis, line_id, start_pos - 1);
    let right_cell = coord_at(axis, line_id, end_pos + 1);
    let opp = player.opponent();
    let open_ends = u8::from(left_open) + u8::from(right_open);

    match (length, open_ends) {
        (5, 2) => push_s0(
            out,
            ThreatKind::OpenFive,
            pieces,
            smallvec_two(left_cell, right_cell),
            |c| c.open_5 = c.open_5.saturating_add(1),
        ),
        (5, 1) => {
            let def = if left_open { left_cell } else { right_cell };
            push_s0(
                out,
                ThreatKind::ClosedFive,
                pieces,
                smallvec_one(def),
                |c| c.closed_5 = c.closed_5.saturating_add(1),
            );
        }
        (4, 2) => push_s0(
            out,
            ThreatKind::OpenFour,
            pieces,
            smallvec_two(left_cell, right_cell),
            |c| c.open_4 = c.open_4.saturating_add(1),
        ),
        (4, 1) => {
            // Closed-4 needs the cell *beyond* the open neighbour to be
            // non-opp; otherwise extending to 5 produces a fully-boxed run
            // and no 6-in-row is possible.
            let (def_cell, beyond) = if left_open {
                (left_cell, coord_at(axis, line_id, start_pos - 2))
            } else {
                (right_cell, coord_at(axis, line_id, end_pos + 2))
            };
            if !board.axes().is_player(beyond, opp) {
                push_s0(
                    out,
                    ThreatKind::ClosedFour,
                    pieces,
                    smallvec_one(def_cell),
                    |c| c.closed_4 = c.closed_4.saturating_add(1),
                );
            }
        }
        (3, 2) => {
            // Open-3 (S1, Phase 28D-3 D3-A.1): `_XXX_` with BOTH
            // immediate neighbours empty AND both 2-cells-beyond
            // non-opp. The 2-beyond gate excludes dead runs that
            // can extend to a 4 but never to a winning 6 — e.g.
            // `O_XXX_O` would form `_XXXX_` immediately surrounded
            // by O on both sides, dying as a boxed 4. Mirrors the
            // closed-4 "beyond non-opp" growth check, applied to
            // both sides because open-3 needs growth viability on
            // both ends.
            let beyond_left = coord_at(axis, line_id, start_pos - 2);
            let beyond_right = coord_at(axis, line_id, end_pos + 2);
            let bm = board.axes();
            if !bm.is_player(beyond_left, opp) && !bm.is_player(beyond_right, opp) {
                bump_s1(out, |c| c.open_3 = c.open_3.saturating_add(1));
            }
        }
        (3, 1) => {
            // Closed-3 (S1, Phase 28D-3 D3-A.2): `OXXX_` with the
            // open side's 2-cell-beyond non-opp. The closed side is
            // already blocked by an opp stone, so growth there is
            // dead; viability lives entirely on the open side. The
            // 2-beyond gate mirrors the closed-4 growth check —
            // `OXXX_O` would extend to `OXXXX_O` then dies as a
            // doubly-boxed 5 (no winning 6 possible).
            //
            // "Blocked" is opp-stone only; off-board cells satisfy
            // `is_player(_, opp) == false` and are treated as open
            // by the existing detector framework (matches the
            // closed-4 convention). This is consistent across all
            // arms in this match.
            let (open_cell_beyond, _closed_cell) = if left_open {
                (coord_at(axis, line_id, start_pos - 2), right_cell)
            } else {
                (coord_at(axis, line_id, end_pos + 2), left_cell)
            };
            if !board.axes().is_player(open_cell_beyond, opp) {
                bump_s1(out, |c| c.closed_3 = c.closed_3.saturating_add(1));
            }
        }
        (2, 2) => {
            // Open-2 (S1, Phase 28D-3 D3-A.3): `_XX_` with BOTH
            // immediate neighbours empty AND both 2-cells-beyond
            // non-opp. The 2-beyond gate mirrors open-3's growth
            // check — `O_XX_O` would extend to `_XXX_` already
            // boxed on both sides by O, dying as a dead 3 with no
            // path to a winning 6. Conservative gate applied
            // symmetrically since open-2 needs viability on both
            // ends. Closed-2 (`OXX_`) is intentionally NOT in
            // scope: lowest-value S1 shape, single-sided viability
            // adds noise without commensurate signal per D3-DIAG.
            let beyond_left = coord_at(axis, line_id, start_pos - 2);
            let beyond_right = coord_at(axis, line_id, end_pos + 2);
            let bm = board.axes();
            if !bm.is_player(beyond_left, opp) && !bm.is_player(beyond_right, opp) {
                bump_s1(out, |c| c.open_2 = c.open_2.saturating_add(1));
            }
        }
        _ => {}
    }
}

/// Bump an S1 (Phase 28D-3) shape count. S1 shapes do not enter the
/// `s0_instances` defense-cells hypergraph (Layer 3 fork detection is
/// S0-only by design) — they contribute solely via Layer 2 weighted
/// sums, so the helper updates `counts` and skips the
/// `ThreatInstance` push entirely.
#[inline]
fn bump_s1(out: &mut ThreatSet, bump: impl FnOnce(&mut ThreatCounts)) {
    bump(&mut out.counts);
}

fn push_s0(
    out: &mut ThreatSet,
    kind: ThreatKind,
    pieces: SmallVec<[Coord; 5]>,
    defense_cells: SmallVec<[Coord; 4]>,
    bump: impl FnOnce(&mut ThreatCounts),
) {
    if out.s0_instances.len() >= MAX_S0_INSTANCES {
        return;
    }
    bump(&mut out.counts);
    out.s0_instances.push(ThreatInstance {
        kind,
        pieces,
        defense_cells,
    });
}

#[inline]
fn smallvec_one(a: Coord) -> SmallVec<[Coord; 4]> {
    let mut v = SmallVec::new();
    v.push(a);
    v
}

#[inline]
fn smallvec_two(a: Coord, b: Coord) -> SmallVec<[Coord; 4]> {
    let mut v = SmallVec::new();
    v.push(a);
    v.push(b);
    v
}

fn run_pieces(axis: Axis, line_id: i16, start: i16, end: i16) -> SmallVec<[Coord; 5]> {
    let mut v = SmallVec::new();
    let mut p = start;
    while p <= end {
        v.push(coord_at(axis, line_id, p));
        p += 1;
    }
    v
}

/// Reconstruct an axis-line cell from its `(line_id, pos)` pair.
#[inline]
fn coord_at(axis: Axis, line_id: i16, pos: i16) -> Coord {
    match axis {
        Axis::Q => Coord::new(pos, line_id),
        Axis::R => Coord::new(line_id, pos),
        Axis::S => Coord::new(pos, line_id - pos),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Rhombus detection (Phase 28E-2 Stage 1)
//
// Per `HeXOpedia` §4.3, a rhombus is 4 pieces arranged in a diamond shape.
// On axial hex coordinates this is equivalent to 4 cells whose pairwise
// distances are `{1,1,1,1,1,2}` — 5 unit-length edges plus one
// long diagonal of distance 2. Per Threat Theory it is a 3-1-2 threat
// (W=3, S=1, C=2) and per Radius Theory a single opp stone in Ring C
// (hex_distance ≤ 3) of the centroid defends. We therefore only credit
// rhombi that are *isolated* — no opp inside Ring C of the rhombus
// centroid. This binary gate (count-or-skip) matches the `HeXOpedia`
// "guaranteed win in isolation" framing; partial-credit shadings are
// future-phase scope.
// ─────────────────────────────────────────────────────────────────────────────

/// Six axial unit-direction vectors. Together with the pairwise
/// adjacency check `hex_distance(u, v) == 1` they yield exactly the 6
/// distinct rhombus generators per anchor.
const HEX_UNITS: [Coord; 6] = [
    Coord::new(1, 0),
    Coord::new(0, 1),
    Coord::new(1, -1),
    Coord::new(-1, 0),
    Coord::new(0, -1),
    Coord::new(-1, 1),
];

/// Round-half-away-from-zero divide-by-4 used to map a 4-cell sum onto
/// the nearest integer hex cell (rhombus centroid). Bounded inputs
/// (4 × `i16::MAX` fits in `i32`) so the cast back is exact in
/// practice; `i32::try_into` is used to surface any future overflow as
/// a debug panic rather than silently wrapping.
#[inline]
fn round_div4(x: i32) -> i32 {
    if x >= 0 { (x + 2) / 4 } else { -((-x + 2) / 4) }
}

/// Detect isolated rhombi for `player` and bump `counts.rhombus`.
///
/// Algorithm:
///   1. For each own anchor piece `P`, enumerate unordered pairs of
///      unit-direction vectors `(u, v)` with `hex_distance(u, v) == 1`
///      (i.e. directions that share a hex edge). There are exactly 6
///      such pairs across the 6 unit directions.
///   2. The candidate rhombus is `{P, P+u, P+v, P+u+v}`. All four
///      cells must be in `own_set` (a hashset of own pieces).
///   3. Canonicalize by sorting the 4 vertices into a `[Coord; 4]`
///      ascending-`(q, r)`, and dedup via the `seen` set — each
///      rhombus is generated up to 4 times (once per anchor vertex).
///   4. Isolation check: compute the centroid as the round-half-away
///      mean of the 4 vertices, then reject if any opp piece sits
///      within `iso_radius` (`hex_distance`) of the centroid.
///   5. Surviving rhombi increment `counts.rhombus` (saturating u8).
///
/// `iso_radius < 0` collapses to "no isolation" (every rhombus
/// counted). `iso_radius == 0` collapses to "centroid cell empty of
/// opp" — accepted in practice because that cell is a vertex when the
/// rhombus has the canonical sum-divisible-by-4 layout.
fn detect_rhombi(
    own_pieces: &[Coord],
    own_set: &FxHashSet<Coord>,
    opp_set: &FxHashSet<Coord>,
    seen: &mut FxHashSet<[Coord; 4]>,
    iso_radius: i32,
    counts: &mut ThreatCounts,
) {
    // Capped per-call iteration ceiling: own_pieces.len() ≤ board
    // capacity; the inner double loop is 15 pairs (6 choose 2 with
    // adjacency-1) per anchor. No allocation in the hot loop —
    // `seen`, `own_set`, `opp_set` keep their capacity across calls.
    let iso_radius_i16 = i16::try_from(iso_radius.max(0)).unwrap_or(i16::MAX);
    for &p in own_pieces {
        for (i, &u) in HEX_UNITS.iter().enumerate() {
            for &v in HEX_UNITS.iter().skip(i + 1) {
                // Adjacency-1 filter: u and v must be hex-neighbours,
                // i.e. P, P+u, P+v form a unit triangle. Anything else
                // (opposite direction, distance-2 pair) cannot complete
                // the {1,1,1,1,1,2} rhombus signature.
                if hex_distance(u, v) != 1 {
                    continue;
                }
                let p_along_u = p.add(u);
                let p_along_v = p.add(v);
                let p_diag = p_along_u.add(v);
                if !own_set.contains(&p_along_u) {
                    continue;
                }
                if !own_set.contains(&p_along_v) {
                    continue;
                }
                if !own_set.contains(&p_diag) {
                    continue;
                }
                // Canonicalize: sort by (q, r).
                let mut verts = [p, p_along_u, p_along_v, p_diag];
                verts.sort_by_key(|a| (a.q, a.r));
                if !seen.insert(verts) {
                    continue;
                }
                // Isolation: opp within Ring C of centroid?
                let sum_q: i32 = verts.iter().map(|c| i32::from(c.q)).sum();
                let sum_r: i32 = verts.iter().map(|c| i32::from(c.r)).sum();
                #[allow(clippy::cast_possible_truncation)]
                let centroid = Coord::new(
                    round_div4(sum_q) as i16,
                    round_div4(sum_r) as i16,
                );
                let mut isolated = true;
                for &opp in opp_set {
                    if hex_distance(opp, centroid) <= iso_radius_i16 {
                        isolated = false;
                        break;
                    }
                }
                if isolated {
                    counts.rhombus = counts.rhombus.saturating_add(1);
                }
            }
        }
    }
}

