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
//!
//! The caller owns the destination buffer; `generate` clears it before
//! writing. Search threads per-ply slots through `SearchScratch`
//! (Phase 25.5) so the underlying allocation amortises to one per slot
//! across an entire search.

use crate::board::Board;
use crate::config::{MAX_PIECE_DISTANCE, MOVE_GEN_INNER_RADIUS};
use crate::coords::{Coord, ORIGIN, for_each_in_range};
use fxhash::FxHashSet;

/// Historical hint at the post-truncation `MOVE_GEN_CAP`. Kept as a public
/// constant for back-compat with downstream callers that sized their own
/// scratch buffers against it; the in-tree search no longer relies on the
/// value because per-ply `Vec<Coord>` slots in `SearchScratch` retain
/// capacity across nodes.
pub const MOVE_GEN_CAP_INLINE: usize = 32;

/// Generate candidate moves on `board` within `radius` of any existing
/// piece, writing into `out`.
///
/// `out` is cleared first; on return it holds the (un-ordered) candidate
/// list. `radius` is effectively clamped on both ends:
/// - Any `radius <= MOVE_GEN_INNER_RADIUS` returns the maintained inner
///   candidate set (the inner refcount is the smallest grain we maintain).
/// - Any `radius > MAX_PIECE_DISTANCE` is clamped to `MAX_PIECE_DISTANCE`
///   since no cell beyond that is ever legal.
///
/// See module docs for path dispatch. The returned list is in arbitrary
/// (insertion) order; ordering and `MOVE_GEN_CAP` truncation are the
/// ordering module's job — `generate` never truncates.
pub fn generate(board: &Board, radius: i16, out: &mut Vec<Coord>) {
    out.clear();

    if board.ply() == 0 {
        out.push(ORIGIN);
        return;
    }

    if radius <= MOVE_GEN_INNER_RADIUS {
        out.extend(board.inner_candidates());
        return;
    }

    let r = radius.min(MAX_PIECE_DISTANCE);
    sweep_neighbourhood(board, r, out);
}

/// Hex-area excluding center: `3 r (r + 1)`. Used to size the dedup scratch.
#[inline]
fn hex_area_excl_center(r: i16) -> usize {
    let r = usize::try_from(r).unwrap_or(0);
    3 * r * (r + 1)
}

/// Forward-sweep the `r`-hex neighbourhood of every piece, deduping into
/// `out`. One scratch `FxHashSet` allocation per call, pre-reserved with a
/// loose upper bound. Caller guarantees `r >= 1`.
fn sweep_neighbourhood(board: &Board, r: i16, out: &mut Vec<Coord>) {
    // Loose upper bound on unique cells visited: `pieces * hex_area(r)`.
    // Heavy overlap in real games means the set rarely fills this much, but
    // the cost of over-reserving an FxHashSet briefly is cheaper than the
    // rehash cascade we get from under-sizing.
    let mut seen: FxHashSet<Coord> = FxHashSet::default();
    seen.reserve(hex_area_excl_center(r) * board.piece_count());

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
