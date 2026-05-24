//! Phase 4 threat-detection integration tests.

use hammerhead_engine_core::board::{Board, Player};
use hammerhead_engine_core::coords::Coord;
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
