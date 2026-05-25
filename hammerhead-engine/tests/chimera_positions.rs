//! Sprint 3B chimera-positions invariant tests.
//!
//! Post-search invariant: `board.candidates()` collected as a set must
//! equal the from-scratch oracle reconstructed by sweeping `pieces()` ×
//! `r=8` and excluding occupied cells. Catches the Sprint 2C failure
//! mode (place/undo-cycle candidate corruption) where the in-search
//! candidate maintenance diverges from a clean state.
//!
//! At Sprint 3 entry the tests pass against the current `place` / `undo`
//! impl. After Sprint 3B Phase B.2.3 (search switched to
//! `place_for_search` / `undo_for_search`), the tests confirm the new
//! variants preserve the candidate invariant across a full search.

use hammerhead_engine_core::board::{Board, Player, player_at_ply};
use hammerhead_engine_core::config::MAX_PIECE_DISTANCE;
use hammerhead_engine_core::coords::{Coord, hex_distance};
use hammerhead_engine_core::engine::Engine;
use std::collections::HashSet;

/// Rebuild the outer (r=8) candidate set from scratch: every empty cell
/// within `MAX_PIECE_DISTANCE` of some placed piece, excluding pieces.
fn oracle_candidates(board: &Board) -> HashSet<Coord> {
    let pieces: Vec<Coord> = board.history().to_vec();
    if pieces.is_empty() {
        // ply==0 special case: ORIGIN is the unique legal cell.
        return [Coord::new(0, 0)].into_iter().collect();
    }
    let occupied: HashSet<Coord> = pieces.iter().copied().collect();
    let mut out: HashSet<Coord> = HashSet::new();
    for &p in &pieces {
        for dq in -MAX_PIECE_DISTANCE..=MAX_PIECE_DISTANCE {
            for dr in -MAX_PIECE_DISTANCE..=MAX_PIECE_DISTANCE {
                let c = Coord::new(p.q + dq, p.r + dr);
                if hex_distance(p, c) > MAX_PIECE_DISTANCE {
                    continue;
                }
                if occupied.contains(&c) {
                    continue;
                }
                out.insert(c);
            }
        }
    }
    out
}

/// Build a board by placing the given coord sequence (parity-correct
/// X/O alternation per `HeXO` turn rules).
fn play(moves: &[(i16, i16)]) -> Board {
    let mut b = Board::new();
    for &(q, r) in moves {
        let p: Player = player_at_ply(b.ply());
        b.place_for_test(Coord::new(q, r), p);
    }
    b
}

/// Drive `Engine` through a depth-6 search and confirm post-search
/// `board.candidates()` matches the oracle.
fn assert_candidates_oracle(engine: &mut Engine, label: &str) {
    let pre: HashSet<Coord> = engine.board.candidates().collect();
    let pre_oracle = oracle_candidates(&engine.board);
    assert_eq!(
        pre, pre_oracle,
        "{label}: pre-search candidates diverged from oracle"
    );

    let _ = engine.best_move(None, Some(6));

    let post: HashSet<Coord> = engine.board.candidates().collect();
    let post_oracle = oracle_candidates(&engine.board);
    assert_eq!(
        post, post_oracle,
        "{label}: post-search candidates diverged from oracle ({} vs {})",
        post.len(),
        post_oracle.len(),
    );
}

#[test]
fn chimera_threat_split_axis() {
    // midgame_12 fixture: 12 pieces clustered around origin. Search
    // explores threat moves at the r=2/r=8 boundary — exactly the
    // case Sprint 2C corrupted (candidates leaking on the outer ring).
    let moves = [
        (0, 0), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1),
        (1, 0), (-2, 0), (-2, 1), (-2, 2), (-1, -1), (-1, 2),
    ];
    let board = play(&moves);
    let mut engine = Engine::new(16);
    // Replay history into the engine so the engine's parity and ply
    // counter stay consistent with the board state we built.
    for c in board.history() {
        engine.board.place_for_test(*c, player_at_ply(engine.board.ply()));
    }
    assert_candidates_oracle(&mut engine, "chimera_threat_split_axis");
}

