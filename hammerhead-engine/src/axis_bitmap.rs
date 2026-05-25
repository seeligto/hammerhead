//! Sparse per-axis line bitmaps.
//!
//! Each `(axis, player, line_id)` maps to a `LineBitmap` — a packed
//! `SmallVec` of `u64` words indexed by position offset along the line.
//! Shared infrastructure for win detection, window-scan eval, and shape
//! detection. Maintained incrementally by `Board::place` / `Board::undo`.
//!
//! Storage is a fixed-length flat array per `(axis, player)` indexed by
//! `(line_id - LINE_ID_OFFSET)`. Line IDs are bounded by `±ZOBRIST_WINDOW`
//! (the same window that constrains the per-cell zobrist key table); the
//! array length is `LINE_ID_RANGE = 2 * ZOBRIST_WINDOW + 1`. Phase 13
//! replaced the prior `FxHashMap<i16, LineBitmap>` storage after the Phase
//! 12 flamegraph identified hashbrown probes inside the window / `is_set`
//! / `line` scans as the largest user-space cost. See `SPEC_ENGINE.md`.

// All `as usize` / `as i16` casts in this module index into a bitmap whose
// range has been validated by `in_range` or `word_index`. Pedantic clippy
// can't track that invariant, so we allow the cast lints locally.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]

