//! Phase 4 threat-detection integration tests.

use hammerhead_engine_core::board::{Board, Player};
use hammerhead_engine_core::coords::Coord;
use hammerhead_engine_core::eval_overrides::EvalOverrides;
use hammerhead_engine_core::threats::{ThreatCounts, ThreatKind, compute};

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

/// Enable the Phase 28E-2 Stage 1 rhombus detector on `b` so the
/// detection pass actually runs in `compute()`. The detector is
/// behaviour-gated on a non-zero `EvalOverrides::rhombus` weight
/// (see `threats::compute_with_scratch`) to keep the byte-equivalent
/// default-callers contract; the Stage 1 rhombus-test suite flips
/// the weight on so the detector path is exercised. `iso_radius`
/// stays at its codegen'd Ring-C default (= 3) unless overridden.
fn enable_rhombus(b: &mut Board) {
    let mut ov = b.eval_overrides();
    ov.rhombus = 1; // any non-zero value is sufficient to arm detection
    b.set_eval_overrides(ov);
}

/// Variant of `enable_rhombus` that also sets an explicit isolation
/// radius; reserved for future tests that want to vary the gate.
#[allow(dead_code)]
fn enable_rhombus_with_radius(b: &mut Board, radius: i32) {
    let ov = EvalOverrides {
        rhombus: 1,
        rhombus_isolation_radius: radius,
        ..b.eval_overrides()
    };
    b.set_eval_overrides(ov);
}

// ─────────────────────────────────────────────────────────────────────────────
// 1-2: trivial states
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn empty_board_no_threats() {
    let b = fresh();
    let tx = compute(&b, Player::X);
    let to = compute(&b, Player::O);
    assert_eq!(tx.counts, ThreatCounts::default());
    assert_eq!(to.counts, ThreatCounts::default());
    assert!(tx.s0_instances.is_empty());
    assert!(to.s0_instances.is_empty());
}

#[test]
fn single_piece_no_threats() {
    let mut b = fresh();
    x(&mut b, &[(0, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts, ThreatCounts::default());
    assert!(t.s0_instances.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// 3-9: linear shapes
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn open_five_axis_q() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_5, 1);
    assert_eq!(t.s0_instances.len(), 1);
    let i = &t.s0_instances[0];
    assert_eq!(i.kind, ThreatKind::OpenFive);
    assert_eq!(i.defense_cells.len(), 2);
    assert!(i.defense_cells.contains(&Coord::new(-1, 0)));
    assert!(i.defense_cells.contains(&Coord::new(5, 0)));
}

#[test]
fn closed_five_axis_q() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
    o(&mut b, &[(-1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_5, 1);
    assert_eq!(t.s0_instances.len(), 1);
    let i = &t.s0_instances[0];
    assert_eq!(i.kind, ThreatKind::ClosedFive);
    assert_eq!(i.defense_cells.len(), 1);
    assert_eq!(i.defense_cells[0], Coord::new(5, 0));
}

#[test]
fn open_four_axis_q() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_4, 1);
    assert_eq!(t.s0_instances.len(), 1);
    let i = &t.s0_instances[0];
    assert_eq!(i.kind, ThreatKind::OpenFour);
    assert_eq!(i.defense_cells.len(), 2);
    assert!(i.defense_cells.contains(&Coord::new(-1, 0)));
    assert!(i.defense_cells.contains(&Coord::new(4, 0)));
}

#[test]
fn closed_four_axis_q() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    o(&mut b, &[(-1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_4, 1);
    assert_eq!(t.s0_instances.len(), 1);
    let i = &t.s0_instances[0];
    assert_eq!(i.kind, ThreatKind::ClosedFour);
    assert_eq!(i.defense_cells.len(), 1);
    assert_eq!(i.defense_cells[0], Coord::new(4, 0));
}

#[test]
fn closed_four_blocked_extension_is_not_threat() {
    // OXXXXO_ where p+5 is also O makes the run dead: extending to 5 gives
    // a boxed run with no 6-in-row possibility.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    o(&mut b, &[(-1, 0), (5, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_4, 0);
    assert!(t.s0_instances.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// per-axis equivalents
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn open_five_axis_r() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (0, 1), (0, 2), (0, 3), (0, 4)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_5, 1);
    let i = &t.s0_instances[0];
    assert!(i.defense_cells.contains(&Coord::new(0, -1)));
    assert!(i.defense_cells.contains(&Coord::new(0, 5)));
}

#[test]
fn open_five_axis_s() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, -1), (2, -2), (3, -3), (4, -4)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_5, 1);
    let i = &t.s0_instances[0];
    assert!(i.defense_cells.contains(&Coord::new(-1, 1)));
    assert!(i.defense_cells.contains(&Coord::new(5, -5)));
}

