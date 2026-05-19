//! Threat / shape detection per WSC theory (tenderloin345).
//!
//! Produces per-player [`ThreatCounts`] and a list of S0 [`ThreatInstance`]s
//! with defense cells. Cached on [`Board`], recomputed incrementally
//! within `THREAT_RECOMPUTE_RADIUS` of the last change center.
//!
//! The current implementation always does a full recompute on a dirty read;
//! the `center`/`prior` arguments to [`compute`] reserve the API surface for
//! true incremental scanning, planned for Phase 8.

// `span` is guaranteed to be in `[2, 5]` by the surrounding range check
// before each `as u8` cast in `walk_linear_runs`.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]

use crate::axis_bitmap::Axis;
use crate::board::{Board, Player};
use crate::config::MAX_S0_INSTANCES;
use crate::coords::Coord;
use fxhash::FxHashSet;
use smallvec::SmallVec;

/// Per-player count of every detected shape. All u8 — saturated at 255 by
/// the detection loop (deep enough never to be reached in legal play).
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
    /// `_XXX_` with room to grow to a 6-window.
    pub open_3: u8,
    /// 4-piece hex parallelogram.
    pub rhombus: u8,
    /// 3-piece L-shape (one bend on the hex grid).
    pub arch: u8,
    /// 5-piece bowtie (two triangles sharing an edge).
    pub bone: u8,
    /// 5-piece trapezoid / pentagon.
    pub trapezoid: u8,
    /// 2-piece run isolated from opponent within 2 cells on the same axis.
    pub open_2: u8,
    /// `OXXX_` (one end empty).
    pub closed_3: u8,
    /// 3 mutually-adjacent stones.
    pub triangle: u8,
}

/// Tag of an S0 (mate-in-one-turn) threat. Cross-axis shapes are S1/S2 and
/// are not represented here — they only contribute to [`ThreatCounts`].
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
}

/// One detected S0 threat with its participating pieces and the minimal
/// blocker set that denies completion next stone.
#[derive(Clone, Debug)]
pub struct ThreatInstance {
    /// Shape category.
    pub kind: ThreatKind,
    /// Stones forming the run, in axis-order.
    pub pieces: SmallVec<[Coord; 5]>,
    /// Cells whose occupation by the opponent denies completion. Size 1 for
    /// closed shapes, size 2 for open shapes.
    pub defense_cells: SmallVec<[Coord; 4]>,
}

/// Per-player threat snapshot. Cheap to clone (counts + small Vec).
#[derive(Clone, Debug, Default)]
pub struct ThreatSet {
    /// Shape counts across all detected threats.
    pub counts: ThreatCounts,
    /// S0 threats (mate-in-one-turn). One entry per distinct run.
    pub s0_instances: Vec<ThreatInstance>,
}

/// Reusable scratch buffers for `compute`. Owned by `Board` and reset
/// between calls so the `FxHashSet` `seen` dedup and the per-player
/// pieces `Vec` keep their backing capacity instead of reallocating on
/// every dirty read. Cleared (not shrunk) at the start of each
/// `compute_with_scratch`.
#[derive(Debug, Default)]
pub struct ThreatScratch {
    seen: FxHashSet<(Axis, i16, i16)>,
    pieces: Vec<Coord>,
}

impl ThreatScratch {
    #[inline]
    fn reset(&mut self) {
        self.seen.clear();
        self.pieces.clear();
    }
}

impl ThreatSet {
    /// `true` iff at least two S0 threats exist and no single cell is in
    /// every threat's `defense_cells`. Conservative: a real fork-mate
    /// requires no 2-cell response covers all threats; this returns `true`
    /// for the simpler "no 1-cell response covers all" — a primitive used by
    /// Phase 5's full fork detector.
    #[must_use]
    pub fn is_mate_pending(&self) -> bool {
        self.s0_instances.len() >= 2 && !single_cell_blocks_all(&self.s0_instances)
    }
}

