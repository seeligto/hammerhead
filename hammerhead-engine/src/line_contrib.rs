//! Phase-27 per-`(axis, line_id)` Layer-1 contribution cache.
//!
//! Flat slice keyed by `(axis_index * LINE_ID_RANGE) + line_id_idx`. Each
//! entry holds the signed Layer-1 line contribution as a single `i32`
//! (X-positive, both players folded — same convention as the
//! `WINDOW_SCORE_8` lookup that feeds it). The sentinel `i32::MIN`
//! doubles as the dirty / unpopulated marker so the hot-path read is a
//! single bounds-checked load plus an immediate compare.
//!
//! Lifecycle:
//! - [`LineContrib::new`] allocates one boxed slice of `3 * LINE_ID_RANGE`
//!   `i32`s, sentinel-filled. Lazy-populate on first read per slot.
//! - [`LineContrib::reset`] re-sentinels via `fill` without reallocating.
//! - [`LineContrib::invalidate`] re-sentinels one slot on `place` / `undo`.
//!
//! Sized identically to the `axis_bitmap` per-axis flat arrays (the same
//! `LINE_ID_RANGE` derived from `ZOBRIST_WINDOW`), so any `line_id` that
//! `axis_bitmap` admits is in-range here. See `SPEC_EVAL.md` § Caching
//! and `SPEC_ENGINE.md` § Board.
//!
//! Consumed in Phase 27 C-03 (eval Layer-1 rewrite). Scaffold-only until
//! then.

use crate::axis_bitmap::{Axis, LINE_ID_OFFSET, LINE_ID_RANGE};
use crate::coords::Coord;

/// Number of axes. Mirrors `Axis::all().len()` (which is structurally
/// `[Axis; 3]`); kept as a named const so the indexing arithmetic does
/// not embed a bare `3`.
pub(crate) const NUM_AXES: usize = Axis::all().len();

/// Dirty / unpopulated sentinel. Callers must not store this as a real
/// contribution — guarded by a `debug_assert_ne!` in [`LineContrib::set`].
const SENTINEL: i32 = i32::MIN;

/// Flat per-`(axis, line_id)` Layer-1 contribution cache.
///
/// Allocated once at `Board::new`; reset (not reallocated) on
/// `Board::reset`. Lazy-populated: untouched entries stay sentinel and
/// pay zero on init.
pub(crate) struct LineContrib {
    /// Length = `NUM_AXES * LINE_ID_RANGE`. Index = `axis as usize *
    /// LINE_ID_RANGE + line_id_idx`. `SENTINEL` ⟹ dirty / unpopulated.
    slots: Box<[i32]>,
}

impl LineContrib {
    /// Allocate the cache with every slot sentinel-filled.
    #[cold]
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            slots: vec![SENTINEL; NUM_AXES * LINE_ID_RANGE].into_boxed_slice(),
        }
    }

    /// Wipe every slot back to the sentinel. Keeps the allocation alive.
    #[cold]
    pub(crate) fn reset(&mut self) {
        self.slots.fill(SENTINEL);
    }

    /// Mark one slot dirty.
    #[inline]
    pub(crate) fn invalidate(&mut self, axis: Axis, line_id: i16) {
        let idx = slot_index(axis, line_id);
        debug_assert!(idx < self.slots.len(), "slot_index {idx} out of range");
        // SAFETY: `slot_index` debug-asserts `line_id` ∈ window bounds,
        // and `self.slots.len() == NUM_AXES * LINE_ID_RANGE` by ctor;
        // therefore `idx < self.slots.len()` for every legal `line_id`.
        unsafe { *self.slots.get_unchecked_mut(idx) = SENTINEL };
    }

    /// Mark the 3 lines (Q, R, S) through `c` as dirty. Called from
    /// `Board::place` / `Board::undo` / `Board::place_for_test` on every
    /// mutation so the cache stays consistent with `AxisBitmaps`.
    #[inline]
    pub(crate) fn invalidate_coord(&mut self, c: Coord) {
        for axis in Axis::all() {
            self.invalidate(axis, axis.line_id(c));
        }
    }

    /// Read the cached contribution.
    ///
    /// Returns `Some(value)` for a populated slot, `None` if the slot is
    /// sentinel (never written, or explicitly invalidated).
    #[inline]
    #[allow(dead_code)] // consumed in Phase 27 C-03
    pub(crate) fn get(&self, axis: Axis, line_id: i16) -> Option<i32> {
        let v = self.slots[slot_index(axis, line_id)];
        if v == SENTINEL { None } else { Some(v) }
    }

    /// Store a contribution.
    ///
    /// NOTE: `value == i32::MIN` is the dirty sentinel; callers must
    /// never pass it. A subsequent `get` would observe the slot as dirty
    /// and recompute. Guarded by `debug_assert_ne!`.
    #[inline]
    #[allow(dead_code)] // consumed in Phase 27 C-03
    pub(crate) fn set(&mut self, axis: Axis, line_id: i16, value: i32) {
        debug_assert_ne!(
            value, SENTINEL,
            "LineContrib::set received the reserved dirty sentinel value"
        );
        let idx = slot_index(axis, line_id);
        self.slots[idx] = value;
    }
}

