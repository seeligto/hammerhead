//! 128-bit Zobrist hashing.
//!
//! Strategy: preallocated window for `q, r ∈ [-W, W]` (default `W = 127`,
//! ~2 MB), plus a lazy `FxHashMap` fallback for coords outside the window.
//! Two independent PRNG streams (window seed vs. lazy seed) so the two
//! domains never collide on the same `u128`.

// All `as usize` casts in this module compute indices into a bounded
// 2W+1 square (default 255 × 255) after validating coord ∈ [-W, W].
// Sign loss / truncation cannot happen for in-window coords.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]

use crate::board::Player;
use crate::config::ZOBRIST_WINDOW;
use crate::coords::Coord;
use fxhash::FxHashMap;
use rand_xoshiro::Xoshiro256PlusPlus;
use rand_xoshiro::rand_core::{Rng, SeedableRng};

/// Fixed seed for the in-window table. Determinism = reproducible hashes
/// across runs, which is helpful for debugging and TT tests.
const WINDOW_SEED: u64 = 0x5EED_DEAD_BEEF_0BAD;

/// Distinct seed for the lazy out-of-window stream so it never coincides with
/// the windowed stream on the same `u128` values.
const LAZY_SEED: u64 = 0x0C0F_FEEB_ADF0_0D42;

/// XOR'd into [`crate::board::Board::hash`] whenever it is X's turn at the
/// start of a turn (halfmove == 0). Toggles each full turn.
///
/// Stored as an independent `pub const` literal so adding parity bits does
/// not consume any draws from `WINDOW_SEED` or `LAZY_SEED` — existing
/// per-cell keys stay byte-identical to pre-Phase-6 builds.
pub const Z_TURN_X: u128 = 0xA0B1_C2D3_E4F5_0617_2839_4A5B_6C7D_8E9F;

/// XOR'd into [`crate::board::Board::hash`] whenever the current stone is
/// the second of a 2-stone turn (halfmove == 1).
pub const Z_HALFMOVE: u128 = 0x1F2E_3D4C_5B6A_7988_8776_6554_4332_2110;

const W: i16 = ZOBRIST_WINDOW;
const SIDE: usize = (2 * W as usize) + 1;
const WINDOW_LEN: usize = SIDE * SIDE * 2;

/// 128-bit Zobrist key table. Constructed once per `Board`.
pub struct ZobristTable {
    window: Box<[u128]>,
    lazy: FxHashMap<(Coord, Player), u128>,
    lazy_rng: Xoshiro256PlusPlus,
}

// Deliberate: ZobristTable owns a non-trivial allocation, so we expose only
// `new()`. A `Default` impl would invite accidental reconstruction. The
// owning `Board` reuses one table across its lifetime.
#[allow(clippy::new_without_default)]
impl ZobristTable {
    /// Allocate and seed the table.
    #[must_use]
    pub fn new() -> Self {
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(WINDOW_SEED);
        let mut window = vec![0u128; WINDOW_LEN].into_boxed_slice();
        for slot in &mut window {
            *slot = next_u128(&mut rng);
        }
        Self {
            window,
            lazy: FxHashMap::default(),
            lazy_rng: Xoshiro256PlusPlus::seed_from_u64(LAZY_SEED),
        }
    }

    /// Hash key for `(c, p)`. `O(1)` for in-window coords (array load);
    /// lazy insert (amortised `O(1)`) for far cells.
    #[inline]
    pub fn key(&mut self, c: Coord, p: Player) -> u128 {
        if in_window(c) {
            self.window[index(c, p)]
        } else {
            self.lazy_key(c, p)
        }
    }

    #[cold]
    fn lazy_key(&mut self, c: Coord, p: Player) -> u128 {
        if let Some(&k) = self.lazy.get(&(c, p)) {
            return k;
        }
        let k = next_u128(&mut self.lazy_rng);
        self.lazy.insert((c, p), k);
        k
    }
}

#[inline]
fn in_window(c: Coord) -> bool {
    c.q >= -W && c.q <= W && c.r >= -W && c.r <= W
}

#[inline]
fn index(c: Coord, p: Player) -> usize {
    let q = (c.q + W) as usize;
    let r = (c.r + W) as usize;
    (q * SIDE + r) * 2 + p as usize
}

#[inline]
fn next_u128(rng: &mut Xoshiro256PlusPlus) -> u128 {
    let lo = u128::from(rng.next_u64());
    let hi = u128::from(rng.next_u64());
    (hi << 64) | lo
}
