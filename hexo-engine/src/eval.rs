//! Static evaluation. X-positive globally.
//!
//! Three layers plus an advisory tempo term:
//!
//! 1. **Layer 1** — sliding 6-cell window scan over every populated axis
//!    line. Each window decoded into a ternary index (`0..=728`) keyed
//!    into the build-time `WINDOW_SCORE` table. An extension-factor
//!    check on the two cells immediately outside the window separates
//!    open / half-open / dead runs.
//! 2. **Layer 2** — weighted sum of [`ThreatCounts`] from
//!    [`Board::threats`]. Per-player, X-positive globally.
//! 3. **Layer 3** — minimum vertex cover of the S0
//!    defense-cells hypergraph. Cover ≥ 3 is forced mate.
//!
//! The tempo term contributes `tempo_weight * (X.open_3 - O.open_3)`.
//!
//! Mate-distance: terminal positions and Layer 3 mate sentinels return
//! `±(MATE_SCORE - ply)` so the search prefers shorter mates.

#![allow(clippy::must_use_candidate)]
// All `as i32` casts apply to `ply: u32` (bounded above by the legal
// stone count of HeXO) and `ThreatCounts` u8 fields (bounded by
// `MAX_S0_INSTANCES`). Pedantic clippy can't see those invariants.
#![allow(clippy::cast_possible_wrap)]

use crate::axis_bitmap::{Axis, AxisBitmaps, LineBitmap};
use crate::board::{Board, Player};
use crate::config::{
    ARCH_SCORE, BONE_SCORE, CLOSED_3_SCORE, CLOSED_4_SCORE, CLOSED_5_SCORE,
    CLOSED_EXTENSION_FACTOR, FORK_COVER2_BONUS, OPEN_2_SCORE, OPEN_3_SCORE, OPEN_4_SCORE,
    OPEN_5_SCORE, OPEN_EXTENSION_FACTOR, RHOMBUS_SCORE, TEMPO_WEIGHT, TRAPEZOID_SCORE,
    TRIANGLE_SCORE, WINDOW_SCORE,
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
    score += layer1_window_scan(board);
    score += layer2_shapes(&tx.counts) - layer2_shapes(&to.counts);
    score += fork_x - fork_o;
    score += tempo_score(&tx, &to);
    score
}

/// `true` iff Layer 3 reports a cover-≥-3 fork mate for `player`.
/// Cheap to call: reuses the same cached [`ThreatSet`] as [`eval`].
#[must_use]
pub fn is_mate_for(board: &Board, player: Player) -> bool {
    let threats = board.threats(player);
    layer3_fork_bonus(&threats) == i32::MAX
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

const POW3: [u16; 6] = [1, 3, 9, 27, 81, 243];

/// Sum of all 6-cell windows on every populated axis line. Each window
/// looked up in `WINDOW_SCORE` (build-time table) and multiplied by the
/// extension factor of the two cells immediately outside the window.
fn layer1_window_scan(board: &Board) -> i32 {
    let bitmaps = board.axes();
    let mut total: i32 = 0;

    for axis in Axis::all() {
        // Collect every line_id with stones for either player. Capacity
        // 32 fits a typical 60-piece HeXO position on the stack — only
        // pathological clusters force a heap spill.
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
            total += scan_line(bitmaps, axis, line_id);
        }
    }
    total
}

/// Sum the 6-cell window scores for a single `(axis, line_id)` line.
/// Walks the populated range `[min_pos - 5, max_pos]` exactly once.
fn scan_line(bitmaps: &AxisBitmaps, axis: Axis, line_id: i16) -> i32 {
    let xl = bitmaps.line(axis, Player::X, line_id);
    let ol = bitmaps.line(axis, Player::O, line_id);
    let xr = xl.and_then(LineBitmap::populated_range);
    let or_ = ol.and_then(LineBitmap::populated_range);
    let (min_pos, max_pos) = match (xr, or_) {
        (Some((xa, xb)), Some((oa, ob))) => (xa.min(oa), xb.max(ob)),
        (Some(r), None) | (None, Some(r)) => r,
        (None, None) => return 0,
    };

    let mut total: i32 = 0;
    let mut base_pos = min_pos - 5;
    while base_pos <= max_pos {
        let x_bits = xl.map_or(0, |l| l.window6(base_pos));
        let o_bits = ol.map_or(0, |l| l.window6(base_pos));
        let idx = encode_ternary(x_bits, o_bits);
        let base = WINDOW_SCORE[idx as usize];
        if base != 0 {
            let factor = extension_factor(bitmaps, axis, line_id, base_pos, base);
            total += base * factor;
        }
        base_pos += 1;
    }
    total
}

