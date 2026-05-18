//! Sparse per-axis line bitmaps.
//!
//! Each `(axis, player, line_id)` maps to a `LineBitmap` — a packed
//! `SmallVec` of `u64` words indexed by position offset along the line.
//! Shared infrastructure for win detection, window-scan eval, and shape
//! detection. Maintained incrementally by `Board::place` / `Board::undo`.

// All `as usize` / `as i16` casts in this module index into a bitmap whose
// range has been validated by `in_range` or `word_index`. Pedantic clippy
// can't track that invariant, so we allow the cast lints locally.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]

use crate::board::Player;
use crate::coords::Coord;
use fxhash::FxHashMap;
use smallvec::SmallVec;

/// One of the three axes of the hex grid.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
#[repr(u8)]
pub enum Axis {
    /// Horizontal axis (constant `r`).
    Q = 0,
    /// Diagonal 1 (constant `q`).
    R = 1,
    /// Diagonal 2 (constant `q + r`).
    S = 2,
}

impl Axis {
    /// All three axes, in declaration order.
    #[inline]
    #[must_use]
    pub const fn all() -> [Axis; 3] {
        [Axis::Q, Axis::R, Axis::S]
    }

    /// Identifier of the line `c` lies on for this axis. Adjacent cells on
    /// the same line share the same `line_id`.
    #[inline]
    #[must_use]
    pub const fn line_id(self, c: Coord) -> i16 {
        match self {
            Axis::Q => c.r,
            Axis::R => c.q,
            Axis::S => c.q + c.r,
        }
    }

    /// Position of `c` along its line for this axis. Adjacent cells on the
    /// same line have consecutive `pos` values.
    #[inline]
    #[must_use]
    pub const fn pos(self, c: Coord) -> i16 {
        match self {
            Axis::R => c.r,
            // Axes Q and S both use `q` as position; they are distinguished
            // by their `line_id`, not their pos formula.
            Axis::Q | Axis::S => c.q,
        }
    }
}

/// Packed bits for one line. Bit `i` of word `words[w]` corresponds to
/// position `base_pos + w * 64 + i`. Empty (no `set` calls yet) → no words.
#[derive(Clone, Debug, Default)]
pub struct LineBitmap {
    words: SmallVec<[u64; 4]>,
    base_pos: i16,
}

impl LineBitmap {
    /// `true` iff bit at `pos` is set. Out-of-range → false.
    #[inline]
    #[must_use]
    pub fn get(&self, pos: i16) -> bool {
        let Some((wi, bi)) = self.indices(pos) else {
            return false;
        };
        (self.words[wi] >> bi) & 1 != 0
    }

    /// Set bit at `pos`. Grows the bitmap if `pos` is outside the covered
    /// range; the in-range path is branchless on the bit.
    #[inline]
    pub fn set(&mut self, pos: i16) {
        if !self.in_range(pos) {
            self.grow_to_include(pos);
        }
        let rel = (i32::from(pos) - i32::from(self.base_pos)) as usize;
        self.words[rel / 64] |= 1u64 << (rel % 64);
    }

    /// Clear bit at `pos`. Never shrinks the underlying allocation.
    #[inline]
    pub fn clear(&mut self, pos: i16) {
        let Some((wi, bi)) = self.indices(pos) else {
            return;
        };
        self.words[wi] &= !(1u64 << bi);
    }

