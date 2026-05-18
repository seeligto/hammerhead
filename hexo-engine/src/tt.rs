//! Transposition table. Two-bucket, generation-aged, u128-verified.
//!
//! Indexed by low N bits of position hash. Each index slot holds a
//! depth-preferred bucket and an always-replace bucket. On probe, the
//! full `u128` is compared so index collisions cannot cross-contaminate.

// `hash as u64 as usize` deliberately truncates a 128-bit Zobrist key to
// the index domain. The full hash is still stored in each `TTEntry` so
// collisions are caught on probe. The `cast_possible_truncation` lint
// flags this pair of casts despite the truncation being load-bearing.
#![allow(clippy::cast_possible_truncation)]

use crate::coords::{Coord, ORIGIN};

/// Score bound stored alongside a TT entry.
///
/// `Empty` is the sentinel for unused slots — kept inside the variant rather
/// than as a separate `is_present` flag so a single `match` covers every
/// state.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum TTFlag {
    /// Slot is unused. Probes must reject.
    Empty,
    /// Exact minimax score.
    Exact,
    /// `score` is a lower bound (alpha cutoff: fail-high).
    LowerBound,
    /// `score` is an upper bound (beta cutoff: fail-low).
    UpperBound,
}

/// A single transposition-table entry.
///
/// `depth == -1` and `flag == Empty` mark an unused slot. The depth field is
/// `i8` so leaf entries (`depth == 0`) and quiescence sentinels (negative
/// depth) round-trip without widening.
#[derive(Copy, Clone, Debug)]
pub struct TTEntry {
    /// Full 128-bit position hash. Bucket index is `(hash as u64) & mask`;
    /// the full value is stored so probes can verify against collisions.
    pub hash: u128,
    /// Best move from this position, or [`ORIGIN`] when none is recorded.
    pub best_move: Coord,
    /// Stored minimax score.
    pub score: i32,
    /// Search depth that produced `score`. `-1` for empty slots.
    pub depth: i8,
    /// Bound classification of `score`.
    pub flag: TTFlag,
    /// Generation tag — used by [`TranspositionTable::store`] to decide
    /// whether the depth-preferred slot is stale and may be overwritten
    /// regardless of depth.
    pub generation: u8,
}

impl TTEntry {
    /// Sentinel value for an unused slot.
    pub const EMPTY: TTEntry = TTEntry {
        hash: 0,
        best_move: ORIGIN,
        score: 0,
        depth: -1,
        flag: TTFlag::Empty,
        generation: 0,
    };

    /// `true` iff this slot has never been written (or has been cleared).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self.flag, TTFlag::Empty)
    }
}

/// Lightweight occupancy/diagnostic snapshot. Computed on demand by
/// [`TranspositionTable::stats`] — not maintained incrementally.
#[derive(Default, Clone, Copy, Debug)]
pub struct TTStats {
    /// Power-of-two bucket count (each bucket holds two entries).
    pub n_slots: usize,
    /// Number of buckets with at least one non-empty entry.
    pub occupied: usize,
    /// Current generation tag.
    pub generation: u8,
}

/// Two-bucket transposition table.
///
/// Each index slot stores `(depth_preferred, always_replace)`:
/// - `depth_preferred` is reserved for the deepest result we've seen.
/// - `always_replace` catches everything else so shallower searches still
///   benefit from their own work.
///
/// `mask` is `n_slots - 1` with `n_slots` rounded down to a power of two;
/// indexing is therefore a single `AND` on the low 64 bits of the hash.
pub struct TranspositionTable {
    buckets: Box<[(TTEntry, TTEntry)]>,
    mask: usize,
    generation: u8,
}

impl TranspositionTable {
    /// Allocate a TT sized to roughly `size_mb` megabytes.
    ///
    /// The slot count is rounded down to a power of two so the lookup mask
    /// is a single `AND`. A non-positive request still produces a 1-slot
    /// table — search code can probe/store unconditionally.
    #[must_use]
    pub fn new(size_mb: usize) -> Self {
        let slot_bytes = std::mem::size_of::<(TTEntry, TTEntry)>();
        let total_bytes = size_mb.saturating_mul(1024 * 1024);
        let raw_slots = (total_bytes / slot_bytes).max(1);
        let n_slots = floor_pow2(raw_slots);
        let mask = n_slots - 1;
        let buckets = vec![(TTEntry::EMPTY, TTEntry::EMPTY); n_slots].into_boxed_slice();
        Self {
            buckets,
            mask,
            generation: 0,
        }
    }

    /// Probe `hash`. Returns the depth-preferred entry when both buckets
    /// match (the deeper or older-but-protected result wins); otherwise the
    /// matching always-replace entry; otherwise `None`.
    #[inline]
    #[must_use]
    pub fn probe(&self, hash: u128) -> Option<&TTEntry> {
        let idx = (hash as u64 as usize) & self.mask;
        let (a, b) = &self.buckets[idx];
        if !a.is_empty() && a.hash == hash {
            return Some(a);
        }
        if !b.is_empty() && b.hash == hash {
            return Some(b);
        }
        None
    }

    /// Store an entry. Two-bucket replacement policy:
    ///
    /// - The depth-preferred slot is overwritten when its prior occupant is
    ///   empty, stale (different generation), or no deeper than the new
    ///   entry. The displaced entry, if it would dominate the existing
    ///   always-replace occupant on depth, migrates into the always-replace
    ///   slot rather than being discarded.
    /// - Otherwise the new entry lands in the always-replace slot.
    #[inline]
    pub fn store(&mut self, hash: u128, depth: i8, score: i32, flag: TTFlag, best_move: Coord) {
        let idx = (hash as u64 as usize) & self.mask;
        let (a, b) = &mut self.buckets[idx];
        let new = TTEntry {
            hash,
            best_move,
            score,
            depth,
            flag,
            generation: self.generation,
        };
        let aged = !a.is_empty() && a.generation != self.generation;
        if a.is_empty() || aged || depth >= a.depth {
            if !a.is_empty() && a.depth > b.depth {
                *b = *a;
            }
            *a = new;
        } else {
            *b = new;
        }
    }

    /// Bump the generation tag. Subsequent `store` calls treat entries
    /// from any earlier generation as eligible for depth-preferred
    /// replacement.
    #[inline]
    pub fn new_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Wipe every slot and reset the generation tag.
    #[cold]
    pub fn clear(&mut self) {
        for slot in &mut self.buckets {
            *slot = (TTEntry::EMPTY, TTEntry::EMPTY);
        }
        self.generation = 0;
    }

    /// Occupancy snapshot. Iterates the bucket array — acceptable outside
    /// hot paths but should not be called inside the search loop.
    #[must_use]
    pub fn stats(&self) -> TTStats {
        let mut occupied = 0usize;
        for (a, b) in &*self.buckets {
            if !a.is_empty() || !b.is_empty() {
                occupied += 1;
            }
        }
        TTStats {
            n_slots: self.buckets.len(),
            occupied,
            generation: self.generation,
        }
    }

    /// Bucket count (`mask + 1`). Exposed for tests and diagnostics.
    #[inline]
    #[must_use]
    pub fn slot_count(&self) -> usize {
        self.buckets.len()
    }

    /// Index mask. Bucket index = `(hash as u64 as usize) & mask`.
    #[inline]
    #[must_use]
    pub fn mask(&self) -> usize {
        self.mask
    }
}

/// Largest power of two ≤ `n`. `floor_pow2(0)` returns 1 so the caller
/// never ends up with a zero-sized allocation.
#[inline]
#[must_use]
fn floor_pow2(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    1usize << (usize::BITS - 1 - n.leading_zeros())
}
