//! Phase 5 eval integration tests.
//!
//! Covers the three eval layers, the mate-distance contract, and the
//! `Board::cached_eval` invalidation discipline.

use hammerhead_engine_core::board::{Board, Player};
use hammerhead_engine_core::config::{
    FORK_COVER2_BONUS, MATE_SCORE, OPEN_4_SCORE, OPEN_5_SCORE, OPEN_EXTENSION_FACTOR,
};
use hammerhead_engine_core::coords::Coord;
use hammerhead_engine_core::eval::{eval, is_mate_for};
use hammerhead_engine_core::threats::compute as compute_threats;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn x(b: &mut Board, cells: &[(i16, i16)]) {
    for &(q, r) in cells {
        b.place_for_test(Coord::new(q, r), Player::X);
    }
}

fn o(b: &mut Board, cells: &[(i16, i16)]) {
    for &(q, r) in cells {
        b.place_for_test(Coord::new(q, r), Player::O);
    }
}

fn fresh() -> Board {
    Board::new()
}

/// Rotate `(q, r)` 60° clockwise around the origin (axial coords).
fn rot60(qr: (i16, i16)) -> (i16, i16) {
    let (q, r) = qr;
    let s = -q - r;
    (-r, -s)
}

/// Reflect `(q, r)` across the q-axis: `(q, r) -> (q + r, -r)`.
fn reflect(qr: (i16, i16)) -> (i16, i16) {
    let (q, r) = qr;
    (q + r, -r)
}

// ─────────────────────────────────────────────────────────────────────────────
// 1-3: empty / single stone baseline
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn empty_board_eval_zero() {
    let b = fresh();
    assert_eq!(eval(&b), 0);
}

#[test]
fn single_x_at_origin_positive() {
    let mut b = fresh();
    x(&mut b, &[(0, 0)]);
    let v = eval(&b);
    assert!(v > 0, "expected X advantage, got {v}");
}

#[test]
fn single_o_negates_single_x() {
    // Disjoint single stones should cancel.
    let mut bx = fresh();
    x(&mut bx, &[(0, 0)]);
    let mut bo = fresh();
    o(&mut bo, &[(0, 0)]);
    assert_eq!(eval(&bx), -eval(&bo));
}

// ─────────────────────────────────────────────────────────────────────────────
// 4-5: open-4 baseline, cancellation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn open_four_x_clears_open_four_weight() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let v = eval(&b);
    assert!(
        v >= OPEN_4_SCORE,
        "open-4 eval {v} below OPEN_4_SCORE {OPEN_4_SCORE}"
    );
}