/// Compute the threat set for `player` on `board`.
///
/// `center = Some(c)` and `prior = Some(_)` are an incremental hint — drop
/// instances inside the dirty radius of `c`, rescan only that neighbourhood,
/// merge with the prior set. `center = None` forces a full recompute (used
/// after [`Board::reset`]).
///
/// The current implementation always does a full recompute; the hint is
/// accepted but ignored. The API is stable for the planned Phase 8
/// incremental optimisation.
///
/// This convenience wrapper allocates a fresh `ThreatScratch` per call.
/// `Board::threats` uses [`compute_with_scratch`] directly so the
/// search hot path reuses backing storage across nodes.
#[must_use]
pub fn compute(
    board: &Board,
    player: Player,
    center: Option<Coord>,
    prior: Option<&ThreatSet>,
) -> ThreatSet {
    let mut scratch = ThreatScratch::default();
    compute_with_scratch(board, player, center, prior, &mut scratch)
}

/// Variant of [`compute`] that reuses caller-provided scratch buffers.
/// `scratch` is reset on entry, so the caller can freely reuse the same
/// buffers across many calls — only the buffers' capacities are
/// retained, eliminating the per-call allocation seen in the
/// flamegraph's threats compute frame.
#[must_use]
#[allow(unused_variables)]
pub fn compute_with_scratch(
    board: &Board,
    player: Player,
    center: Option<Coord>,
    prior: Option<&ThreatSet>,
    scratch: &mut ThreatScratch,
) -> ThreatSet {
    full_recompute(board, player, scratch)
}