#[test]
fn rotation_preserves_counts() {
    // Rotate a Q-axis open-4 to an R-axis open-4 (different orientation,
    // same shape): counts must match.
    let mut bq = fresh();
    x(&mut bq, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let mut br = fresh();
    x(&mut br, &[(0, 0), (0, 1), (0, 2), (0, 3)]);
    let tq = compute(&bq, Player::X);
    let tr = compute(&br, Player::X);
    assert_eq!(tq.counts, tr.counts);
    assert_eq!(tq.counts.open_4, 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// 18-21: defense-cell semantics and fork primitives
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn defense_cells_actually_block() {
    // After placing opp at the OpenFour defense, recompute drops the threat.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let before = compute(&b, Player::X);
    assert_eq!(before.s0_instances.len(), 1);
    let def = before.s0_instances[0].defense_cells[0];

    o(&mut b, &[(def.q, def.r)]);
    let after = compute(&b, Player::X);
    // 4-run with one immediate neighbour now opp → closed_4 or dead.
    assert_eq!(after.counts.open_4, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// 22-25: caching + invalidation + dirty scope
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn cache_invalidates_on_place() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let before_open4 = b.threats(Player::X).counts.open_4;
    assert_eq!(before_open4, 1);
    // Add another stone to extend to 5 — now open_5, not open_4.
    x(&mut b, &[(4, 0)]);
    let after = b.threats(Player::X);
    assert_eq!(after.counts.open_4, 0);
    assert_eq!(after.counts.open_5, 1);
}

#[test]
fn cache_consistent_after_undo() {
    let mut b = fresh();
    // First the simple position.
    b.place(Coord::new(0, 0)).unwrap(); // ply 0, X
    let before_counts = b.threats(Player::X).counts;
    // Place + undo, expect identical state.
    b.place(Coord::new(1, 0)).unwrap(); // ply 1, O
    b.undo().unwrap();
    let after_counts = b.threats(Player::X).counts;
    assert_eq!(before_counts, after_counts);
}

#[test]
fn distant_placement_does_not_affect_existing_threat_counts() {
    // Threat far from the new piece should retain the same counts.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]); // X open-4
    let before = b.threats(Player::X).counts;
    x(&mut b, &[(50, 0)]); // far away, isolated single piece
    let after = b.threats(Player::X).counts;
    assert_eq!(after, before);
}

#[test]
fn opponent_threats_independent() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]); // X open-4
    let tx = compute(&b, Player::X);
    let to = compute(&b, Player::O);
    assert_eq!(tx.counts.open_4, 1);
    assert_eq!(to.counts.open_4, 0);
}

#[test]
fn overline_six_in_row_does_not_register_as_threat() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)]);
    let t = compute(&b, Player::X);
    // 6-run is a win, not a "threat"; no S0 instances.
    assert!(t.s0_instances.is_empty());
    assert_eq!(t.counts.open_5, 0);
    assert_eq!(t.counts.closed_5, 0);
}

#[test]
fn opponent_open_four_visible_to_o() {
    let mut b = fresh();
    o(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let to = compute(&b, Player::O);
    assert_eq!(to.counts.open_4, 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// S1 — open-3 (Phase 28D-3 D3-A.1)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn open_three_axis_q() {
    // _XXX_ on the q axis, both 2-beyond cells empty → open-3.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_3, 1);
    // S1 shapes do not surface as ThreatInstance entries.
    assert!(t.s0_instances.is_empty());
}

#[test]
fn open_three_axis_r() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (0, 1), (0, 2)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_3, 1);
}

#[test]
fn open_three_axis_s() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, -1), (2, -2)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_3, 1);
}