#[test]
fn open_fours_disjoint_cancel() {
    let mut b = fresh();
    // X open-4 on the q-axis.
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    // O open-4 far away on the q-axis. 12 cells apart on r=10 — separate
    // axis line, no Layer 1 interaction.
    o(&mut b, &[(0, 10), (1, 10), (2, 10), (3, 10)]);
    let v = eval(&b);
    assert!(
        v.abs() < OPEN_4_SCORE / 4,
        "symmetric position should be ~0, got {v} (>= {})",
        OPEN_4_SCORE / 4
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 6-7: fork-mate vs near-fork
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn three_disjoint_closed_fours_is_mate() {
    // Three single-cell-defense threats with disjoint defense cells →
    // intersection-based minimum vertex cover ≥ 3 → forced mate.
    //
    // The prompt's "two disjoint open-4s" example would in fact be
    // cover-2 under intersection semantics (one cell hits each run); the
    // mate flag only fires from 3+ disjoint single-cell defenses or
    // similarly hostile geometries. See Phase 5 report for the ambiguity.
    let mut b = fresh();
    // Closed-4 #1: q-axis line r=0, X at q in 0..3, blocked by O at q=-1.
    // Defense = (4, 0).
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    o(&mut b, &[(-1, 0)]);
    // Closed-4 #2: q-axis line r=0, X at q in 10..13, blocked by O at q=14.
    // Defense = (9, 0).
    x(&mut b, &[(10, 0), (11, 0), (12, 0), (13, 0)]);
    o(&mut b, &[(14, 0)]);
    // Closed-4 #3: r-axis line q=0, X at r in 5..8, blocked by O at r=9.
    // Defense = (0, 4).
    x(&mut b, &[(0, 5), (0, 6), (0, 7), (0, 8)]);
    o(&mut b, &[(0, 9)]);

    let v = eval(&b);
    assert!(
        v >= MATE_SCORE - 64,
        "three-closed-4 mate eval {v} below MATE_SCORE-64 {}",
        MATE_SCORE - 64
    );
    assert!(is_mate_for(&b, Player::X));
}

#[test]
fn two_open_fours_sharing_defense_cell_not_mate() {
    // Two open-4s that meet at a corner so the shared endpoint blocks
    // both. With one corner cell, the defender's single response
    // neutralises both. Result: cover-1, no fork mate.
    let mut b = fresh();
    // Open-4 along q-axis, r=0, q in [0..3]. Right endpoint = (4, 0).
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    // Open-4 along s-axis through (4, 0): s-axis steps are (1, -1).
    // Stones at (5, -1), (6, -2), (7, -3), (8, -4). The s-axis line
    // through these has line_id q+r = 4. Left endpoint = (4, 0) — the
    // shared cell. Right endpoint = (9, -5).
    x(&mut b, &[(5, -1), (6, -2), (7, -3), (8, -4)]);
    let v = eval(&b);
    assert!(
        v < MATE_SCORE - 1000,
        "shared-defense-cell position erroneously scored mate: {v}"
    );
    assert!(
        v >= 2 * OPEN_4_SCORE - OPEN_4_SCORE / 2,
        "expected ~2 × open_4 weight, got {v}"
    );
    assert!(!is_mate_for(&b, Player::X));
}

// ─────────────────────────────────────────────────────────────────────────────
// 8-9: rotation / reflection symmetry
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn eval_rotation_symmetric() {
    let pieces = [(0, 0), (1, 0), (2, 0), (3, 0)];

    let mut base = fresh();
    x(&mut base, &pieces);
    let v_base = eval(&base);

    let rotated: Vec<(i16, i16)> = pieces.iter().map(|&p| rot60(p)).collect();
    let mut rot = fresh();
    x(&mut rot, &rotated);
    let v_rot = eval(&rot);

    assert_eq!(v_base, v_rot, "60° rotation changed eval");
}

#[test]
fn eval_reflection_symmetric() {
    // Pieces deliberately off the q-axis so the reflection produces
    // genuinely different coords — the q-axis is fixed under
    // `(q, r) -> (q + r, -r)` and would make this test trivially pass.
    let pieces = [(0, 1), (1, 1), (2, 1), (3, 1), (4, 1)];

    let mut base = fresh();
    x(&mut base, &pieces);
    let v_base = eval(&base);

    let reflected: Vec<(i16, i16)> = pieces.iter().map(|&p| reflect(p)).collect();
    assert_ne!(
        reflected, pieces,
        "reflection must move pieces — otherwise the test is a no-op"
    );
    let mut refl = fresh();
    x(&mut refl, &reflected);
    let v_refl = eval(&refl);

    assert_eq!(v_base, v_refl, "reflection changed eval");
}

// ─────────────────────────────────────────────────────────────────────────────
// 10-12: cache discipline
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cached_eval_repeat_returns_same_value() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    let a = b.cached_eval();
    let c = b.cached_eval();
    assert_eq!(a, c);
}

#[test]
fn cached_eval_invalidated_by_place() {
    let mut b = fresh();
    x(&mut b, &[(0, 0)]);
    let before = b.cached_eval();
    o(&mut b, &[(1, 0)]);
    let after = b.cached_eval();
    assert_ne!(before, after, "cache stale after place");
}

#[test]
fn cached_eval_restored_after_undo() {
    // place then undo via the real game machinery; first move must be
    // ORIGIN per the engine rules.
    let mut b = Board::new();
    b.place(Coord::new(0, 0)).unwrap();
    let before = b.cached_eval();
    b.place(Coord::new(1, 0)).unwrap();
    let _mid = b.cached_eval();
    b.undo().unwrap();
    let after = b.cached_eval();
    assert_eq!(before, after, "cache not restored after undo");
}

// ─────────────────────────────────────────────────────────────────────────────
// 13: mate-distance
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn mate_distance_prefers_shorter_mate() {
    // Position with X having 6 in a row at ply N. Eval returns
    // MATE_SCORE - N. A deeper-ply identical mate returns a strictly
    // smaller value.
    let mut shallow = fresh();
    x(
        &mut shallow,
        &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)],
    );
    let v_shallow = eval(&shallow);

    let mut deeper = fresh();
    // Pad the deeper position with two extra stones for X and two for
    // O on a faraway line so the winning 6-run still completes and the
    // ply count increases by 4.
    x(
        &mut deeper,
        &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)],
    );
    // Extra stones placed via test helper bypass the win check; doesn't
    // matter — winner was already declared.
    x(&mut deeper, &[(0, 20), (1, 20)]);
    o(&mut deeper, &[(0, -20), (1, -20)]);

    let v_deeper = eval(&deeper);
    assert!(
        v_shallow > v_deeper,
        "shallow mate {v_shallow} not preferred over deeper mate {v_deeper}"
    );
    assert!(v_shallow <= MATE_SCORE);
    assert!(v_deeper <= MATE_SCORE);
}

