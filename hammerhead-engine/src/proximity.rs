//! Flat-array proximity structures (Phase 16).
//!
//! Replaces the four coord-keyed `FxHashMap` / `FxHashSet` proximity
//! fields on `Board` with bounded-key flat arrays. The Phase 15
//! flamegraph put `for_each_in_range<board::add_proximity>` at the #2
//! user-space position: each `place` walked the r=8 neighbourhood
//! (~217 cells) and probed hashbrown 4×. Flat arrays cut the per-cell
//! cost to ~4 array indexes. See `SPEC_ENGINE.md § Candidate maintenance`.

use crate::axis_bitmap::AxisBitmaps;
use crate::config::{MAX_PIECE_DISTANCE, ZOBRIST_WINDOW};
use crate::coords::{Coord, for_each_in_range};

/// Half-extent of the flat proximity field. Covers the zobrist window
/// (`ZOBRIST_WINDOW`, the max placed-piece coordinate magnitude) plus
/// the `MAX_PIECE_DISTANCE` proximity halo: `add_proximity` touches
/// empty cells up to `MAX_PIECE_DISTANCE` beyond a placed piece, and
/// those cells must still index in-bounds. The prompt's
/// `2 * ZOBRIST_WINDOW + 1` sizing omitted the halo — a piece at the
/// window edge would overflow the field.
pub(crate) const PROX_HALF: i32 = ZOBRIST_WINDOW as i32 + MAX_PIECE_DISTANCE as i32;

/// Side length of the square flat field: `2 * PROX_HALF + 1`.
pub(crate) const PROX_COORD_RANGE: usize = (2 * PROX_HALF + 1) as usize;

/// Total flat-field cell count (`PROX_COORD_RANGE` squared).
pub(crate) const PROX_FIELD_SIZE: usize = PROX_COORD_RANGE * PROX_COORD_RANGE;

/// Flat index of `c` into a `PROX_FIELD_SIZE` array.
///
/// `idx(c) = (c.q + PROX_HALF) * PROX_COORD_RANGE + (c.r + PROX_HALF)`.
#[inline]
#[allow(clippy::cast_sign_loss)] // PROX_HALF offset guarantees non-negative
pub(crate) fn prox_idx(c: Coord) -> usize {
    let q = (i32::from(c.q) + PROX_HALF) as usize;
    let r = (i32::from(c.r) + PROX_HALF) as usize;
    debug_assert!(
        q < PROX_COORD_RANGE && r < PROX_COORD_RANGE,
        "coord {c:?} out of proximity field",
    );
    q * PROX_COORD_RANGE + r
}

/// Initial `members` capacity — matches `Board::INITIAL_MAP_CAPACITY`.
const INITIAL_MEMBERS_CAPACITY: usize = 256;

/// Insertion-ordered set of coords with O(1) `insert`, O(1) swap-remove,
/// O(1) `contains`, and O(N) iteration over a contiguous `Vec`.
///
/// Backed by a dense `members: Vec<Coord>` (the iteration source) plus a
/// flat `slot` array mapping `prox_idx(c)` to the member's position.
/// `remove` does `swap_remove`, which perturbs iteration order — every
/// caller that iterates a `SparseCellSet` must be order-insensitive.
/// See `SPEC_ENGINE.md § Candidate maintenance`.
pub struct SparseCellSet {
    /// Live members in (swap-perturbed) insertion order. Iteration source.
    members: Vec<Coord>,
    /// `slot[prox_idx(c)] == members-position + 1`, or `0` when `c` is
    /// absent. The `+ 1` bias lets `0` double as the absent sentinel.
    slot: Box<[u32]>,
}

impl Default for SparseCellSet {
    fn default() -> Self {
        Self::new()
    }
}

impl SparseCellSet {
    /// Empty set. Allocates the flat `slot` field once (~290 KB at the
    /// default window); reused across `clear`, never reallocated.
    #[must_use]
    #[cold]
    pub fn new() -> Self {
        Self {
            members: Vec::with_capacity(INITIAL_MEMBERS_CAPACITY),
            slot: vec![0u32; PROX_FIELD_SIZE].into_boxed_slice(),
        }
    }

    /// Insert `c`. Returns `true` iff it was newly inserted.
    #[inline]
    #[allow(clippy::cast_possible_truncation)] // len <= PROX_FIELD_SIZE < u32::MAX
    pub fn insert(&mut self, c: Coord) -> bool {
        let i = prox_idx(c);
        // SAFETY: `prox_idx` debug-asserts `i < PROX_FIELD_SIZE`, and
        // `self.slot.len() == PROX_FIELD_SIZE` by ctor.
        let slot = unsafe { self.slot.get_unchecked_mut(i) };
        if *slot != 0 {
            return false;
        }
        self.members.push(c);
        *slot = self.members.len() as u32; // position + 1
        true
    }