#[test]
fn open_three_blocked_on_one_side_is_not_open_three() {
    // OXXX_ — left neighbour is opp, so open_ends == 1, classifier
    // sees a non-open-3 (no closed-3 detector yet at A.1 either).
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(-1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_3, 0);
}

#[test]
fn open_three_2beyond_blocked_left_is_not_open_three() {
    // O_XXX_ — both immediate neighbours empty, but left 2-beyond is
    // opp. Extending left gives _XXXX_-against-O which dies as a
    // boxed 4 (no winning 6-line possible). Conservative gate skips.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(-2, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_3, 0);
}

#[test]
fn open_three_2beyond_blocked_right_is_not_open_three() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(4, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_3, 0);
}

#[test]
fn open_three_2beyond_own_stone_still_counts() {
    // X_XXX_ — own 2-beyond stone is not opp; extension viability
    // gate is "non-opp", not "empty", so this still registers.
    let mut b = fresh();
    x(&mut b, &[(-2, 0), (0, 0), (1, 0), (2, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_3, 1);
}

#[test]
fn open_three_does_not_fire_for_length_two() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_3, 0);
}

#[test]
fn open_three_does_not_fire_for_length_four() {
    // Length-4 with both ends open is open_4, not open_3.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_3, 0);
    assert_eq!(t.counts.open_4, 1);
}

#[test]
fn open_three_per_player_isolation() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(0, 5), (1, 5), (2, 5)]);
    let tx = compute(&b, Player::X);
    let to = compute(&b, Player::O);
    assert_eq!(tx.counts.open_3, 1);
    assert_eq!(to.counts.open_3, 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// S1 — closed-3 (Phase 28D-3 D3-A.2)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn closed_three_left_blocked_axis_q() {
    // OXXX_ on the q axis; right 2-beyond empty → closed-3.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(-1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_3, 1);
    assert_eq!(t.counts.open_3, 0);
    // S1 shapes do not surface as ThreatInstance entries.
    assert!(t.s0_instances.is_empty());
}

#[test]
fn closed_three_right_blocked_axis_q() {
    // _XXXO on the q axis; left 2-beyond empty → closed-3.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(3, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_3, 1);
    assert_eq!(t.counts.open_3, 0);
}

#[test]
fn closed_three_axis_r() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (0, 1), (0, 2)]);
    o(&mut b, &[(0, -1)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_3, 1);
}

#[test]
fn closed_three_axis_s() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, -1), (2, -2)]);
    o(&mut b, &[(-1, 1)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_3, 1);
}

#[test]
fn closed_three_both_blocked_is_not_closed_three() {
    // OXXXO — open_ends == 0, no S1 fire.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(-1, 0), (3, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_3, 0);
    assert_eq!(t.counts.open_3, 0);
}

#[test]
fn closed_three_open_side_2beyond_blocked_is_not_closed_three() {
    // OXXX_O — left blocker plus right 2-beyond opp. Extending the
    // open side gives OXXXX_O, a doubly-boxed 5 that cannot reach
    // 6. Conservative gate skips, mirroring closed-4's "beyond
    // non-opp" growth check.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(-1, 0), (4, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_3, 0);
}

#[test]
fn closed_three_open_side_2beyond_own_stone_still_counts() {
    // OXXX_X — open side's 2-beyond is own (non-opp); viability
    // gate accepts. Mirrors open-3's analogous "own 2-beyond" case.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (4, 0)]);
    o(&mut b, &[(-1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_3, 1);
}

#[test]
fn closed_three_does_not_fire_for_length_two() {
    // OXX_ — length 2, no closed-3.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0)]);
    o(&mut b, &[(-1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_3, 0);
}

#[test]
fn closed_three_does_not_fire_for_length_four() {
    // OXXXX_ — length 4 with one blocker is closed_4, not closed_3.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    o(&mut b, &[(-1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.closed_3, 0);
    assert_eq!(t.counts.closed_4, 1);
}

#[test]
fn closed_three_per_player_isolation() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(-1, 0)]);
    o(&mut b, &[(0, 5), (1, 5), (2, 5)]);
    x(&mut b, &[(-1, 5)]);
    let tx = compute(&b, Player::X);
    let to = compute(&b, Player::O);
    assert_eq!(tx.counts.closed_3, 1);
    assert_eq!(to.counts.closed_3, 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// S1 — open-2 (Phase 28D-3 D3-A.3)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn open_two_axis_q() {
    // _XX_ on the q axis, both 2-beyond cells empty → open-2.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_2, 1);
    // S1 shapes do not surface as ThreatInstance entries.
    assert!(t.s0_instances.is_empty());
}

#[test]
fn open_two_axis_r() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (0, 1)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_2, 1);
}