    /// Count consecutive set bits backward from `pos - 1` down to `pos - 5`.
    /// Returns `0..=5`.
    #[inline]
    #[must_use]
    pub fn run_backward(&self, pos: i16) -> u8 {
        let mut count: u8 = 0;
        for i in 1i16..=5 {
            if self.get(pos.wrapping_sub(i)) {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Count consecutive set bits forward from `pos + 1` up to `pos + 5`.
    /// Returns `0..=5`.
    #[inline]
    #[must_use]
    pub fn run_forward(&self, pos: i16) -> u8 {
        let mut count: u8 = 0;
        for i in 1i16..=5 {
            if self.get(pos.wrapping_add(i)) {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// 6-bit window. Bit `i` (LSB-first) is `get(pos + i)`. Used by Layer-1
    /// eval window scan.
    #[inline]
    #[must_use]
    pub fn window6(&self, pos: i16) -> u8 {
        let mut out: u8 = 0;
        for i in 0i16..6 {
            if self.get(pos.wrapping_add(i)) {
                out |= 1 << i;
            }
        }
        out
    }

    #[inline]
    fn in_range(&self, pos: i16) -> bool {
        if self.words.is_empty() {
            return false;
        }
        let rel = i32::from(pos) - i32::from(self.base_pos);
        rel >= 0 && (rel as usize) < self.words.len() * 64
    }

    #[inline]
    fn indices(&self, pos: i16) -> Option<(usize, u32)> {
        if self.words.is_empty() {
            return None;
        }
        let rel = i32::from(pos) - i32::from(self.base_pos);
        if rel < 0 {
            return None;
        }
        let urel = rel as usize;
        let wi = urel / 64;
        if wi >= self.words.len() {
            return None;
        }
        Some((wi, (urel % 64) as u32))
    }

    #[cold]
    fn grow_to_include(&mut self, pos: i16) {
        if self.words.is_empty() {
            // Center the first word so subsequent neighbours sit in the same word.
            self.base_pos = pos.wrapping_sub(32);
            self.words.push(0);
            return;
        }
        let rel = i32::from(pos) - i32::from(self.base_pos);
        if rel < 0 {
            let n_new = ((-rel) as usize).div_ceil(64);
            for _ in 0..n_new {
                self.words.insert(0, 0);
            }
            self.base_pos = self.base_pos.wrapping_sub((n_new as i16) * 64);
        } else {
            let needed = (rel as usize) / 64 + 1;
            while self.words.len() < needed {
                self.words.push(0);
            }
        }
    }
}

/// Per-axis per-player line bitmaps. The only mutators are `set` / `clear`;
/// readers borrow through `Board::axes()`.
#[derive(Clone, Debug, Default)]
pub struct AxisBitmaps {
    lines: [[FxHashMap<i16, LineBitmap>; 2]; 3],
}

impl AxisBitmaps {
    /// Empty maps for all axes / players.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the bit for `(c, p)` on all three axes.
    #[inline]
    pub fn set(&mut self, c: Coord, p: Player) {
        for axis in Axis::all() {
            let id = axis.line_id(c);
            let pos = axis.pos(c);
            self.lines[axis as usize][p as usize]
                .entry(id)
                .or_default()
                .set(pos);
        }
    }

    /// Clear the bit for `(c, p)` on all three axes. No-op if absent.
    #[inline]
    pub fn clear(&mut self, c: Coord, p: Player) {
        for axis in Axis::all() {
            let id = axis.line_id(c);
            let pos = axis.pos(c);
            if let Some(line) = self.lines[axis as usize][p as usize].get_mut(&id) {
                line.clear(pos);
            }
        }
    }

    /// Length of the longest contiguous run of `p`'s stones through `c` on
    /// `axis`. Returns `0` when `c` is not occupied by `p` on that line.
    /// Walks at most ±5 positions; bounded O(1).
    #[must_use]
    pub fn run_length_through(&self, c: Coord, axis: Axis, p: Player) -> u8 {
        let id = axis.line_id(c);
        let pos = axis.pos(c);
        let Some(line) = self.lines[axis as usize][p as usize].get(&id) else {
            return 0;
        };
        if !line.get(pos) {
            return 0;
        }
        1 + line.run_backward(pos) + line.run_forward(pos)
    }

    /// 6-bit window at `(axis, line_id, pos)` for `p`. `0` if the line has
    /// no stones for `p` yet.
    #[must_use]
    pub fn window6(&self, axis: Axis, line_id: i16, pos: i16, p: Player) -> u8 {
        let Some(line) = self.lines[axis as usize][p as usize].get(&line_id) else {
            return 0;
        };
        line.window6(pos)
    }

    /// Endpoints `(start_pos, end_pos)` of the maximal `p`-run on `axis`
    /// through `c`. Returns `None` if `c` is not occupied by `p` on the line.
    /// Inclusive on both sides. The underlying scan walks at most ±5 cells,
    /// so runs longer than 11 are truncated — fine for threat / win
    /// classification, where length ≥ 6 already means terminal.
    #[must_use]
    pub fn run_endpoints(&self, c: Coord, axis: Axis, p: Player) -> Option<(i16, i16)> {
        let id = axis.line_id(c);
        let pos = axis.pos(c);
        let line = self.lines[axis as usize][p as usize].get(&id)?;
        if !line.get(pos) {
            return None;
        }
        let back = i16::from(line.run_backward(pos));
        let fwd = i16::from(line.run_forward(pos));
        Some((pos - back, pos + fwd))
    }
}
