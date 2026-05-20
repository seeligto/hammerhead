//! Win detection.
//!
//! `HeXO` wins on 6-in-row (or longer overlines). Checked locally through
//! the just-placed stone via per-axis line bitmaps — bounded `O(1)`.

use crate::axis_bitmap::Axis;
use crate::board::{Board, Player};
use crate::coords::Coord;

/// Returns `true` iff placing `c` (already done by `p` on `board`) produces
/// a run of ≥ 6 stones on any of the three axes.
///
/// Bounded `O(1)`: scans ±5 along each of 3 axes via the axis bitmap.
#[inline]
#[must_use]
pub fn is_winning_move(board: &Board, c: Coord, p: Player) -> bool {
    for axis in Axis::all() {
        if board.axes().run_length_through(c, axis, p) >= 6 {
            return true;
        }
    }
    false
}