/// Pack a 6-cell window into the ternary index used by `WINDOW_SCORE`.
/// `0 = empty`, `1 = X`, `2 = O`. Mixed windows naturally map to entries
/// with both X and O contributions, which the codegen table fills with 0.
#[inline]
fn encode_ternary(x_bits: u8, o_bits: u8) -> u16 {
    let mut idx: u16 = 0;
    for (i, pow) in POW3.iter().enumerate() {
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

#[derive(Copy, Clone, PartialEq, Eq)]
enum ExtCell {
    Empty,
    Opp,
    Same,
}

/// Multiplier applied to a non-zero window score based on the two cells
/// immediately outside the 6-window.
///
/// `base_score > 0` ⇒ X-only window; `< 0` ⇒ O-only. Cases:
/// - both extension cells empty → `OPEN_EXTENSION_FACTOR`
/// - one empty, one opponent → `CLOSED_EXTENSION_FACTOR`
/// - both opponent → 0 (window is sealed off)
/// - either extension cell holds a same-color stone → 0
///   (a wider window already covers this k-run, avoid double counting)
#[inline]
fn extension_factor(
    bitmaps: &AxisBitmaps,
    axis: Axis,
    line_id: i16,
    base_pos: i16,
    base_score: i32,
) -> i32 {
    let (own, other) = if base_score > 0 {
        (Player::X, Player::O)
    } else {
        (Player::O, Player::X)
    };
    let left = classify(bitmaps, axis, line_id, base_pos - 1, own, other);
    let right = classify(bitmaps, axis, line_id, base_pos + 6, own, other);
    match (left, right) {
        (ExtCell::Empty, ExtCell::Empty) => OPEN_EXTENSION_FACTOR,
        (ExtCell::Empty, ExtCell::Opp) | (ExtCell::Opp, ExtCell::Empty) => {
            CLOSED_EXTENSION_FACTOR
        }
        (ExtCell::Opp, ExtCell::Opp)
        | (ExtCell::Same, _)
        | (_, ExtCell::Same) => 0,
    }
}

#[inline]
fn classify(
    bitmaps: &AxisBitmaps,
    axis: Axis,
    line_id: i16,
    pos: i16,
    own: Player,
    other: Player,
) -> ExtCell {
    if bitmaps.is_set(axis, line_id, pos, own) {
        ExtCell::Same
    } else if bitmaps.is_set(axis, line_id, pos, other) {
        ExtCell::Opp
    } else {
        ExtCell::Empty
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 2: shape weights
// ─────────────────────────────────────────────────────────────────────────────

/// Weighted sum of every shape count in `c`. Returned as a per-player
/// magnitude; the top-level eval subtracts the two players to get the
/// signed contribution.
#[inline]
fn layer2_shapes(c: &ThreatCounts) -> i32 {
    OPEN_5_SCORE * i32::from(c.open_5)
        + CLOSED_5_SCORE * i32::from(c.closed_5)
        + OPEN_4_SCORE * i32::from(c.open_4)
        + CLOSED_4_SCORE * i32::from(c.closed_4)
        + OPEN_3_SCORE * i32::from(c.open_3)
        + RHOMBUS_SCORE * i32::from(c.rhombus)
        + ARCH_SCORE * i32::from(c.arch)
        + BONE_SCORE * i32::from(c.bone)
        + TRAPEZOID_SCORE * i32::from(c.trapezoid)
        + OPEN_2_SCORE * i32::from(c.open_2)
        + CLOSED_3_SCORE * i32::from(c.closed_3)
        + TRIANGLE_SCORE * i32::from(c.triangle)
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

// ─────────────────────────────────────────────────────────────────────────────
// Tempo (advisory)
// ─────────────────────────────────────────────────────────────────────────────

/// Tempo term. v1 covers only the `open_3` delta; the `+0` / `-1` cases
/// from the spec are deferred until the shape detector exposes the
/// matching counts.
#[inline]
fn tempo_score(tx: &ThreatSet, to: &ThreatSet) -> i32 {
    TEMPO_WEIGHT * (i32::from(tx.counts.open_3) - i32::from(to.counts.open_3))
}
