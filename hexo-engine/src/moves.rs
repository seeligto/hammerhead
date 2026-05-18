//! Radius-aware move generation.
//!
//! Per-stone generation: search calls this once per ply, not once per turn —
//! the two stones of a `HeXO` turn each get their own ordering and pruning.
//!
//! Three paths:
//! 1. Empty board → `{ORIGIN}` (forced first move).
//! 2. `radius <= MOVE_GEN_INNER_RADIUS` → copy the maintained inner
//!    candidate set; no scanning.
//! 3. `radius >  MOVE_GEN_INNER_RADIUS` → forward-sweep: union the
//!    `radius`-hex neighbourhood of each piece, dedup via a scratch
//!    `FxHashSet`.
//!
//! Results are in insertion order (arbitrary). Ordering and the
//! `MOVE_GEN_CAP` truncation are the ordering module's job (Phase 7).

use crate::board::Board;
use crate::config::{MAX_PIECE_DISTANCE, MOVE_GEN_INNER_RADIUS};
use crate::coords::{Coord, ORIGIN, for_each_in_range};
use fxhash::FxHashSet;
use smallvec::SmallVec;

/// `SmallVec` inline capacity for [`MoveList`]. Slightly above the typical
/// `MOVE_GEN_CAP` of 30 so the common case stays on-stack.
pub const MOVE_GEN_CAP_INLINE: usize = 32;

/// Result list returned by [`generate`]. Inline-allocated up to
/// [`MOVE_GEN_CAP_INLINE`] entries; spills to heap beyond that.
pub type MoveList = SmallVec<[Coord; MOVE_GEN_CAP_INLINE]>;

/// Generate candidate moves on `board` within `radius` of any existing piece.
///
/// See module docs for the path dispatch. The returned list is in arbitrary
/// (insertion) order; the ordering module is responsible for ranking and
/// applying `MOVE_GEN_CAP`. `generate` never truncates.
#[must_use]
pub fn generate(board: &Board, radius: i16) -> MoveList {
    let mut out: MoveList = SmallVec::new();

    if board.ply() == 0 {
        out.push(ORIGIN);
        return out;
    }

    if radius <= MOVE_GEN_INNER_RADIUS {
        out.extend(board.inner_candidates());
        return out;
    }

    let r = radius.min(MAX_PIECE_DISTANCE);
    sweep_neighbourhood(board, r, &mut out);
    out
}

/// Forward-sweep the `r`-hex neighbourhood of every piece, deduping into
/// `out`. One scratch `FxHashSet` allocation per call, pre-reserved.
fn sweep_neighbourhood(board: &Board, r: i16, out: &mut MoveList) {
    let mut seen: FxHashSet<Coord> = FxHashSet::default();
    seen.reserve(board.piece_count().saturating_mul(8));

    for (piece, _) in board.pieces() {
        for_each_in_range(piece, r, |d| {
            if d == piece {
                return;
            }
            if board.is_empty_cell(d) && seen.insert(d) {
                out.push(d);
            }
        });
    }
}
