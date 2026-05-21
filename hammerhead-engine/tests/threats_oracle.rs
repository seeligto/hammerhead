//! Threat-detection correctness tests.
//!
//! Covers compute determinism, place/undo round-trip stability, and
//! winning-move detection.

// Test fixtures intentionally use a long string-building style for the
// drift report. The pedantic lints below add noise but no value here.
#![allow(
    clippy::cast_possible_truncation,
    clippy::format_push_string,
    clippy::stable_sort_primitive,
    clippy::type_complexity,
    clippy::items_after_statements
)]

use hammerhead_engine_core::board::{Board, Player};
use hammerhead_engine_core::coords::Coord;
use hammerhead_engine_core::moves;
use hammerhead_engine_core::threats::{ThreatInstance, ThreatSet, compute as compute_threats};

type InstanceKey = (u8, Vec<(i16, i16)>, Vec<(i16, i16)>);

/// `(kind, sorted_pieces, sorted_defense_cells)` — canonical form
/// for equivalence comparison. `anchor` is intentionally ignored.
/// Coords are projected to `(q, r)` tuples so we can sort with `Ord`.
fn instance_key(inst: &ThreatInstance) -> InstanceKey {
    let mut p: Vec<(i16, i16)> = inst.pieces.iter().map(|c| (c.q, c.r)).collect();
    p.sort();
    let mut d: Vec<(i16, i16)> = inst.defense_cells.iter().map(|c| (c.q, c.r)).collect();
    d.sort();
    (inst.kind as u8, p, d)
}

fn threat_set_equiv(a: &ThreatSet, b: &ThreatSet) -> bool {
    if a.counts != b.counts {
        return false;
    }
    if a.s0_instances.len() != b.s0_instances.len() {
        return false;
    }
    let mut ak: Vec<_> = a.s0_instances.iter().map(instance_key).collect();
    let mut bk: Vec<_> = b.s0_instances.iter().map(instance_key).collect();
    ak.sort();
    bk.sort();
    ak == bk
}

fn report_diff(a: &ThreatSet, b: &ThreatSet, header: &str) -> String {
    let mut s = String::new();
    s.push_str(header);
    s.push('\n');
    if a.counts != b.counts {
        s.push_str(&format!(
            "  counts: incr={:?} full={:?}\n",
            a.counts, b.counts
        ));
    }
    s.push_str(&format!(
        "  s0 lens: incr={} full={}\n",
        a.s0_instances.len(),
        b.s0_instances.len()
    ));
    let ak: Vec<_> = a.s0_instances.iter().map(instance_key).collect();
    let bk: Vec<_> = b.s0_instances.iter().map(instance_key).collect();
    let mut aks = ak.clone();
    aks.sort();
    let mut bks = bk.clone();
    bks.sort();
    for k in &aks {
        if !bks.contains(k) {
            s.push_str(&format!("  incr-only: {k:?}\n"));
        }
    }
    for k in &bks {
        if !aks.contains(k) {
            s.push_str(&format!("  full-only: {k:?}\n"));
        }
    }
    s
}

/// Builds a midgame board via the regular `place` path (no
/// `place_for_test`), which is what the search hot path uses.
fn play_moves(b: &mut Board, mvs: &[Coord]) {
    for &c in mvs {
        b.place(c).unwrap();
    }
}

fn build_midgame_12() -> Board {
    let mut b = Board::new();
    let mvs = [
        (0, 0),
        (-1, 0),
        (-1, 1),
        (0, -1),
        (0, 1),
        (1, -1),
        (1, 0),
        (-2, 0),
        (-2, 1),
        (-2, 2),
        (-1, -1),
        (-1, 2),
    ];
    play_moves(&mut b, &mvs.map(|(q, r)| Coord::new(q, r)));
    b
}

#[test]
fn instance_pieces_are_deterministic_for_open_four() {
    // Two computations of the same shape on the same pieces produce
    // identical instance pieces. (Phase 15 originally stored a derived
    // `anchor: Coord` field too, but it was unused metadata and was
    // dropped; this test still guards instance determinism.)
    let mut b = Board::new();
    let mvs = [(0, 0), (4, 4), (-4, 4), (1, 0), (2, 0), (4, 3), (-4, 3), (3, 0)];
    play_moves(&mut b, &mvs.map(|(q, r)| Coord::new(q, r)));

    let a = compute_threats(&b, Player::X);
    let bset = compute_threats(&b, Player::X);
    assert_eq!(a.s0_instances.len(), bset.s0_instances.len());
    for (ia, ib) in a.s0_instances.iter().zip(bset.s0_instances.iter()) {
        assert_eq!(ia.kind, ib.kind);
        let pa: Vec<_> = ia.pieces.iter().map(|c| (c.q, c.r)).collect();
        let pb: Vec<_> = ib.pieces.iter().map(|c| (c.q, c.r)).collect();
        assert_eq!(pa, pb, "pieces mismatch across recomputes");
    }
}

#[test]
fn incremental_handles_place_then_undo_round_trip() {
    // place(c), threats read, undo(c), threats read — should match the
    // pre-place threats.
    let mut b = build_midgame_12();
    let before_x = b.threats(Player::X).clone();
    let before_o = b.threats(Player::O).clone();

    let legal = moves::generate(&b, 8);
    assert!(!legal.is_empty());
    let mv = legal[0];
    b.place(mv).unwrap();
    let _ = b.threats(Player::X); // populate cache, exercise incremental
    b.undo().unwrap();

    let after_x = b.threats(Player::X);
    let after_o = b.threats(Player::O);
    assert!(
        threat_set_equiv(&before_x, &after_x),
        "{}",
        report_diff(&after_x, &before_x, "X round-trip drift")
    );
    assert!(
        threat_set_equiv(&before_o, &after_o),
        "{}",
        report_diff(&after_o, &before_o, "O round-trip drift")
    );
}

#[test]
fn incremental_handles_winning_move() {
    // Build a 5-in-row by X, then place the 6th to win. Threats before
    // the winning move should match the oracle. After the winning move
    // we don't probe (post-terminal).
    let mut b = Board::new();
    // X must walk a 6-in-row through six X-placements. Use the same
    // padding pattern as board_tests `winner_after_six_in_row_q`.
    let line = |k: i16| Coord::new(k, 0);
    let p1 = Coord::new(0, 4); // O pads on a different line.
    let p2 = Coord::new(0, -4);
    let pad = |k: i16, base: Coord| Coord::new(base.q + k, base.r);

    b.place(line(0)).unwrap(); // X ply 0
    b.place(pad(0, p1)).unwrap(); // O ply 1
    b.place(pad(0, p2)).unwrap(); // O ply 2
    b.place(line(1)).unwrap(); // X ply 3
    b.place(line(2)).unwrap(); // X ply 4
    b.place(pad(1, p1)).unwrap(); // O ply 5
    b.place(pad(1, p2)).unwrap(); // O ply 6
    b.place(line(3)).unwrap(); // X ply 7
    b.place(line(4)).unwrap(); // X ply 8
    b.place(pad(2, p1)).unwrap(); // O ply 9
    b.place(pad(2, p2)).unwrap(); // O ply 10
    // The next X stone makes line 0..5 → six-in-row (3 X stones already
    // present; pad pieces force the parity correctly).
    // Before reading, force a read to populate the cache via incremental.
    let incr_x = b.threats(Player::X).clone();
    let oracle_x = compute_threats(&b, Player::X);
    assert!(
        threat_set_equiv(&incr_x, &oracle_x),
        "{}",
        report_diff(&incr_x, &oracle_x, "pre-win X")
    );
}
