//! Axial hex coordinates.
//!
//! `s = -q - r` is implicit. `Coord` packs into a 32-bit value and passes in a
//! register. `RANGE_OFFSETS` is a precomputed const slice of all offsets from
//! the origin with `1 <= hex_distance <= MAX_PIECE_DISTANCE` — board uses it
//! to update proximity counts in `place`/`undo` without allocating.

use crate::config::MAX_PIECE_DISTANCE;

/// Axial hex coordinate. `s` is implicit (`-q - r`).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
#[repr(C)]
pub struct Coord {
    pub q: i16,
    pub r: i16,
}

/// The origin `(0, 0)`. Required first-move cell.
pub const ORIGIN: Coord = Coord { q: 0, r: 0 };

/// Horizontal axis unit step.
pub const AXIS_Q: Coord = Coord::new(1, 0);
/// Diagonal axis unit step (down-right).
pub const AXIS_R: Coord = Coord::new(0, 1);
/// Diagonal axis unit step (down-left).
pub const AXIS_S: Coord = Coord::new(1, -1);

/// All three axis unit steps. Win-line scans iterate over this.
pub const AXES: [Coord; 3] = [AXIS_Q, AXIS_R, AXIS_S];

impl Coord {
    /// Build a coord.
    #[inline]
    pub const fn new(q: i16, r: i16) -> Self {
        Self { q, r }
    }

    /// Cube `s` coordinate. Invariant: `q + r + s == 0`.
    #[inline]
    pub const fn s(self) -> i16 {
        -self.q - self.r
    }

    /// Component-wise add.
    #[inline]
    pub const fn add(self, other: Coord) -> Coord {
        Coord { q: self.q + other.q, r: self.r + other.r }
    }

    /// Component-wise sub.
    #[inline]
    pub const fn sub(self, other: Coord) -> Coord {
        Coord { q: self.q - other.q, r: self.r - other.r }
    }
}

/// Hex distance between two coords.
#[inline]
pub fn hex_distance(a: Coord, b: Coord) -> i16 {
    let dq = a.q - b.q;
    let dr = a.r - b.r;
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}

/// `hex_distance(a, b) <= range`.
#[inline]
pub fn within_range(a: Coord, b: Coord, range: i16) -> bool {
    hex_distance(a, b) <= range
}

/// Hex of radius `range` around `center`, inclusive of center.
///
/// Allocation-free. Used by board `place`/`undo` to walk the r8 hex.
#[inline]
pub fn for_each_in_range<F: FnMut(Coord)>(center: Coord, range: i16, mut f: F) {
    let r = range;
    let mut dq = -r;
    while dq <= r {
        let lo = if -dq - r > -r { -dq - r } else { -r };
        let hi = if -dq + r < r { -dq + r } else { r };
        let mut dr = lo;
        while dr <= hi {
            f(Coord { q: center.q + dq, r: center.r + dr });
            dr += 1;
        }
        dq += 1;
    }
}

const RANGE: i16 = MAX_PIECE_DISTANCE;

/// Number of cells at distance `1..=MAX_PIECE_DISTANCE` from origin: `3 R (R+1)`.
pub const RANGE_OFFSET_COUNT: usize = 3 * RANGE as usize * (RANGE as usize + 1);

/// All offsets `d` with `1 <= hex_distance(ORIGIN, d) <= MAX_PIECE_DISTANCE`.
/// Excludes origin. Length: `3 * R * (R + 1)`.
pub const RANGE_OFFSETS: [Coord; RANGE_OFFSET_COUNT] = compute_range_offsets();

const fn abs_i16(x: i16) -> i16 {
    if x < 0 { -x } else { x }
}

const fn compute_range_offsets() -> [Coord; RANGE_OFFSET_COUNT] {
    let r = RANGE;
    let mut out = [Coord { q: 0, r: 0 }; RANGE_OFFSET_COUNT];
    let mut idx = 0usize;
    let mut dq = -r;
    while dq <= r {
        let mut dr = -r;
        while dr <= r {
            let ds = -dq - dr;
            let dist = (abs_i16(dq) + abs_i16(dr) + abs_i16(ds)) / 2;
            if dist >= 1 && dist <= r {
                out[idx] = Coord { q: dq, r: dr };
                idx += 1;
            }
            dr += 1;
        }
        dq += 1;
    }
    // Compile-time guard: every slot filled.
    assert!(idx == RANGE_OFFSET_COUNT);
    out
}