use crate::board::Player;
use crate::config::ZOBRIST_WINDOW;
use crate::coords::Coord;
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
///
/// `#[repr(align(64))]` pads each `LineBitmap` to a full cache line so
/// consecutive entries in the `AxisBitmaps::lines` flat array never
/// straddle a cache line — the Layer-1 eval walks one line at a time,
/// and the metadata + first inline word both want to live in a single
/// load.
#[derive(Clone, Debug, Default)]
#[repr(align(64))]
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

    /// 8-bit window. Bit `i` (LSB-first) is `get(pos + i)`. Used by the
    /// Phase-17 Layer-1 8-cell window scan.
    #[inline]
    #[must_use]
    pub fn window8(&self, pos: i16) -> u8 {
        let mut out: u8 = 0;
        for i in 0i16..8 {
            if self.get(pos.wrapping_add(i)) {
                out |= 1 << i;
            }
        }
        out
    }

    /// Emit `count` consecutive 8-bit windows starting at `start_pos`
    /// into `out[..count]`. Extracts each 8-bit window straight out of
    /// the underlying `u64` storage, sharing the `(word_index,
    /// bit_offset)` math across windows. The Layer-1 eval window scan
    /// calls this once per `(axis, line)`.
    ///
    /// # Panics
    ///
    /// Panics if `out.len() < count`.
    #[inline]
    pub fn windows8_run(&self, start_pos: i16, count: usize, out: &mut [u8]) {
        assert!(out.len() >= count, "windows8_run out slice too short");
        let out = &mut out[..count];
        if out.is_empty() {
            return;
        }
        if self.words.is_empty() {
            out.fill(0);
            return;
        }
        let base = i32::from(self.base_pos);
        let nbits = (self.words.len() as i32) * 64;
        for (k, slot) in out.iter_mut().enumerate() {
            let pos = i32::from(start_pos.wrapping_add(k as i16));
            let rel = pos - base;
            // Window8 reads bits [pos, pos+7]. Whole window out of
            // range → emit 0 fast.
            if rel + 8 <= 0 || rel >= nbits {
                *slot = 0;
                continue;
            }
            // Fast path: window fully inside the populated range,
            // bits drawn from at most two adjacent words.
            if rel >= 0 && rel + 8 <= nbits {
                let wi = (rel / 64) as usize;
                let bi = (rel % 64) as u32;
                let lo = self.words[wi] >> bi;
                let bits = if bi <= 56 {
                    lo
                } else {
                    lo | (self.words[wi + 1] << (64 - bi))
                };
                *slot = bits as u8;
                continue;
            }
            // Slow path: window straddles the populated-range boundary.
            *slot = self.window8(pos as i16);
        }
    }

    /// `(min_pos, max_pos)` of the bits that are set, or `None` if empty.
    /// Walks the word array; O(words). Used by Layer 1 to bound the
    /// window-scan range per line.
    #[must_use]
    pub fn populated_range(&self) -> Option<(i16, i16)> {
        let mut first: Option<i16> = None;
        let mut last: Option<i16> = None;
        for (wi, &w) in self.words.iter().enumerate() {
            if w == 0 {
                continue;
            }
            let wi_i16 = wi as i16;
            let word_base = self.base_pos.wrapping_add(wi_i16 * 64);
            if first.is_none() {
                let bi = w.trailing_zeros() as i16;
                first = Some(word_base.wrapping_add(bi));
            }
            let bi = w.ilog2() as i16;
            last = Some(word_base.wrapping_add(bi));
        }
        first.zip(last)
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

/// Fixed length of each `(axis, player)` flat array. Line IDs lie in
/// `[-2*ZOBRIST_WINDOW, +2*ZOBRIST_WINDOW]` because axis-S `line_id` is
/// `q + r`, which can reach `±2*ZOBRIST_WINDOW` even when both `q` and
/// `r` stay inside `[-ZOBRIST_WINDOW, +ZOBRIST_WINDOW]` (the
/// per-coordinate zobrist window). Axes Q and R only reach `±ZOBRIST_WINDOW`
/// individually, but we keep a single uniform range across all axes for
/// simplicity (per-axis sizing saves only ~50 KB out of ~150 KB total).
/// Default `ZOBRIST_WINDOW = 127` → `LINE_ID_RANGE = 509`.
pub(crate) const LINE_ID_RANGE: usize = (4 * ZOBRIST_WINDOW + 1) as usize;
pub(crate) const LINE_ID_OFFSET: i16 = -2 * ZOBRIST_WINDOW;

/// Per-axis per-player line bitmaps. The only mutators are `set` / `clear`;
/// readers borrow through `Board::axes()`. Storage is a flat array indexed
/// by `(line_id - LINE_ID_OFFSET)` so every probe is a bounds-checked
/// array load instead of a hashbrown chase.
#[derive(Clone, Debug)]
pub struct AxisBitmaps {
    /// `[axis][player]` → fixed-length flat array of optional line bitmaps.
    /// `None` until the first `set` on that line; `clear` does not
    /// deallocate (keeps the `Some(empty)` slot for re-use).
    lines: [[Box<[Option<LineBitmap>]>; 2]; 3],
    /// `[axis][player]` → list of every `line_id` ever touched by `set`
    /// (insertion order, never removed). Backs `line_ids()` so the eval
    /// hot path enumerates populated lines without scanning the full
    /// LINE_ID_RANGE-long flat array. Mirrors the prior `FxHashMap` key
    /// retention: a line stays listed even after a `clear` empties it,
    /// matching the old hashmap's "key persists" semantics.
    populated_ids: [[SmallVec<[i16; 32]>; 2]; 3],
    /// `[axis]` → unified occupancy bitmap (no player dimension). Set
    /// whenever either player places at the cell; cleared on any `clear`
    /// (`HeXO` has at most one stone per cell, so the other player can't
    /// own it). Backs `is_occupied(c)` as a single per-axis probe — the
    /// hot path inside `Board::add_proximity`'s neighbour-occupancy
    /// check fires hundreds of times per place, so a single bitmap load
    /// beats two per-player probes by ~6% NPS.
    occupied: [Box<[Option<LineBitmap>]>; 3],
}

impl Default for AxisBitmaps {
    fn default() -> Self {
        Self::new()
    }
}

impl AxisBitmaps {
    /// All slots empty.
    #[must_use]
    pub fn new() -> Self {
        Self {
            lines: std::array::from_fn(|_| std::array::from_fn(|_| empty_line_slots())),
            populated_ids: std::array::from_fn(|_| std::array::from_fn(|_| SmallVec::new())),
            occupied: std::array::from_fn(|_| empty_line_slots()),
        }
    }

    /// Convert a `line_id` to a flat-array index. Panics in debug builds
    /// when out of `[-2*ZOBRIST_WINDOW, +2*ZOBRIST_WINDOW]`; in release it
    /// wraps silently — callers must ensure coords stay in the zobrist
    /// window.
    #[inline]
    fn idx(line_id: i16) -> usize {
        debug_assert!(
            (LINE_ID_OFFSET..=-LINE_ID_OFFSET).contains(&line_id),
            "line_id {line_id} out of zobrist window [{LINE_ID_OFFSET}, {}]",
            -LINE_ID_OFFSET
        );
        (line_id - LINE_ID_OFFSET) as usize
    }

    /// Set the bit for `(c, p)` on all three axes (and update the unified
    /// occupancy bitmap so `is_occupied` is a single per-axis probe).
    #[inline]
    pub fn set(&mut self, c: Coord, p: Player) {
        for axis in Axis::all() {
            let id = axis.line_id(c);
            let pos = axis.pos(c);
            let idx = Self::idx(id);
            let slot = &mut self.lines[axis as usize][p as usize][idx];
            if slot.is_none() {
                // First touch of this line — register it so `line_ids` can
                // enumerate populated lines without walking the full array.
                self.populated_ids[axis as usize][p as usize].push(id);
            }
            slot.get_or_insert_with(LineBitmap::default).set(pos);
            self.occupied[axis as usize][idx]
                .get_or_insert_with(LineBitmap::default)
                .set(pos);
        }
    }

    /// Clear the bit for `(c, p)` on all three axes. No-op if the line slot
    /// is empty. Always clears the unified occupancy bit because `HeXO`
    /// permits at most one stone per cell, so the other player cannot
    /// own it.
    #[inline]
    pub fn clear(&mut self, c: Coord, p: Player) {
        for axis in Axis::all() {
            let id = axis.line_id(c);
            let pos = axis.pos(c);
            let idx = Self::idx(id);
            if let Some(line) = &mut self.lines[axis as usize][p as usize][idx] {
                line.clear(pos);
            }
            if let Some(line) = &mut self.occupied[axis as usize][idx] {
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
        let i = Self::idx(id);
        // SAFETY: see `is_set`. Sprint 3D.
        let Some(line) = unsafe { self.lines[axis as usize][p as usize].get_unchecked(i) }.as_ref()
        else {
            return 0;
        };
        if !line.get(pos) {
            return 0;
        }
        1 + line.run_backward(pos) + line.run_forward(pos)
    }

    /// `true` iff `p` has a stone at `(axis, line_id, pos)`. Cheap single-bit
    /// probe used by the Layer 1 extension-factor check in eval and by
    /// `Board::piece_at` / `is_empty_cell` (Phase 13).
    #[inline]
    #[must_use]
    pub fn is_set(&self, axis: Axis, line_id: i16, pos: i16, p: Player) -> bool {
        let i = Self::idx(line_id);
        // SAFETY: `Self::idx` debug-asserts `line_id` in the zobrist window;
        // `lines[axis][p]` length is LINE_ID_RANGE by ctor (see
        // `empty_line_slots`). Sprint 3D — Phase 25 guardrail elided
        // bounds-check on hot read path.
        match unsafe { self.lines[axis as usize][p as usize].get_unchecked(i) }.as_ref() {
            Some(line) => line.get(pos),
            None => false,
        }
    }

    /// `true` iff `p` owns `c`. Single-probe shortcut against the
    /// Q-axis bitmap — equivalent to `is_set(Axis::Q, line_id(c),
    /// pos(c), p)`. Threat detection's `matches_pattern` and the
    /// flank-cell checks in `walk_linear_runs` use this to avoid the
    /// two-probe `piece_at` path that disambiguates `Some(X) /
    /// Some(O) / None` from the unified occupancy bitmap.
    #[inline]
    #[must_use]
    pub fn is_player(&self, c: Coord, p: Player) -> bool {
        let id = Axis::Q.line_id(c);
        let pos = Axis::Q.pos(c);
        self.is_set(Axis::Q, id, pos, p)
    }

    /// Player owning `c`, or `None` if empty. Short-circuits on the
    /// unified occupancy bitmap when the cell is empty (the common case
    /// for `piece_at` queries on flank cells in threat detection), then
    /// probes one player bitmap to disambiguate. Replaces
    /// `Board::pieces`'s `HashMap` `get` (Phase 13).
    #[inline]
    #[must_use]
    pub fn player_at(&self, c: Coord) -> Option<Player> {
        if !self.is_occupied(c) {
            return None;
        }
        let id = Axis::Q.line_id(c);
        let pos = Axis::Q.pos(c);
        if self.is_set(Axis::Q, id, pos, Player::X) {
            Some(Player::X)
        } else {
            Some(Player::O)
        }
    }

    /// `true` iff either player has a stone at `c`. Single-probe lookup
    /// against the unified per-axis occupancy bitmap maintained by
    /// `set` / `clear`. Used inside the proximity-maintenance loop
    /// (`Board::add_proximity`) where this fires ~470 times per
    /// placement; a single load beats two per-player probes.
    #[inline]
    #[must_use]
    pub fn is_occupied(&self, c: Coord) -> bool {
        let id = Axis::Q.line_id(c);
        let pos = Axis::Q.pos(c);
        let i = Self::idx(id);
        // SAFETY: `Self::idx` debug-asserts in the zobrist window;
        // `occupied[Axis::Q]` length is LINE_ID_RANGE by ctor. Sprint 3D
        // — Phase 25 guardrail elided bounds-check on hot read path.
        unsafe { self.occupied[Axis::Q as usize].get_unchecked(i) }
            .as_ref()
            .is_some_and(|l| l.get(pos))
    }

    /// Borrow the underlying [`LineBitmap`] for `(axis, p, line_id)`, if any
    /// stones have been placed on that line for that player.
    #[inline]
    #[must_use]
    pub fn line(&self, axis: Axis, p: Player, line_id: i16) -> Option<&LineBitmap> {
        let i = Self::idx(line_id);
        // SAFETY: see `is_set`. Sprint 3D.
        unsafe { self.lines[axis as usize][p as usize].get_unchecked(i) }.as_ref()
    }

    /// Iterate the `line_id`s of every line that has ever been touched by
    /// `set` for `(axis, p)`. Yielded in insertion order (mirrors the
    /// prior `FxHashMap` semantics: keys persist after the line's bits
    /// are cleared). Iterates over a packed `SmallVec`, so the cost is
    /// `O(populated_lines)`, not `O(LINE_ID_RANGE)`.
    pub fn line_ids(&self, axis: Axis, p: Player) -> impl Iterator<Item = i16> + '_ {
        self.populated_ids[axis as usize][p as usize].iter().copied()
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
        let line = self.lines[axis as usize][p as usize][Self::idx(id)].as_ref()?;
        if !line.get(pos) {
            return None;
        }
        let back = i16::from(line.run_backward(pos));
        let fwd = i16::from(line.run_forward(pos));
        Some((pos - back, pos + fwd))
    }
}

/// Allocate a fresh `LINE_ID_RANGE`-long boxed slice of `None` slots.
fn empty_line_slots() -> Box<[Option<LineBitmap>]> {
    let mut v: Vec<Option<LineBitmap>> = Vec::with_capacity(LINE_ID_RANGE);
    v.resize_with(LINE_ID_RANGE, || None);
    v.into_boxed_slice()
}
