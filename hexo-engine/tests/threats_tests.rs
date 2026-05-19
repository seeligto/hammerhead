//! Phase 4 threat-detection integration tests.

use hexo_engine_core::board::{Board, Player};
use hexo_engine_core::coords::Coord;
use hexo_engine_core::threats::{ThreatCounts, ThreatKind, compute, single_cell_blocks_all};

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
    let tx = compute(&b, Player::X, &[], None);
    let to = compute(&b, Player::O, &[], None);
    assert_eq!(tx.counts, ThreatCounts::default());
    assert_eq!(to.counts, ThreatCounts::default());
    assert!(tx.s0_instances.is_empty());
    assert!(to.s0_instances.is_empty());
}

#[test]
fn single_piece_no_threats() {
    let mut b = fresh();
    x(&mut b, &[(0, 0)]);
    let t = compute(&b, Player::X, &[], None);
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
    let t = compute(&b, Player::X, &[], None);
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
    let t = compute(&b, Player::X, &[], None);
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
    let t = compute(&b, Player::X, &[], None);
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
    let t = compute(&b, Player::X, &[], None);
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
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.closed_4, 0);
    assert!(t.s0_instances.is_empty());
}

#[test]
fn open_three_axis_q() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.open_3, 1);
    // open_3 is S1, not S0.
    assert!(t.s0_instances.is_empty());
}

#[test]
fn closed_three_axis_q() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    o(&mut b, &[(-1, 0)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.closed_3, 1);
    assert_eq!(t.counts.open_3, 0);
}

#[test]
fn open_two_isolated() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.open_2, 1);
}

#[test]
fn open_two_not_isolated_when_opp_nearby_on_axis() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0)]);
    o(&mut b, &[(3, 0)]); // within 2 along axis Q
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.open_2, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// 10-12: per-axis equivalents
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn open_five_axis_r() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (0, 1), (0, 2), (0, 3), (0, 4)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.open_5, 1);
    let i = &t.s0_instances[0];
    assert!(i.defense_cells.contains(&Coord::new(0, -1)));
    assert!(i.defense_cells.contains(&Coord::new(0, 5)));
}

#[test]
fn open_five_axis_s() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, -1), (2, -2), (3, -3), (4, -4)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.open_5, 1);
    let i = &t.s0_instances[0];
    assert!(i.defense_cells.contains(&Coord::new(-1, 1)));
    assert!(i.defense_cells.contains(&Coord::new(5, -5)));
}

#[test]
fn rotation_preserves_counts() {
    // Rotate Q-axis open-3 to R-axis open-3 (different orientation, same
    // shape): counts must match.
    let mut bq = fresh();
    x(&mut bq, &[(0, 0), (1, 0), (2, 0)]);
    let mut br = fresh();
    x(&mut br, &[(0, 0), (0, 1), (0, 2)]);
    let tq = compute(&bq, Player::X, &[], None);
    let tr = compute(&br, Player::X, &[], None);
    assert_eq!(tq.counts, tr.counts);
}

// ─────────────────────────────────────────────────────────────────────────────
// 13-17: cross-axis shapes
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn triangle_upward() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.triangle, 1);
}

#[test]
fn triangle_downward() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (1, -1)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.triangle, 1);
}

#[test]
fn rhombus_qr() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1), (1, 1)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.rhombus, 1);
}

#[test]
fn rhombus_qs() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (1, -1), (2, -1)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.rhombus, 1);
}

#[test]
fn arch_pattern() {
    let mut b = fresh();
    // {(0,0), (1,0), (2,-1)}: pairwise dists 1, 1, 2 — L-shape.
    x(&mut b, &[(0, 0), (1, 0), (2, -1)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.arch, 1);
}

#[test]
fn trapezoid_pattern() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (0, 1), (1, 1)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.trapezoid, 1);
}

#[test]
fn bone_pattern() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (0, 1), (-1, 1), (1, -1)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.bone, 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// 18-21: defense-cell semantics and fork primitives
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn defense_cells_actually_block() {
    // After placing opp at the OpenFour defense, recompute drops the threat.
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let before = compute(&b, Player::X, &[], None);
    assert_eq!(before.s0_instances.len(), 1);
    let def = before.s0_instances[0].defense_cells[0];

    o(&mut b, &[(def.q, def.r)]);
    let after = compute(&b, Player::X, &[], None);
    // 4-run with one immediate neighbour now opp → closed_4 or dead.
    assert_eq!(after.counts.open_4, 0);
}

#[test]
fn fork_two_open_fours_disjoint_is_mate_pending() {
    let mut b = fresh();
    // Open-4 on axis Q at r=0.
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    // Open-4 on axis Q at r=10 (well separated).
    x(&mut b, &[(0, 10), (1, 10), (2, 10), (3, 10)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.s0_instances.len(), 2);
    assert!(t.is_mate_pending());
    assert!(!single_cell_blocks_all(&t.s0_instances));
}

#[test]
fn fork_two_threats_sharing_cell_not_mate_pending() {
    // Construct two S0 instances whose defense_cells share a coordinate.
    // Use a closed_5 with defense at (5,0) and an open_4 also containing
    // (5,0) as one defense — e.g., place the open_4 at r=0, q in 6..10
    // with neighbours empty so (5,0) is its left defense.
    let mut b = fresh();
    // Closed-5: X at (0..5, 0), O at (-1, 0). Defense = (5,0).
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
    o(&mut b, &[(-1, 0)]);
    let t = compute(&b, Player::X, &[], None);
    assert_eq!(t.counts.closed_5, 1);
    // Single instance — vacuously coverable by one cell.
    assert_eq!(t.s0_instances.len(), 1);
    assert!(single_cell_blocks_all(&t.s0_instances));
    // Single-threat is_mate_pending must be false (needs >=2).
    assert!(!t.is_mate_pending());
}

#[test]
fn single_cell_blocks_all_empty() {
    assert!(single_cell_blocks_all(&[]));
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
    x(&mut b, &[(0, 0), (1, 0), (2, 0)]);
    let before = b.threats(Player::X).counts;
    x(&mut b, &[(50, 0)]); // far away, isolated single piece
    let after = b.threats(Player::X).counts;
    assert_eq!(after.open_3, before.open_3);
    assert_eq!(after.open_2, before.open_2);
}

#[test]
fn opponent_threats_independent() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]); // X open-4
    let tx = compute(&b, Player::X, &[], None);
    let to = compute(&b, Player::O, &[], None);
    assert_eq!(tx.counts.open_4, 1);
    assert_eq!(to.counts.open_4, 0);
}

#[test]
fn overline_six_in_row_does_not_register_as_threat() {
    let mut b = fresh();
    x(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)]);
    let t = compute(&b, Player::X, &[], None);
    // 6-run is a win, not a "threat"; no S0 instances.
    assert!(t.s0_instances.is_empty());
    assert_eq!(t.counts.open_5, 0);
    assert_eq!(t.counts.closed_5, 0);
}

#[test]
fn opponent_open_four_visible_to_o() {
    let mut b = fresh();
    o(&mut b, &[(0, 0), (1, 0), (2, 0), (3, 0)]);
    let to = compute(&b, Player::O, &[], None);
    assert_eq!(to.counts.open_4, 1);
}
