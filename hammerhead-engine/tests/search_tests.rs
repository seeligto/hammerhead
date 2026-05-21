//! Phase 8 search integration tests. Covers mate-in-N detection, forced
//! defense, ordering effectiveness via TT replay, aspiration robustness,
//! time-budget honoring, depth-cap honoring, quiescence tactic
//! recognition, and the public [`Engine`] entry point.
//!
//! Tests rely on `place_for_test` plus `force_parity_for_test` to build
//! positions outside legal play sequences; `make test` runs
//! `cargo test --release` so the debug-only parity assertions are off.

use std::time::Instant;

use hammerhead_engine_core::board::{Board, Player};
use hammerhead_engine_core::config::MATE_SCORE;
use hammerhead_engine_core::coords::{Coord, ORIGIN, hex_distance};
use hammerhead_engine_core::ordering::OrderingState;
use hammerhead_engine_core::engine::Engine;
use hammerhead_engine_core::search::{SearchConfig, search_root};
use hammerhead_engine_core::tt::TranspositionTable;

// ────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────

fn fresh() -> (TranspositionTable, OrderingState) {
    (TranspositionTable::new(16), OrderingState::new())
}

fn run(b: &mut Board, depth: i8) -> hammerhead_engine_core::search::SearchResult {
    let (mut tt, mut ord) = fresh();
    let cfg = SearchConfig {
        max_depth: depth,
        time_ms: None,
        ..Default::default()
    };
    search_root(b, &mut tt, &mut ord, &cfg)
}

/// Place `count` X stones along the q-axis starting at `(start, 0)`.
fn x_row(b: &mut Board, start: i16, count: i16) {
    for q in start..(start + count) {
        b.place_for_test(Coord::new(q, 0), Player::X);
    }
}

fn o_row(b: &mut Board, start: i16, count: i16) {
    for q in start..(start + count) {
        b.place_for_test(Coord::new(q, 0), Player::O);
    }
}

// ────────────────────────────────────────────────────────────────────────
// 1. Mate-in-1 recognition
// ────────────────────────────────────────────────────────────────────────

#[test]
fn mate_in_one_from_open_five() {
    let mut b = Board::new();
    x_row(&mut b, 0, 5);
    b.force_parity_for_test(Player::X, 0);

    let r = run(&mut b, 4);
    let winners = [Coord::new(-1, 0), Coord::new(5, 0)];
    assert!(
        winners.contains(&r.best_move),
        "expected open-5 completion, got {:?}",
        r.best_move
    );
    assert_eq!(
        r.score,
        MATE_SCORE - 1,
        "mate-in-1 should score MATE_SCORE - 1"
    );
}

// ────────────────────────────────────────────────────────────────────────
// 2. Forced block — O has closed-5, X must block the remaining endpoint
// ────────────────────────────────────────────────────────────────────────

#[test]
fn forced_block_of_opp_closed_five() {
    let mut b = Board::new();
    b.place_for_test(Coord::new(-1, 0), Player::X); // X closes left flank
    o_row(&mut b, 0, 5);
    b.force_parity_for_test(Player::X, 0);

    let r = run(&mut b, 4);
    assert_eq!(
        r.best_move,
        Coord::new(5, 0),
        "must block the only remaining endpoint, got {:?}",
        r.best_move
    );
    // After blocking, O's 5-run is fully sealed and the position is
    // close to even. A regression that wrongly scored the block as
    // catastrophic (e.g. -100_000) would still satisfy a loose
    // > -MATE/2 bound, so use a tight envelope.
    assert!(
        r.score.abs() < 50_000,
        "post-block score must be near-neutral, got {}",
        r.score
    );
}

// ────────────────────────────────────────────────────────────────────────
// 3. Mate-in-2 — X has open-4, plays 2-stone sequence to mate
// ────────────────────────────────────────────────────────────────────────

#[test]
fn mate_in_two_from_open_four() {
    let mut b = Board::new();
    x_row(&mut b, 0, 4);
    b.force_parity_for_test(Player::X, 0);

    let r = run(&mut b, 4);
    let valid_first = [Coord::new(-1, 0), Coord::new(4, 0)];
    assert!(
        valid_first.contains(&r.best_move),
        "expected open-4 extension, got {:?}",
        r.best_move
    );
    assert!(
        r.score > MATE_SCORE / 2,
        "mate-class score required, got {}",
        r.score
    );
}

