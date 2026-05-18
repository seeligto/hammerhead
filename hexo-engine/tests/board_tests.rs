use hexo_engine::board::{Board, BoardError, Player};
use hexo_engine::config::MAX_PIECE_DISTANCE;
use hexo_engine::coords::{Coord, ORIGIN, RANGE_OFFSETS, hex_distance};
use std::collections::HashSet;

fn place_ok(b: &mut Board, c: Coord) {
    b.place(c).unwrap_or_else(|e| panic!("place({c:?}) failed: {e:?}"));
}

#[test]
fn new_board_state() {
    let b = Board::new();
    assert_eq!(b.ply(), 0);
    assert_eq!(b.hash(), 0);
    assert_eq!(b.to_move(), Player::X);
    assert_eq!(b.piece_count(), 0);
    let cands: Vec<_> = b.candidates().collect();
    assert_eq!(cands, vec![ORIGIN]);
    assert!(b.is_legal(ORIGIN));
    assert!(!b.is_legal(Coord::new(1, 0)));
}

#[test]
fn first_move_must_be_origin() {
    let mut b = Board::new();
    let err = b.place(Coord::new(1, 0)).unwrap_err();
    assert_eq!(err, BoardError::MustStartAtOrigin(1, 0));
    assert_eq!(b.ply(), 0);
}

#[test]
fn first_move_at_origin() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    assert_eq!(b.ply(), 1);
    assert_eq!(b.to_move(), Player::O);
    assert_eq!(b.piece_at(ORIGIN), Some(Player::X));
    assert_ne!(b.hash(), 0);
}

#[test]
fn parity_sequence() {
    let mut b = Board::new();
    let expected = [
        Player::X, Player::O, Player::O, Player::X, Player::X, Player::O, Player::O, Player::X,
    ];
    let moves = [
        Coord::new(0, 0),
        Coord::new(1, 0),
        Coord::new(2, 0),
        Coord::new(3, 0),
        Coord::new(4, 0),
        Coord::new(5, 0),
        Coord::new(6, 0),
        Coord::new(7, 0),
    ];
    for (i, &c) in moves.iter().enumerate() {
        assert_eq!(b.to_move(), expected[i], "to_move at ply {i}");
        place_ok(&mut b, c);
    }
}

#[test]
fn candidates_after_one_piece() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    let cands: HashSet<Coord> = b.candidates().collect();
    assert_eq!(cands.len(), RANGE_OFFSETS.len());
    assert!(!cands.contains(&ORIGIN));
    assert!(cands.contains(&Coord::new(MAX_PIECE_DISTANCE, 0)));
    assert!(!cands.contains(&Coord::new(MAX_PIECE_DISTANCE + 1, 0)));
    for d in cands.iter() {
        assert!(hex_distance(*d, ORIGIN) <= MAX_PIECE_DISTANCE);
        assert!(hex_distance(*d, ORIGIN) >= 1);
    }
}

#[test]
fn candidates_after_extension() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    place_ok(&mut b, Coord::new(MAX_PIECE_DISTANCE, 0));
    let cands: HashSet<Coord> = b.candidates().collect();
    assert!(cands.contains(&Coord::new(2 * MAX_PIECE_DISTANCE, 0)));
    assert!(!cands.contains(&Coord::new(2 * MAX_PIECE_DISTANCE + 1, 0)));
    assert!(!cands.contains(&ORIGIN));
    assert!(!cands.contains(&Coord::new(MAX_PIECE_DISTANCE, 0)));
}

#[test]
fn undo_restores_state() {
    let mut b = Board::new();
    let initial_cands: HashSet<Coord> = b.candidates().collect();
    let initial_hash = b.hash();
    let initial_ply = b.ply();

    let moves = [
        ORIGIN,
        Coord::new(2, 0),
        Coord::new(0, 3),
    ];
    for &m in &moves {
        place_ok(&mut b, m);
    }
    for _ in 0..moves.len() {
        b.undo().unwrap();
    }

    assert_eq!(b.hash(), initial_hash);
    assert_eq!(b.ply(), initial_ply);
    let after_cands: HashSet<Coord> = b.candidates().collect();
    assert_eq!(after_cands, initial_cands);
    assert_eq!(b.piece_count(), 0);
}

#[test]
fn undo_intermediate() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    place_ok(&mut b, Coord::new(2, 0));

    let cap_hash = b.hash();
    let cap_ply = b.ply();
    let cap_cands: HashSet<Coord> = b.candidates().collect();

    place_ok(&mut b, Coord::new(-1, 1));
    b.undo().unwrap();

    assert_eq!(b.hash(), cap_hash);
    assert_eq!(b.ply(), cap_ply);
    let after_cands: HashSet<Coord> = b.candidates().collect();
    assert_eq!(after_cands, cap_cands);
}

#[test]
fn out_of_range_rejected_then_in_range_accepted() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    let far = Coord::new(MAX_PIECE_DISTANCE + 1, 0);
    let err = b.place(far).unwrap_err();
    assert_eq!(err, BoardError::OutOfRange(far.q, far.r, MAX_PIECE_DISTANCE));
    place_ok(&mut b, Coord::new(MAX_PIECE_DISTANCE, 0));
}

#[test]
fn already_occupied_rejected() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    let err = b.place(ORIGIN).unwrap_err();
    assert_eq!(err, BoardError::AlreadyOccupied(0, 0));
}

#[test]
fn undo_on_empty_board() {
    let mut b = Board::new();
    let err = b.undo().unwrap_err();
    assert_eq!(err, BoardError::NoHistory);
}

#[test]
fn candidates_excludes_occupied() {
    let mut b = Board::new();
    let moves = [
        ORIGIN,
        Coord::new(2, 0),
        Coord::new(-1, 1),
        Coord::new(0, -2),
    ];
    for &m in &moves {
        place_ok(&mut b, m);
    }
    let cands: HashSet<Coord> = b.candidates().collect();
    for m in moves {
        assert!(!cands.contains(&m), "candidate set contains placed {m:?}");
    }
}

#[test]
fn reset_returns_initial_state() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    place_ok(&mut b, Coord::new(2, 0));
    b.reset();
    assert_eq!(b.ply(), 0);
    assert_eq!(b.hash(), 0);
    assert_eq!(b.piece_count(), 0);
    let cands: Vec<_> = b.candidates().collect();
    assert_eq!(cands, vec![ORIGIN]);
}

#[test]
fn is_legal_first_move_only_origin() {
    let b = Board::new();
    assert!(b.is_legal(ORIGIN));
    for c in [Coord::new(1, 0), Coord::new(0, 1), Coord::new(2, -1)] {
        assert!(!b.is_legal(c));
    }
}

#[test]
fn is_legal_after_origin_matches_range() {
    let mut b = Board::new();
    place_ok(&mut b, ORIGIN);
    assert!(b.is_legal(Coord::new(MAX_PIECE_DISTANCE, 0)));
    assert!(!b.is_legal(Coord::new(MAX_PIECE_DISTANCE + 1, 0)));
    // Occupied is not legal.
    assert!(!b.is_legal(ORIGIN));
}

#[test]
fn history_records_placements_in_order() {
    let mut b = Board::new();
    let moves = [ORIGIN, Coord::new(2, 0), Coord::new(-1, 1)];
    for &m in &moves {
        place_ok(&mut b, m);
    }
    assert_eq!(b.history(), &moves);
}

#[test]
fn player_opponent_round_trip() {
    assert_eq!(Player::X.opponent(), Player::O);
    assert_eq!(Player::O.opponent(), Player::X);
    assert_eq!(Player::X.opponent().opponent(), Player::X);
}