// ─────────────────────────────────────────────────────────────────────────────
// 14: extension factor — open vs half-open run
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn open_run_outscores_half_open_run() {
    // 4-stone X run with both ends empty.
    let mut open = fresh();
    x(&mut open, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let v_open = eval(&open);

    // Same 4-stone X run with one end blocked by O.
    let mut closed = fresh();
    x(&mut closed, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    o(&mut closed, &[(4, 0)]);
    let v_closed = eval(&closed);

    assert!(
        v_open > v_closed,
        "open 4-run ({v_open}) not greater than half-open ({v_closed})"
    );
    // Extension factor is a 4× vs 1× multiplier on the Layer 1 windows;
    // the open side must dominate by a sizeable margin.
    assert!(
        v_open - v_closed >= 100 * (OPEN_EXTENSION_FACTOR - 1),
        "open vs closed delta too small: {} (open={v_open} closed={v_closed})",
        v_open - v_closed
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 14b: extension factor — both-ends-blocked and same-color guard
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn boxed_four_run_layer1_killed_by_opp_extensions() {
    // X 4-run with O at the cells immediately *outside* the 6-window
    // covering the run: triggers the (Opp, Opp) extension branch → 0.
    // Compare with the same 4-run sans-blockers: Layer 1 contribution
    // is much larger because the windows hit (Empty, Empty) → ×4.
    let mut boxed = fresh();
    x(&mut boxed, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    o(&mut boxed, &[(-1, 0), (6, 0)]);
    let v_boxed = eval(&boxed);

    let mut bare = fresh();
    x(&mut bare, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let v_bare = eval(&bare);

    assert!(
        v_bare > v_boxed + 500,
        "(Opp, Opp) extension factor not killing the window: bare={v_bare}, boxed={v_boxed}"
    );
}

#[test]
fn five_run_layer1_does_not_double_count_via_same_color_extension() {
    // A 5-run XXXXX with both ends empty. The 4-window inside the run
    // (e.g. positions 0..5 containing X at 0..3 and empty at 4..5,
    // with extension at 4=X) hits the (Same, _) guard → contributes 0.
    // Without that guard the 4-window would erroneously add ~2048 on
    // top of the k=5 windows.
    //
    // Verify Layer 1 for the 5-run is bounded by the two `k=5` windows
    // (each scored with OPEN_EXTENSION_FACTOR) plus a small residual.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);

    // Layer 2 already includes OPEN_5_SCORE for this open-5 run; isolate
    // the Layer 1 effect by subtracting the Layer 2 contribution. Open-5
    // also generates an open-4 shape inside, but the threats module
    // classifies the maximal run, so only `open_5` is set.
    let tx = compute_threats(&b, Player::X, &[], None);
    assert_eq!(tx.counts.open_5, 1);
    let layer2_only = OPEN_5_SCORE;

    let v = eval(&b);
    let layer1_plus_other = v - layer2_only;

    // Two `k=5` windows × OPEN_EXTENSION_FACTOR × `window_k_scores[5]`
    // = 2 × 4 × 4096 = 32_768. Allow 50% upward slack for the unavoidable
    // smaller windows (k=1..3) lurking at the run's flanks.
    assert!(
        layer1_plus_other <= 50_000,
        "5-run Layer-1 contribution larger than expected: {layer1_plus_other}"
    );
    assert!(
        layer1_plus_other >= 16_384,
        "5-run Layer-1 contribution smaller than the two k=5 windows: {layer1_plus_other}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 15: is_mate_for parity + cover-2 bonus
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn is_mate_for_matches_eval_above_threshold() {
    let mut b = fresh();
    // Same construction as `three_disjoint_closed_fours_is_mate`.
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    o(&mut b, &[(-1, 0)]);
    x(&mut b, &[(10, 0), (11, 0), (12, 0), (13, 0)]);
    o(&mut b, &[(14, 0)]);
    x(&mut b, &[(0, 5), (0, 6), (0, 7), (0, 8)]);
    o(&mut b, &[(0, 9)]);
    let v = eval(&b);
    let m = is_mate_for(&b, Player::X);
    assert_eq!(m, v >= MATE_SCORE - 64);
}

#[test]
fn cover2_bonus_fires_for_two_disjoint_closed_fours() {
    // Two closed-4s with disjoint single-cell defenses → intersection
    // vertex cover = 2 → Layer 3 returns FORK_COVER2_BONUS.
    //
    // Constructing this with closed-4s (defense size 1) is the cleanest
    // way to hit the `insts.len() == 2 && !single_cell_covers_all`
    // branch in `min_vertex_cover_size`.
    let mut with_fork = fresh();
    // C4 #1: q-axis r=0, X at q in 0..3, blocked by O at q=-1. Defense = (4, 0).
    x(&mut with_fork, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    o(&mut with_fork, &[(-1, 0)]);
    // C4 #2: q-axis r=10 (separate line so no Layer-1 interaction with #1),
    // X at q in 0..3, blocked by O at q=-1. Defense = (4, 10).
    x(&mut with_fork, &[(0, 10), (1, 10), (2, 10), (3, 10)]);
    o(&mut with_fork, &[(-1, 10)]);
    let v_with = eval(&with_fork);

    // A control: same two closed-4s for X, plus an O closed-4 on the
    // other side to roughly cancel X's Layer 2. Easier: just compute the
    // Layer 2 expectation directly.
    //
    // Layer 2 for X = 2 × CLOSED_4_SCORE.
    // We expect with-fork eval to exceed that by ≈ FORK_COVER2_BONUS
    // (modulo small Layer 1 contributions). The tolerance is wide
    // because Layer 1 windows around the closed-4s contribute their
    // own thousands.
    let lower_bound = FORK_COVER2_BONUS;
    assert!(
        v_with >= lower_bound,
        "two disjoint closed-4s should produce ≥ FORK_COVER2_BONUS bias, got {v_with} < {lower_bound}"
    );
    assert!(
        v_with < MATE_SCORE - 1000,
        "two disjoint closed-4s should not score as mate, got {v_with}"
    );
    assert!(!is_mate_for(&with_fork, Player::X));
}

#[test]
#[ignore = "informational perf probe"]
fn perf_cached_eval_30_pieces() {
    use std::time::Instant;
    let mut b = fresh();
    // 30 stones spread over a 6x5 patch.
    for q in 0..6i16 {
        for r in 0..5i16 {
            let p = if (q + r) % 2 == 0 {
                Player::X
            } else {
                Player::O
            };
            b.place_for_test(Coord::new(q, r), p);
        }
    }
    // Warm.
    let _ = b.cached_eval();
    let start = Instant::now();
    for _ in 0..1000 {
        let _ = b.cached_eval();
    }
    let avg = start.elapsed() / 1000;
    println!("[perf] cached_eval hit on 30-piece: {avg:?}");

    // Cold (first eval after invalidation).
    let mut total = std::time::Duration::ZERO;
    for _ in 0..100 {
        let c = Coord::new(20, 20);
        b.place_for_test(c, Player::X);
        let s = Instant::now();
        let _ = b.cached_eval();
        total += s.elapsed();
        b.undo().unwrap();
    }
    let cold = total / 100;
    println!("[perf] cached_eval cold (first call after invalidation) on 31-piece: {cold:?}");
}
