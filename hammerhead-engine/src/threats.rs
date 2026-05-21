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
use crate::coords::Coord;
use fxhash::FxHashSet;
use smallvec::SmallVec;

/// Per-player count of every detected S0 shape. All u8 — saturated at
/// 255 by the detection loop (deep enough never to be reached in legal
/// play).
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
}

/// Tag of an S0 (mate-in-one-turn) threat.
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
    /// Stones forming the run, in axis-order (low pos → high pos).
    pub pieces: SmallVec<[Coord; 5]>,
    /// Cells whose occupation by the opponent denies completion. Size 1 for
    /// closed shapes, size 2 for open shapes.
    pub defense_cells: SmallVec<[Coord; 4]>,
}

/// Per-player threat snapshot. Cheap to clone (counts + small Vec).
/// Search consumers see only `counts` + `s0_instances`.
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
/// every dirty read.
#[derive(Debug, Default)]
pub struct ThreatScratch {
    seen: FxHashSet<(Axis, i16, i16)>,
    pieces: Vec<Coord>,
}

impl ThreatScratch {
    /// Reset the scratch buffers (`seen` dedup and `pieces` work list)
    /// while keeping their backing capacity.
    #[inline]
    fn reset(&mut self) {
        self.seen.clear();
        self.pieces.clear();
    }

    /// Clear every internal buffer. Called from `Board::reset` so a
    /// fresh game starts without stale scratch state.
    #[inline]
    pub fn clear_all(&mut self) {
        self.seen.clear();
        self.pieces.clear();
    }
}

impl ThreatSet {
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
    for (c, p) in board.pieces() {
        if p == player {
            scratch.pieces.push(c);
        }
    }
    walk_linear_runs(board, player, &scratch.pieces, &mut scratch.seen, &mut out);
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

