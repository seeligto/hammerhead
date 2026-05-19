//! Phase 15: oracle correctness test for the incremental threats path.
//!
//! Random-walks several starting positions, alternating `place` / `undo`
//! with growth bias. After every step it reads `Board::threats(player)`
//! (which exercises the incremental path through `reconcile_threats`)
//! and compares the result against a fresh full-recompute oracle. Any
//! drift fails the test with the dirty centers logged for replay.
//!
//! Seed is fixed (`0xHEX0_F00D`) — failures replay byte-identically.

// Test fixtures intentionally use a long string-building style for the
// drift report. The pedantic lints below add noise but no value here.
#![allow(
    clippy::cast_possible_truncation,
    clippy::format_push_string,
    clippy::stable_sort_primitive,
    clippy::type_complexity,
    clippy::items_after_statements
)]

use hexo_engine_core::board::{Board, Player};
use hexo_engine_core::coords::Coord;
use hexo_engine_core::moves;
use hexo_engine_core::threats::{
    self, ThreatInstance, ThreatScratch, ThreatSet, compute as compute_threats,
};
use rand_xoshiro::Xoshiro256PlusPlus;
use rand_xoshiro::rand_core::{Rng, SeedableRng};

const ORACLE_SEED: u64 = 0xDEAD_F00D_CAFE_BEEF_u64;
const TARGET_POSITIONS: usize = 10_000;
const MIN_POSITIONS: usize = 5_000;

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

/// Replicates the bench-fixture builders so the oracle test does not
/// depend on the bench harness module tree. The boards are constructed
/// via the regular `place` path (no `place_for_test`), which is what
/// the search hot path uses. We restrict to legal openings to keep the
/// initial position valid for the random walk.
fn build_empty() -> Board {
    Board::new()
}

fn build_single_origin() -> Board {
    let mut b = Board::new();
    b.place(Coord::new(0, 0)).unwrap();
    b
}

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

fn build_midgame_30() -> Board {
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
        (0, -2),
        (0, 2),
        (1, -2),
        (1, 1),
        (2, -2),
        (2, -1),
        (2, 0),
        (-3, 0),
        (-3, 1),
        (-3, 2),
        (-3, 3),
        (-2, -1),
        (-2, 3),
        (-1, -2),
        (-1, 3),
        (0, -3),
        (0, 3),
        (1, -3),
    ];
    play_moves(&mut b, &mvs.map(|(q, r)| Coord::new(q, r)));
    b
}

#[test]
fn anchor_is_deterministic_for_open_four() {
    // Two computations of the same shape on the same pieces produce
    // identical anchor coords. The anchor is `pieces[len/2]` per spec.
    let mut b = Board::new();
    let mvs = [(0, 0), (4, 4), (-4, 4), (1, 0), (2, 0), (4, 3), (-4, 3), (3, 0)];
    play_moves(&mut b, &mvs.map(|(q, r)| Coord::new(q, r)));

    let a = compute_threats(&b, Player::X, &[], None);
    let bset = compute_threats(&b, Player::X, &[], None);
    assert_eq!(a.s0_instances.len(), bset.s0_instances.len());
    for (ia, ib) in a.s0_instances.iter().zip(bset.s0_instances.iter()) {
        assert_eq!(ia.kind, ib.kind);
        assert_eq!(ia.anchor, ib.anchor, "anchor mismatch across recomputes");
        // Sanity: anchor is in pieces.
        assert!(ia.pieces.contains(&ia.anchor));
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
fn incremental_handles_overflow_fallback() {
    // Place enough stones without reading between to overflow the
    // dirty-centers vec. The next read uses full-recompute fallback
    // and must still match the oracle.
    use hexo_engine_core::config::MAX_INCREMENTAL_CENTERS;
    let mut b = build_midgame_12();
    let legal = moves::generate(&b, 4);
    let n_to_place = MAX_INCREMENTAL_CENTERS + 2;
    for &m in legal.iter().take(n_to_place) {
        if b.is_legal(m) {
            b.place(m).unwrap();
        }
    }
    assert!(b.threats_dirty_overflow_for_test());

    let incr_x = b.threats(Player::X).clone();
    let oracle_x = compute_threats(&b, Player::X, &[], None);
    assert!(
        threat_set_equiv(&incr_x, &oracle_x),
        "{}",
        report_diff(&incr_x, &oracle_x, "X overflow-fallback drift")
    );

    let incr_o = b.threats(Player::O).clone();
    let oracle_o = compute_threats(&b, Player::O, &[], None);
    assert!(
        threat_set_equiv(&incr_o, &oracle_o),
        "{}",
        report_diff(&incr_o, &oracle_o, "O overflow-fallback drift")
    );
}

type Builder = fn() -> Board;

#[test]
fn incremental_matches_full_recompute_10k_positions() {
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(ORACLE_SEED);
    let starts: &[Builder] = &[
        build_empty,
        build_single_origin,
        build_midgame_12,
        build_midgame_30,
    ];

    let mut positions_tested = 0usize;
    let mut scratch = ThreatScratch::default();
    let per_builder = TARGET_POSITIONS.div_ceil(starts.len());
    'outer: for start_builder in starts {
        let mut board = start_builder();
        let mut from_this = 0usize;
        while from_this < per_builder {
            if positions_tested >= TARGET_POSITIONS {
                break 'outer;
            }
            // 70% place, 30% undo (when there's history).
            let coin = rng.next_u64();
            let action_is_place = board.ply() == 0 || (coin % 10) < 7;
            let action_succeeded = if action_is_place {
                let legal = moves::generate(&board, 8);
                if legal.is_empty() {
                    false
                } else {
                    let pick = rng.next_u64();
                    let mv = legal[(pick as usize) % legal.len()];
                    board.place(mv).is_ok()
                }
            } else {
                board.undo().is_ok()
            };
            if !action_succeeded {
                // Position is stuck (no legal moves and no history) —
                // restart with this builder's fresh state.
                board = start_builder();
                continue;
            }
            if board.winner().is_some() {
                // Don't probe threats past a terminal position — undo
                // back to a non-terminal state before continuing.
                board.undo().unwrap();
                continue;
            }

            // Force the cache via the incremental path:
            let inc_x = board.threats(Player::X).clone();
            let inc_o = board.threats(Player::O).clone();
            // Oracle: fresh full recompute (scratch reused).
            let full_x = threats::compute_with_scratch(
                &board,
                Player::X,
                &mut scratch,
                &[],
                None,
            );
            let full_o = threats::compute_with_scratch(
                &board,
                Player::O,
                &mut scratch,
                &[],
                None,
            );
            assert!(
                threat_set_equiv(&inc_x, &full_x),
                "X drift at ply {} (positions_tested={positions_tested})\ndirty centers were: {:?}\n{}",
                board.ply(),
                board.threats_dirty_centers_for_test(),
                report_diff(&inc_x, &full_x, "X"),
            );
            assert!(
                threat_set_equiv(&inc_o, &full_o),
                "O drift at ply {} (positions_tested={positions_tested})\ndirty centers were: {:?}\n{}",
                board.ply(),
                board.threats_dirty_centers_for_test(),
                report_diff(&inc_o, &full_o, "O"),
            );
            positions_tested += 1;
            from_this += 1;
        }
    }
    assert!(
        positions_tested >= MIN_POSITIONS,
        "tested too few positions: {positions_tested}"
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
    let oracle_x = compute_threats(&b, Player::X, &[], None);
    assert!(
        threat_set_equiv(&incr_x, &oracle_x),
        "{}",
        report_diff(&incr_x, &oracle_x, "pre-win X")
    );
}