    /// Remove `c` via `swap_remove`. Returns `true` iff it was present.
    #[inline]
    pub fn remove(&mut self, c: Coord) -> bool {
        let i = prox_idx(c);
        // SAFETY: identical to `insert` — `prox_idx` debug-asserts in range.
        let pos1 = unsafe { *self.slot.get_unchecked(i) };
        if pos1 == 0 {
            return false;
        }
        let pos = (pos1 - 1) as usize;
        let last = self.members.len() - 1;
        if pos != last {
            let moved = self.members[last];
            self.members[pos] = moved;
            // `moved` now sits at `pos`; its slot value is `pos + 1 == pos1`.
            let moved_idx = prox_idx(moved);
            // SAFETY: `prox_idx` debug-asserts in range.
            unsafe { *self.slot.get_unchecked_mut(moved_idx) = pos1 };
        }
        self.members.pop();
        // SAFETY: `i` validated above.
        unsafe { *self.slot.get_unchecked_mut(i) = 0 };
        true
    }

    /// `true` iff `c` is present. One flat-array probe.
    #[inline]
    #[must_use]
    pub fn contains(&self, c: Coord) -> bool {
        let i = prox_idx(c);
        // SAFETY: `prox_idx` debug-asserts in range.
        unsafe { *self.slot.get_unchecked(i) != 0 }
    }

    /// Iterate the members. Order is insertion order perturbed by every
    /// prior `swap_remove` — do not depend on it.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = Coord> + '_ {
        self.members.iter().copied()
    }

    /// Number of live members.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// `true` iff there are no members.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Drop every member. O(N) — clears only the touched `slot` entries,
    /// keeping both allocations for reuse.
    pub fn clear(&mut self) {
        for &c in &self.members {
            self.slot[prox_idx(c)] = 0;
        }
        self.members.clear();
    }
}

/// Outer (r=8, legality) and inner (r=2, move-gen) per-cell proximity
/// refcounts as flat `u8` arrays.
///
/// `u8` is sufficient: a cell's count is the number of pieces within
/// range, bounded by `hex_area(8) ≈ 217 < 255`. `place` bumps via
/// `saturating_add` so a pathological position cannot wrap; a
/// `debug_assert` flags it in dev builds. A `0` count is exactly
/// "no piece in range" — the flat array has no absent/present
/// distinction, which removes the old `remove_proximity`
/// panic-on-missing invariant.
pub struct ProximityCounts {
    /// r=8 legality refcount, indexed by [`prox_idx`].
    pub(crate) outer: Box<[u8]>,
    /// r=2 move-gen refcount, indexed by [`prox_idx`].
    pub(crate) inner: Box<[u8]>,
}

impl Default for ProximityCounts {
    fn default() -> Self {
        Self::new()
    }
}

impl ProximityCounts {
    /// Two zeroed flat fields. Allocated once; reused across [`Self::clear`].
    #[must_use]
    #[cold]
    pub fn new() -> Self {
        Self {
            outer: vec![0u8; PROX_FIELD_SIZE].into_boxed_slice(),
            inner: vec![0u8; PROX_FIELD_SIZE].into_boxed_slice(),
        }
    }

    /// Zero both fields, keeping the allocations.
    pub fn clear(&mut self) {
        self.outer.fill(0);
        self.inner.fill(0);
    }

    /// Outer (r=8) refcount at `c`. `> 0` ⟺ `c` is within legality range.
    #[inline]
    #[must_use]
    pub fn outer_at(&self, c: Coord) -> u8 {
        let i = prox_idx(c);
        // SAFETY: `prox_idx` debug-asserts `i < PROX_FIELD_SIZE`, and
        // `self.outer.len() == PROX_FIELD_SIZE` by ctor.
        unsafe { *self.outer.get_unchecked(i) }
    }

    /// Inner (r=2) refcount at `c`. `> 0` ⟺ `c` is move-gen-adjacent.
    #[inline]
    #[must_use]
    pub fn inner_at(&self, c: Coord) -> u8 {
        let i = prox_idx(c);
        // SAFETY: identical to `outer_at` — `prox_idx` debug-asserts in range.
        unsafe { *self.inner.get_unchecked(i) }
    }
}

/// Increment the flat proximity `count` field around `center` and insert
/// any cell whose count rose from 0 into `candidates` (if it's empty).
///
/// Used for both the outer (`r8`, legality) and inner
/// (`MOVE_GEN_INNER_RADIUS`, move-gen) fields. `u8` counts are bumped
/// with `saturating_add`; `hex_area(8) ≈ 217 < 255`, so a real position
/// never saturates — the `debug_assert` flags a pathological one.
#[inline]
pub(crate) fn add_proximity(
    count: &mut [u8],
    candidates: &mut SparseCellSet,
    center: Coord,
    radius: i16,
    axes: &AxisBitmaps,
) {
    for_each_in_range(center, radius, |d| {
        let i = prox_idx(d);
        // SAFETY: `prox_idx` debug-asserts `i < PROX_FIELD_SIZE` and
        // `count.len() == PROX_FIELD_SIZE` by ctor (Board owns both
        // halves of the field with matching lengths).
        let cell = unsafe { count.get_unchecked_mut(i) };
        let was_zero = *cell == 0;
        *cell = cell.saturating_add(1);
        debug_assert!(*cell != u8::MAX, "proximity count overflow at {d:?}");
        if d != center && was_zero && !axes.is_occupied(d) {
            candidates.insert(d);
        }
    });
}