#[test]
fn open_two_axis_s() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, -1)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_2, 1);
}

#[test]
fn open_two_blocked_on_one_side_is_not_open_two() {
    // OXX_ — left neighbour is opp, so open_ends == 1; this is a
    // closed-2 (not in scope for A.3 detection), not an open-2.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0)]);
    o(&mut b, &[(-1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_2, 0);
}

#[test]
fn open_two_2beyond_blocked_left_is_not_open_two() {
    // O_XX_ — both immediate neighbours empty, but left 2-beyond
    // is opp. Extending left gives _XXX_-against-O which dies as
    // a boxed 3 with no winning-6 path. Conservative gate skips.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0)]);
    o(&mut b, &[(-2, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_2, 0);
}

#[test]
fn open_two_2beyond_blocked_right_is_not_open_two() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0)]);
    o(&mut b, &[(3, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_2, 0);
}

#[test]
fn open_two_2beyond_own_stone_still_counts() {
    // X_XX_ — own 2-beyond stone is not opp; viability gate is
    // "non-opp", not "empty", so this still registers. Mirrors
    // the open-3 analogous case.
    let mut b = fresh();
    x(&mut b, &[(-2, 0), (0, 0), (1, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_2, 1);
}

#[test]
fn open_two_does_not_fire_for_length_three() {
    // Length-3 with both ends open is open_3, not open_2.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_2, 0);
    assert_eq!(t.counts.open_3, 1);
}

#[test]
fn open_two_does_not_fire_for_single_stone() {
    let mut b = fresh();
    x(&mut b, &[(0, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_2, 0);
}

#[test]
fn open_two_surrounded_by_opp_is_not_open_two() {
    // OXXO — both ends blocked, open_ends == 0, no S1 fire.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0)]);
    o(&mut b, &[(-1, 0), (2, 0)]);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.open_2, 0);
}

#[test]
fn open_two_per_player_isolation() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0)]);
    o(&mut b, &[(0, 5), (1, 5)]);
    let tx = compute(&b, Player::X);
    let to = compute(&b, Player::O);
    assert_eq!(tx.counts.open_2, 1);
    assert_eq!(to.counts.open_2, 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// Rhombus — cross-axis cluster shape (Phase 28E-2 Stage 1)
//
// Per HeXOpedia §4.3, a rhombus is 4 pieces arranged in a diamond.
// On axial hex coordinates this is 4 cells whose pairwise distances
// are {1,1,1,1,1,2} — 5 unit-length edges plus one long diagonal of
// distance 2. Per the Threat Theory PDF a rhombus is a 3-1-2 threat
// (W=3, S=1, C=2). Per Radius Theory a single opp cell in Ring C
// (hex_distance ≤ 3) of the rhombus centroid defends — so we only
// credit isolated rhombi (no opp inside Ring C of centroid).
//
// Detector enumerates own pieces; for each anchor `P` and each pair of
// adjacent unit-direction vectors `(u, v)` with hex_distance(u, v) == 1,
// the candidate rhombus is `{P, P+u, P+v, P+u+v}` provided all four
// cells hold an own stone. Dedup canonicalizes via sorted 4-tuple.
// ─────────────────────────────────────────────────────────────────────────────

// ── positive cases (5) — isolated rhombi in different rotations
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn rhombus_isolated_canonical_q_r() {
    // Vertices (0,0)(1,0)(0,1)(1,1) — directions (1,0) and (0,1).
    // No opponent stones anywhere → isolated.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 1);
    // S2 shapes do not surface as ThreatInstance entries.
    assert!(t.s0_instances.is_empty());
}

#[test]
fn rhombus_isolated_q_s_axis() {
    // Vertices (0,0)(1,0)(1,-1)(2,-1) — directions (1,0) and (1,-1).
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (1, -1), (2, -1)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 1);
}

#[test]
fn rhombus_isolated_r_negative_s_axis() {
    // Vertices (0,0)(0,1)(-1,1)(-1,2) — directions (0,1) and (-1,1).
    let mut b = fresh();
    x(&mut b, &[(0, 0), (0, 1), (-1, 1), (-1, 2)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 1);
}

#[test]
fn rhombus_isolated_negative_quadrant() {
    // Translated rhombus deep in negative quadrant.
    let mut b = fresh();
    x(&mut b, &[(-5, -5), (-4, -5), (-5, -4), (-4, -4)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 1);
}

#[test]
fn rhombus_isolated_with_distant_opp() {
    // Rhombus at origin; opp far outside Ring C of centroid (1,1).
    // Distance from (1,1) to (10,10) = 18 ≫ 3 → isolated.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
    o(&mut b, &[(10, 10), (-10, -10)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 1);
}

// ── negative cases (5) — geometric rhombus but isolation fails
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn rhombus_with_opp_at_centroid_not_counted() {
    // Centroid of canonical rhombus is (1,1); opp on that cell.
    // Actually (1,1) is itself a vertex (own X), so opp must be
    // elsewhere — pick a cell at distance 1 from centroid.
    // Centroid (1,1); opp at (2,1) → dist 1 ≤ 3 → reject.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
    o(&mut b, &[(2, 1)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 0);
}

#[test]
fn rhombus_with_opp_at_centroid_radius_2_not_counted() {
    // Centroid (1,1); opp at (3,1) → hex_distance 2 ≤ 3 → reject.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
    o(&mut b, &[(3, 1)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 0);
}

#[test]
fn rhombus_with_opp_at_centroid_radius_3_not_counted() {
    // Centroid (1,1); opp at (4,1) → hex_distance 3 == 3 → reject.
    // Ring C boundary inclusive per Radius Theory.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
    o(&mut b, &[(4, 1)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 0);
}

#[test]
fn rhombus_with_opp_inside_bounding_region_not_counted() {
    // Centroid (1,1); opp at (1,2) → hex_distance 1 → reject.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
    o(&mut b, &[(1, 2)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 0);
}

#[test]
fn rhombus_with_opp_adjacent_to_vertex_within_ring_c_not_counted() {
    // Centroid (1,1); opp at (-1,0) → hex_distance to centroid:
    // dq=-2, dr=-1, ds=3 → (2+1+3)/2 = 3 → within Ring C → reject.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
    o(&mut b, &[(-1, 0)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 0);
}

// ── edge cases (3)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn rhombus_on_far_position_translation_invariant() {
    // Same shape, translated to (20, 20) region. Detector must be
    // translation-invariant (no edge effects — board is infinite).
    let mut b = fresh();
    x(&mut b, &[(20, 20), (21, 20), (20, 21), (21, 21)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 1);
}

#[test]
fn overlapping_rhombi_in_2_by_3_block_all_count() {
    // Six X stones filling a 2x3 axial block. On axial hex coords
    // (1,0) and (0,1) are themselves neighbours, so three distinct
    // rhombi sit inside this block (verified by brute-force
    // enumeration of every 4-cell subset with pairwise distances
    // `{1,1,1,1,1,2}`):
    //   A = (0,0),(1,0),(0,1),(1,1)
    //   B = (1,0),(2,0),(0,1),(1,1)  — cross-axis "middle" rhombus
    //   C = (1,0),(2,0),(1,1),(2,1)
    // No opp anywhere → all three isolated → rhombus count = 3.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 3);
}

#[test]
fn rhombus_coexists_with_nearby_linear_shapes() {
    // Rhombus at (0,0)..(1,1) plus a separate linear open-3 far away
    // on a different axis. Rhombus must count AND open_3 must count.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
    // Far open-3 on q axis at y=10.
    x(&mut b, &[(0, 10), (1, 10), (2, 10)]);
    enable_rhombus(&mut b);
    let t = compute(&b, Player::X);
    assert_eq!(t.counts.rhombus, 1);
    assert_eq!(t.counts.open_3, 1);
}
