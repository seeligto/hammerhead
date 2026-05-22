//! Static evaluation. X-positive globally.
//!
//! Three layers:
//!
//! 1. **Layer 1** — sliding 8-cell window scan over every populated axis
//!    line. Each window decoded into a ternary index (`0..=6560`) keyed
//!    into the build-time `WINDOW_SCORE_8` table, which has the
//!    extension factor (open / half-open / dead) folded in — so the
//!    scan is a single lookup, no boundary probes, no runtime multiply.
//! 2. **Layer 2** — weighted sum of the S0 [`ThreatCounts`] (open /
//!    closed 4 & 5) from [`Board::threats`]. Per-player, X-positive
//!    globally. (Phase 20 removed the S1/S2 shapes — see the history
//!    note in `SPEC_EVAL.md`.)
//! 3. **Layer 3** — minimum vertex cover of the S0
//!    defense-cells hypergraph. Cover ≥ 3 is forced mate.
//!
//! Mate-distance: terminal positions and Layer 3 mate sentinels return
//! `±(MATE_SCORE - ply)` so the search prefers shorter mates.

#![allow(clippy::must_use_candidate)]
// All `as i32` casts apply to `ply: u32` (bounded above by the legal
// stone count of HeXO) and `ThreatCounts` u8 fields (bounded by
// `MAX_S0_INSTANCES`). Pedantic clippy can't see those invariants.
// `as i16` casts in the Layer-1 chunked window scan apply to chunk
// indices bounded by the 64-byte stack buffer used by
// `LineBitmap::windows8_run`. `total_count` is computed from
// non-negative `(max_pos - start + 1)` clamped via `.max(0)` before
// the cast.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use crate::axis_bitmap::{Axis, AxisBitmaps, LineBitmap};
use crate::board::{Board, Player};
use crate::config::{
    CLOSED_4_SCORE, CLOSED_5_SCORE, FORK_COVER2_BONUS, OPEN_4_SCORE, OPEN_5_SCORE, WINDOW_SCORE_8,
};
use crate::coords::Coord;
use crate::threats::{ThreatCounts, ThreatInstance, ThreatSet};
use smallvec::SmallVec;

/// Mate score re-exported from config so callers don't need a second
/// `use` line.
pub const MATE_SCORE: i32 = crate::config::MATE_SCORE;

/// Static eval of `board`. Positive = X advantage.
///
/// Returns `±(MATE_SCORE - ply)` in terminal positions or when Layer 3
/// detects a cover-≥-3 fork mate for either player.
#[must_use]
pub fn eval(board: &Board) -> i32 {
    if let Some(winner) = board.winner() {
        return mate_score_for(winner, board.ply());
    }

    let tx = board.threats(Player::X);
    let to = board.threats(Player::O);

    // Layer 3 first: a mate sentinel intercepts before any arithmetic
    // so we never mix mate-class scores with positional sums.
    let fork_x = layer3_fork_bonus(&tx);
    let fork_o = layer3_fork_bonus(&to);
    if fork_x == i32::MAX && fork_o == i32::MAX {
        // Both sides have a forced mate; the side about to move wins.
        return mate_score_for(board.to_move(), board.ply());
    }
    if fork_x == i32::MAX {
        return MATE_SCORE - board.ply() as i32;
    }
    if fork_o == i32::MAX {
        return -(MATE_SCORE - board.ply() as i32);
    }

    let mut score = 0;
    score += layer1_window_scan_8cell(board);
    score += layer2_shapes(tx.counts) - layer2_shapes(to.counts);
    score += fork_x - fork_o;
    score
}

/// `true` iff Layer 3 reports a cover-≥-3 fork mate for `player`.
/// Cheap to call: reuses the same cached [`ThreatSet`] as [`eval`].
#[must_use]
pub fn is_mate_for(board: &Board, player: Player) -> bool {
    let threats = board.threats(player);
    layer3_fork_bonus(&threats) == i32::MAX
}