/// Decrement the flat proximity `count` field around `center`. When a
/// count reaches 0 the cell is removed from `candidates`. A 0 count is
/// simply "no piece in range" — there is no separate presence entry to
/// drop, so the old panic-on-missing invariant becomes a `debug_assert`.
#[inline]
pub(crate) fn remove_proximity(
    count: &mut [u8],
    candidates: &mut SparseCellSet,
    center: Coord,
    radius: i16,
) {
    for_each_in_range(center, radius, |d| {
        let i = prox_idx(d);
        // SAFETY: `prox_idx` debug-asserts `i < PROX_FIELD_SIZE`.
        let cell = unsafe { count.get_unchecked_mut(i) };
        debug_assert!(*cell > 0, "proximity count underflow at {d:?}");
        *cell -= 1;
        if *cell == 0 {
            candidates.remove(d);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use fxhash::FxHashSet;
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    fn c(q: i16, r: i16) -> Coord {
        Coord::new(q, r)
    }

    #[test]
    fn insert_remove_round_trip() {
        let mut s = SparseCellSet::new();
        assert!(s.is_empty());
        assert!(s.insert(c(0, 0)));
        assert!(!s.insert(c(0, 0))); // already present
        assert!(s.contains(c(0, 0)));
        assert_eq!(s.len(), 1);
        assert!(s.remove(c(0, 0)));
        assert!(!s.remove(c(0, 0))); // already absent
        assert!(!s.contains(c(0, 0)));
        assert!(s.is_empty());
    }

    #[test]
    fn negative_coords_round_trip() {
        let mut s = SparseCellSet::new();
        for &(q, r) in &[(-7, 3), (12, -9), (-1, -1), (0, 5)] {
            assert!(s.insert(c(q, r)));
        }
        assert_eq!(s.len(), 4);
        for &(q, r) in &[(-7, 3), (12, -9), (-1, -1), (0, 5)] {
            assert!(s.contains(c(q, r)));
        }
        assert!(!s.contains(c(3, 3)));
    }

    #[test]
    fn iteration_yields_exactly_the_members() {
        // swap_remove perturbs order, so compare as a set, not a sequence.
        let mut s = SparseCellSet::new();
        for q in 0..10 {
            s.insert(c(q, 0));
        }
        s.remove(c(3, 0));
        s.remove(c(7, 0));
        let got: FxHashSet<Coord> = s.iter().collect();
        let want: FxHashSet<Coord> =
            (0..10).filter(|&q| q != 3 && q != 7).map(|q| c(q, 0)).collect();
        assert_eq!(got, want);
        assert_eq!(s.len(), want.len());
    }

    #[test]
    fn clear_resets_to_empty_and_is_reusable() {
        let mut s = SparseCellSet::new();
        for q in -5..5 {
            s.insert(c(q, q));
        }
        s.clear();
        assert!(s.is_empty());
        assert!(!s.contains(c(0, 0)));
        // Reusable after clear.
        assert!(s.insert(c(1, 1)));
        assert!(s.contains(c(1, 1)));
    }

    #[test]
    fn proximity_counts_start_zero() {
        let p = ProximityCounts::new();
        assert_eq!(p.outer_at(c(0, 0)), 0);
        assert_eq!(p.inner_at(c(13, -8)), 0);
    }

    #[test]
    fn proximity_counts_clear_zeroes_field() {
        let mut p = ProximityCounts::new();
        p.outer[prox_idx(c(2, 2))] = 7;
        p.inner[prox_idx(c(-3, 1))] = 4;
        p.clear();
        assert_eq!(p.outer_at(c(2, 2)), 0);
        assert_eq!(p.inner_at(c(-3, 1)), 0);
    }

    #[test]
    fn random_walk_matches_fxhashset_oracle() {
        let mut rng = StdRng::seed_from_u64(0x5AFE_CE11_CAFE_F00D);
        let mut s = SparseCellSet::new();
        let mut oracle: FxHashSet<Coord> = FxHashSet::default();
        for _ in 0..10_000 {
            let q = rng.random_range(-40..=40);
            let r = rng.random_range(-40..=40);
            let cell = c(q, r);
            if rng.random_bool(0.5) {
                assert_eq!(s.insert(cell), oracle.insert(cell));
            } else {
                assert_eq!(s.remove(cell), oracle.remove(&cell));
            }
            assert_eq!(s.len(), oracle.len());
            assert_eq!(s.contains(cell), oracle.contains(&cell));
        }
        let got: FxHashSet<Coord> = s.iter().collect();
        assert_eq!(got, oracle);
    }
}