// ────────────────────────────────────────────────────────────────────────
// 4. Fork mate — two disjoint open-4s; X completes one on its turn
// ────────────────────────────────────────────────────────────────────────

#[test]
fn fork_mate_two_disjoint_open_fours() {
    let mut b = Board::new();
    // Open-4 #1 on q-axis at r=0
    x_row(&mut b, 0, 4);
    // Open-4 #2 on q-axis at r=5
    for q in 0..4_i16 {
        b.place_for_test(Coord::new(q, 5), Player::X);
    }
    b.force_parity_for_test(Player::X, 0);

    let r = run(&mut b, 4);
    let valid = [
        Coord::new(-1, 0),
        Coord::new(4, 0),
        Coord::new(-1, 5),
        Coord::new(4, 5),
    ];
    assert!(
        valid.contains(&r.best_move),
        "expected an open-4 endpoint, got {:?}",
        r.best_move
    );
    assert!(
        r.score > MATE_SCORE / 2,
        "fork mate must produce mate-class score, got {}",
        r.score
    );
}

// ────────────────────────────────────────────────────────────────────────
// 5. Sane opening — ORIGIN forced first, then within move-gen radius
// ────────────────────────────────────────────────────────────────────────

#[test]
fn sane_opening_moves() {
    let mut e = Engine::new(16);
    let r = e.best_move(Some(500), Some(2));
    assert_eq!(r.best_move, ORIGIN, "empty board must return ORIGIN");

    e.place(ORIGIN).expect("origin placement");
    let r2 = e.best_move(Some(500), Some(2));
    let d = hex_distance(r2.best_move, ORIGIN);
    assert!(
        d > 0 && d <= 2,
        "second stone must be near origin (got dist {d})"
    );
}

// ────────────────────────────────────────────────────────────────────────
// 6. TT replay — warm TT reduces nodes vs a cleared TT
// ────────────────────────────────────────────────────────────────────────

#[test]
fn tt_replay_reduces_nodes() {
    let mut b = Board::new();
    x_row(&mut b, 0, 3);
    for q in 0..3_i16 {
        b.place_for_test(Coord::new(q, 3), Player::O);
    }
    b.force_parity_for_test(Player::X, 0);

    let mut tt = TranspositionTable::new(16);
    let mut ord = OrderingState::new();
    let cfg = SearchConfig {
        max_depth: 4,
        time_ms: None,
        ..Default::default()
    };

    let r1 = search_root(&mut b, &mut tt, &mut ord, &cfg);
    let r2 = search_root(&mut b, &mut tt, &mut ord, &cfg);
    // Strict reduction: a broken TT (probes always miss) would satisfy
    // a `<=` bound. Require warm to be noticeably smaller than cold.
    assert!(
        r2.nodes * 2 < r1.nodes,
        "warm TT must materially reduce nodes: cold={}, warm={}",
        r1.nodes,
        r2.nodes
    );

    tt.clear();
    let r3 = search_root(&mut b, &mut tt, &mut ord, &cfg);
    assert!(
        r3.nodes > r2.nodes,
        "cleared TT must re-explore more than the warm pass: warm={}, cleared={}",
        r2.nodes,
        r3.nodes
    );
}

// ────────────────────────────────────────────────────────────────────────
// 7. Aspiration widening — tiny initial window must still converge
// ────────────────────────────────────────────────────────────────────────

#[test]
fn aspiration_widening_converges() {
    let mut b = Board::new();
    x_row(&mut b, 0, 3);
    for q in 0..2_i16 {
        b.place_for_test(Coord::new(q, 4), Player::O);
    }
    b.force_parity_for_test(Player::X, 0);

    let mut tt = TranspositionTable::new(16);
    let mut ord = OrderingState::new();
    let cfg = SearchConfig {
        max_depth: 6,
        time_ms: Some(5_000),
        asp_window_initial: 1,
        ..Default::default()
    };
    let r = search_root(&mut b, &mut tt, &mut ord, &cfg);
    assert!(
        r.depth_reached >= 4,
        "aspiration must reach at least depth 4, got {}",
        r.depth_reached
    );
    assert_ne!(r.best_move, ORIGIN, "must return a real move");
}

// ────────────────────────────────────────────────────────────────────────
// 8. Time budget honored — search returns within 2× the requested slack
// ────────────────────────────────────────────────────────────────────────