/// Bench-only: isolated Layer-1 window scan. Hidden from rustdoc; exposed
/// only so criterion micro-benches in a sibling crate can time each layer
/// without re-implementing it.
#[doc(hidden)]
#[must_use]
pub fn bench_layer1_window_scan(board: &Board) -> i32 {
    layer1_window_scan_8cell(board)
}

/// Bench-only: isolated Layer-2 weighted shape sum (X minus O).
#[doc(hidden)]
#[must_use]
pub fn bench_layer2_shapes(board: &Board) -> i32 {
    let tx = board.threats(Player::X);
    let to = board.threats(Player::O);
    layer2_shapes(tx.counts) - layer2_shapes(to.counts)
}

/// Bench-only: isolated Layer-3 fork bonus for `player`.
#[doc(hidden)]
#[must_use]
pub fn bench_layer3_fork_bonus(board: &Board, player: Player) -> i32 {
    let threats = board.threats(player);
    layer3_fork_bonus(&threats)
}

/// Signed mate score with mate-distance accounting.
#[inline]
fn mate_score_for(winner: Player, ply: u32) -> i32 {
    let mag = MATE_SCORE - ply as i32;
    match winner {
        Player::X => mag,
        Player::O => -mag,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 1: window scan
// ─────────────────────────────────────────────────────────────────────────────

const POW3_8: [u16; 8] = [1, 3, 9, 27, 81, 243, 729, 2187];

/// Sum of all 8-cell windows on every populated axis line. Each window
/// keys an 8-cell ternary index into the build-time `WINDOW_SCORE_8`
/// table, which already has the extension factor folded in — so the
/// scan is a single lookup with no boundary `is_set` probes and no
/// runtime multiply.
///
/// Phase-27: consults the per-`(axis, line_id)` `LineContrib` cache on
/// `Board`. On a hit the cached signed contribution is used directly; on
/// a miss `scan_line_8cell` recomputes and stores into the cache. Eval
/// scores per line are bounded well below `i32::MAX`, so the cache's
/// `i32::MIN` dirty sentinel can never collide with a legitimate
/// contribution (asserted in debug by `LineContrib::set`).
fn layer1_window_scan_8cell(board: &Board) -> i32 {
    let bitmaps = board.axes();
    // Single `borrow_mut` held for the whole scan: nothing else borrows
    // `line_contrib` during Layer-1, and a single borrow avoids the
    // per-line borrow-check bookkeeping a mixed read/write path would
    // incur. Drops at end of function.
    let mut cache = board.line_contrib().borrow_mut();
    let mut total: i32 = 0;

    for axis in Axis::all() {
        let mut line_ids: SmallVec<[i16; 32]> = SmallVec::new();
        for id in bitmaps.line_ids(axis, Player::X) {
            line_ids.push(id);
        }
        for id in bitmaps.line_ids(axis, Player::O) {
            if !line_ids.contains(&id) {
                line_ids.push(id);
            }
        }
        for &line_id in &line_ids {
            let v = if let Some(v) = cache.get(axis, line_id) {
                v
            } else {
                let v = scan_line_8cell(bitmaps, axis, line_id);
                cache.set(axis, line_id, v);
                v
            };
            total += v;
        }
    }
    total
}

/// Sum the 8-cell window scores for a single `(axis, line_id)` line.
/// The inner-6 window slides over `[min_pos - 5, max_pos]`; each lookup
/// keys the full 8-cell window `[p-1, p+6]` into `WINDOW_SCORE_8`.
#[inline]
fn scan_line_8cell(bitmaps: &AxisBitmaps, axis: Axis, line_id: i16) -> i32 {
    let xl = bitmaps.line(axis, Player::X, line_id);
    let ol = bitmaps.line(axis, Player::O, line_id);
    let xr = xl.and_then(LineBitmap::populated_range);
    let or_ = ol.and_then(LineBitmap::populated_range);
    let (min_pos, max_pos) = match (xr, or_) {
        (Some((xa, xb)), Some((oa, ob))) => (xa.min(oa), xb.max(ob)),
        (Some(r), None) | (None, Some(r)) => r,
        (None, None) => return 0,
    };

    let start = min_pos - 5;
    let total_count = (max_pos - start + 1).max(0) as usize;
    let mut x_buf = [0u8; 64];
    let mut o_buf = [0u8; 64];
    let mut idx_buf = [0u16; 64];
    let mut total: i32 = 0;
    let mut emitted = 0usize;
    while emitted < total_count {
        let chunk = (total_count - emitted).min(x_buf.len());
        // The 8-cell window for inner-start `p` begins at `p - 1`.
        let win_start = start + emitted as i16 - 1;
        if let Some(l) = xl {
            l.windows8_run(win_start, chunk, &mut x_buf[..chunk]);
        } else {
            x_buf[..chunk].fill(0);
        }
        if let Some(l) = ol {
            l.windows8_run(win_start, chunk, &mut o_buf[..chunk]);
        } else {
            o_buf[..chunk].fill(0);
        }
        encode_ternary_8_batch(&x_buf[..chunk], &o_buf[..chunk], &mut idx_buf[..chunk]);
        for &idx in &idx_buf[..chunk] {
            total += WINDOW_SCORE_8[idx as usize];
        }
        emitted += chunk;
    }
    total
}

/// Pack an 8-cell window into the ternary index used by `WINDOW_SCORE_8`.
/// `0 = empty`, `1 = X`, `2 = O`; `idx ∈ [0, 6561)`.
#[inline]
fn encode_ternary_8(x_bits: u8, o_bits: u8) -> u16 {
    let mut idx: u16 = 0;
    for (i, pow) in POW3_8.iter().enumerate() {
        let x = (x_bits >> i) & 1;
        let o = (o_bits >> i) & 1;
        let cell = if x != 0 {
            1u16
        } else if o != 0 {
            2u16
        } else {
            0
        };
        idx += cell * pow;
    }
    idx
}

/// Batch encode `count` 8-cell (`x_bits`, `o_bits`) pairs into ternary
/// indices. Routes to the AVX2 implementation when `simd_eval` is on
/// and the host advertises AVX2; otherwise the scalar reference path.
#[inline]
fn encode_ternary_8_batch(x_bits: &[u8], o_bits: &[u8], out: &mut [u16]) {
    debug_assert_eq!(x_bits.len(), o_bits.len());
    debug_assert_eq!(x_bits.len(), out.len());
    #[cfg(all(target_arch = "x86_64", feature = "simd_eval"))]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: AVX2 verified at runtime above.
            unsafe { encode_ternary_8_batch_avx2(x_bits, o_bits, out) };
            return;
        }
    }
    encode_ternary_8_batch_scalar(x_bits, o_bits, out);
}