#[cold]
fn full_recompute(board: &Board, player: Player, scratch: &mut ThreatScratch) -> ThreatSet {
    let mut out = ThreatSet::default();
    scratch.reset();
    for (c, p) in board.pieces() {
        if p == player {
            scratch.pieces.push(c);
        }
    }

    walk_linear_runs(board, player, &scratch.pieces, &mut scratch.seen, &mut out);
    walk_cross_axis(board, player, &scratch.pieces, &mut out.counts);

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
            let left_open = board.piece_at(left_cell) != Some(opp);
            let right_open = board.piece_at(right_cell) != Some(opp);
            debug_assert!(
                board.piece_at(left_cell) != Some(player),
                "non-maximal run on left"
            );
            debug_assert!(
                board.piece_at(right_cell) != Some(player),
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
            if board.piece_at(beyond) != Some(opp) {
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
            if has_room_for_six(board, player, axis, line_id, start_pos, end_pos) {
                out.counts.open_3 = out.counts.open_3.saturating_add(1);
            }
        }
        (3, 1) => {
            out.counts.closed_3 = out.counts.closed_3.saturating_add(1);
        }
        (2, 2) => {
            if is_isolated_open_two(board, player, axis, line_id, start_pos) {
                out.counts.open_2 = out.counts.open_2.saturating_add(1);
            }
        }
        _ => {}
    }
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

/// For a 3-run `_XXX_` at `[start..=end]`: at least one 6-cell window
/// containing the run is opp-free. The two flank cells `start-1` /
/// `end+1` are already empty (callers ensure `open_ends == 2`), so this
/// reduces to "at least one of the cells 2 beyond the run is not opp".
fn has_room_for_six(
    board: &Board,
    player: Player,
    axis: Axis,
    line_id: i16,
    start: i16,
    end: i16,
) -> bool {
    let beyond_left = coord_at(axis, line_id, start - 2);
    let beyond_right = coord_at(axis, line_id, end + 2);
    let opp = player.opponent();
    board.piece_at(beyond_left) != Some(opp) || board.piece_at(beyond_right) != Some(opp)
}

/// Open-2 qualifier: no opponent stone within 2 cells either side along
/// the axis. Run is at `[start..=start+1]`.
fn is_isolated_open_two(
    board: &Board,
    player: Player,
    axis: Axis,
    line_id: i16,
    start: i16,
) -> bool {
    let opp = player.opponent();
    for delta in [-2_i16, -1, 2, 3] {
        let c = coord_at(axis, line_id, start + delta);
        if board.piece_at(c) == Some(opp) {
            return false;
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Cross-axis shape detection
// ─────────────────────────────────────────────────────────────────────────────

/// 4-piece rhombus patterns: 3 orientations, one per axis-pair. Each entry
/// is the offset list of the other three stones relative to the anchor at
/// `(0,0)`. The anchor is the lex-min stone of the rhombus; offsets are all
/// strictly lex-positive so each rhombus is enumerated exactly once.
const RHOMBUS_PATTERNS: &[[(i16, i16); 3]] = &[
    // axes (Q, R): {(0,0), (1,0), (0,1), (1,1)}
    [(1, 0), (0, 1), (1, 1)],
    // axes (Q, S): {(0,0), (1,0), (1,-1), (2,-1)}
    [(1, 0), (1, -1), (2, -1)],
    // axes (R, S): {(0,0), (0,1), (1,-1), (1,0)} — lex-min still (0,0).
    [(0, 1), (1, -1), (1, 0)],
];

/// 3 mutually-adjacent stones. Two orientations (upward / downward).
/// Anchor = lex-min stone.
const TRIANGLE_PATTERNS: &[[(i16, i16); 2]] = &[
    // Upward: {(0,0), (1,0), (0,1)}
    [(1, 0), (0, 1)],
    // Downward: {(0,0), (1,0), (1,-1)}
    [(1, 0), (1, -1)],
];

/// 3-piece arches (L-shape): two adjacent pairs, one distance-2 pair.
/// Anchor = lex-min stone. Patterns enumerated by axis-pair / chirality.
const ARCH_PATTERNS: &[[(i16, i16); 2]] = &[
    // {(0,0), (1,0), (1,1)}
    [(1, 0), (1, 1)],
    // {(0,0), (1,0), (2,-1)}
    [(1, 0), (2, -1)],
    // {(0,0), (0,1), (-1,2)}
    [(0, 1), (-1, 2)],
    // {(0,0), (1,-1), (2,-1)}
    [(1, -1), (2, -1)],
];

/// 5-piece trapezoid: parallel long-edge pair plus short closing edge.
const TRAPEZOID_PATTERNS: &[[(i16, i16); 4]] = &[
    // axes Q-long, R-short:
    [(1, 0), (2, 0), (0, 1), (1, 1)],
    // axes Q-long, S-short:
    [(1, 0), (2, 0), (1, -1), (2, -1)],
    // axes R-long, S-short:
    [(0, 1), (0, 2), (1, -1), (1, 0)],
];

/// 5-piece bone / bowtie: two triangles sharing an edge.
const BONE_PATTERNS: &[[(i16, i16); 4]] = &[
    [(1, 0), (0, 1), (-1, 1), (1, -1)],
    [(1, 0), (1, -1), (2, -1), (0, 1)],
    [(0, 1), (1, -1), (1, 0), (-1, 2)],
];

fn walk_cross_axis(board: &Board, player: Player, pieces: &[Coord], out: &mut ThreatCounts) {
    for &anchor in pieces {
        for pat in TRIANGLE_PATTERNS {
            if matches_pattern(board, player, anchor, pat) {
                out.triangle = out.triangle.saturating_add(1);
            }
        }
        for pat in ARCH_PATTERNS {
            if matches_pattern(board, player, anchor, pat) {
                out.arch = out.arch.saturating_add(1);
            }
        }
        for pat in RHOMBUS_PATTERNS {
            if matches_pattern(board, player, anchor, pat) {
                out.rhombus = out.rhombus.saturating_add(1);
            }
        }
        for pat in BONE_PATTERNS {
            if matches_pattern(board, player, anchor, pat) {
                out.bone = out.bone.saturating_add(1);
            }
        }
        for pat in TRAPEZOID_PATTERNS {
            if matches_pattern(board, player, anchor, pat) {
                out.trapezoid = out.trapezoid.saturating_add(1);
            }
        }
    }
}

#[inline]
fn matches_pattern<const N: usize>(
    board: &Board,
    player: Player,
    anchor: Coord,
    offsets: &[(i16, i16); N],
) -> bool {
    for (dq, dr) in offsets {
        let c = Coord::new(anchor.q + dq, anchor.r + dr);
        if board.piece_at(c) != Some(player) {
            return false;
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Fork primitives
// ─────────────────────────────────────────────────────────────────────────────

/// `true` iff a single cell appears in every instance's `defense_cells`.
/// Empty `insts` returns `true` (vacuously coverable). Used by
/// [`ThreatSet::is_mate_pending`] and by Phase 5's fork-mate scorer.
#[inline]
#[must_use]
pub fn single_cell_blocks_all(insts: &[ThreatInstance]) -> bool {
    let Some(first) = insts.first() else {
        return true;
    };
    'outer: for candidate in &first.defense_cells {
        for inst in &insts[1..] {
            if !inst.defense_cells.contains(candidate) {
                continue 'outer;
            }
        }
        return true;
    }
    false
}