#[test]
fn chimera_postblock() {
    // midgame_30 fixture: 30-piece position after both sides built
    // overlapping threats. Exercises place/undo cycles deep into the
    // tree where one side's threat extension touches the outer halo
    // of the other side's blocking moves.
    let moves = [
        (0, 0), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1),
        (1, 0), (-2, 0), (-2, 1), (-2, 2), (-1, -1), (-1, 2),
        (0, -2), (0, 2), (1, -2), (1, 1), (2, -2), (2, -1),
        (2, 0), (-3, 0), (-3, 1), (-3, 2), (-3, 3), (-2, -1),
        (-2, 3), (-1, -2), (-1, 3), (0, -3), (0, 3), (1, -3),
    ];
    let board = play(&moves);
    let mut engine = Engine::new(16);
    for c in board.history() {
        engine.board.place_for_test(*c, player_at_ply(engine.board.ply()));
    }
    assert_candidates_oracle(&mut engine, "chimera_postblock");
}

#[test]
fn chimera_endgame_full() {
    // endgame_60 fixture: 60 pieces, dense board where outer halos of
    // many pieces overlap. Largest candidate set; most chances for the
    // r=8 walk to mis-track a count.
    let moves = [
        (0, 0), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1),
        (1, 0), (-2, 0), (-2, 1), (-2, 2), (-1, -1), (-1, 2),
        (0, -2), (0, 2), (1, -2), (1, 1), (2, -2), (2, -1),
        (2, 0), (-3, 0), (-3, 1), (-3, 2), (-3, 3), (-2, -1),
        (-2, 3), (-1, -2), (-1, 3), (0, -3), (0, 3), (1, -3),
        (1, 2), (2, -3), (2, 1), (3, -3), (3, -2), (3, -1),
        (3, 0), (-4, 0), (-4, 1), (-4, 2), (-4, 3), (-4, 4),
        (-3, -1), (-3, 4), (-2, -2), (-2, 4), (-1, -3), (-1, 4),
        (0, -4), (0, 4), (1, -4), (1, 3), (2, -4), (2, 2),
        (3, -4), (3, 1), (4, -4), (4, -3), (4, -2), (4, -1),
    ];
    let board = play(&moves);
    let mut engine = Engine::new(16);
    for c in board.history() {
        engine.board.place_for_test(*c, player_at_ply(engine.board.ply()));
    }
    assert_candidates_oracle(&mut engine, "chimera_endgame_full");
}

#[test]
fn chimera_repeated_place_undo_cycle() {
    // Direct exercise of the Sprint 2C failure path: build a position,
    // place a stone, run a search, undo, and verify the candidate set
    // matches the oracle for the pre-place state. If `place_for_search`
    // and `undo_for_search` skip outer ops without symmetric handling,
    // this asserts the corruption.
    let moves = [
        (0, 0), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1),
        (1, 0), (-2, 0), (-2, 1), (-2, 2), (-1, -1), (-1, 2),
    ];
    let mut engine = Engine::new(16);
    for &(q, r) in &moves {
        engine.board.place_for_test(Coord::new(q, r), player_at_ply(engine.board.ply()));
    }
    let baseline: HashSet<Coord> = engine.board.candidates().collect();
    let baseline_oracle = oracle_candidates(&engine.board);
    assert_eq!(baseline, baseline_oracle, "baseline candidates oracle mismatch");

    // Run a search at depth 6 on the baseline. Search must restore
    // candidates exactly.
    let _ = engine.best_move(None, Some(6));
    let after_search: HashSet<Coord> = engine.board.candidates().collect();
    assert_eq!(
        after_search, baseline_oracle,
        "post-search candidates diverged from baseline (loss = {}, gain = {})",
        baseline_oracle.difference(&after_search).count(),
        after_search.difference(&baseline_oracle).count(),
    );
}