#[inline]
fn encode_ternary_8_batch_scalar(x_bits: &[u8], o_bits: &[u8], out: &mut [u16]) {
    for ((x, o), slot) in x_bits.iter().zip(o_bits.iter()).zip(out.iter_mut()) {
        *slot = encode_ternary_8(*x, *o);
    }
}

/// AVX2 8-cell ternary encode: 16 windows per loop body in a single
/// 256-bit register (16 u16 lanes). Eight `step!` digits (bits 0..=7),
/// powers `1, 3, 9, 27, 81, 243, 729, 2187`. The largest index is
/// `2 * 3280 = 6560`, and the largest partial term `2 * 2187 = 4374` —
/// both well inside u16, so the `_mm256_mullo_epi16` low-16 products
/// and the running sum never wrap.
///
/// Correctness gate: `simd_8cell_matches_scalar_all_6561` drives every
/// legal `(x_bits, o_bits)` pair through this path and the scalar
/// reference, asserting byte-identical output.
// `_mm_loadu_si128` / `_mm256_storeu_si256` are unaligned-safe; the
// AVX2 lane ops are all in-register. Pedantic clippy can't track the
// AVX2-target_feature contract, so silence the pointer lints locally.
#[cfg(all(target_arch = "x86_64", feature = "simd_eval"))]
#[target_feature(enable = "avx2")]
#[allow(clippy::cast_ptr_alignment, clippy::ptr_as_ptr)]
unsafe fn encode_ternary_8_batch_avx2(x_bits: &[u8], o_bits: &[u8], out: &mut [u16]) {
    use std::arch::x86_64::{
        __m128i, __m256i, _mm_loadu_si128, _mm256_add_epi16, _mm256_and_si256,
        _mm256_cvtepu8_epi16, _mm256_mullo_epi16, _mm256_or_si256, _mm256_set1_epi16,
        _mm256_setzero_si256, _mm256_slli_epi16, _mm256_srli_epi16, _mm256_storeu_si256,
    };

    // SAFETY for the whole function body:
    // - AVX2 is enabled by `target_feature(enable = "avx2")` so every
    //   `_mm256_*` / `_mm_*` intrinsic is legal.
    // - `*_loadu_*` / `*_storeu_*` are documented as alignment-safe.
    // - `ptr::add` stays within `x_bits` / `o_bits` / `out` because the
    //   surrounding `while` keeps `i + 16 <= n` and `n` is each slice's
    //   length (debug_assert checks the equal-length invariant in the
    //   dispatcher).
    unsafe {
        macro_rules! step {
            ($x:ident, $o:ident, $acc:ident, $one:ident, $shift:literal, $pow:literal) => {{
                let xi = _mm256_and_si256(_mm256_srli_epi16::<$shift>($x), $one);
                let oi = _mm256_and_si256(_mm256_srli_epi16::<$shift>($o), $one);
                let cell = _mm256_or_si256(xi, _mm256_slli_epi16::<1>(oi));
                let pow_vec = _mm256_set1_epi16($pow);
                let term = _mm256_mullo_epi16(cell, pow_vec);
                $acc = _mm256_add_epi16($acc, term);
            }};
        }

        let n = x_bits.len();
        let mut i = 0;
        while i + 16 <= n {
            let x_u8 = _mm_loadu_si128(x_bits.as_ptr().add(i).cast::<__m128i>());
            let o_u8 = _mm_loadu_si128(o_bits.as_ptr().add(i).cast::<__m128i>());
            let x = _mm256_cvtepu8_epi16(x_u8);
            let o = _mm256_cvtepu8_epi16(o_u8);
            let one = _mm256_set1_epi16(1);
            let mut acc = _mm256_setzero_si256();
            step!(x, o, acc, one, 0, 1);
            step!(x, o, acc, one, 1, 3);
            step!(x, o, acc, one, 2, 9);
            step!(x, o, acc, one, 3, 27);
            step!(x, o, acc, one, 4, 81);
            step!(x, o, acc, one, 5, 243);
            step!(x, o, acc, one, 6, 729);
            step!(x, o, acc, one, 7, 2187);
            _mm256_storeu_si256(out.as_mut_ptr().add(i).cast::<__m256i>(), acc);
            i += 16;
        }
        if i < n {
            encode_ternary_8_batch_scalar(&x_bits[i..], &o_bits[i..], &mut out[i..]);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 2: shape weights
// ─────────────────────────────────────────────────────────────────────────────

/// Weighted sum of the S0 shape counts in `c` (open / closed 4 & 5).
/// Returned as a per-player magnitude; the top-level eval subtracts
/// the two players to get the signed contribution.
#[inline]
fn layer2_shapes(c: ThreatCounts) -> i32 {
    OPEN_5_SCORE * i32::from(c.open_5)
        + CLOSED_5_SCORE * i32::from(c.closed_5)
        + OPEN_4_SCORE * i32::from(c.open_4)
        + CLOSED_4_SCORE * i32::from(c.closed_4)
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 3: fork detection (minimum vertex cover)
// ─────────────────────────────────────────────────────────────────────────────

/// Fork bonus per player. Returns one of:
/// - `0` — 0 or 1 S0 instances, or cover-1 multi-instance set
/// - `FORK_COVER2_BONUS` — cover-2 multi-instance set
/// - `i32::MAX` — cover ≥ 3 (defender's two stones cannot stop every
///   threat → forced mate). Caller is responsible for converting this
///   sentinel into a mate-distance score.
#[inline]
fn layer3_fork_bonus(threats: &ThreatSet) -> i32 {
    let insts = &threats.s0_instances;
    if insts.len() < 2 {
        return 0;
    }
    match min_vertex_cover_size(insts) {
        1 => 0,
        2 => FORK_COVER2_BONUS,
        _ => i32::MAX,
    }
}

/// Minimum vertex cover size for the defense-cells hypergraph. Returns
/// `1`, `2`, or `3` — `3` is the saturated answer meaning "≥ 3" and
/// implies forced mate. Empty / single-instance inputs are filtered by
/// the caller; calling with `< 2` instances returns `1`.
#[cold]
fn min_vertex_cover_size(insts: &[ThreatInstance]) -> u8 {
    if insts.len() < 2 {
        return 1;
    }
    if single_cell_covers_all(insts) {
        return 1;
    }
    // n == 2 and no shared cell → cover exactly 2 (pick one cell from each).
    if insts.len() == 2 {
        return 2;
    }

    // n ≥ 3: try every 2-cell subset drawn from the union.
    let mut union: SmallVec<[Coord; 16]> = SmallVec::new();
    for inst in insts {
        for &c in &inst.defense_cells {
            if !union.contains(&c) {
                union.push(c);
            }
        }
    }

    for i in 0..union.len() {
        for j in (i + 1)..union.len() {
            let a = union[i];
            let b = union[j];
            if pair_covers_all(insts, a, b) {
                return 2;
            }
        }
    }
    3
}

#[inline]
fn single_cell_covers_all(insts: &[ThreatInstance]) -> bool {
    let Some(first) = insts.first() else {
        return true;
    };
    'outer: for candidate in &first.defense_cells {
        for inst in &insts[1..] {
            if !inst.defense_cells.contains(candidate) {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

#[inline]
fn pair_covers_all(insts: &[ThreatInstance], a: Coord, b: Coord) -> bool {
    for inst in insts {
        let cells = &inst.defense_cells;
        if !cells.contains(&a) && !cells.contains(&b) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{encode_ternary_8, encode_ternary_8_batch, encode_ternary_8_batch_scalar};

    /// Decode an 8-cell ternary index `0..6561` into the matching
    /// `(x_bits, o_bits)` pair. Each cell holds at most one stone, so
    /// the two bit-masks are disjoint by construction.
    fn decode_8cell_ternary(mut idx: u16) -> (u8, u8) {
        let (mut x_bits, mut o_bits) = (0u8, 0u8);
        for i in 0..8 {
            match idx % 3 {
                1 => x_bits |= 1 << i,
                2 => o_bits |= 1 << i,
                _ => {}
            }
            idx /= 3;
        }
        (x_bits, o_bits)
    }

    /// Phase 17 STEP 5 byte-identity gate: every legal 8-cell window
    /// encodes to the same ternary index via the scalar reference
    /// `encode_ternary_8`, the scalar batch wrapper, and the dispatch
    /// `encode_ternary_8_batch` (which routes through the AVX2 path
    /// when the host advertises it). All 6561 indices; the batch run
    /// of 6561 exercises 410 full AVX2 iterations plus the scalar tail.
    #[test]
    fn simd_8cell_matches_scalar_all_6561() {
        let mut x_in = vec![0u8; 6561];
        let mut o_in = vec![0u8; 6561];
        let mut expected = vec![0u16; 6561];
        for idx in 0..6561u16 {
            let (x, o) = decode_8cell_ternary(idx);
            x_in[idx as usize] = x;
            o_in[idx as usize] = o;
            let scalar = encode_ternary_8(x, o);
            assert_eq!(scalar, idx, "8-cell scalar round-trip broke at {idx}");
            expected[idx as usize] = scalar;
        }
        let mut got = vec![0u16; 6561];
        encode_ternary_8_batch(&x_in, &o_in, &mut got);
        assert_eq!(got, expected, "encode_ternary_8_batch diverges from scalar");

        let mut got_scalar = vec![0u16; 6561];
        encode_ternary_8_batch_scalar(&x_in, &o_in, &mut got_scalar);
        assert_eq!(
            got_scalar, expected,
            "encode_ternary_8_batch_scalar diverges from scalar"
        );
    }
}