/// Flat-array index. Same `(line_id - LINE_ID_OFFSET)` mapping as
/// `AxisBitmaps::idx`. Out-of-range `line_id` panics via the slice
/// bounds check at the call site.
#[inline]
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
fn slot_index(axis: Axis, line_id: i16) -> usize {
    debug_assert!(
        (LINE_ID_OFFSET..=-LINE_ID_OFFSET).contains(&line_id),
        "line_id {line_id} out of zobrist window [{LINE_ID_OFFSET}, {}]",
        -LINE_ID_OFFSET
    );
    // Bounds-check above ensures `line_id - LINE_ID_OFFSET` is in
    // `[0, LINE_ID_RANGE)`, so the cast to `usize` cannot lose the sign.
    let line_idx = (line_id - LINE_ID_OFFSET) as usize;
    axis as usize * LINE_ID_RANGE + line_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_all_none() {
        let c = LineContrib::new();
        // Spot-check a few representative coords across all 3 axes and
        // both line-id ends of the range.
        for axis in Axis::all() {
            for line_id in [LINE_ID_OFFSET, 0_i16, -LINE_ID_OFFSET] {
                assert_eq!(c.get(axis, line_id), None);
            }
        }
    }

    #[test]
    fn set_then_get_round_trips() {
        let mut c = LineContrib::new();
        c.set(Axis::Q, 5, 42);
        assert_eq!(c.get(Axis::Q, 5), Some(42));
        // Untouched neighbours stay dirty.
        assert_eq!(c.get(Axis::Q, 6), None);
        assert_eq!(c.get(Axis::R, 5), None);
        assert_eq!(c.get(Axis::S, 5), None);
    }

    #[test]
    fn invalidate_clears_a_set_slot() {
        let mut c = LineContrib::new();
        c.set(Axis::R, -3, 7);
        assert_eq!(c.get(Axis::R, -3), Some(7));
        c.invalidate(Axis::R, -3);
        assert_eq!(c.get(Axis::R, -3), None);
    }

    #[test]
    fn invalidate_coord_marks_three_axes() {
        use crate::coords::ORIGIN;
        let mut c = LineContrib::new();
        // Pre-populate the 3 lines through ORIGIN with non-sentinel values.
        for axis in Axis::all() {
            c.set(axis, axis.line_id(ORIGIN), 42);
        }
        for axis in Axis::all() {
            assert_eq!(c.get(axis, axis.line_id(ORIGIN)), Some(42));
        }
        // Invalidate at ORIGIN.
        c.invalidate_coord(ORIGIN);
        // All 3 must be None now.
        for axis in Axis::all() {
            assert_eq!(c.get(axis, axis.line_id(ORIGIN)), None);
        }
    }

    #[test]
    fn reset_wipes_every_slot() {
        let mut c = LineContrib::new();
        c.set(Axis::Q, 0, 1);
        c.set(Axis::R, 0, 2);
        c.set(Axis::S, 0, 3);
        c.reset();
        for axis in Axis::all() {
            assert_eq!(c.get(axis, 0), None);
        }
    }

    #[test]
    fn negative_value_round_trips() {
        // Confirm signed contributions (O-positive) survive — the
        // sentinel only collides at the extreme `i32::MIN`.
        let mut c = LineContrib::new();
        c.set(Axis::S, 10, -12_345);
        assert_eq!(c.get(Axis::S, 10), Some(-12_345));
        // Smallest legal value above the sentinel still round-trips.
        c.set(Axis::S, 11, i32::MIN + 1);
        assert_eq!(c.get(Axis::S, 11), Some(i32::MIN + 1));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "reserved dirty sentinel")]
    fn set_rejects_sentinel_in_debug() {
        // `debug_assert_ne!` is a no-op in release; gate this test on
        // debug so `cargo test --release` doesn't surface a false miss.
        let mut c = LineContrib::new();
        c.set(Axis::Q, 0, i32::MIN);
    }
}