#[test]
fn time_budget_honored() {
    let mut b = Board::new();
    b.place_for_test(ORIGIN, Player::X);

    let mut tt = TranspositionTable::new(16);
    let mut ord = OrderingState::new();
    let cfg = SearchConfig {
        max_depth: 64,
        time_ms: Some(100),
        ..Default::default()
    };

    let start = Instant::now();
    let r = search_root(&mut b, &mut tt, &mut ord, &cfg);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 400,
        "time-budget overrun: {}ms",
        elapsed.as_millis()
    );
    assert!(
        r.depth_reached >= 1,
        "must complete at least depth 1 within budget"
    );
}

// ────────────────────────────────────────────────────────────────────────
// 9. Depth cap honored — search stops at configured max_depth
// ────────────────────────────────────────────────────────────────────────

#[test]
fn depth_cap_honored() {
    let mut b = Board::new();
    b.place_for_test(ORIGIN, Player::X);

    let mut tt = TranspositionTable::new(16);
    let mut ord = OrderingState::new();
    let cfg = SearchConfig {
        max_depth: 2,
        time_ms: None,
        ..Default::default()
    };
    let r = search_root(&mut b, &mut tt, &mut ord, &cfg);
    assert_eq!(r.depth_reached, 2, "expected depth_reached == 2");
}

// ────────────────────────────────────────────────────────────────────────
// 10. Quiescence catches a tactical follow-up missed by static eval alone
// ────────────────────────────────────────────────────────────────────────

#[test]
fn quiescence_finds_two_stone_mate() {
    // X open-4 — full mate-in-2-stones is below the search's static
    // horizon at depth=1. Quiescence must lift the static score to
    // mate-class. Compared against qsearch disabled (max_plies = 0)
    // which only stands pat at depth 0.
    let mut b = Board::new();
    x_row(&mut b, 0, 4);
    b.force_parity_for_test(Player::X, 0);

    // Disable check extensions so the tactic isn't surfaced by extension
    // alone; quiescence must do the lifting.
    let cfg_q = SearchConfig {
        max_depth: 1,
        time_ms: None,
        qsearch_max_plies: 8,
        max_check_extensions: 0,
        ..Default::default()
    };
    let cfg_no_q = SearchConfig {
        qsearch_max_plies: 0,
        ..cfg_q
    };

    let (mut tt1, mut ord1) = fresh();
    let r_q = search_root(&mut b, &mut tt1, &mut ord1, &cfg_q);

    let (mut tt2, mut ord2) = fresh();
    let r_noq = search_root(&mut b, &mut tt2, &mut ord2, &cfg_no_q);

    assert!(
        r_q.score > r_noq.score,
        "quiescence must surface a higher score: q={} no-q={}",
        r_q.score,
        r_noq.score
    );
    // Strong assertion: qsearch must reach within a few plies of MATE,
    // not just any large positional eval. A bug where qsearch stands
    // pat at the layer-2 open-5 score (~800k) would otherwise pass.
    assert!(
        r_q.score >= MATE_SCORE - 8,
        "quiescence must reach mate-class score (within 8 plies), got {}",
        r_q.score
    );
    // The no-q path stands pat with the static eval; it must NOT reach
    // mate-class scoring (proves the qsearch path is doing the work).
    assert!(
        r_noq.score < MATE_SCORE - 32,
        "no-qsearch must not reach mate-class via standpat, got {}",
        r_noq.score
    );
}

// ────────────────────────────────────────────────────────────────────────
// 11. Integration smoke — full pipeline returns a sensible move
// ────────────────────────────────────────────────────────────────────────

#[test]
fn integration_full_pipeline() {
    let mut e = Engine::new(16);
    e.place(ORIGIN).expect("X first stone at origin");
    // O plays adjacent
    e.place(Coord::new(0, 1)).expect("O first stone");
    e.place(Coord::new(1, 0)).expect("O second stone");

    let r = e.best_move(Some(1_500), Some(4));
    assert!(r.depth_reached >= 1, "must complete at least depth 1");
    assert!(r.nodes > 0, "should explore some nodes");
    // Best move must be a legal cell, not ORIGIN (occupied) nor far away.
    assert_ne!(r.best_move, ORIGIN);
    let d = hex_distance(r.best_move, ORIGIN);
    assert!(
        d <= 4,
        "best move should stay near current cluster, dist={d}"
    );
}
